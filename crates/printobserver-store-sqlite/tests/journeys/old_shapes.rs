//! Events printobserver 0.4.3 wrote, read by this build's store.
//!
//! Every payload below is a literal in the shape 0.4.3 wrote it — the text a
//! store holds, not one serialized by this build — written as a row of the
//! database 0.4.3 wrote and read back through the store's own history, then
//! as its own kind; so a shape this build cannot read is refused here rather
//! than by a reader of an operator's history, and one it cannot read whole is
//! answered as it was stored.

use printobserver_core::kinds::{
    ChargePendedPayload, FilamentChangeSignalledPayload, FilamentConsumedPayload,
    PreflightCheckedPayload, PrintStartedPayload, SpoolCommandRejectedPayload,
    SpoolRegisteredPayload,
};
use printobserver_core::{
    PolicyDecision, PreflightObjection, PreflightToolResult, RejectionReason, SpoolField,
    SpoolLifecycle, SpoolRefusalCause,
};
use printobserver_types::serde_json::{Map, Value, json};
use printobserver_types::{EventBody, EventPayload};
use tempfile::TempDir;

use crate::block_on::block_on;
use crate::fixture::Store;
use crate::upgrading::{PRINT, fixture};
use printobserver_core::store::HistoryQuery;
use printobserver_store_sqlite::{DATABASE_FILE_NAME, SqliteStore, connect};

/// One body, written as a row of the database 0.4.3 wrote and read back
/// through this build's store: the history of the fixture's print answers it.
fn read(kind: &str, payload: Value) -> EventBody {
    let dir = TempDir::new().expect("a temporary state directory");
    let path = dir.path().join(DATABASE_FILE_NAME);
    std::fs::copy(fixture(), &path).expect("the fixture copies");
    let id = printobserver_types::EventId::new().to_string();
    let mut body = Map::new();
    body.insert("kind".to_owned(), json!(kind));
    body.insert("payload".to_owned(), payload);
    let stored = Value::Object(body).to_string();
    connect(&path)
        .expect("the database opens")
        .execute(
            "INSERT INTO events (id, print_id, source, received_at, kind, payload, raw) \
             VALUES (?1, ?2, 'system', '2026-03-01T13:00:00.000000000Z', ?3, ?4, NULL)",
            [id.as_str(), PRINT, kind, stored.as_str()],
        )
        .expect("the row is written");
    let store = SqliteStore::open(dir.path()).expect("the 0.4.3 database opens");
    let port: &dyn Store = &store;
    let history = block_on(port.history(HistoryQuery {
        print_id: PRINT.parse().expect("an id"),
        kinds: Vec::new(),
        since: None,
        until: None,
        limit: Some(1_000),
    }))
    .expect("the print's history reads");
    history
        .into_iter()
        .find(|event| event.id.to_string() == id)
        .expect("the history answers the row")
        .body
}

/// One body, written and read back as above, and then read as its own kind.
fn read_as<P: EventPayload>(payload: Value) -> P {
    let body = read(P::KIND, payload);
    printobserver_types::serde_json::from_value(body.payload.clone())
        .unwrap_or_else(|error| panic!("{} reads as {error}: {}", P::KIND, body.payload))
}

/// A spool every literal names.
const SPOOL: &str = "01a176c4-5488-7532-8297-73e157be17bc";
/// An instant.
const AT: &str = "2026-03-01T12:00:00Z";

/// A charge 0.4.3 wrote against a spool reads with its calculation whole; one
/// it wrote with no spool reads with none; one naming a spool with half a
/// calculation, or already in this build's shape, is answered as stored.
#[test]
fn a_consumption_reads_with_its_calculation_whole_or_absent() {
    let key = json!({ "key": "print", "print_id": PRINT, "tool": 1, "segment": 0 });
    let charged: FilamentConsumedPayload = read_as(json!({
        "charge_key": key, "tool": 1, "spool_id": SPOOL, "method": "gcode_extrusion",
        "length_mm": 4000.0, "grams": 12.0, "diameter_mm": 1.75, "density_g_cm3": 1.27,
        "remaining_before_g": 80.0, "remaining_after_g": 68.0,
    }));
    let whole = charged.charged.clone().expect("the calculation is whole");
    assert_eq!(whole.spool_id.to_string(), SPOOL);
    assert!((whole.remaining_after_g - 68.0).abs() < 1e-9);
    let uncharged: FilamentConsumedPayload = read_as(json!({
        "charge_key": key, "tool": 1, "method": "slicer_estimate", "length_mm": 10.0,
    }));
    assert_eq!(uncharged.charged, None);
    let half = json!({
        "charge_key": key, "tool": 1, "spool_id": SPOOL, "method": "gcode_extrusion",
        "length_mm": 4000.0, "grams": 12.0,
    });
    assert_eq!(read("filament_consumed", half.clone()).payload, half);
    let current = printobserver_types::serde_json::to_value(&charged).expect("it writes");
    assert_eq!(read("filament_consumed", current.clone()).payload, current);
    assert_eq!(read("filament_consumed", json!(7)).payload, json!(7));
}

/// A filament change 0.4.3 wrote reads with its position carrying its own
/// freshness, or with no position and no claim; one already in this build's
/// shape is answered as stored.
#[test]
fn a_filament_change_reads_with_its_position_carrying_its_freshness() {
    let fresh: FilamentChangeSignalledPayload = read_as(json!({
        "position": { "bytes": 40960, "observed_at": AT },
        "position_read_at_signal": true, "source_kind": "obico_printer_notification",
        "spool_id": SPOOL, "tool": 1,
    }));
    let position = fresh.position.expect("a position");
    assert!(position.read_at_signal);
    assert_eq!(position.position.bytes, 40_960);
    let none: FilamentChangeSignalledPayload = read_as(json!({
        "position_read_at_signal": false, "source_kind": "obico_printer_notification",
    }));
    assert_eq!(none.position, None);
    let current = json!({ "source_kind": "obico_printer_notification" });
    assert_eq!(
        read("filament_change_signalled", current.clone()).payload,
        current
    );
    assert_eq!(
        read("filament_change_signalled", json!("x")).payload,
        json!("x")
    );
}

/// A spool 0.4.3 registered reads with the lifecycle its loaded tool and its
/// retirement say.
#[test]
fn a_registered_spool_reads_with_its_lifecycle() {
    let spool = |extra: Value| {
        let mut spool = json!({
            "id": SPOOL, "name": "PLA", "material": "PLA", "diameter_mm": 1.75,
            "density_g_cm3": 1.24, "initial_g": 1000.0, "remaining_g": 640.0,
            "trust": { "state": "verified" }, "registered_at": AT, "updated_at": AT,
        });
        if let (Some(spool), Some(extra)) = (spool.as_object_mut(), extra.as_object()) {
            spool.extend(extra.clone());
        }
        json!({ "spool": spool, "actor": "operator", "reason": "bought" })
    };
    for (extra, lifecycle) in [
        (
            json!({ "loaded_on_tool": 0 }),
            SpoolLifecycle::Loaded { tool: 0 },
        ),
        (json!({}), SpoolLifecycle::Shelved),
        (
            json!({ "loaded_on_tool": 0, "retired_at": AT }),
            SpoolLifecycle::Retired {
                at: AT.parse().expect("an instant"),
            },
        ),
    ] {
        let registered: SpoolRegisteredPayload = read_as(spool(extra));
        assert_eq!(registered.spool.lifecycle, lifecycle);
    }
    let current = spool(json!({ "lifecycle": { "state": "shelved" } }));
    assert_eq!(read("spool_registered", current.clone()).payload, current);
    let not_a_spool = json!({ "spool": 3 });
    assert_eq!(
        read("spool_registered", not_a_spool.clone()).payload,
        not_a_spool
    );
}

/// A pending charge 0.4.3 held reads keyed by its print segment alone, when
/// what it repeated is what its key names, and is answered as stored when not.
#[test]
fn a_pending_charge_reads_keyed_by_its_print_segment() {
    let pending = |print: &str, tool: i64| {
        json!({ "pending": {
            "charge_key": { "key": "print", "print_id": PRINT, "tool": 1, "segment": 1 },
            "print_id": print, "tool": tool, "length_mm": 250.0, "method": "gcode_extrusion",
            "pended_at": AT,
        } })
    };
    let pended: ChargePendedPayload = read_as(pending(PRINT, 1));
    assert_eq!(pended.pending.charge_key.segment, 1);
    for disagreeing in [pending(PRINT, 2), pending(SPOOL, 1)] {
        assert_eq!(
            read("charge_pended", disagreeing.clone()).payload,
            disagreeing
        );
    }
    let unobserved = json!({ "pending": { "charge_key": { "key": "unobserved" } } });
    assert_eq!(
        read("charge_pended", unobserved.clone()).payload,
        unobserved
    );
}

/// What one read result must be.
type Holds = fn(&PreflightToolResult) -> bool;

/// One tool of a filament check as 0.4.3 wrote it.
fn tool(verdict: &str, figures: &Value) -> Value {
    let mut tool = json!({ "tool": 0, "verdict": verdict });
    if let (Some(tool), Some(figures)) = (tool.as_object_mut(), figures.as_object()) {
        tool.extend(figures.clone());
    }
    tool
}

/// Every verdict a filament check of 0.4.3 carried reads as one tagged result
/// with that verdict's evidence, a check let through keeping what it objected
/// to — and a tool whose figures its verdict cannot rest on leaves the check
/// as stored.
#[test]
fn every_preflight_verdict_reads_as_one_tagged_result() {
    let spool = json!({ "spool_id": SPOOL, "spool_name": "PETG", "remaining_g": 80.0 });
    let need = json!({ "required_g": 120.0, "buffer_g": 12.0 });
    let with = |extra: &Value| {
        let mut figures = spool.clone();
        if let (Some(figures), Some(extra)) = (figures.as_object_mut(), extra.as_object()) {
            figures.extend(extra.clone());
        }
        figures
    };
    let marks = json!({ "marks": [{ "reason": { "reason": "uncharged_print", "print_id": PRINT }, "since": AT }] });
    let enough = with(&json!({ "required_g": 10.0, "buffer_g": 1.0 }));
    let cases: Vec<(Value, Holds)> = vec![
        (tool("unchecked", &json!({})), |result| {
            *result == PreflightToolResult::Unchecked
        }),
        (tool("passed", &enough), |result| {
            matches!(result, PreflightToolResult::Passed { .. })
        }),
        (
            tool("insufficient", &with(&need)),
            |result| matches!(result, PreflightToolResult::Insufficient(short) if short.marks.is_empty()),
        ),
        (
            tool("unverified_spool", &with(&marks)),
            |result| matches!(result, PreflightToolResult::UnverifiedSpool(doubted) if doubted.need.is_none()),
        ),
        (tool("no_analysis", &spool), |result| {
            matches!(result, PreflightToolResult::NoAnalysis(_))
        }),
        (tool("overridden", &with(&need)), |result| {
            matches!(
                result,
                PreflightToolResult::Overridden {
                    objection: PreflightObjection::Insufficient(_)
                }
            )
        }),
        (tool("warned", &with(&marks)), |result| {
            matches!(
                result,
                PreflightToolResult::Warned {
                    objection: PreflightObjection::UnverifiedSpool(_)
                }
            )
        }),
        (tool("warned", &spool), |result| {
            matches!(
                result,
                PreflightToolResult::Warned {
                    objection: PreflightObjection::NoAnalysis(_)
                }
            )
        }),
    ];
    for (old, holds) in cases {
        let checked: PreflightCheckedPayload = read_as(json!({ "preflight": {
            "file_name": "bracket.gcode", "checked_at": AT, "verdict": "unchecked",
            "tools": [old.clone()],
        } }));
        let result = &checked.preflight.tools[0].result;
        assert!(holds(result), "{old} read as {result:?}");
    }
    let started: PrintStartedPayload = read_as(json!({
        "file_name": "bracket.gcode", "loaded": [],
        "preflight": { "file_name": "bracket.gcode", "checked_at": AT, "verdict": "passed",
            "tools": [tool("passed", &enough)] },
    }));
    assert!(started.preflight.is_some());
    for unreadable in [
        tool("passed", &spool),
        tool("insufficient", &spool),
        tool("no_analysis", &json!({ "spool_id": SPOOL })),
        tool("a_verdict_from_nowhere", &spool),
        json!({ "tool": 0 }),
        json!(4),
    ] {
        let check = json!({ "preflight": { "tools": [unreadable] } });
        assert_eq!(read("preflight_checked", check.clone()).payload, check);
    }
    let no_tools = json!({ "preflight": { "file_name": "bracket.gcode" } });
    assert_eq!(
        read("preflight_checked", no_tools.clone()).payload,
        no_tools
    );
}

/// One command of 0.4.3's, with these fields beside the ones it always carries.
fn command(name: &str, always: Value, figures: &Value) -> Value {
    let mut command = always;
    command["command"] = json!(name);
    command["reason"] = json!("r");
    command["actor"] = json!("operator");
    if let (Some(command), Some(figures)) = (command.as_object_mut(), figures.as_object()) {
        command.extend(figures.clone());
    }
    command
}

/// A load 0.4.3 refused.
fn load() -> Value {
    command(
        "load_spool",
        json!({ "spool_id": SPOOL, "tool": 1 }),
        &json!({}),
    )
}

/// A correction 0.4.3 refused, naming these figures.
fn adjust(figures: &Value) -> Value {
    command("adjust_spool", json!({ "spool_id": SPOOL }), figures)
}

/// A registration 0.4.3 refused, naming these figures.
fn register(figures: &Value) -> Value {
    command(
        "register_spool",
        json!({ "name": "n", "material": " PEEK ", "initial_g": 0.0 }),
        figures,
    )
}

/// The refusal 0.4.3 wrote for a command, read in this build's shape.
fn refused_for(command: &Value, detail: &str) -> PolicyDecision {
    let rejected: SpoolCommandRejectedPayload = read_as(json!({
        "command": command,
        "decision": { "rejected": { "spool_refused": { "detail": detail } } },
    }));
    rejected.decision
}

/// The decision a refusal of `detail` for `cause` reads as.
fn reads_as(cause: SpoolRefusalCause, detail: &str) -> PolicyDecision {
    PolicyDecision::Rejected(RejectionReason::SpoolRefused {
        cause,
        message: detail.to_owned(),
    })
}

/// Every refusal sentence 0.4.3 wrote about a spool, a tool or a correction
/// reads with the cause it stated, the cause's fields taken from the command
/// it refused.
#[test]
fn every_refusal_sentence_about_a_spool_or_a_correction_reads_with_its_cause() {
    let spool_id = SPOOL.parse().expect("an id");
    let cases = [
        (
            load(),
            "there is no spool x; list the spools",
            SpoolRefusalCause::UnknownSpool { spool_id },
        ),
        (
            load(),
            "spool PLA is retired and takes no further command; register",
            SpoolRefusalCause::RetiredSpool { spool_id },
        ),
        (
            load(),
            "spool PLA is already on tool 1, and nothing is pending there to confirm",
            SpoolRefusalCause::NothingToConfirm { tool: 1 },
        ),
        (
            load(),
            "spool PLA is not loaded on any tool, so there is nothing to unload",
            SpoolRefusalCause::NotLoaded { spool_id },
        ),
        (
            load(),
            "there is no tool 1; tools are numbered from 0",
            SpoolRefusalCause::UnknownTool { tool: 1 },
        ),
        (
            load(),
            "the ledger refused the change on spools.loaded_on_tool",
            SpoolRefusalCause::LedgerRefused,
        ),
        (
            adjust(&json!({ "gross_g": 900.0 })),
            "spool PLA has no tare, so a scale reading cannot be turned into a figure",
            SpoolRefusalCause::TareRequired { spool_id },
        ),
        (
            adjust(&json!({ "remaining_g": 1.0, "gross_g": 2.0 })),
            "a correction names exactly one of remaining_g and gross_g",
            SpoolRefusalCause::ConflictingFields {
                fields: vec![SpoolField::RemainingG, SpoolField::GrossG],
            },
        ),
        (
            adjust(&json!({})),
            "a correction names exactly one of remaining_g and gross_g",
            SpoolRefusalCause::MissingField {
                field: SpoolField::RemainingG,
            },
        ),
        (
            adjust(&json!({ "gross_g": -5.0 })),
            "the figure given is not a weight in grams",
            SpoolRefusalCause::InvalidAdjustmentFigure {
                field: SpoolField::GrossG,
                value: -5.0,
            },
        ),
        (
            adjust(&json!({ "remaining_g": -5.0 })),
            "the figure given is not a weight in grams",
            SpoolRefusalCause::InvalidAdjustmentFigure {
                field: SpoolField::RemainingG,
                value: -5.0,
            },
        ),
    ];
    for (command, detail, cause) in cases {
        assert_eq!(
            refused_for(&command, detail),
            reads_as(cause, detail),
            "{detail}"
        );
    }
}

/// Every refusal sentence 0.4.3 wrote about a registration reads with the
/// cause it stated; a sentence it never wrote, or one whose figure the command
/// does not carry, is answered as stored.
#[test]
fn every_refusal_sentence_about_a_registration_reads_with_its_cause() {
    let figure = |field, value| SpoolRefusalCause::InvalidAdjustmentFigure { field, value };
    let cases = [
        (
            register(&json!({})),
            "a spool needs a name to be known by",
            SpoolRefusalCause::MissingField {
                field: SpoolField::Name,
            },
        ),
        (
            register(&json!({})),
            "a spool needs its material, such as PLA",
            SpoolRefusalCause::MissingField {
                field: SpoolField::Material,
            },
        ),
        (
            register(&json!({})),
            "PEEK has no known density, so the spool needs one",
            SpoolRefusalCause::DensityRequired {
                spool_id: None,
                material: "PEEK".to_owned(),
            },
        ),
        (
            register(&json!({})),
            "a diameter of 0mm is not one a filament has",
            figure(SpoolField::DiameterMm, 1.75),
        ),
        (
            register(&json!({ "diameter_mm": 0.0 })),
            "a diameter of 0mm is not one a filament has",
            figure(SpoolField::DiameterMm, 0.0),
        ),
        (
            register(&json!({ "density_g_cm3": -1.0 })),
            "a density of -1g/cm3 is not one a material has",
            figure(SpoolField::DensityGCm3, -1.0),
        ),
        (
            register(&json!({})),
            "a full spool of 0g holds nothing",
            figure(SpoolField::InitialG, 0.0),
        ),
        (
            register(&json!({ "remaining_g": 3.0 })),
            "the remaining figure is not a number",
            figure(SpoolField::RemainingG, 3.0),
        ),
        (
            register(&json!({ "tare_g": -2.0 })),
            "an empty spool's weight is at least zero grams",
            figure(SpoolField::TareG, -2.0),
        ),
    ];
    for (command, detail, cause) in cases {
        assert_eq!(
            refused_for(&command, detail),
            reads_as(cause, detail),
            "{detail}"
        );
    }
    for unread in [
        json!({ "command": load(), "decision": { "rejected": { "spool_refused": { "detail": "a sentence 0.4.3 never wrote" } } } }),
        json!({ "command": register(&json!({})), "decision": { "rejected": { "spool_refused": { "detail": "a density of 2g/cm3 is not one a material has" } } } }),
        json!({ "decision": "accepted" }),
    ] {
        assert_eq!(
            read("spool_command_rejected", unread.clone()).payload,
            unread
        );
    }
}

/// A kind no earlier shape is known of is answered as stored.
#[test]
fn a_kind_with_no_earlier_shape_is_answered_as_stored() {
    let payload = json!({ "anything": [1, 2, 3] });
    assert_eq!(
        read("kind_from_a_newer_server", payload.clone()).payload,
        payload
    );
}

/// A row in an earlier shape whose fields its kind cannot hold — a freshness
/// that is no flag, a loaded tool that is no number, a calculation that is no
/// figure, a doubt with no mark — is answered as it was stored rather than
/// rewritten into a payload its kind cannot be.
#[test]
fn an_old_row_whose_fields_its_kind_cannot_hold_is_answered_as_stored() {
    let key = json!({ "key": "print", "print_id": PRINT, "tool": 1, "segment": 0 });
    for (kind, payload) in [
        (
            "filament_change_signalled",
            json!({ "position": { "bytes": 1, "observed_at": AT }, "position_read_at_signal": "yes",
                    "source_kind": "obico_printer_notification" }),
        ),
        (
            "spool_registered",
            json!({ "spool": { "id": SPOOL, "loaded_on_tool": null }, "actor": "operator", "reason": "r" }),
        ),
        (
            "filament_change_signalled",
            json!({ "position_read_at_signal": "yes", "source_kind": "obico_printer_notification" }),
        ),
        (
            "preflight_checked",
            json!({ "preflight": { "file_name": "f", "checked_at": AT, "verdict": "insufficient",
                    "tools": [tool("insufficient", &json!({ "spool_id": SPOOL, "spool_name": "P",
                        "remaining_g": 1.0, "required_g": 120.0 }))] } }),
        ),
        (
            "preflight_checked",
            json!({ "preflight": { "file_name": "f", "checked_at": AT, "verdict": "unchecked",
                    "tools": [tool("unchecked", &json!({ "spool_id": SPOOL }))] } }),
        ),
        (
            "preflight_checked",
            json!({ "preflight": { "file_name": "f", "checked_at": AT, "verdict": "passed",
                    "tools": [tool("passed", &json!({ "spool_id": SPOOL, "spool_name": "P",
                        "remaining_g": 100.0, "required_g": 1.0, "buffer_g": 1.0,
                        "marks": [{ "reason": { "reason": "uncharged_print", "print_id": PRINT }, "since": AT }] }))] } }),
        ),
        (
            "filament_consumed",
            json!({ "charge_key": key, "tool": 1, "spool_id": SPOOL, "method": "gcode_extrusion",
                    "length_mm": 1.0, "grams": "twelve", "diameter_mm": 1.75, "density_g_cm3": 1.27,
                    "remaining_before_g": 80.0, "remaining_after_g": 68.0 }),
        ),
        (
            "filament_consumed",
            json!({ "charge_key": key, "tool": 1, "method": "gcode_extrusion", "length_mm": 1.0,
                    "grams": 12.0 }),
        ),
        (
            "preflight_checked",
            json!({ "preflight": { "file_name": "f", "checked_at": AT, "verdict": "unverified_spool",
                    "tools": [tool("unverified_spool", &json!({ "spool_id": SPOOL, "spool_name": "P", "remaining_g": 1.0 }))] } }),
        ),
    ] {
        assert_eq!(read(kind, payload.clone()).payload, payload, "{kind}");
    }
}
