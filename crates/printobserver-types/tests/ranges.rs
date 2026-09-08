//! The seven plausibility ranges, and their separation from the safety envelope.
//!
//! Each of the seven fields this crate types as `Reported<f64>` declares
//! exactly the range the contract states for it, and its `out_of_range` flag is
//! true exactly when the value is non-finite or lies outside that range — in
//! both directions, on parse as well as on emission.
//!
//! The corpus ranges over each range's own **boundary** rather than over its
//! interior alone, because a below-inside-above corpus never asks what happens
//! at the two values the word *inclusive* is about. It carries `NaN` for the
//! same reason from the other side: an implementation computing the flag as
//! `value < min || value > max` answers every other value here correctly and
//! fails on `NaN` alone.

use std::collections::BTreeMap;

use printobserver_types::contract::{RANGED_FIELDS, TypeContract, declared, schema_of};
use printobserver_types::{
    ActionKind, ActorClass, Adjustable, COMPLETION_RANGE, EffectiveBounds, FAN_PERCENT_RANGE,
    FEEDRATE_FACTOR_RANGE, FLOWRATE_FACTOR_RANGE, HEATER_ACTUAL_C_RANGE, HEATER_OFFSET_C_RANGE,
    HEATER_TARGET_C_RANGE, HeaterSnapshot, JobSnapshot, PrinterSnapshot, Range, Reported,
    SafetyEnvelope,
};
use serde_json::{Value, json};

/// The contract for one declared type, by name.
fn contract(type_name: &str) -> TypeContract {
    declared()
        .into_iter()
        .find(|entry| entry.name == type_name)
        .unwrap_or_else(|| panic!("{type_name} is declared"))
}

/// Each of the seven fields declares exactly the range the contract states.
#[test]
fn the_seven_fields_declare_exactly_the_stated_ranges() {
    let stated: [(&str, &str, f64, f64); 7] = [
        ("PrinterSnapshot", "feedrate_factor", 0.1, 10.0),
        ("PrinterSnapshot", "flowrate_factor", 0.1, 10.0),
        ("PrinterSnapshot", "fan_percent", 0.0, 100.0),
        ("JobSnapshot", "completion", 0.0, 1.0),
        ("HeaterSnapshot", "actual_c", -20.0, 500.0),
        ("HeaterSnapshot", "target_c", 0.0, 500.0),
        ("HeaterSnapshot", "offset_c", -50.0, 50.0),
    ];
    let declared: Vec<(&str, &str, f64, f64)> = RANGED_FIELDS
        .iter()
        .map(|field| {
            (
                field.type_name,
                field.field,
                field.range.min,
                field.range.max,
            )
        })
        .collect();
    assert_eq!(declared, stated.to_vec());
}

/// The constants the seven fields are declared against are the same seven.
#[test]
fn the_declared_constants_are_the_ranges_the_fields_carry() {
    assert_eq!(FEEDRATE_FACTOR_RANGE, Range::new(0.1, 10.0));
    assert_eq!(FLOWRATE_FACTOR_RANGE, Range::new(0.1, 10.0));
    assert_eq!(FAN_PERCENT_RANGE, Range::new(0.0, 100.0));
    assert_eq!(COMPLETION_RANGE, Range::new(0.0, 1.0));
    assert_eq!(HEATER_ACTUAL_C_RANGE, Range::new(-20.0, 500.0));
    assert_eq!(HEATER_TARGET_C_RANGE, Range::new(0.0, 500.0));
    assert_eq!(HEATER_OFFSET_C_RANGE, Range::new(-50.0, 50.0));
}

/// The boundary corpus for one range: the value, and the flag it must carry.
fn boundary_corpus(range: Range) -> Vec<(f64, bool)> {
    vec![
        (range.min, false),
        (range.max, false),
        (range.min.next_down(), true),
        (range.max.next_up(), true),
        (range.min - 1_000.0, true),
        (f64::midpoint(range.min, range.max), false),
        (range.max + 1_000.0, true),
    ]
}

/// A value of the owning type carrying one reported value in one field.
fn owning_value(entry: &TypeContract, field: &str, reported: Value) -> Value {
    let mut value = entry.minimal();
    value[field] = reported;
    value
}

/// Every one of the seven flags its own range, at the boundary and beyond it.
#[test]
fn every_ranged_field_flags_its_own_range_at_the_boundary() {
    for ranged in RANGED_FIELDS {
        let entry = contract(ranged.type_name);
        for (value, out_of_range) in boundary_corpus(ranged.range) {
            let reported = json!({ "value": value, "out_of_range": out_of_range });
            let carried = owning_value(&entry, ranged.field, reported.clone());
            let round = entry.round_trip(carried.clone()).unwrap_or_else(|error| {
                panic!(
                    "{}.{} refused {value}: {error}",
                    ranged.type_name, ranged.field
                )
            });
            assert_eq!(
                round, carried,
                "{}.{} did not carry {value} through as reported",
                ranged.type_name, ranged.field
            );

            let disagreeing = json!({ "value": value, "out_of_range": !out_of_range });
            let carried = owning_value(&entry, ranged.field, disagreeing);
            assert!(
                entry.round_trip(carried).is_err(),
                "{}.{} accepted {value} flagged {}",
                ranged.type_name,
                ranged.field,
                !out_of_range
            );
        }
    }
}

/// A non-finite value is out of range, whichever of the three it is.
#[test]
fn every_non_finite_value_is_out_of_range() {
    for ranged in RANGED_FIELDS {
        for value in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
            let reported = Reported::new(value, ranged.range);
            assert!(
                reported.out_of_range(),
                "{}.{} reads {value} as inside {:?}",
                ranged.type_name,
                ranged.field,
                ranged.range
            );
        }
    }
}

/// A non-finite value is refused on emission rather than emitted as `null`.
#[test]
fn a_non_finite_value_is_refused_on_emission() {
    for value in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
        let reported = Reported::new(value, FEEDRATE_FACTOR_RANGE);
        let answer = serde_json::to_value(reported);
        assert!(
            answer.is_err(),
            "{value} emitted as {answer:?} rather than being refused"
        );

        let snapshot = PrinterSnapshot {
            feedrate_factor: Some(Reported::new(value, FEEDRATE_FACTOR_RANGE)),
            ..printobserver_types::contract::Sample::sample_minimal()
        };
        assert!(
            serde_json::to_value(snapshot).is_err(),
            "a snapshot carrying {value} emitted rather than being refused"
        );
    }
}

/// The JSON spellings a producer might reach for a non-finite value.
const NON_FINITE_SPELLINGS: [&str; 5] = ["NaN", "Infinity", "-Infinity", "\"NaN\"", "\"Infinity\""];

/// Parse one owning type from text, so that a raw JSON token can be driven.
fn parse_owning(type_name: &str, text: &str) -> Result<(), String> {
    let answer = match type_name {
        "PrinterSnapshot" => serde_json::from_str::<PrinterSnapshot>(text).map(|_| ()),
        "JobSnapshot" => serde_json::from_str::<JobSnapshot>(text).map(|_| ()),
        "HeaterSnapshot" => serde_json::from_str::<HeaterSnapshot>(text).map(|_| ()),
        other => panic!("{other} owns no ranged field"),
    };
    answer.map_err(|error| error.to_string())
}

/// One owning value's text, with one reported field spelled out verbatim.
fn text_with_reported(entry: &TypeContract, field: &str, reported: &str) -> String {
    let mut value = entry.minimal();
    value[field] = json!(null);
    serde_json::to_string(&value)
        .expect("the minimal value serializes")
        .replace(
            &format!("\"{field}\":null"),
            &format!("\"{field}\":{reported}"),
        )
}

/// The wire is closed to a non-finite value, in every spelling.
///
/// The finite control is what makes the refusals mean what they say: a value
/// far outside the range, flagged as out of range, parses, so a refusal below
/// is a refusal of the spelling rather than of the flag it carries.
#[test]
fn the_wire_refuses_every_non_finite_spelling() {
    for ranged in RANGED_FIELDS {
        let entry = contract(ranged.type_name);
        let control = format!(
            "{{\"value\":{},\"out_of_range\":true}}",
            ranged.range.max + 1_000.0
        );
        let text = text_with_reported(&entry, ranged.field, &control);
        assert!(
            parse_owning(ranged.type_name, &text).is_ok(),
            "{}.{} refused the finite control {control}",
            ranged.type_name,
            ranged.field
        );

        for spelling in NON_FINITE_SPELLINGS {
            let reported = format!("{{\"value\":{spelling},\"out_of_range\":true}}");
            let text = text_with_reported(&entry, ranged.field, &reported);
            assert!(
                parse_owning(ranged.type_name, &text).is_err(),
                "{}.{} accepted {spelling}",
                ranged.type_name,
                ranged.field
            );
        }
    }
}

/// The `Adjustable` member each of the seven corresponds to, if one exists.
const CORRESPONDING_MEMBERS: [(&str, &str, Option<&str>); 8] = [
    ("PrinterSnapshot", "feedrate_factor", Some("feedrate")),
    ("PrinterSnapshot", "flowrate_factor", Some("flowrate")),
    ("PrinterSnapshot", "fan_percent", Some("fan")),
    (
        "HeaterSnapshot",
        "target_c (a tool's)",
        Some("tool_target:0"),
    ),
    ("HeaterSnapshot", "target_c (the bed's)", Some("bed_target")),
    ("JobSnapshot", "completion", None),
    ("HeaterSnapshot", "actual_c", None),
    ("HeaterSnapshot", "offset_c", None),
];

/// The member spellings a field with no corresponding member would take.
const MEMBERS_THAT_DO_NOT_EXIST: [&str; 6] = [
    "completion",
    "chamber_target",
    "tool_actual",
    "bed_actual",
    "tool_offset",
    "offset",
];

/// The members the committed `Adjustable` vocabulary declares.
fn declared_members(pattern: &str) -> Vec<String> {
    pattern
        .trim_start_matches("^(")
        .trim_end_matches(")$")
        .split('|')
        .map(str::to_owned)
        .collect()
}

/// The pattern the committed `Adjustable` vocabulary declares.
fn adjustable_pattern() -> String {
    schema_of::<Adjustable>()
        .get("pattern")
        .and_then(Value::as_str)
        .expect("the vocabulary declares its members")
        .to_owned()
}

/// An envelope granting one member one range, and every other a distant one.
fn envelope(member: Adjustable, range: Range) -> SafetyEnvelope {
    let mut allowed = BTreeMap::from([
        (Adjustable::Feedrate, Range::new(-1_000.0, -999.0)),
        (Adjustable::Flowrate, Range::new(-1_000.0, -999.0)),
        (Adjustable::Fan, Range::new(-1_000.0, -999.0)),
        (Adjustable::BedTarget, Range::new(-1_000.0, -999.0)),
        (
            Adjustable::ToolTarget { tool: 0 },
            Range::new(-1_000.0, -999.0),
        ),
    ]);
    allowed.insert(member, range);
    SafetyEnvelope {
        allowed,
        actions: BTreeMap::from([(ActorClass::Agent, vec![ActionKind::Pause])]),
        agent_min_interval_s: 60,
    }
}

/// Four of the seven have a corresponding member, and it stays a separate
/// contract: an envelope inside the plausibility range and one outside it are
/// both representable and both survive a round trip exactly as given.
#[test]
fn an_envelope_inside_or_outside_a_plausibility_range_is_carried_exactly() {
    let inside_and_outside: [(Adjustable, Range, Range); 5] = [
        (
            Adjustable::Feedrate,
            Range::new(0.5, 1.5),
            Range::new(20.0, 40.0),
        ),
        (
            Adjustable::Flowrate,
            Range::new(0.9, 1.1),
            Range::new(-5.0, -1.0),
        ),
        (
            Adjustable::Fan,
            Range::new(20.0, 80.0),
            Range::new(200.0, 400.0),
        ),
        (
            Adjustable::ToolTarget { tool: 0 },
            Range::new(190.0, 230.0),
            Range::new(900.0, 1_200.0),
        ),
        (
            Adjustable::BedTarget,
            Range::new(40.0, 70.0),
            Range::new(900.0, 1_200.0),
        ),
    ];
    for (member, inside, outside) in inside_and_outside {
        for range in [inside, outside] {
            let held = envelope(member, range);
            let round: SafetyEnvelope =
                serde_json::from_value(serde_json::to_value(&held).expect("an envelope emits"))
                    .expect("an envelope parses");
            assert_eq!(
                round.allowed.get(&member),
                Some(&range),
                "{member} was not carried exactly"
            );

            let bounds = EffectiveBounds {
                allowed: BTreeMap::from([(member, range)]),
            };
            let round: EffectiveBounds =
                serde_json::from_value(serde_json::to_value(&bounds).expect("bounds emit"))
                    .expect("bounds parse");
            assert_eq!(
                round.allowed.get(&member),
                Some(&range),
                "{member} was not carried exactly"
            );
        }
    }
}

/// The other three, and the chamber's target, have no member to keep apart from.
#[test]
fn the_fields_with_no_corresponding_member_have_none_to_be_confused_with() {
    let members = declared_members(&adjustable_pattern());
    assert_eq!(
        members,
        vec![
            "feedrate",
            "flowrate",
            "bed_target",
            "fan",
            "tool_target:-?[0-9]+"
        ],
        "the vocabulary is not the five members the contract states"
    );
    for absent in MEMBERS_THAT_DO_NOT_EXIST {
        assert!(
            !members.iter().any(|member| member == absent),
            "the vocabulary declares {absent}, and a field with no member now has one"
        );
    }
    let with_no_member: Vec<&str> = CORRESPONDING_MEMBERS
        .iter()
        .filter(|(_, _, member)| member.is_none())
        .map(|(_, field, _)| *field)
        .collect();
    assert_eq!(with_no_member, vec!["completion", "actual_c", "offset_c"]);
}

/// A fixture vocabulary that gains a member for one of those fields.
const FIXTURE_VOCABULARY: &str = "^(feedrate|flowrate|bed_target|chamber_target|fan)$";

/// The reading refuses a vocabulary that gains a member it should not have.
#[test]
fn the_vocabulary_reading_refuses_a_fixture_that_gains_a_member() {
    let members = declared_members(FIXTURE_VOCABULARY);
    let gained: Vec<&&str> = MEMBERS_THAT_DO_NOT_EXIST
        .iter()
        .filter(|absent| members.iter().any(|member| member == *absent))
        .collect();
    assert_eq!(
        gained,
        vec![&"chamber_target"],
        "a gained member was not seen"
    );
}

/// An envelope granting every member the same range.
fn envelope_of(range: Range) -> SafetyEnvelope {
    let mut held = envelope(Adjustable::Feedrate, range);
    for member in [
        Adjustable::Flowrate,
        Adjustable::Fan,
        Adjustable::BedTarget,
        Adjustable::ToolTarget { tool: 0 },
    ] {
        held.allowed.insert(member, range);
    }
    held
}

/// The flag a value reports is the same under two envelopes that differ in
/// every member, because it is computed from the plausibility range alone.
///
/// The two values are chosen so that an implementation reading the flag off an
/// envelope would answer differently: one lies inside every plausibility range
/// and outside every range the narrow envelope allows, and the other lies
/// outside its plausibility range and inside every range the wide one allows.
#[test]
fn the_flag_is_the_same_under_two_envelopes_that_differ_everywhere() {
    let narrow = envelope_of(Range::new(-1_000.0, -999.0));
    let wide = envelope_of(Range::new(-1e9, 1e9));
    assert_ne!(
        narrow.allowed, wide.allowed,
        "the two envelopes do not differ"
    );

    for ranged in RANGED_FIELDS {
        let plausible = f64::midpoint(ranged.range.min, ranged.range.max);
        assert!(
            narrow
                .allowed
                .values()
                .all(|range| !range.contains(plausible)),
            "the narrow envelope admits {plausible}, so this proves nothing"
        );
        assert!(wide.allowed.values().all(|range| range.contains(plausible)));
        assert!(
            !Reported::new(plausible, ranged.range).out_of_range(),
            "{}.{} reads {plausible} as implausible under an envelope that excludes it",
            ranged.type_name,
            ranged.field
        );

        let implausible = ranged.range.max + 1_000.0;
        assert!(
            wide.allowed
                .values()
                .all(|range| range.contains(implausible)),
            "the wide envelope excludes {implausible}, so this proves nothing"
        );
        assert!(
            Reported::new(implausible, ranged.range).out_of_range(),
            "{}.{} reads {implausible} as plausible under an envelope that admits it",
            ranged.type_name,
            ranged.field
        );
    }
}
