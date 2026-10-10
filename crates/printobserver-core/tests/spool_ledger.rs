//! The filament ledger's vocabulary holds the wire contract the plan states.
//!
//! Everything here reads the serialized form a consumer receives rather than
//! the value that survives a round trip, because the contract is the wire: a
//! key spelled one way, an empty value left off rather than written `null`, a
//! rejection carrying exactly what it states. Beside it stand the stored forms
//! written before the ledger existed — a start with no override, held as an
//! action's request — which must read with the stated default and write back
//! byte for byte, so a row written by an earlier server is untouched by this
//! one.

#[path = "support/records.rs"]
mod records;

use printobserver_core::{
    ActionKind, ChargeKey, CoreConfig, PreflightMode, PreflightTool, PreflightVerdict, PrintAction,
    RejectionReason, SpoolCommand, SpoolId, SpoolPolicy, SpoolRecord, SpoolTrust, UnverifiedMark,
    UnverifiedReason, changes_the_machine, rejection_detail, valid_from,
};
use printobserver_printer_api::PrinterState;
use printobserver_types::contract::{Sample, schema_of};
use printobserver_types::serde_json::{self, Value, json};
use printobserver_types::{PrintId, Timestamp};
use records::declared;

/// The ledger's records, commands and the vocabulary they reach.
const LEDGER_RECORDS: [&str; 17] = [
    "ChargeKey",
    "ChargeMethod",
    "ClearedMark",
    "FilePosition",
    "LoadedSpool",
    "PendingCharge",
    "PendingTaken",
    "Preflight",
    "PreflightTool",
    "PreflightVerdict",
    "PrintOutcome",
    "RejectionReason",
    "SpoolCommand",
    "SpoolId",
    "SpoolRecord",
    "SpoolTrust",
    "UnverifiedReason",
];

/// One fixed instant, `seconds` after the samples' own.
fn at(seconds: i64) -> Timestamp {
    Timestamp::sample_full()
        .plus_seconds(seconds)
        .expect("an instant")
}

/// The keys one serialized object carries.
fn keys(value: &Value) -> Vec<String> {
    value
        .as_object()
        .expect("an object")
        .keys()
        .cloned()
        .collect()
}

/// Every canonical value of every ledger record, and every variant of the
/// spool command vocabulary, is one its own generated schema admits; and the
/// schema refuses what the type refuses.
#[test]
fn every_ledger_record_validates_against_its_generated_schema() {
    let entries = declared();
    for name in LEDGER_RECORDS {
        let entry = entries
            .iter()
            .find(|entry| entry.name == name)
            .unwrap_or_else(|| panic!("{name} is declared into the schema set"));
        let validator =
            jsonschema::validator_for(&entry.schema()).expect("the generated schema compiles");
        for value in entry.samples() {
            assert!(
                validator.is_valid(&value),
                "{name}'s schema refused {value}"
            );
        }
    }

    let commands: Vec<String> = std::iter::once(SpoolCommand::sample_full())
        .chain(SpoolCommand::sample_alternates())
        .map(|command| {
            serde_json::to_value(&command).expect("a command writes")["command"]
                .as_str()
                .expect("a tagged command")
                .to_owned()
        })
        .collect();
    for variant in [
        "register_spool",
        "load_spool",
        "unload_spool",
        "adjust_spool",
        "retire_spool",
    ] {
        assert!(
            commands.iter().any(|command| command == variant),
            "no canonical command drives {variant}: {commands:?}"
        );
    }

    let validator =
        jsonschema::validator_for(&schema_of::<SpoolCommand>()).expect("the schema compiles");
    let mut unknown = serde_json::to_value(SpoolCommand::sample_full()).expect("a command writes");
    unknown["gcode"] = json!("M112");
    assert!(!validator.is_valid(&unknown));
    assert!(serde_json::from_value::<SpoolCommand>(unknown).is_err());
    let untagged = json!({ "command": "melt_spool", "reason": "no", "actor": "operator" });
    assert!(!validator.is_valid(&untagged));
    assert!(serde_json::from_value::<SpoolCommand>(untagged).is_err());
}

/// A charge key is spelled the one way the store keys by, and two keys that
/// differ only in tally generation or ordinal are two spellings.
#[test]
fn a_charge_key_has_one_canonical_spelling() {
    let print_id: PrintId = "0191f0a0-0000-7000-8000-000000000001"
        .parse()
        .expect("a fixed identifier");
    let watched = ChargeKey::Print {
        print_id,
        tool: 1,
        segment: 2,
    };
    assert_eq!(
        watched.canonical(),
        "print:0191f0a0-0000-7000-8000-000000000001:1:2"
    );

    let unobserved = |generation, print_number| ChargeKey::Unobserved {
        origin: "local".to_owned(),
        file_name: "parts/a:b.gcode".to_owned(),
        generation,
        print_number,
        tool: 0,
    };
    assert_eq!(
        unobserved(3, 7).canonical(),
        "unobserved:local:3:7:0:parts/a:b.gcode"
    );
    assert_ne!(unobserved(3, 7).canonical(), unobserved(4, 7).canonical());
    assert_ne!(unobserved(3, 7).canonical(), unobserved(3, 8).canonical());

    assert_eq!(
        serde_json::to_value(&watched).expect("a key writes"),
        json!({
            "key": "print",
            "print_id": "0191f0a0-0000-7000-8000-000000000001",
            "tool": 1,
            "segment": 2
        })
    );
}

/// An unloaded, unretired spool with no tare and no colour is written with
/// none of those keys, and reads back with each absent.
#[test]
fn an_empty_spool_field_is_left_off_the_wire() {
    let spool = SpoolRecord::sample_minimal();
    let written = serde_json::to_value(&spool).expect("a spool writes");
    for absent in ["loaded_on_tool", "retired_at", "tare_g", "color"] {
        assert!(
            !keys(&written).iter().any(|key| key == absent),
            "an empty spool is written with {absent}: {written}"
        );
    }
    assert_eq!(written["trust"], json!({ "state": "verified" }));
    let read: SpoolRecord = serde_json::from_value(written).expect("the spool reads back");
    assert_eq!(read.loaded_on_tool, None);
    assert_eq!(read.retired_at, None);
    assert_eq!(read.tare_g, None);
    assert_eq!(read.color, None);
    assert_eq!(read, spool);
}

/// A tool whose spool holds no mark is written without `marks`, and one whose
/// spool holds marks carries them oldest first.
#[test]
fn a_preflight_tool_with_no_marks_is_written_without_them() {
    let clean = PreflightTool {
        tool: 0,
        spool_id: Some(SpoolId::sample_full()),
        spool_name: Some("Galaxy black".to_owned()),
        required_g: Some(40.0),
        buffer_g: Some(5.0),
        remaining_g: Some(700.0),
        marks: Vec::new(),
        verdict: PreflightVerdict::Passed,
    };
    let written = serde_json::to_value(&clean).expect("a tool writes");
    assert!(
        !keys(&written).iter().any(|key| key == "marks"),
        "{written}"
    );
    assert_eq!(written["verdict"], json!("passed"));

    let marked = PreflightTool::sample_full();
    let written = serde_json::to_value(&marked).expect("a tool writes");
    assert_eq!(
        written["marks"][0]["reason"]["reason"],
        json!("uncharged_print")
    );
}

/// A short spool that is verified is refused without `unverified`; a short
/// spool that is also unverified is refused naming the spool and its oldest
/// mark, so the one refusal states the shortfall and the doubt together.
#[test]
fn an_insufficient_filament_rejection_names_the_oldest_mark_only_when_there_is_one() {
    let refusal = |trust: &SpoolTrust| RejectionReason::InsufficientFilament {
        tool: 0,
        spool_id: SpoolId::sample_full(),
        spool_name: "Galaxy black".to_owned(),
        required_g: 120.0,
        buffer_g: 12.0,
        remaining_g: 80.0,
        unverified: trust.oldest_mark().map(|mark| mark.reason.clone()),
    };

    let verified = serde_json::to_value(refusal(&SpoolTrust::Verified)).expect("a refusal writes");
    let body = &verified["insufficient_filament"];
    assert_eq!(body["spool_name"], json!("Galaxy black"));
    assert!(!keys(body).iter().any(|key| key == "unverified"), "{body}");

    let print_id = PrintId::sample_full();
    let trust = SpoolTrust::Verified
        .marked(UnverifiedMark {
            reason: UnverifiedReason::TallyReset {
                file_name: "benchy.gcode".to_owned(),
                origin: "local".to_owned(),
            },
            since: at(0),
        })
        .marked(UnverifiedMark {
            reason: UnverifiedReason::UnchargedPrint { print_id },
            since: at(60),
        });
    let unverified = serde_json::to_value(refusal(&trust)).expect("a refusal writes");
    let body = &unverified["insufficient_filament"];
    assert_eq!(body["spool_name"], json!("Galaxy black"));
    assert_eq!(
        body["unverified"],
        json!({ "reason": "tally_reset", "file_name": "benchy.gcode", "origin": "local" })
    );
    let read: RejectionReason = serde_json::from_value(unverified).expect("a refusal reads");
    assert_eq!(read, refusal(&trust));
}

/// A start stored before a start could override the filament check, as the
/// action's request held it.
const STORED_START: &str = concat!(
    r#"{"action":"start_print","file_name":"benchy.gcode","#,
    r#""manifest":{"file_name":"benchy.gcode","material":"PLA","nozzle_diameter_mm":0.4,"#,
    r#""slicer_profile":"0.2mm standard","allowed":{},"metadata":{}},"#,
    r#""reason":"the operator started it","actor":"operator"}"#
);

/// A start stored without the override reads as not overriding, and writes
/// back byte for byte; one that overrides says so, and only then.
#[test]
fn a_start_stored_without_the_override_reads_as_not_overriding() {
    let start: PrintAction = serde_json::from_str(STORED_START).expect("a stored start reads");
    let PrintAction::StartPrint {
        override_filament_check,
        ..
    } = &start
    else {
        panic!("a stored start reads as a start, not {start:?}");
    };
    assert!(!override_filament_check);
    assert_eq!(
        serde_json::to_string(&start).expect("a start writes"),
        STORED_START
    );

    let mut overriding: Value = serde_json::from_str(STORED_START).expect("a stored start reads");
    overriding["override_filament_check"] = json!(true);
    let read: PrintAction = serde_json::from_value(overriding.clone()).expect("an override reads");
    assert!(matches!(
        read,
        PrintAction::StartPrint {
            override_filament_check: true,
            ..
        }
    ));
    assert_eq!(
        serde_json::to_value(&read).expect("an override writes"),
        overriding
    );
    let validator =
        jsonschema::validator_for(&schema_of::<PrintAction>()).expect("the schema compiles");
    assert!(validator.is_valid(&overriding));
}

/// A configuration made the ordinary way checks spools before every start,
/// refusing a short one, with a tenth's buffer of at least five grams, and
/// waits five minutes for a file's analysis.
#[test]
fn a_new_configuration_carries_the_stated_spool_policy() {
    let envelope = printobserver_core::SafetyEnvelope::sample_full();
    let config = CoreConfig::new(envelope, "printobserver context".to_owned());
    assert_eq!(
        config.spools,
        SpoolPolicy {
            preflight: PreflightMode::Refuse,
            buffer_percent: 10.0,
            buffer_min_g: 5.0,
            analysis_wait_s: 300,
        }
    );
    assert_eq!(config.spools, SpoolPolicy::default());
    for (mode, spelling) in [
        (PreflightMode::Refuse, "refuse"),
        (PreflightMode::Warn, "warn"),
        (PreflightMode::Off, "off"),
    ] {
        assert_eq!(
            serde_json::to_value(mode).expect("a mode writes"),
            json!(spelling)
        );
    }
    let partial: SpoolPolicy =
        serde_json::from_value(json!({ "preflight": "warn" })).expect("a partial policy reads");
    assert_eq!(
        partial,
        SpoolPolicy {
            preflight: PreflightMode::Warn,
            ..SpoolPolicy::default()
        }
    );
}

/// The five spool commands are kinds of their own, named the way an envelope
/// grants them, valid from every printer state and changing nothing at the
/// machine; and each command answers the kind, reason and actor it carries.
#[test]
fn the_spool_commands_are_kinds_no_printer_state_gates() {
    let commands: Vec<SpoolCommand> = std::iter::once(SpoolCommand::sample_full())
        .chain(SpoolCommand::sample_alternates())
        .collect();
    let stated = [
        (ActionKind::RegisterSpool, "register_spool"),
        (ActionKind::LoadSpool, "load_spool"),
        (ActionKind::UnloadSpool, "unload_spool"),
        (ActionKind::AdjustSpool, "adjust_spool"),
        (ActionKind::RetireSpool, "retire_spool"),
    ];
    for (kind, spelling) in stated {
        assert_eq!(
            serde_json::to_value(kind).expect("a kind writes"),
            json!(spelling)
        );
        assert!(!changes_the_machine(kind), "{kind:?} changes the machine");
        for state in [
            PrinterState::Operational,
            PrinterState::Printing,
            PrinterState::Paused,
            PrinterState::Cancelling,
            PrinterState::Error,
            PrinterState::Offline,
            PrinterState::Unknown("Starting".to_owned()),
        ] {
            assert!(valid_from(kind, &state), "{kind:?} is gated by {state:?}");
        }
        let command = commands
            .iter()
            .find(|command| command.kind() == kind)
            .unwrap_or_else(|| panic!("no canonical command is a {kind:?}"));
        let written = serde_json::to_value(command).expect("a command writes");
        assert_eq!(written["command"], json!(spelling));
        assert_eq!(written["reason"], json!(command.reason()));
        assert_eq!(
            written["actor"],
            serde_json::to_value(command.actor()).expect("an actor writes")
        );
    }
    assert!(changes_the_machine(ActionKind::Pause));
    assert!(!valid_from(ActionKind::Pause, &PrinterState::Operational));

    let granted: printobserver_core::SafetyEnvelope = serde_json::from_value(json!({
        "allowed": {},
        "actions": { "operator": ["register_spool", "load_spool", "adjust_spool"] },
        "agent_min_interval_s": 60
    }))
    .expect("an envelope granting spool commands reads");
    assert_eq!(
        granted
            .actions
            .get(&printobserver_core::ActorClass::Operator),
        Some(&vec![
            ActionKind::RegisterSpool,
            ActionKind::LoadSpool,
            ActionKind::AdjustSpool
        ])
    );
}

/// An unverified spool with no mark is not a state the ledger has: the type
/// and the generated schema both refuse it, and a spool with one reads.
#[test]
fn an_unverified_spool_with_no_mark_is_refused() {
    let validator =
        jsonschema::validator_for(&schema_of::<SpoolTrust>()).expect("the schema compiles");
    let empty = json!({ "state": "unverified", "marks": [] });
    assert!(!validator.is_valid(&empty), "the schema admitted {empty}");
    let refused = serde_json::from_value::<SpoolTrust>(empty).expect_err("an empty mark list");
    assert!(
        refused.to_string().contains("at least one mark"),
        "{refused}"
    );

    let one = serde_json::to_value(SpoolTrust::Verified.marked(UnverifiedMark {
        reason: UnverifiedReason::sample_full(),
        since: at(0),
    }))
    .expect("a trust writes");
    assert!(validator.is_valid(&one), "the schema refused {one}");
    assert!(serde_json::from_value::<SpoolTrust>(one).is_ok());
}

/// Each of the ledger's four rejections reads back in words naming what a
/// caller acts on, and no two read the same.
#[test]
fn every_ledger_rejection_reads_back_in_words_a_caller_can_act_on() {
    let rejections: Vec<RejectionReason> = RejectionReason::sample_alternates();
    let said: Vec<String> = rejections.iter().map(rejection_detail).collect();
    let insufficient = &said[0];
    for fact in ["Galaxy black", "tool 0", "80g", "120g", "12g"] {
        assert!(insufficient.contains(fact), "{insufficient} omits {fact}");
    }
    assert!(said[2].contains("Galaxy black") && said[2].contains("unverified"));
    assert!(said[3].contains("benchy.gcode") && said[3].contains("300s"));
    assert!(said[4].contains("retired"), "{}", said[4]);
    let mut distinct: Vec<&String> = said.iter().skip(1).collect();
    distinct.sort();
    distinct.dedup();
    assert_eq!(
        distinct.len(),
        said.len() - 1,
        "two ledger rejections read the same"
    );
}
