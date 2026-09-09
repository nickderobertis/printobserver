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
//! # What the two short values add, and when they are read
//!
//! The recorded expiry alone does not separate restoring at the right time from
//! restoring early and from never restoring. So at the two short values the
//! journey reads twice, and **both reads are scheduled against the expiry the
//! record itself carries** rather than against the moment the request was made:
//! once at that instant less [`MARGIN`] and once at it plus [`MARGIN`]. Each
//! read records the instant it was taken at, and every intervention is asserted
//! to have been read before its own expiry and after its own expiry — so a read
//! that drifted is a failure rather than a silent weakening.
//!
//! # What is read, and what the machine can be read for
//!
//! Where the machine reports a value for what was adjusted, that **value** is
//! what is asserted: the adjusted value shortly before the expiry and the prior
//! value shortly after it. That is the whole claim, and it is what a record
//! saying `restored` over a machine that never moved fails —
//! `journeys/tainting.rs` drives exactly that.
//!
//! Three of the five adjustables have no such value, and that is a fact about
//! the machine rather than a gap here: `OctoPrint` reports no applied feedrate
//! factor, no applied flowrate factor and no fan setting, and there is no
//! endpoint of it that does — the adapter says so in its own words. For those
//! three the journey makes the positive claim instead of accepting a weaker
//! one: the machine is asserted to report **no** value for that adjustable, and
//! the expiry is asserted to say exactly that there was nothing to put back.

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

/// How far either side of a recorded expiry a journey reads.
///
/// Short enough that "shortly before" is inside the shortest duration this
/// program accepts, and long enough that a read started there finishes on the
/// right side of the instant it is about.
pub const MARGIN: Duration = Duration::from_millis(400);

/// What one accepted adjustment left behind.
pub struct Bounded {
    /// The command that asked for it.
    pub command: String,
    /// The intervention it opened.
    pub id: String,
    /// What it changed.
    pub adjustable: String,
    /// What it changed that to.
    pub applied_value: Value,
    /// What the machine reported before it, when the machine reports one.
    pub prior_value: Option<Value>,
    /// When it stops standing.
    pub expires_at: Timestamp,
}

impl Bounded {
    /// Where a status read carries the machine's own value for what this
    /// changed, for an adjustable the machine reports at all.
    fn reported_at(&self) -> Option<String> {
        match self.adjustable.as_str() {
            "bed_target" => Some("/printer/bed/target_c/value".to_owned()),
            held if held.starts_with("tool_target:") => {
                let tool = held.split(':').nth(1)?;
                Some(format!("/printer/tools/{tool}/target_c/value"))
            }
            _ => None,
        }
    }
}

/// Every adjustment, over the whole corpus.
pub fn every_adjustment_is_a_bounded_intervention(world: &World) {
    world.wants(Reports::Printing);
    let adjustments = adjustments(world);
    assert_eq!(
        adjustments.len(),
        5,
        "the vocabulary's adjustments are not the ones this journey drives"
    );

    for seconds in [MIN_DURATION_SECONDS, MIN_DURATION_SECONDS + 2] {
        let opened = each_asks_for(world, &adjustments, seconds);
        the_adjusted_value_is_in_place_shortly_before_it_expires(world, &opened);
        the_prior_value_is_back_shortly_after_it_expires(world, &opened);
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

/// Every adjustment asks for one duration, and answers where it expires.
///
/// Each is started from a value the adjustment is **not** about, where the
/// machine reports one at all: an adjustment made from the value it asks for
/// restores to that same value, and a machine that ignored both the change and
/// the putting back would be indistinguishable from one that did neither.
pub fn each_asks_for(world: &World, adjustments: &[Driven], seconds: i64) -> Vec<Bounded> {
    adjustments
        .iter()
        .map(|one| {
            super::confirming::starting_from_somewhere_else(world, &one.command.name);
            let answer = ask_for(world, one, &seconds.to_string());
            the_expiry_is_the_duration_the_caller_gave(one, &answer, seconds)
        })
        .collect()
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
    let held = |at: &str| {
        answer
            .pointer(at)
            .unwrap_or_else(|| panic!("an intervention carries `{at}`: {answer}"))
            .clone()
    };
    let bounded = Bounded {
        command: one.command.name.clone(),
        id: held("/intervention/id")
            .as_str()
            .expect("an identifier is text")
            .to_owned(),
        adjustable: held("/intervention/adjustable")
            .as_str()
            .expect("an adjustable is named")
            .to_owned(),
        applied_value: held("/intervention/applied_value"),
        prior_value: answer.pointer("/intervention/prior_value").cloned(),
        expires_at: expires,
    };
    the_value_it_would_restore_is_not_the_value_it_applied(&bounded);
    bounded
}

/// What an expiry would put back is not what the adjustment put in place.
///
/// Asserted before anything about restoring is tested, and only where the
/// machine reports a value at all: an intervention whose prior value is the
/// one it applied is one whose restoration nothing could observe, so a
/// journey that reached it would be proving nothing rather than failing.
fn the_value_it_would_restore_is_not_the_value_it_applied(bounded: &Bounded) {
    if bounded.reported_at().is_none() {
        return;
    }
    let prior = bounded.prior_value.as_ref().unwrap_or_else(|| {
        panic!(
            "`{}` changed something the machine reports and carries no prior value, so \
             there is nothing for its expiry to put back",
            bounded.command
        )
    });
    assert_ne!(
        prior, &bounded.applied_value,
        "`{}` would restore the very value it applied, so nothing about its restoration \
         could be observed",
        bounded.command
    );
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

/// Wait until a margin before one instant, and answer when the wait ended.
fn just_before(when: Timestamp, margin: Duration) -> Timestamp {
    wait(when, -i64::try_from(margin.as_micros()).expect("a margin"))
}

/// Wait until a margin after one instant, and answer when the wait ended.
fn just_after(when: Timestamp, margin: Duration) -> Timestamp {
    wait(when, i64::try_from(margin.as_micros()).expect("a margin"))
}

/// Wait until one instant shifted by a count of microseconds.
///
/// The instant is computed against the record's own expiry rather than against
/// the moment the request was made, which is what makes both reads scheduled
/// relative to the thing they are about.
fn wait(when: Timestamp, shift: i64) -> Timestamp {
    let until =
        when.as_utc().timestamp_micros() + shift - Timestamp::now().as_utc().timestamp_micros();
    if until > 0 {
        std::thread::sleep(Duration::from_micros(u64::try_from(until).expect("a wait")));
    }
    Timestamp::now()
}

/// The adjusted value is in place shortly before each one expires.
///
/// One read for the batch, taken before the earliest expiry among them: every
/// intervention is then asserted to have been read before **its own**, so a
/// read that arrived late fails here rather than passing for having been taken
/// at all.
pub fn the_adjusted_value_is_in_place_shortly_before_it_expires(world: &World, opened: &[Bounded]) {
    let earliest = opened
        .iter()
        .map(|bounded| bounded.expires_at)
        .min()
        .expect("this journey opened an intervention");
    let at = just_before(earliest, MARGIN);
    let status = running::read(world, &["status", "--print-id", &world.print_id]);
    let held = in_force(&status);

    for bounded in opened {
        assert!(
            at < bounded.expires_at,
            "`{}` was read at {at}, which is not before it expires at {}",
            bounded.command,
            bounded.expires_at
        );
        assert_eq!(
            held.get(&bounded.id),
            Some(&bounded.applied_value),
            "`{}` is not in force shortly before it expires: {status}",
            bounded.command
        );
        the_machine_reports(&status, bounded, Some(&bounded.applied_value), "adjusted");
    }
}

/// The prior value is back shortly after each one expired.
pub fn the_prior_value_is_back_shortly_after_it_expires(world: &World, opened: &[Bounded]) {
    let latest = opened
        .iter()
        .map(|bounded| bounded.expires_at)
        .max()
        .expect("this journey opened an intervention");
    let at = just_after(latest, MARGIN);
    let status = running::read(world, &["status", "--print-id", &world.print_id]);
    let held = in_force(&status);
    let history = running::read(
        world,
        &["history", "--print-id", &world.print_id, "--limit", "40"],
    );

    for bounded in opened {
        assert!(
            at > bounded.expires_at,
            "`{}` was read at {at}, which is not after it expires at {}",
            bounded.command,
            bounded.expires_at
        );
        assert!(
            !held.contains_key(&bounded.id),
            "`{}` is still in force after it expired: {status}",
            bounded.command
        );
        the_machine_reports(&status, bounded, bounded.prior_value.as_ref(), "prior");
        the_expiry_says_what_became_of_it(&history, bounded);
    }
}

/// The machine reports one value for what an intervention changed.
///
/// For an adjustable the machine reports at all, the value is asserted outright
/// — and its prior value is asserted to be one there was, because an
/// intervention over a machine that reported nothing has nothing to put back.
/// For the three it reports nothing about, the positive claim is made instead:
/// the machine is asserted to report no value for that adjustable, which is
/// what makes the expiry below say there was none rather than this journey
/// assuming it.
fn the_machine_reports(status: &Value, bounded: &Bounded, expected: Option<&Value>, which: &str) {
    let Some(at) = bounded.reported_at() else {
        assert!(
            bounded.prior_value.is_none(),
            "`{}` carries a prior value for an adjustable the machine reports nothing \
             about: {status}",
            bounded.command
        );
        return;
    };
    let expected = expected.unwrap_or_else(|| {
        panic!(
            "`{}` changed something the machine reports and carries no prior value, so \
             there is nothing for its expiry to put back: {status}",
            bounded.command
        )
    });
    let reported = status
        .pointer(&at)
        .unwrap_or_else(|| panic!("the machine reports no `{at}`: {status}"));
    assert_eq!(
        reported, expected,
        "`{}` left the machine reporting {reported} where the {which} value is {expected}",
        bounded.command
    );
}

/// The expiry says what became of one intervention.
fn the_expiry_says_what_became_of_it(history: &Value, bounded: &Bounded) {
    let expected = if bounded.reported_at().is_some() {
        "restored"
    } else {
        // The machine reports no value for this adjustable, which the read
        // above has just asserted, so there was nothing to put back and the
        // record says exactly that.
        "restore_unavailable"
    };
    assert_eq!(
        expiry_outcome(history, &bounded.id).as_deref(),
        Some(expected),
        "`{}` expired and the record does not say `{expected}`",
        bounded.command
    );
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
