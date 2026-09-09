//! Reading a real printer and a real job, field by field.
//!
//! The walk is over the fields the two snapshot contracts declare, and for each
//! it asks the same question: does this field carry what the instance reported,
//! and is it absent where the instance reported nothing? What the instance
//! reported is read from `OctoPrint`'s own JSON, by a reader that shares no
//! parsing with the adapter — a reader that did would agree with the adapter
//! about a mapping they were both wrong about.
//!
//! Two readings bracket every one the adapter takes, because a running printer
//! moves between them: a field is accepted when it lies inside what the instance
//! itself reported either side of the call, widened by however far those two
//! readings were apart. A stable field brackets exactly.

use std::time::Duration;

use printobserver_printer_api::PrinterPort as _;
use printobserver_types::{PrinterState, Timestamp};

use crate::block_on::block_on;
use crate::env::Scripted;
use crate::numbers::exactly;
use crate::raw;
use crate::wait;

/// How long the completion pin will wait for a running print to advance.
const ADVANCE_LIMIT: Duration = Duration::from_secs(120);

/// The path the printer is read from.
const PRINTER_PATH: &str = "/api/printer?history=false";

/// The path the job is read from.
const JOB_PATH: &str = "/api/job";

/// Every field of the two snapshot contracts, against a real instance.
///
/// # Panics
///
/// Panics when a field does not carry what the instance reported.
pub fn walk(instance: &Scripted) {
    walk_the_printer(instance);
    walk_the_job(instance);
}

/// Every field `PrinterSnapshot` declares.
fn walk_the_printer(instance: &Scripted) {
    let printer = instance.printer();
    let before = raw::get(instance, PRINTER_PATH);
    let taken_from = Timestamp::now();
    let snapshot = block_on(printer.snapshot()).expect("a printer snapshot");
    let taken_to = Timestamp::now();
    let after = raw::get(instance, PRINTER_PATH);

    // `connection` — reported, as flags.
    assert_eq!(
        before["state"]["flags"]["printing"], true,
        "this walk needs the hold print running; run `just octoprint-up` first"
    );
    assert_eq!(snapshot.connection, PrinterState::Printing);

    // `tools`, and every field of `HeaterSnapshot` — reported.
    assert!(!snapshot.tools.is_empty(), "the instance reports a tool");
    let tool = &snapshot.tools[0];
    bracketed(
        "tool0 actual_c",
        tool.actual_c.expect("tool0's temperature").value(),
        &before["temperature"]["tool0"]["actual"],
        &after["temperature"]["tool0"]["actual"],
    );
    bracketed(
        "tool0 target_c",
        tool.target_c.expect("tool0's target").value(),
        &before["temperature"]["tool0"]["target"],
        &after["temperature"]["tool0"]["target"],
    );
    bracketed(
        "tool0 offset_c",
        tool.offset_c.expect("tool0's offset").value(),
        &before["temperature"]["tool0"]["offset"],
        &after["temperature"]["tool0"]["offset"],
    );

    // `bed` — reported.
    let bed = snapshot.bed.as_ref().expect("the instance reports a bed");
    bracketed(
        "bed actual_c",
        bed.actual_c.expect("the bed's temperature").value(),
        &before["temperature"]["bed"]["actual"],
        &after["temperature"]["bed"]["actual"],
    );

    // `chamber` — not reported, so absent.
    assert!(
        before["temperature"].get("chamber").is_none(),
        "this instance reports a chamber after all: {}",
        before["temperature"]
    );
    assert_eq!(snapshot.chamber, None);

    // `feedrate_factor`, `flowrate_factor`, `fan_percent` — not reported by any
    // endpoint this port reads, so absent.
    for absent in ["feedrate", "flowrate", "fan"] {
        assert!(
            !before.to_string().contains(absent),
            "GET {PRINTER_PATH} reports {absent} after all: {before}"
        );
    }
    assert_eq!(snapshot.feedrate_factor, None);
    assert_eq!(snapshot.flowrate_factor, None);
    assert_eq!(snapshot.fan_percent, None);

    // `observed_at` — this adapter's own, and inside the call it was taken in.
    assert!(
        snapshot.observed_at >= taken_from && snapshot.observed_at <= taken_to,
        "{:?} is not inside the call it was taken in",
        snapshot.observed_at
    );
}

/// Every field `JobSnapshot` declares.
fn walk_the_job(instance: &Scripted) {
    let printer = instance.printer();
    let before = raw::get(instance, JOB_PATH);
    let job = block_on(printer.job()).expect("a job snapshot");
    let after = raw::get(instance, JOB_PATH);

    // `file_name`, `file_origin`, `size_bytes` — reported, and stable.
    assert_eq!(
        job.file_name.as_deref(),
        before["job"]["file"]["name"].as_str(),
        "the file name the instance reported"
    );
    assert!(job.file_name.is_some(), "the instance reports a file");
    assert_eq!(
        job.file_origin.as_deref(),
        before["job"]["file"]["origin"].as_str()
    );
    assert_eq!(job.size_bytes, before["job"]["file"]["size"].as_i64());

    // `estimated_print_time_s` — the instance's own estimate, rounded, or absent
    // when the instance has none to give.
    match before["job"]["estimatedPrintTime"].as_f64() {
        Some(estimate) => {
            let carried = as_real(job.estimated_print_time_s.expect("the estimate"));
            assert!(
                (carried - estimate).abs() <= 0.5,
                "the instance estimated {estimate} seconds and this port carried {carried}"
            );
        }
        None => assert_eq!(job.estimated_print_time_s, None),
    }

    // `completion` — reported, as a percentage, carried as a fraction.
    reported_or_absent(
        "completion",
        job.completion.map(|reported| reported.value() * 100.0),
        &before["progress"]["completion"],
        &after["progress"]["completion"],
    );

    // `print_time_s` and `print_time_left_s` — in seconds, where the instance
    // has them: it reports no time left until it has an estimate to take one
    // from, and an absent field is what "did not report" means.
    reported_or_absent(
        "print_time_s",
        job.print_time_s.map(as_real),
        &before["progress"]["printTime"],
        &after["progress"]["printTime"],
    );
    reported_or_absent(
        "print_time_left_s",
        job.print_time_left_s.map(as_real),
        &before["progress"]["printTimeLeft"],
        &after["progress"]["printTimeLeft"],
    );

    // `state` — reported, in the instance's own word.
    assert_eq!(before["state"].as_str(), Some("Printing"));
    assert_eq!(job.state, PrinterState::Printing);

    // `error` — not reported, so absent.
    assert!(
        before.get("error").is_none(),
        "this job reports an error after all: {before}"
    );
    assert_eq!(job.error, None);
}

/// The completion conversion, pinned against `OctoPrint`'s own figure.
///
/// Pinned at two distinct points of a running print, each read while the
/// instance's own figure is unchanged either side of the call, so a mapping that
/// answered a constant inside the declared range fails here rather than
/// satisfying a range.
///
/// # Panics
///
/// Panics when the fraction is not what the instance's own percentage converts
/// to, or when the print never advances.
pub fn pin_completion(instance: &Scripted) {
    let first = stable_point(instance, None);
    let second = stable_point(instance, Some(first.0));

    for (reported, answered) in [first, second] {
        exactly(
            &format!("the fraction this port answered for {reported} percent complete"),
            answered,
            reported / 100.0,
        );
    }
    assert!(
        (first.0 - second.0).abs() > f64::EPSILON,
        "the two points are the same reading ({}), so nothing was pinned twice",
        first.0
    );
}

/// One point at which the instance's own figure did not move across the call.
///
/// When `differing_from` is given, waits for the print to advance past it first.
fn stable_point(instance: &Scripted, differing_from: Option<f64>) -> (f64, f64) {
    let printer = instance.printer();
    if let Some(previous) = differing_from {
        wait::until("the running print to advance", ADVANCE_LIMIT, || {
            let now = completion_of(instance);
            if (now - previous).abs() > f64::EPSILON {
                Ok(())
            } else {
                Err(format!("{now} percent, unchanged"))
            }
        });
    }
    wait::until(
        "a completion the instance did not move across the call",
        ADVANCE_LIMIT,
        || {
            let before = completion_of(instance);
            let answered = block_on(printer.job())
                .expect("a job snapshot")
                .completion
                .expect("the completion")
                .value();
            let after = completion_of(instance);
            if (before - after).abs() > f64::EPSILON {
                return Err(format!("{before} percent moving to {after} percent"));
            }
            Ok((before, answered))
        },
    )
}

/// `OctoPrint`'s own completion figure, in `OctoPrint`'s own units.
///
/// Waited for rather than demanded: a print that has just been selected reports
/// none for as long as it takes the instance to have one.
fn completion_of(instance: &Scripted) -> f64 {
    wait::until("the instance to report a completion", ADVANCE_LIMIT, || {
        raw::get(instance, JOB_PATH)["progress"]["completion"]
            .as_f64()
            .ok_or_else(|| "no completion reported".to_owned())
    })
}

/// A field the instance reported is carried, and one it did not is absent.
fn reported_or_absent(
    field: &str,
    answered: Option<f64>,
    before: &serde_json::Value,
    after: &serde_json::Value,
) {
    if before.as_f64().is_none() && after.as_f64().is_none() {
        assert_eq!(
            answered, None,
            "{field} is carried, and the instance reported none either side of the call"
        );
        return;
    }
    let answered =
        answered.unwrap_or_else(|| panic!("{field} is absent, and the instance reported one"));
    bracketed(field, answered, before, after);
}

/// A count of seconds as a real number.
///
/// Through its own decimal spelling rather than a cast: an `i64` spells exactly
/// one real number, and reading it back is exact for every count of seconds a
/// print can carry.
fn as_real(value: i64) -> f64 {
    value
        .to_string()
        .parse()
        .expect("an i64 spells a real number")
}

/// One reading lies inside what the instance itself reported either side of it.
///
/// The bracket is widened by however far the two readings were apart, so a field
/// the instance holds still is compared exactly and one that is moving is
/// compared against the movement the instance itself showed.
fn bracketed(field: &str, answered: f64, before: &serde_json::Value, after: &serde_json::Value) {
    let before = before
        .as_f64()
        .unwrap_or_else(|| panic!("the instance reported no {field} before the call"));
    let after = after
        .as_f64()
        .unwrap_or_else(|| panic!("the instance reported no {field} after the call"));
    let spread = (after - before).abs();
    let low = before.min(after) - spread;
    let high = before.max(after) + spread;
    assert!(
        answered >= low && answered <= high,
        "{field} answered {answered}, which is outside {low}..={high} — the instance \
         reported {before} before the call and {after} after it"
    );
}
