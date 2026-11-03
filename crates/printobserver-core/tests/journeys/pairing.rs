//! A start is checked against the material its file was sliced for and the
//! build sheet its manifest declares.
//!
//! Each journey drives a start through the real action path, with the file's
//! settings answered by the printer double, and reads back the decision, the
//! `preflight_checked` it wrote and whether the printer was ever asked to
//! start. The server's own journeys drive the same checks through the HTTP
//! route against the scripted `OctoPrint`; these hold what the mode, the
//! actor and an unreadable file do to them.

use printobserver_core::kinds::PreflightCheckedPayload;
use printobserver_core::{
    ActionOutcome, Actor, JobManifest, MaterialObjection, MaterialResult, PolicyDecision,
    PreflightMode, PrintAction, RejectionReason, SheetId, SheetObjection, SheetResult, SheetTable,
    SpoolCommand, block_on,
};
use printobserver_printer_api::{PrinterError, PrinterState};
use printobserver_types::{FileName, PrintId};

use crate::fakes::PrinterMethod;
use crate::journal::Call;
use crate::shelf::{FILE, command, every, load, spool_world, spool_world_with};
use crate::world::{World, agent_actor, manifest};

/// A sheet, by its identifier.
fn sheet(id: &str) -> SheetId {
    SheetId::try_from(id.to_owned()).expect("a sheet named by something")
}

/// Register a full spool of one material, and load it onto tool 0.
fn loaded_of(world: &World, material: &str) {
    let spool = command(
        world,
        SpoolCommand::RegisterSpool {
            name: format!("{material} roll"),
            material: material.to_owned(),
            color: None,
            diameter_mm: None,
            density_g_cm3: None,
            initial_g: 1_000.0,
            remaining_g: None,
            tare_g: None,
            reason: "a full roll".to_owned(),
            actor: Actor::Operator,
        },
    )
    .spool
    .expect("registered");
    assert_eq!(load(world, spool.id, 0).decision, PolicyDecision::Accepted);
}

/// The overrides one start asks for.
#[derive(Clone, Copy, Default)]
struct Asked {
    /// Asks past the material check.
    material: bool,
    /// Asks past the sheet check.
    sheet: bool,
}

/// Ask an idle printer to start the file on a sheet, through the real path.
fn start_on(world: &World, sheet: Option<&str>, actor: Actor, asked: Asked) -> ActionOutcome {
    world.printer.reports_state(PrinterState::Operational);
    world.printer.reports_job(None, PrinterState::Operational);
    world.printer.reports_position(None, None);
    world.printer.reports_completion(None);
    let declared = JobManifest {
        sheet: sheet.map(str::to_owned),
        ..manifest(&[])
    };
    world
        .request(
            PrintId::new(),
            PrintAction::StartPrint {
                file_name: FileName::new(FILE).expect("a file name"),
                manifest: declared,
                override_filament_check: false,
                override_material_check: asked.material,
                override_sheet_check: asked.sheet,
                reason: "the next print in the queue".to_owned(),
                actor,
            },
        )
        .expect("the start is recorded")
}

/// The check a start was held to.
fn checked_for(world: &World, outcome: &ActionOutcome) -> PreflightCheckedPayload {
    every::<PreflightCheckedPayload>(world)
        .into_iter()
        .find(|checked| checked.action_id == Some(outcome.record.id))
        .expect("every start writes one check naming it")
}

/// Whether the printer was ever asked to start the file.
fn started(world: &World) -> bool {
    world.journal.count(&Call::Start(FILE.to_owned())) > 0
}

/// A file sliced for PLA, loaded with PETG, is refused naming both; matching
/// is trimmed and without regard to case.
#[test]
fn a_mismatched_material_refuses_and_a_matching_one_in_any_case_passes() {
    let world = spool_world();
    loaded_of(&world, "PETG");
    let refused = start_on(
        &world,
        Some("textured-pei"),
        Actor::Operator,
        Asked::default(),
    );
    let PolicyDecision::Rejected(RejectionReason::MaterialRefused {
        objection: MaterialObjection::Mismatch(compared),
    }) = &refused.record.decision
    else {
        panic!("not a mismatch: {:?}", refused.record.decision);
    };
    assert_eq!(
        (compared.sliced_for.as_str(), compared.loaded.as_str()),
        ("PLA", "PETG")
    );
    assert!(!started(&world));

    world.printer.slices(FILE, &[" petg ", "PLA"]);
    let passed = start_on(
        &world,
        Some("textured-pei"),
        Actor::Operator,
        Asked::default(),
    );
    assert_eq!(passed.record.decision, PolicyDecision::Accepted);
    assert!(matches!(
        checked_for(&world, &passed).preflight.material,
        Some(MaterialResult::Passed(_))
    ));
}

/// A file naming no material, and one whose settings cannot be read, are each
/// refused — and an operator's override lets each through as overridden.
#[test]
fn a_file_naming_no_material_or_unreadable_is_refused_unless_overridden() {
    for unreadable in [false, true] {
        let world = spool_world();
        loaded_of(&world, "PLA");
        if unreadable {
            world.printer.fails(
                PrinterMethod::FilamentTypes,
                PrinterError::Unreachable {
                    detail: "the printer could not be reached".to_owned(),
                },
            );
        } else {
            world.printer.slices(FILE, &[]);
        }
        let refused = start_on(
            &world,
            Some("smooth-pei"),
            Actor::Operator,
            Asked::default(),
        );
        let PolicyDecision::Rejected(RejectionReason::MaterialRefused { objection }) =
            &refused.record.decision
        else {
            panic!(
                "not refused for its material: {:?}",
                refused.record.decision
            );
        };
        assert_eq!(
            matches!(objection, MaterialObjection::Unread { .. }),
            unreadable,
            "{objection:?}"
        );

        let asked = Asked {
            material: true,
            ..Asked::default()
        };
        let overridden = start_on(&world, Some("smooth-pei"), Actor::Operator, asked);
        assert_eq!(overridden.record.decision, PolicyDecision::Accepted);
        assert!(matches!(
            checked_for(&world, &overridden).preflight.material,
            Some(MaterialResult::Overridden { .. })
        ));
        assert!(started(&world));
    }
}

/// An agent asking past either check is refused as though it had not asked.
#[test]
fn an_agents_overrides_are_not_honoured() {
    let world = spool_world();
    loaded_of(&world, "PETG");
    let asked = Asked {
        material: true,
        sheet: true,
    };
    let outcome = start_on(
        &world,
        Some("smooth-pei"),
        agent_actor(PrintId::new()),
        asked,
    );
    assert!(
        matches!(
            outcome.record.decision,
            PolicyDecision::Rejected(RejectionReason::MaterialRefused { .. })
        ),
        "{:?}",
        outcome.record.decision
    );
    assert!(!started(&world));
}

/// Each refusal the sheet table makes names what it compared, and the default
/// table pairs PLA with smooth PEI and PETG with textured PEI.
#[test]
fn the_sheet_table_refuses_each_unsuitable_pairing_naming_it() {
    for (material, sheet, expected) in [
        ("PLA", "textured-pei", Some("not_allowed")),
        ("PETG", "smooth-pei", Some("not_allowed")),
        ("PLA", "glass", Some("unknown_sheet")),
        ("ASA", "smooth-pei", Some("unlisted_material")),
        ("PLA", "Smooth-PEI", None),
        ("PETG", "textured-pei", None),
    ] {
        let world = spool_world();
        loaded_of(&world, material);
        world.printer.slices(FILE, &[material]);
        let outcome = start_on(&world, Some(sheet), Actor::Operator, Asked::default());
        let refused = match &outcome.record.decision {
            PolicyDecision::Rejected(RejectionReason::SheetRefused { objection }) => {
                Some(match objection {
                    SheetObjection::NotAllowed(compared) => {
                        assert_eq!(compared.material.as_str(), material);
                        "not_allowed"
                    }
                    SheetObjection::UnknownSheet { known, .. } => {
                        let known: Vec<&str> = known.iter().map(SheetId::as_str).collect();
                        assert_eq!(known, ["smooth-pei", "textured-pei"]);
                        "unknown_sheet"
                    }
                    SheetObjection::UnlistedMaterial { .. } => "unlisted_material",
                })
            }
            PolicyDecision::Accepted => None,
            other @ PolicyDecision::Rejected(_) => panic!("{material} on {sheet}: {other:?}"),
        };
        assert_eq!(refused, expected, "{material} on {sheet}");
        assert_eq!(started(&world), expected.is_none());
    }
}

/// A manifest declaring no sheet is refused whatever the mode, and whatever
/// an operator overrides; with nothing loaded, a declared sheet is unchecked.
#[test]
fn a_manifest_declaring_no_sheet_is_refused_in_every_mode() {
    for mode in [
        PreflightMode::Refuse,
        PreflightMode::Warn,
        PreflightMode::Off,
    ] {
        let world = spool_world_with(|config| config.spools.preflight = mode);
        let asked = Asked {
            material: true,
            sheet: true,
        };
        for declared in [None, Some("  ")] {
            let outcome = start_on(&world, declared, Actor::Operator, asked);
            assert_eq!(
                outcome.record.decision,
                PolicyDecision::Rejected(RejectionReason::SheetNotDeclared),
                "{mode:?}"
            );
            assert_eq!(
                checked_for(&world, &outcome).preflight.sheet,
                Some(SheetResult::Undeclared)
            );
        }
        assert!(!started(&world));

        let unchecked = start_on(&world, Some("glass"), Actor::Operator, Asked::default());
        assert_eq!(unchecked.record.decision, PolicyDecision::Accepted);
        let checked = checked_for(&world, &unchecked).preflight;
        assert_eq!(checked.material, Some(MaterialResult::Unchecked));
        assert_eq!(checked.sheet, Some(SheetResult::Unchecked));
    }
}

/// Mode `warn` lets a mismatch and an unsuitable sheet through as warned, and
/// mode `off` reads no settings at all.
#[test]
fn mode_warn_warns_and_mode_off_reads_nothing() {
    let warned = spool_world_with(|config| config.spools.preflight = PreflightMode::Warn);
    loaded_of(&warned, "PETG");
    let outcome = start_on(
        &warned,
        Some("smooth-pei"),
        Actor::Operator,
        Asked::default(),
    );
    assert_eq!(outcome.record.decision, PolicyDecision::Accepted);
    let checked = checked_for(&warned, &outcome).preflight;
    assert!(matches!(
        checked.material,
        Some(MaterialResult::Warned { .. })
    ));
    assert!(matches!(checked.sheet, Some(SheetResult::Warned { .. })));

    let off = spool_world_with(|config| config.spools.preflight = PreflightMode::Off);
    loaded_of(&off, "PETG");
    let outcome = start_on(&off, Some("smooth-pei"), Actor::Operator, Asked::default());
    assert_eq!(outcome.record.decision, PolicyDecision::Accepted);
    assert_eq!(off.journal.count(&Call::FilamentTypes(FILE.to_owned())), 0);
}

/// A configured table replaces the defaults, and the read-only check reports
/// the sheet only when it is given one.
#[test]
fn a_configured_table_replaces_the_defaults_and_the_read_reports_a_given_sheet() {
    let table = SheetTable::new(
        [("PETG".to_owned(), vec!["smooth-pei".to_owned()])]
            .into_iter()
            .collect(),
    )
    .expect("a table");
    let world = spool_world_with(|config| config.sheets = table);
    loaded_of(&world, "PETG");
    world.printer.slices(FILE, &["PETG"]);
    let file = FileName::new(FILE).expect("a file name");

    let unsheeted = block_on(world.core.preflight(&file, None)).expect("the check is read");
    assert!(matches!(
        unsheeted.material,
        Some(MaterialResult::Passed(_))
    ));
    assert_eq!(unsheeted.sheet, None);
    let sheeted = block_on(world.core.preflight(&file, Some(&sheet("smooth-pei"))))
        .expect("the check is read");
    assert!(matches!(sheeted.sheet, Some(SheetResult::Passed(_))));

    let outcome = start_on(
        &world,
        Some("smooth-pei"),
        Actor::Operator,
        Asked::default(),
    );
    assert_eq!(outcome.record.decision, PolicyDecision::Accepted);
}

/// A table is refused with a blank material or sheet, a material listing no
/// sheet, or two materials differing only in case.
#[test]
fn a_table_that_cannot_work_is_refused() {
    for entries in [
        vec![(" ", vec!["smooth-pei"])],
        vec![("PLA", vec![" "])],
        vec![("PLA", vec![])],
        vec![("PLA", vec!["smooth-pei"]), (" pla", vec!["textured-pei"])],
    ] {
        let entries = entries
            .into_iter()
            .map(|(material, sheets)| {
                (
                    material.to_owned(),
                    sheets.into_iter().map(str::to_owned).collect(),
                )
            })
            .collect();
        assert!(SheetTable::new(entries).is_err());
    }
}

/// A `preflight_checked` and a `print_started` written before these checks
/// existed: a check carrying neither result.
const STORED_CHECK: &str = concat!(
    r#"{"file_name":"benchy.gcode","tools":[{"tool":0,"result":{"verdict":"unchecked"}}],"#,
    r#""verdict":"unchecked","checked_at":"2026-09-01T10:00:00Z"}"#
);

/// A start written before the sheet and the two overrides existed.
const STORED_START: &str = concat!(
    r#"{"action":"start_print","file_name":"benchy.gcode","#,
    r#""manifest":{"file_name":"benchy.gcode","material":"PLA","nozzle_diameter_mm":0.4,"#,
    r#""slicer_profile":"0.2mm standard","allowed":{},"metadata":{}},"#,
    r#""reason":"the operator started it","actor":"operator"}"#
);

/// Events and starts stored before these checks still load, read as carrying
/// no result, no sheet and no override, and write back byte for byte.
#[test]
fn checks_and_starts_stored_before_these_checks_still_load() {
    use printobserver_core::Preflight;
    use printobserver_core::kinds::PrintStartedPayload;
    use printobserver_types::serde_json;

    let check: Preflight = serde_json::from_str(STORED_CHECK).expect("a stored check reads");
    assert_eq!(
        (check.material.as_ref(), check.sheet.as_ref()),
        (None, None)
    );
    assert_eq!(
        serde_json::to_string(&check).expect("a check writes"),
        STORED_CHECK
    );
    let checked: PreflightCheckedPayload =
        serde_json::from_str(&format!(r#"{{"preflight":{STORED_CHECK}}}"#))
            .expect("a stored preflight_checked reads");
    assert_eq!(checked.preflight, check);
    let started: PrintStartedPayload =
        serde_json::from_str(&format!(r#"{{"loaded":[],"preflight":{STORED_CHECK}}}"#))
            .expect("a stored print_started reads");
    assert_eq!(started.preflight, Some(check));

    let start: PrintAction = serde_json::from_str(STORED_START).expect("a stored start reads");
    let PrintAction::StartPrint {
        manifest,
        override_material_check,
        override_sheet_check,
        ..
    } = &start
    else {
        panic!("a stored start reads as a start, not {start:?}");
    };
    assert_eq!(manifest.sheet, None);
    assert!(!override_material_check && !override_sheet_check);
    assert_eq!(
        serde_json::to_string(&start).expect("a start writes"),
        STORED_START
    );
}

/// Each material and sheet refusal reads back in words naming what was
/// compared, and the missing sheet says no override lets it through.
#[test]
fn each_material_and_sheet_refusal_reads_back_naming_what_was_compared() {
    use printobserver_core::rejection_detail;
    use printobserver_types::contract::Sample as _;

    let said: Vec<String> = RejectionReason::sample_alternates()
        .iter()
        .filter(|reason| {
            matches!(
                reason,
                RejectionReason::MaterialRefused { .. }
                    | RejectionReason::SheetRefused { .. }
                    | RejectionReason::SheetNotDeclared
            )
        })
        .map(rejection_detail)
        .collect();
    assert_eq!(said.len(), 3, "{said:?}");
    for fact in ["sliced for PLA", "Galaxy black", "tool 0", "PETG"] {
        assert!(said[0].contains(fact), "{} omits {fact}", said[0]);
    }
    for fact in ["PETG on textured-pei", "not on smooth-pei"] {
        assert!(said[1].contains(fact), "{} omits {fact}", said[1]);
    }
    assert!(said[2].contains("`sheet`") && said[2].contains("no override"));

    for objection in MaterialObjection::sample_alternates() {
        let said = rejection_detail(&RejectionReason::MaterialRefused { objection });
        assert!(
            said.contains("benchy.gcode") && said.contains("PETG"),
            "{said}"
        );
    }
    for objection in SheetObjection::sample_alternates() {
        let said = rejection_detail(&RejectionReason::SheetRefused { objection });
        assert!(said.contains("sheet table"), "{said}");
    }
}
