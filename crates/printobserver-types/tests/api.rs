//! What a consumer reads off these types, beyond their wire forms.
//!
//! The accessors here are how core matches on a value without destructuring it,
//! so each is held to agreeing with the data it reads: an action's kind is the
//! variant it is, an actor's class is the actor it is, and a reported value's
//! flag is what its range says.

use std::str::FromStr as _;

use chrono::{DateTime, TimeZone as _, Utc};
use printobserver_types::contract::Sample;
use printobserver_types::{
    AcknowledgementDisposition, ActionKind, Actor, ActorClass, Adjustable, EventKind, EventPayload,
    FEEDRATE_FACTOR_RANGE, FileName, FileNameRefusal, ObicoTimestamp, PrintAction, PrintId, Range,
    RawBytes, Reported, Timestamp,
};

/// Every action reads back the kind, the reason and the actor it carries.
#[test]
fn every_action_reads_back_what_it_carries() {
    let mut kinds = Vec::new();
    for action in
        std::iter::once(PrintAction::sample_full()).chain(PrintAction::sample_alternates())
    {
        assert!(
            !action.reason().is_empty(),
            "{action:?} carries an empty reason"
        );
        assert_eq!(action.actor().class(), action.actor().class());
        kinds.push(action.kind());
    }
    kinds.sort();
    kinds.dedup();
    assert_eq!(kinds.len(), 10, "the corpus does not carry every kind");
    assert_eq!(
        PrintAction::sample_full().kind(),
        ActionKind::SetFeedrateFactor
    );
}

/// Every actor reports the class a safety envelope grants actions to.
#[test]
fn every_actor_reports_its_class() {
    assert_eq!(
        Actor::Agent {
            session_name: "print-1".to_owned()
        }
        .class(),
        ActorClass::Agent
    );
    assert_eq!(Actor::Operator.class(), ActorClass::Operator);
    assert_eq!(Actor::System.class(), ActorClass::System);
}

/// A minted identifier is a version 7 UUID, displayed in the one spelling.
#[test]
fn a_minted_identifier_is_a_version_seven_uuid() {
    let minted = PrintId::new();
    assert_eq!(minted.as_uuid().get_version_num(), 7);
    let displayed = minted.to_string();
    assert_eq!(displayed.parse::<PrintId>(), Ok(minted));
    assert_eq!(PrintId::default().as_uuid().get_version_num(), 7);
    assert_ne!(
        PrintId::new(),
        PrintId::new(),
        "two mintings are the same identifier"
    );
}

/// A refused identifier says which type refused it and why.
#[test]
fn a_refused_identifier_says_which_type_refused_it() {
    let error = PrintId::from_str("nonsense").expect_err("the spelling is refused");
    assert_eq!(error.type_name(), "PrintId");
    assert!(error.detail().contains("nonsense"), "{}", error.detail());
    assert!(error.to_string().contains("PrintId"));

    for refused in [
        "0191f0a0-0000-4000-8000-000000000001",
        "0191F0A0-0000-7000-8000-000000000001",
        "{0191f0a0-0000-7000-8000-000000000001}",
        "0191f0a0000070008000000000000001",
    ] {
        assert!(
            PrintId::from_str(refused).is_err(),
            "{refused} was accepted"
        );
    }
}

/// An instant is the UTC instant it was built from, however it was built.
#[test]
fn an_instant_is_the_utc_instant_it_was_built_from() {
    let epoch = Timestamp::from_unix_seconds(0).expect("the epoch is representable");
    assert_eq!(epoch.to_string(), "1970-01-01T00:00:00Z");
    assert_eq!(
        *epoch.as_utc(),
        DateTime::<Utc>::from_timestamp(0, 0).expect("the epoch")
    );

    let from_chrono: Timestamp = Utc.with_ymd_and_hms(2026, 3, 1, 12, 0, 0).unwrap().into();
    assert_eq!(from_chrono, Timestamp::sample_full());

    assert!(Timestamp::now() > epoch, "now is not after the epoch");
    assert!(
        Timestamp::from_unix_seconds(i64::MAX).is_err(),
        "an unrepresentable instant parsed"
    );
}

/// A refused instant says what was wrong with it.
#[test]
fn a_refused_instant_says_what_was_wrong() {
    let error = Timestamp::from_str("the first of March").expect_err("the string is refused");
    assert!(
        error.detail().contains("the first of March"),
        "{}",
        error.detail()
    );
    assert!(error.to_string().contains("RFC 3339"));
    assert!(
        Timestamp::from_str("2026-03-01T12:00:00").is_err(),
        "an offsetless instant parsed"
    );
}

/// Every adjustable displays and parses back as itself.
#[test]
fn every_adjustable_round_trips_through_its_string() {
    let members = [
        (Adjustable::Feedrate, "feedrate"),
        (Adjustable::Flowrate, "flowrate"),
        (Adjustable::BedTarget, "bed_target"),
        (Adjustable::Fan, "fan"),
        (Adjustable::ToolTarget { tool: 0 }, "tool_target:0"),
        (Adjustable::ToolTarget { tool: -1 }, "tool_target:-1"),
    ];
    for (member, spelling) in members {
        assert_eq!(member.to_string(), spelling);
        assert_eq!(Adjustable::from_str(spelling), Ok(member));
    }
}

/// A string that names no adjustable is refused, saying why.
#[test]
fn a_string_that_names_no_adjustable_is_refused() {
    let error = Adjustable::from_str("chamber_target").expect_err("no such member");
    assert!(
        error.detail().contains("chamber_target"),
        "{}",
        error.detail()
    );
    assert!(error.to_string().contains("adjustable"));

    let error = Adjustable::from_str("tool_target:left").expect_err("no such tool");
    assert!(error.detail().contains("tool number"), "{}", error.detail());
}

/// Whether two values are the same one bit for bit.
///
/// "Carried through as reported" is an exact claim, so this is an exact
/// comparison rather than a tolerance: a value that came back near enough is a
/// value this type coerced.
fn identical(left: f64, right: f64) -> bool {
    left.to_bits() == right.to_bits()
}

/// A reported value carries what it was given, and its range says the rest.
#[test]
fn a_reported_value_carries_what_it_was_given() {
    let inside = Reported::new(1.0, FEEDRATE_FACTOR_RANGE);
    assert!(
        identical(inside.value(), 1.0),
        "{} is not the value it was given",
        inside.value()
    );
    assert!(!inside.out_of_range());
    assert_eq!(inside.to_string(), "1");

    let outside = Reported::new(40.0, FEEDRATE_FACTOR_RANGE);
    assert!(
        identical(outside.value(), 40.0),
        "{} is not the value it was given",
        outside.value()
    );
    assert!(outside.out_of_range());
    assert_eq!(outside.to_string(), "40 (out of range)");

    let range = Range::new(0.0, 1.0);
    assert!(range.contains(0.0) && range.contains(1.0) && range.contains(0.5));
    assert!(!range.contains(-0.1) && !range.contains(1.1) && !range.contains(f64::NAN));
}

/// Raw bytes are held exactly as received, and displayed as base64.
#[test]
fn raw_bytes_are_held_exactly_as_received() {
    let held = RawBytes::new(b"{\"event\":{}}".to_vec());
    assert_eq!(held.as_slice(), b"{\"event\":{}}");
    assert_eq!(held.to_string(), "eyJldmVudCI6e319");
    assert!(RawBytes::default().as_slice().is_empty());
    assert!(
        serde_json::from_value::<RawBytes>(serde_json::json!("not base64!")).is_err(),
        "a string that is no base64 was read as bytes"
    );
}

/// A file name is the characters it was given, and says why one is refused.
#[test]
fn a_file_name_is_the_characters_it_was_given() {
    let name = FileName::new("benchy.gcode").expect("a name carrying no forbidden property");
    assert_eq!(name.as_str(), "benchy.gcode");
    assert_eq!(name.to_string(), "benchy.gcode");
    assert_eq!(FileName::from_str("benchy.gcode"), Ok(name));

    let error = FileName::new("../etc/passwd").expect_err("a parent-directory segment");
    assert_eq!(error.refusal(), FileNameRefusal::Separator);
    assert!(error.to_string().contains("separator"), "{error}");
    for refusal in FileNameRefusal::ALL {
        assert!(!refusal.detail().is_empty());
        assert_eq!(refusal.to_string(), refusal.detail());
    }
}

/// A payload's kind is the one kind it belongs to.
#[test]
fn a_payload_belongs_to_one_kind() {
    assert_eq!(
        EventPayload::sample_full().kind(),
        EventKind::ObicoFailureAlert
    );
    assert_eq!(EventKind::ALL.len(), 11);
    assert_eq!(
        AcknowledgementDisposition::sample_full(),
        AcknowledgementDisposition::Watch
    );
}

/// The producer's two timestamp forms are both held, and nothing else is.
#[test]
fn the_producers_two_timestamp_forms_are_both_held() {
    assert_eq!(
        serde_json::from_value::<ObicoTimestamp>(serde_json::json!(17)).expect("a number"),
        ObicoTimestamp::Seconds(17.0)
    );
    assert_eq!(
        serde_json::from_value::<ObicoTimestamp>(serde_json::json!("")).expect("an empty string"),
        ObicoTimestamp::NotReported
    );
    assert!(
        serde_json::from_value::<ObicoTimestamp>(serde_json::json!("yesterday")).is_err(),
        "a string that is no timestamp was read as one"
    );
}
