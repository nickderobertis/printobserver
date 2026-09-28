//! The wait for a supervisor's announcement is bounded, and says why it failed.
//!
//! Every journey that starts a supervisor waits for it to say where it is
//! serving, through `support/announced.rs`. What is proven here is that wait
//! itself, against two stand-in supervisors: this same test binary, run for
//! one ignored test in a process of its own, printing what a supervisor
//! prints on the way up. One goes silent and never announces; the other
//! announces and then goes on running. A real supervisor cannot be made to
//! print part of a start and then stall, which is the case a failure has to
//! explain, so the stand-in is what makes that case reachable.

use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

use crate::announced;

/// The variable that tells a stand-in it was started by a journey here rather
/// than run directly, where it would hold whoever ran it for ten minutes.
const STANDING_IN: &str = "PRINTOBSERVER_STANDING_IN_FOR_A_SUPERVISOR";

/// What a stand-in prints before it goes silent or announces.
const ON_THE_WAY_UP: [&str; 2] = [
    "printobserver is reading its configuration",
    "printobserver is asking the printer how it is",
];

/// Longer than any wait here, so a stand-in still running when a wait ends is
/// one the wait left running.
const STANDS_FOR: Duration = Duration::from_secs(600);

/// The address the announcing stand-in names.
const ANNOUNCED: &str = "127.0.0.1:4242";

/// A supervisor that never says it is serving is stopped within the deadline,
/// and the failure carries everything it printed.
pub fn a_supervisor_that_never_announces_is_stopped_and_reported() {
    let deadline = Duration::from_secs(3);
    let mut child = stand_in("announcing::the_stand_in_never_announces");
    let started = Instant::now();

    let Err(unannounced) = announced::within(&mut child, deadline) else {
        panic!("a supervisor that never announced itself was answered as serving");
    };
    let waited = started.elapsed();

    assert!(
        waited >= deadline,
        "the wait gave up after {waited:?}, before its deadline of {deadline:?}"
    );
    assert!(
        waited < deadline + Duration::from_secs(30),
        "the wait for a silent supervisor took {waited:?} against a deadline of {deadline:?}"
    );
    let ended = child
        .try_wait()
        .expect("the stand-in's state reads")
        .is_some();
    assert!(
        ended,
        "the wait gave up and left the silent supervisor running"
    );
    let said = unannounced.to_string();
    for line in ON_THE_WAY_UP {
        assert!(
            said.contains(line),
            "the failure left out `{line}`, which the supervisor printed: {said}"
        );
    }
    assert!(
        said.contains("the deadline ran out"),
        "the failure did not say the deadline ran out: {said}"
    );
}

/// A supervisor that says it is serving is answered the moment it does, long
/// before the deadline, with what it printed kept.
pub fn a_supervisor_that_announces_is_answered_at_once() {
    let mut child = stand_in("announcing::the_stand_in_announces");
    let started = Instant::now();

    let (address, stream) = announced::within(&mut child, STANDS_FOR)
        .unwrap_or_else(|unannounced| panic!("the stand-in: {unannounced}"));
    let waited = started.elapsed();

    assert_eq!(address, ANNOUNCED, "the wait answered another address");
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
    let said = stream.through_the_end();
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

/// Start one ignored test of this binary as a stand-in supervisor.
fn stand_in(test_name: &str) -> Child {
    let binary = std::env::current_exe().expect("this test binary has a path");
    Command::new(binary)
        .args([test_name, "--exact", "--ignored", "--nocapture"])
        .env(STANDING_IN, "1")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .expect("the stand-in starts")
}

/// Print the way up, then stay running.
fn stand(announcing: bool) {
    assert!(
        std::env::var_os(STANDING_IN).is_some(),
        "{STANDING_IN} is set by the journey that starts this; it is not run directly"
    );
    for line in ON_THE_WAY_UP {
        eprintln!("{line}");
    }
    if announcing {
        eprintln!("{}{ANNOUNCED}", announced::SERVING_ON);
    }
    std::thread::sleep(STANDS_FOR);
}

/// A supervisor that prints part of a start and then goes silent.
#[test]
#[ignore = "run in a process of its own by a_supervisor_that_never_announces_is_stopped_and_reported"]
fn the_stand_in_never_announces() {
    stand(false);
}

/// A supervisor that announces itself and goes on running.
#[test]
#[ignore = "run in a process of its own by a_supervisor_that_announces_is_answered_at_once"]
fn the_stand_in_announces() {
    stand(true);
}
