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
//! journey reads twice around each intervention, and **both reads are
//! scheduled against the expiry that intervention's own record carries**
//! rather than against the moment the request was made, or against any other
//! intervention's: once at that instant less [`MARGIN`] and once at it plus
//! [`MARGIN`]. The five adjustments are taken one at a time — asked for, read
//! before its expiry, read after it — so that what has to fit between an
//! intervention's request and its first read is that request's own tail and
//! nothing else. A batch of five requests followed by one read before the
//! earliest expiry had to fit four more programs into the same span, and at
//! the shortest duration this program accepts that span is under a second.
//! That shortest duration is the first timed value everywhere but on Windows,
//! whose tracer makes the request's own tail longer than a second —
//! [`WINDOWS_TRACED_REQUEST_SECONDS`] says how and why.
//!
//! **The read before the expiry is made in this process**, through the
//! client crate, and not through a spawned program. Every other read a journey
//! makes goes through the built program under the tracer, and that is the
//! right surface for what those reads prove; but this one is about the
//! **server's** state at an instant, and a spawned, traced, coverage-
//! instrumented program has no bound on how long it takes under the load a
//! gate applies — one traced `status` has been seen to take longer than
//! [`MARGIN`], and a read that lands after the expiry it was scheduled before
//! proves nothing about either side of it. The program's own `status` command
//! is proven elsewhere in the walk, and still answers the reads after the
//! expiry below. What the pre-expiry assertion compares against the recorded
//! expiry is the instant the read's **answer arrived**, not the instant the
//! wait ended: the answer is evidence that the intervention was in force only
//! if it was composed before the expiry, so a read that was issued in time and
//! answered late fails naming that read rather than passing for having started
//! on time.
//!
//! **The reads after the expiry stay in the traced program** — the status and
//! the history — because there a slow read can only land later, which is the
//! side of the expiry the read is about. Each is still asserted to have been
//! taken after its own intervention's expiry, so a read that drifted is a
//! failure rather than a silent weakening.
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
use std::time::Instant;

use printobserver::failure::Exit;
use printobserver::surface::{MAX_DURATION_SECONDS, MIN_DURATION_SECONDS};
use printobserver_sdk::{Actor, Client};
use printobserver_types::Timestamp;
use printobserver_types::serde_json::Value;

use crate::machine::Reports;
use crate::walk::{self, Driven};
use crate::world::{self, World};

use super::failures::succeeding;
use super::running;

/// The durations this program refuses, each sitting outside what it accepts.
const REFUSED: [&str; 4] = ["0", "-1", "quickly", "86401"];

/// How far either side of a recorded expiry a journey reads.
///
/// Short enough that "shortly before" is inside the shortest duration this
/// program accepts, and long enough that a read started there finishes on the
/// right side of the instant it is about. One value on every platform: no
/// tracer sits inside the window before an expiry, so no tracer's cost is
/// allowed for in it — what Windows' tracer costs is allowed for at
/// [`WINDOWS_TRACED_REQUEST_SECONDS`], and is the request's rather than a
/// read's.
pub const MARGIN: Duration = Duration::from_millis(400);

/// What the Windows tracer adds to the shortest timed duration.
///
/// The first timed value of the corpus is the shortest duration this program
/// accepts, on every platform but one. On Windows the traced request itself
/// answers late: `traced` there stops the kernel logger and decodes its
/// session with `tracerpt` **after** the program has exited and **before** it
/// answers, so the recorded expiry — which is what the pre-expiry read is
/// scheduled from — is in hand only after the server's `requested_at` plus
/// that decode. Nothing here can make that read before it knows the instant
/// it is about, and one second would not survive a decode this host cannot
/// measure. So Windows alone starts the corpus this much above the minimum;
/// it is an allowance for the request's own tail and not for any read, which
/// is why [`MARGIN`] does not grow with it. What removes it is a pre-expiry
/// read scheduled from a deadline known before the traced request returns.
const WINDOWS_TRACED_REQUEST_SECONDS: i64 = 10;

/// The first timed value of the corpus on this host.
fn shortest_timed(windows: bool) -> i64 {
    if windows {
        MIN_DURATION_SECONDS + WINDOWS_TRACED_REQUEST_SECONDS
    } else {
        MIN_DURATION_SECONDS
    }
}

#[test]
fn every_host_but_windows_starts_the_corpus_at_the_minimum() {
    assert_eq!(shortest_timed(false), MIN_DURATION_SECONDS);
    assert_eq!(
        shortest_timed(true),
        MIN_DURATION_SECONDS + WINDOWS_TRACED_REQUEST_SECONDS
    );
    assert!(
        MARGIN < Duration::from_secs(u64::try_from(MIN_DURATION_SECONDS).expect("a duration")),
        "the margin is not inside the shortest duration this program accepts"
    );
}

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

    let first = shortest_timed(cfg!(windows));
    for seconds in [first, first + 2] {
        // One at a time, so that each intervention's two reads wait on its
        // own request and on nothing another adjustment did.
        for one in &adjustments {
            let opened = each_asks_for(world, std::slice::from_ref(one), seconds);
            the_adjusted_value_is_in_place_shortly_before_it_expires(world, &opened);
            the_prior_value_is_back_shortly_after_it_expires(world, &opened);
        }
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
/// the putting back would be indistinguishable from one that did neither. The
/// settling comes before the request, and so before every timed window the
/// reads around that request open.
pub fn each_asks_for(world: &World, adjustments: &[Driven], seconds: i64) -> Vec<Bounded> {
    let asked = seconds.to_string();
    adjustments
        .iter()
        .map(|one| {
            super::confirming::starting_from_somewhere_else(world, &one.command.name);
            let answer = ask_for(world, one, &asked);
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
    until(when.as_utc().timestamp_micros() - micros(margin))
}

/// Wait until a margin after one instant, and answer when the wait ended.
///
/// Held on both clocks. The supervisor notices an expiry on a poll of its own,
/// whose cadence is elapsed time rather than the wall clock, so a wall clock
/// stepped forward across the instant would end a wait kept on it alone with
/// the supervisor given almost none of the margin to notice. So the margin is
/// also counted as elapsed time from the moment the wall clock was seen past
/// the instant, and the wait ends only when both have run out.
fn just_after(when: Timestamp, margin: Duration) -> Timestamp {
    let instant = when.as_utc().timestamp_micros();
    until(instant);
    let crossed = Instant::now();
    until(instant + micros(margin));
    if let Some(left) = (crossed + margin).checked_duration_since(Instant::now()) {
        std::thread::sleep(left);
    }
    Timestamp::now()
}

/// One margin, as the count of microseconds an instant is shifted by.
fn micros(margin: Duration) -> i64 {
    i64::try_from(margin.as_micros()).expect("a margin")
}

/// How long a wait sleeps before looking at the clock again.
const WAIT_SLICE: Duration = Duration::from_millis(10);

/// Wait until the wall clock reaches one instant, in microseconds since the
/// epoch, and answer when it did.
///
/// The instant is computed against the record's own expiry rather than against
/// the moment the request was made, which is what makes both reads scheduled
/// relative to the thing they are about.
///
/// It is waited for on the clock it is an instant of. Every instant this
/// system records is wall-clock time, and a host may step its wall clock while
/// a journey runs — a guest resynchronising against its host does, by seconds
/// at a time — so one sleep for the whole remainder, which counts elapsed time,
/// would end at the wrong wall-clock instant and read on the wrong side of the
/// expiry it was scheduled against. Sleeping a slice at a time and looking at
/// the wall clock between slices ends the wait when that clock says so,
/// whatever it did in the meantime.
fn until(instant: i64) -> Timestamp {
    loop {
        let remaining = instant - Timestamp::now().as_utc().timestamp_micros();
        if remaining <= 0 {
            return Timestamp::now();
        }
        let remaining = Duration::from_micros(u64::try_from(remaining).expect("a wait"));
        std::thread::sleep(remaining.min(WAIT_SLICE));
    }
}

/// The adjusted value is in place shortly before each one expires.
///
/// One read per intervention, scheduled [`MARGIN`] before **its own** recorded
/// expiry and made in this process rather than through the traced program, so
/// that nothing spawned sits between the scheduled instant and the answer.
/// Three instants say what the read proves. `at` is when the wait ended,
/// which is the schedule, and it is held to be no earlier than the margin
/// before the expiry. `answered` is when the answer was in hand, and it is
/// held to be before the expiry: an answer composed after it says nothing
/// about the intervention having been in force, so a read whose answer arrived
/// late fails here naming that read. And the status's own
/// `printer.observed_at` is when the server looked at the machine to compose
/// that answer, which it stamps before reading which interventions still
/// stand, so the machine's value the read carries is one observed before the
/// expiry too.
pub fn the_adjusted_value_is_in_place_shortly_before_it_expires(world: &World, opened: &[Bounded]) {
    for bounded in opened {
        let at = just_before(bounded.expires_at, MARGIN);
        let (status, answered) = status_in_process(world);
        let observed = instant(&status, "/printer/observed_at");
        let held = in_force(&status);

        assert!(
            at.as_utc().timestamp_micros()
                >= bounded.expires_at.as_utc().timestamp_micros() - micros(MARGIN),
            "`{}` was read at {at}, which is earlier than {MARGIN:?} before it expires at {}",
            bounded.command,
            bounded.expires_at
        );
        assert!(
            answered < bounded.expires_at,
            "`{}` was read at {at}, before it expires at {}, but the answer arrived at \
             {answered}, after: the read in this process took longer than the window left",
            bounded.command,
            bounded.expires_at
        );
        assert!(
            observed < bounded.expires_at,
            "`{}` was answered at {answered}, before it expires at {}, but the machine was \
             observed at {observed}, after it",
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

/// One status read of this world's print made in this process, and the instant
/// its answer was in hand.
///
/// The client crate's own `status` operation, through the same recording proxy
/// every traced command reaches the supervisor through, and its typed answer
/// rendered back to the document the server sent — which is what the program
/// prints under `--json`, so every assertion reads the same fields either way.
fn status_in_process(world: &World) -> (Value, Timestamp) {
    let client = Client::new(world.proxy.url(), Actor::Operator).with_credential(world::CREDENTIAL);
    let answer = client
        .status(&world.print_id)
        .unwrap_or_else(|error| panic!("the status could not be read in this process: {error}"));
    let answered = Timestamp::now();
    let status = printobserver_sdk::as_value(&answer)
        .unwrap_or_else(|error| panic!("a status answer this client read renders: {error}"));
    (status, answered)
}

/// The prior value is back shortly after each one expired.
///
/// One status read and one history read per intervention, scheduled
/// [`MARGIN`] after **its own** recorded expiry and made through the traced
/// program: here a slow read can only land later, which is the side of the
/// expiry this read is about, and the read is still asserted to have been
/// taken after it.
pub fn the_prior_value_is_back_shortly_after_it_expires(world: &World, opened: &[Bounded]) {
    for bounded in opened {
        let at = just_after(bounded.expires_at, MARGIN);
        let status = running::read(world, &["status", "--print-id", &world.print_id]);
        let held = in_force(&status);
        let history = running::read(
            world,
            &["history", "--print-id", &world.print_id, "--limit", "40"],
        );

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
