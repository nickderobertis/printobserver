//! The wait for a supervisor's announcement is bounded, and says why it failed.
//!
//! Every journey that starts a supervisor waits for it to say where it is
//! serving, through `support/announced.rs`. What is proven here is that wait
//! itself, against stand-in supervisors: this same test binary, run for one
//! ignored test in a process of its own, printing what a supervisor prints on
//! the way up and then doing the one thing its [`Behaviour`] names. A real
//! supervisor cannot be made to print part of a start and then stall, which is
//! the case a failure has to explain, so the stand-in is what makes that case
//! reachable.

use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

use crate::announced;

/// Set by a journey here to the behaviour it wants. Absent, the stand-in
/// refuses to run, rather than hold whoever ran it directly for ten minutes.
const STANDING_IN: &str = "PRINTOBSERVER_STANDING_IN_FOR_A_SUPERVISOR";

const ON_THE_WAY_UP: [&str; 2] = [
    "printobserver is reading its configuration",
    "printobserver is asking the printer how it is",
];

/// Longer than any wait here, so a stand-in still running when a wait ends is
/// one the wait left running.
const STANDS_FOR: Duration = Duration::from_secs(600);

/// Far longer than [`SHORT`] and [`announced::DRAIN`] together, and far
/// shorter than [`STANDS_FOR`]: the process holding a stopped stand-in's
/// stream open outlasts the wait and the drain by a margin a loaded host
/// cannot close, and then goes away by itself, since nothing here has its
/// identity to stop it.
const HOLDS_FOR: Duration = Duration::from_secs(45);

/// Nothing listens here: the wait reads the announcement and never connects.
const ANNOUNCED: &str = "127.0.0.1:4242";

/// Short enough to keep the tier quick; the stand-ins go silent at once.
const SHORT: Duration = Duration::from_secs(3);

/// What a stand-in does once it has printed [`ON_THE_WAY_UP`].
#[derive(Clone, Copy)]
enum Behaviour {
    /// Goes silent and stays running.
    Silent,
    /// Says where it is serving and stays running.
    Announces,
    /// Says it is serving somewhere that is no socket address.
    Misannounces,
    /// Exits.
    Exits,
    /// Starts a silent process sharing its standard error, which outlives it
    /// once it is stopped, and goes silent itself.
    HandsItsStreamOn,
    /// What [`Behaviour::HandsItsStreamOn`] starts.
    Holds,
}

impl Behaviour {
    const ALL: [Self; 6] = [
        Self::Silent,
        Self::Announces,
        Self::Misannounces,
        Self::Exits,
        Self::HandsItsStreamOn,
        Self::Holds,
    ];

    fn name(self) -> &'static str {
        match self {
            Self::Silent => "silent",
            Self::Announces => "announces",
            Self::Misannounces => "misannounces",
            Self::Exits => "exits",
            Self::HandsItsStreamOn => "hands-its-stream-on",
            Self::Holds => "holds",
        }
    }
}

/// A supervisor that never says it is serving is stopped within the deadline,
/// and the failure carries everything it printed.
pub fn a_supervisor_that_never_announces_is_stopped_and_reported() {
    let (said, waited, mut child) = unannounced(Behaviour::Silent, SHORT);

    assert!(
        waited >= SHORT,
        "the wait gave up after {waited:?}, before its deadline of {SHORT:?}"
    );
    assert!(
        waited < SHORT + Duration::from_secs(30),
        "the wait for a silent supervisor took {waited:?} against a deadline of {SHORT:?}"
    );
    assert_stopped(&mut child, &said);
    assert_reported(&said, "the deadline ran out");
}

/// A supervisor that exits before it says it is serving is answered the moment
/// its output ends, rather than at the deadline.
pub fn a_supervisor_that_exits_unannounced_is_answered_when_its_output_ends() {
    let (said, waited, mut child) = unannounced(Behaviour::Exits, STANDS_FOR);

    assert!(
        waited < Duration::from_secs(60),
        "the wait for a supervisor that exited took {waited:?}"
    );
    assert_stopped(&mut child, &said);
    assert_reported(&said, "its output ended");
}

/// A supervisor announcing something that is no address is stopped at once,
/// and the failure names what it announced.
pub fn a_supervisor_announcing_no_address_is_stopped_and_reported() {
    let (said, waited, mut child) = unannounced(Behaviour::Misannounces, STANDS_FOR);

    assert!(
        waited < Duration::from_secs(60),
        "the wait for a supervisor announcing no address took {waited:?}"
    );
    assert_stopped(&mut child, &said);
    assert_reported(&said, "it announced `nowhere`, which is no address");
}

/// A stopped supervisor whose stream something else still holds open is
/// reported once the drain runs out, rather than when that holder lets go.
pub fn a_stream_held_open_is_read_no_longer_than_the_drain() {
    let started = Instant::now();
    let (said, _, mut child) = unannounced(Behaviour::HandsItsStreamOn, SHORT);
    let reported = started.elapsed();

    assert!(
        reported >= SHORT + announced::DRAIN,
        "the failure came after {reported:?}, before the deadline and the drain had both \
         run out, so the stream was not held open and this proves nothing"
    );
    // The holder started with the stand-in, so it lets go at about
    // `HOLDS_FOR` from the start: a failure near that waited for it.
    assert!(
        reported < HOLDS_FOR.saturating_sub(Duration::from_secs(10)),
        "the failure waited {reported:?}, for the holder to let go of the stream"
    );
    assert_stopped(&mut child, &said);
    assert_reported(&said, "the deadline ran out");
}

/// A supervisor that says it is serving is answered the moment it does, long
/// before the deadline, with what it printed kept.
pub fn a_supervisor_that_announces_is_answered_at_once() {
    let mut child = stand_in(Behaviour::Announces);
    let started = Instant::now();

    let (address, stream) = announced::within(&mut child, STANDS_FOR)
        .unwrap_or_else(|unannounced| panic!("the stand-in: {unannounced}"));
    let waited = started.elapsed();

    assert_eq!(
        address.to_string(),
        ANNOUNCED,
        "the wait answered another address"
    );
    assert!(
        waited < Duration::from_secs(60),
        "the wait took {waited:?} to answer an announcement made at once"
    );
    let running = child
        .try_wait()
        .expect("the stand-in's state reads")
        .is_none();
    assert!(running, "the stand-in stopped before it was asked to");
    let _ = child.kill();
    let _ = child.wait();
    let said = stream.collected();
    for line in ON_THE_WAY_UP {
        assert!(
            said.contains(line),
            "what the supervisor printed lost `{line}`: {said}"
        );
    }
    assert!(
        said.contains(&format!("{}{ANNOUNCED}", announced::SERVING_ON)),
        "what the supervisor printed lost its announcement: {said}"
    );
}

/// Wait on a stand-in that is expected not to announce; answer the failure as
/// it reads, how long the wait took and the stand-in.
fn unannounced(behaviour: Behaviour, deadline: Duration) -> (String, Duration, Child) {
    let mut child = stand_in(behaviour);
    let started = Instant::now();
    let Err(unannounced) = announced::within(&mut child, deadline) else {
        panic!(
            "a `{}` supervisor was answered as serving",
            behaviour.name()
        );
    };
    (unannounced.to_string(), started.elapsed(), child)
}

fn assert_stopped(child: &mut Child, said: &str) {
    let ended = child
        .try_wait()
        .expect("the stand-in's state reads")
        .is_some();
    assert!(
        ended,
        "the wait failed and left the supervisor running: {said}"
    );
}

fn assert_reported(said: &str, why: &str) {
    assert!(said.contains(why), "the failure did not say {why}: {said}");
    for line in ON_THE_WAY_UP {
        assert!(
            said.contains(line),
            "the failure left out `{line}`, which the supervisor printed: {said}"
        );
    }
}

/// Start the stand-in, behaving as `behaviour`, with its standard error piped.
fn stand_in(behaviour: Behaviour) -> Child {
    stand_in_command(behaviour)
        .stderr(Stdio::piped())
        .spawn()
        .expect("the stand-in starts")
}

fn stand_in_command(behaviour: Behaviour) -> Command {
    let binary = std::env::current_exe().expect("this test binary has a path");
    let mut command = Command::new(binary);
    command
        .args([
            "announcing::the_stand_in",
            "--exact",
            "--ignored",
            "--nocapture",
        ])
        .env(STANDING_IN, behaviour.name())
        .stdin(Stdio::null())
        .stdout(Stdio::null());
    command
}

/// A supervisor on the way up, doing what [`STANDING_IN`] names.
#[test]
#[ignore = "run in a process of its own by the journeys in this module"]
fn the_stand_in() {
    let named = std::env::var(STANDING_IN).unwrap_or_else(|_| {
        panic!("{STANDING_IN} is set by the journey that starts this; it is not run directly")
    });
    let behaviour = Behaviour::ALL
        .into_iter()
        .find(|behaviour| behaviour.name() == named)
        .unwrap_or_else(|| panic!("{STANDING_IN} is `{named}`, which names no behaviour"));
    for line in ON_THE_WAY_UP {
        eprintln!("{line}");
    }
    match behaviour {
        Behaviour::Silent => std::thread::sleep(STANDS_FOR),
        Behaviour::Announces => {
            eprintln!("{}{ANNOUNCED}", announced::SERVING_ON);
            std::thread::sleep(STANDS_FOR);
        }
        Behaviour::Misannounces => {
            eprintln!("{}nowhere", announced::SERVING_ON);
            std::thread::sleep(STANDS_FOR);
        }
        Behaviour::Exits => {}
        Behaviour::HandsItsStreamOn => {
            // Inherits this process's standard error, which is the pipe the
            // journey reads: stopping this process leaves it open. Waiting on
            // it is this process going silent until it is stopped.
            let mut holder = stand_in_command(Behaviour::Holds)
                .spawn()
                .expect("the holder starts");
            let _ = holder.wait();
        }
        Behaviour::Holds => std::thread::sleep(HOLDS_FOR),
    }
}
