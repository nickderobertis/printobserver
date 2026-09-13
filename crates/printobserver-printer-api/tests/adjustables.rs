//! An adjustable is the member its string names, both ways.
//!
//! The adjustable is serialized as a string rather than an object, because it
//! is the key type of the manifest's, the envelope's and the effective bounds'
//! maps; so its display, its parse and its refusal are the whole of its wire
//! form, and each is held here to agreeing with the members it declares.

use std::str::FromStr as _;

use printobserver_printer_api::Adjustable;

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
