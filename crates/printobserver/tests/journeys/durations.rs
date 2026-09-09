//! Every adjustment is a bounded intervention, at every value of the corpus.
//!
//! # Why a corpus rather than one value
//!
//! One chosen value would prove that one example works and say nothing about a
//! duration parsed in the wrong unit or scheduled from the wrong origin. The
//! corpus is three values per adjustment: the smallest this program accepts, a
//! value a second or two above it, and one far larger than any journey can wait
//! for. Its two properties are the point — the assertion at every value is one
//! equality, the recorded expiry against the caller's own duration, so a single
//! computation is exercised at each; and it contains the boundary this
//! program's own acceptance rule turns on, with the rejected values sitting
//! immediately beneath it.
//!
//! # What the two short values add
//!
//! The recorded expiry alone does not separate restoring at the right time from
//! restoring early and from never restoring. So at the two short values the
//! journey reads back, through this same program, that the adjustment is still
//! in force shortly **before** that instant and gone shortly **after** it —
//! with the record saying the prior value was put back where the machine
//! reported one, and saying there was none where it did not.

use core::time::Duration;
use std::collections::BTreeMap;

use printobserver::failure::Exit;
use printobserver::surface::{MAX_DURATION_SECONDS, MIN_DURATION_SECONDS};
use printobserver_types::Timestamp;
use printobserver_types::serde_json::Value;

use crate::machine::Reports;
use crate::walk::{self, Driven};
use crate::world::World;

use super::failures::succeeding;
use super::running;

/// The durations this program refuses, each sitting outside what it accepts.
const REFUSED: [&str; 4] = ["0", "-1", "quickly", "86401"];

/// How long after an expiry a journey looks for what became of it.
const AFTER: Duration = Duration::from_millis(600);

/// What one accepted adjustment left behind.
struct Bounded {
    /// The command that asked for it.
    command: String,
    /// The intervention it opened.
    id: String,
    /// Whether the machine had reported a value to put back.
    had_a_prior_value: bool,
}

/// Every adjustment, over the whole corpus.
pub fn every_adjustment_is_a_bounded_intervention(world: &World) {
    world.machine.reports(Reports::Printing);
    let adjustments = adjustments(world);
    assert_eq!(
        adjustments.len(),
        5,
        "the vocabulary's adjustments are not the ones this journey drives"
    );

    for seconds in [MIN_DURATION_SECONDS, MIN_DURATION_SECONDS + 2] {
        let opened = each_asks_for(world, &adjustments, seconds);
        std::thread::sleep(Duration::from_secs(u64::try_from(seconds).expect("seconds")) + AFTER);
        the_prior_value_is_back(world, &opened);
    }
    let _ = each_asks_for(world, &adjustments, MAX_DURATION_SECONDS);

    for one in &adjustments {
        every_refused_duration_records_nothing(world, one);
    }
}

/// The commands of the walk that take a duration.
fn adjustments(world: &World) -> Vec<Driven> {
    walk::walk(world)
        .into_iter()
        .filter(|one| {
            one.command
                .fields
                .iter()
                .any(|field| field.parameter.name == "duration_s")
        })
        .collect()
}

/// Every adjustment asks for one duration, and is still in force before it.
fn each_asks_for(world: &World, adjustments: &[Driven], seconds: i64) -> Vec<Bounded> {
    let mut opened = Vec::new();
    for one in adjustments {
        let answer = ask_for(world, one, &seconds.to_string());
        let bounded = the_expiry_is_the_duration_the_caller_gave(one, &answer, seconds);
        it_is_in_force(world, &bounded, &answer);
        opened.push(bounded);
    }
    opened
}

/// One adjustment, asked for with one duration.
fn ask_for(world: &World, one: &Driven, seconds: &str) -> Value {
    let mut given = succeeding(one);
    let at = given
        .iter()
        .position(|word| word == "--duration-s")
        .expect("an adjustment takes a duration");
    seconds.clone_into(&mut given[at + 1]);
    let asked: Vec<&str> = given.iter().map(String::as_str).collect();
    running::read(world, &asked)
}

/// The recorded expiry is the instant the request was made plus the duration.
fn the_expiry_is_the_duration_the_caller_gave(
    one: &Driven,
    answer: &Value,
    seconds: i64,
) -> Bounded {
    let requested = instant(answer, "/record/request/requested_at");
    let expires = instant(answer, "/intervention/expires_at");
    assert_eq!(
        expires.as_utc().timestamp_micros() - requested.as_utc().timestamp_micros(),
        seconds * 1_000_000,
        "`{}` asked for {seconds} seconds and the record expires somewhere else",
        one.command.name
    );
    Bounded {
        command: one.command.name.clone(),
        id: answer
            .pointer("/intervention/id")
            .and_then(Value::as_str)
            .expect("an intervention has an identifier")
            .to_owned(),
        had_a_prior_value: answer.pointer("/intervention/prior_value").is_some(),
    }
}

/// One instant an answer carries.
fn instant(answer: &Value, at: &str) -> Timestamp {
    answer
        .pointer(at)
        .and_then(Value::as_str)
        .unwrap_or_else(|| panic!("the answer carries no `{at}`: {answer}"))
        .parse()
        .expect("an instant this system wrote")
}

/// The adjustment is still in force shortly before it expires.
fn it_is_in_force(world: &World, bounded: &Bounded, answer: &Value) {
    let applied = answer
        .pointer("/intervention/applied_value")
        .expect("an intervention carries what it changed to")
        .clone();
    let read = running::read(world, &["status", "--print-id", &world.print_id]);
    let held = in_force(&read);
    assert_eq!(
        held.get(&bounded.id),
        Some(&applied),
        "`{}` is not in force shortly before it expires: {read}",
        bounded.command
    );
}

/// The prior value is back shortly after each one expired.
fn the_prior_value_is_back(world: &World, opened: &[Bounded]) {
    let read = running::read(world, &["status", "--print-id", &world.print_id]);
    let held = in_force(&read);
    let history = running::read(
        world,
        &["history", "--print-id", &world.print_id, "--limit", "40"],
    );
    for bounded in opened {
        assert!(
            !held.contains_key(&bounded.id),
            "`{}` is still in force after it expired: {read}",
            bounded.command
        );
        let expected = if bounded.had_a_prior_value {
            "restored"
        } else {
            "restore_unavailable"
        };
        assert_eq!(
            expiry_outcome(&history, &bounded.id).as_deref(),
            Some(expected),
            "`{}` expired and the record does not say the prior value was put back",
            bounded.command
        );
    }
}

/// Every intervention in force, by its identifier and the value it applied.
fn in_force(read: &Value) -> BTreeMap<String, Value> {
    read.get("interventions")
        .and_then(Value::as_array)
        .map(|held| {
            held.iter()
                .filter_map(|one| {
                    Some((
                        one.get("id")?.as_str()?.to_owned(),
                        one.get("applied_value")?.clone(),
                    ))
                })
                .collect()
        })
        .unwrap_or_default()
}

/// What the history says became of one intervention.
fn expiry_outcome(history: &Value, intervention: &str) -> Option<String> {
    history
        .get("events")?
        .as_array()?
        .iter()
        .filter(|event| event.get("kind").and_then(Value::as_str) == Some("intervention_expired"))
        .find(|event| {
            event
                .pointer("/payload/intervention_id")
                .and_then(Value::as_str)
                == Some(intervention)
        })
        .and_then(|event| match event.pointer("/payload/outcome")? {
            Value::String(named) => Some(named.clone()),
            Value::Object(named) => named.keys().next().cloned(),
            _ => None,
        })
}

/// Every duration this program refuses is refused with nothing recorded.
fn every_refused_duration_records_nothing(world: &World, one: &Driven) {
    let before = in_force(&running::read(
        world,
        &["status", "--print-id", &world.print_id],
    ))
    .len();
    for offered in REFUSED {
        let mut given = succeeding(one);
        let at = given
            .iter()
            .position(|word| word == "--duration-s")
            .expect("an adjustment takes a duration");
        offered.clone_into(&mut given[at + 1]);
        let asked: Vec<&str> = given.iter().map(String::as_str).collect();

        world.proxy.forget();
        let ran = running::command(world, &asked);

        assert_eq!(
            ran.code,
            Some(i32::from(Exit::Usage.status())),
            "`{}` accepted a duration of `{offered}`: {}",
            one.command.name,
            ran.said()
        );
        assert_eq!(
            world.proxy.received(),
            Vec::new(),
            "`{}` with a duration of `{offered}` reached the server",
            one.command.name
        );
    }
    let after = in_force(&running::read(
        world,
        &["status", "--print-id", &world.print_id],
    ))
    .len();
    assert_eq!(
        before, after,
        "`{}` recorded an intervention for a duration this program refuses",
        one.command.name
    );
}
