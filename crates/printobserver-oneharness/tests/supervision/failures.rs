//! Every way a turn does not produce an assessment, and what the port does
//! about it.
//!
//! A supervisor whose agent is unreachable, out of credit, slow, or answering
//! nonsense is the ordinary case rather than the exception, and each of these
//! has to reach the caller as the thing it is rather than as a panic or a
//! silence. The recoveries are here too: a state directory that cannot be read
//! or written is reported, and a harness session store lost under a live ledger
//! opens the conversation again rather than stranding the print.

use std::fs;
use std::path::Path;
use std::sync::Arc;

use printobserver_oneharness::{
    HARNESS_SESSIONS_DIRECTORY, OneharnessSupervisor, SESSIONS_DIRECTORY, SupervisorConfig,
    TurnSeam, TurnTimeout, session_name,
};
use printobserver_supervisor_api::{SupervisorError, SupervisorPort};
use printobserver_types::{
    EventPayload, MalformedExternalEventPayload, PrintId, SessionPhase, serde_json,
};

use crate::support::{
    Fixture, HARNESS, Watch, always, assessment, assignment, block_on, config, event,
    generated_assessment_schema, identity, port, schema_read_lock, turn,
};

/// An event to hang a turn off.
fn payload() -> EventPayload {
    EventPayload::MalformedExternalEvent(MalformedExternalEventPayload {
        detail: "the body was not JSON".to_owned(),
    })
}

/// A configuration that would run a turn, answering conformingly.
fn answering(fixture: &Fixture) -> SupervisorConfig {
    config(
        fixture,
        HARNESS,
        &generated_assessment_schema(),
        always("SID-FAIL", &assessment("the print is fine", "high")),
    )
}

/// One turn of a fresh print through a port built from this configuration,
/// with nothing watching it.
fn unwatched_turn(configured: SupervisorConfig) -> Result<(), SupervisorError> {
    let supervisor = OneharnessSupervisor::open(configured)?;
    let print_id = PrintId::new();
    block_on(supervisor.run_turn(turn(print_id, event(print_id, payload()), None))).map(drop)
}

/// One turn of a fresh print, watched, answering what the port made of it.
fn watched_turn(configured: SupervisorConfig) -> Result<(), SupervisorError> {
    let watch = Arc::new(Watch::default());
    let supervisor = port(configured, &watch);
    let print_id = PrintId::new();
    block_on(supervisor.run_turn(turn(print_id, event(print_id, payload()), None))).map(drop)
}

/// The detail one error carries, or the shape it turned out to be.
fn detail(error: &SupervisorError) -> String {
    error.to_string()
}

/// Put one print's ledger on disk, exactly as this document writes it.
fn write_ledger(fixture: &Fixture, print_id: &PrintId, document: &serde_json::Value) {
    let path = fixture
        .state_dir()
        .join(SESSIONS_DIRECTORY)
        .join(format!("{print_id}.json"));
    fs::create_dir_all(path.parent().expect("the ledger has a directory"))
        .expect("the ledger directory");
    fs::write(&path, document.to_string()).expect("a scripted ledger");
}

/// A port needs the skill and the template to be where it was told they are.
#[test]
fn a_configuration_naming_files_that_are_not_there_is_refused() {
    let fixture = Fixture::new("failures-absent");
    for missing in ["skill", "template"] {
        let mut configured = answering(&fixture);
        let absent = fixture.path("nowhere.md");
        if missing == "skill" {
            configured.skill_path = absent.clone();
        } else {
            configured.prompt_template_path = absent.clone();
        }
        let refused = OneharnessSupervisor::open(configured)
            .err()
            .unwrap_or_else(|| panic!("a port was built with no {missing}"));
        let said = detail(&refused);
        assert!(
            said.contains("is unreadable") && said.contains("nowhere.md"),
            "the {missing} was not named: {said}"
        );
    }
}

/// A template that does not declare each slot exactly once is refused, saying
/// which slot and why.
#[test]
fn a_template_that_does_not_declare_each_slot_once_is_refused() {
    let fixture = Fixture::new("failures-template");
    let committed = fs::read_to_string(crate::support::template_path())
        .expect("the committed template is readable");

    let without = fixture.path("no-image.md");
    fs::write(&without, committed.replace("{{image_path}}", "a picture"))
        .expect("a scratch template");
    let twice = fixture.path("twice.md");
    fs::write(
        &twice,
        format!("{committed}\n\nAnd again: {{{{context_command}}}}\n"),
    )
    .expect("a scratch template");

    for (path, naming) in [
        (&without, "declares no {{image_path}} slot"),
        (&twice, "more than once"),
    ] {
        let mut configured = answering(&fixture);
        configured.prompt_template_path = path.clone();
        let refused = OneharnessSupervisor::open(configured)
            .err()
            .unwrap_or_else(|| panic!("a port was built from {}", path.display()));
        let said = detail(&refused);
        assert!(
            said.contains(naming),
            "the template was not refused for {naming}: {said}"
        );
    }
}

/// A port nobody is watching still takes a turn, and the seam says as much.
#[test]
fn a_port_with_nothing_watching_it_still_takes_a_turn() {
    // Every answer this journey drives is judged by the checked-in assessment
    // schema, so it is held still while the journey reads it.
    let _schemas = schema_read_lock();
    let fixture = Fixture::new("failures-unwatched");
    unwatched_turn(answering(&fixture)).expect("an unwatched turn runs");

    let empty = format!("{:?}", TurnSeam::default());
    assert!(
        empty.contains("requests: false")
            && empty.contains("processes: false")
            && empty.contains("reports: false"),
        "the empty seam does not say it is watching nothing: {empty}"
    );
    let watch = Arc::new(Watch::default());
    let watching = format!(
        "{:?}",
        TurnSeam {
            requests: Some(watch.clone()),
            processes: Some(watch.clone()),
            reports: Some(watch),
        }
    );
    assert!(
        watching.contains("requests: true")
            && watching.contains("processes: true")
            && watching.contains("reports: true"),
        "the watching seam does not say what it is watching: {watching}"
    );
}

/// A harness this system cannot reach is unavailable rather than an answer.
#[test]
fn a_harness_that_cannot_run_the_turn_is_unavailable() {
    // Every answer this journey drives is judged by the checked-in assessment
    // schema, so it is held still while the journey reads it.
    let _schemas = schema_read_lock();
    let fixture = Fixture::new("failures-unreachable");

    // A harness identity OneHarness does not know: refused before anything runs.
    let mut unknown = answering(&fixture);
    unknown.harness = identity("not-a-harness");
    let refused = watched_turn(unknown).expect_err("an unknown harness ran a turn");
    assert!(
        matches!(refused, SupervisorError::Unavailable { .. }),
        "an unknown harness was not reported unavailable: {refused:?}"
    );

    // A harness binary that is not on the machine: nothing to run the turn.
    let mut absent = answering(&fixture);
    absent.harness_bin = Some(fixture.path("no-such-harness"));
    let refused = watched_turn(absent).expect_err("an absent harness binary ran a turn");
    let said = detail(&refused);
    assert!(
        said.contains("did not run the turn"),
        "an absent harness binary was not reported as one: {said}"
    );

    // A harness that refuses the request outright — no credential, no turn.
    let mut refusing = answering(&fixture);
    refusing.harness_env = vec![
        assignment("MOCK_EXIT=1"),
        assignment("MOCK_STDERR=unauthorized: log in first"),
    ];
    let refused = watched_turn(refusing).expect_err("a refused request produced an answer");
    let said = detail(&refused);
    assert!(
        said.contains("the harness failed"),
        "a refused request was not reported as a harness failure: {said}"
    );
}

/// A turn that outlives its deadline is a timeout, not a hang.
#[test]
fn a_turn_that_outlives_its_deadline_is_a_timeout() {
    // Every answer this journey drives is judged by the checked-in assessment
    // schema, so it is held still while the journey reads it.
    let _schemas = schema_read_lock();
    let fixture = Fixture::new("failures-timeout");
    let mut slow = answering(&fixture);
    slow.turn_timeout = TurnTimeout::new(1).expect("one second is a bound");
    slow.harness_env.push(assignment("MOCK_SLEEP_MS=20000"));
    // The harness child is killed at the deadline, so its coverage profile is
    // written somewhere the gate's own collection will not pick a truncated
    // file up from.
    slow.harness_env.push(assignment(&format!(
        "LLVM_PROFILE_FILE={}",
        fixture.path("killed-%p.profraw").display()
    )));
    let refused = watched_turn(slow).expect_err("a turn past its deadline answered");
    assert_eq!(refused, SupervisorError::TimedOut);
}

/// An answer the configured schema admits and the assessment type does not is
/// a bad answer rather than a value this port hands on.
#[test]
fn an_answer_the_type_refuses_is_refused_even_when_a_schema_admits_it() {
    let fixture = Fixture::new("failures-permissive");
    let permissive = fixture.path("anything.json");
    fs::write(&permissive, r#"{"type": "object"}"#).expect("a scratch schema");

    let mut configured = config(
        &fixture,
        HARNESS,
        &permissive,
        always(
            "SID-PERMISSIVE",
            &serde_json::json!({ "summary": 17 }).to_string(),
        ),
    );
    configured.assessment_schema_path = permissive;
    let refused = watched_turn(configured).expect_err("an answer the type refuses was handed on");
    assert!(
        matches!(refused, SupervisorError::InvalidAnswer { .. }),
        "an answer the type refuses was not refused: {refused:?}"
    );
}

/// A ledger that cannot be read is reported by every method that reads one.
#[test]
fn a_ledger_that_cannot_be_read_is_reported() {
    // Every answer this journey drives is judged by the checked-in assessment
    // schema, so it is held still while the journey reads it.
    let _schemas = schema_read_lock();
    let fixture = Fixture::new("failures-unreadable");
    let print_id = PrintId::new();
    let ledger = fixture
        .state_dir()
        .join(SESSIONS_DIRECTORY)
        .join(format!("{print_id}.json"));
    fs::create_dir_all(ledger.parent().expect("the ledger has a directory"))
        .expect("the ledger directory");
    fs::write(&ledger, "this is not a ledger").expect("a broken ledger");

    let watch = Arc::new(Watch::default());
    let supervisor = port(answering(&fixture), &watch);

    let refused = block_on(supervisor.run_turn(turn(print_id, event(print_id, payload()), None)))
        .expect_err("a turn ran against a ledger it could not read");
    assert!(
        detail(&refused).contains("is unreadable"),
        "the unreadable ledger was not named: {}",
        detail(&refused)
    );
    supervisor
        .recorded_sessions(&print_id)
        .expect_err("an unreadable ledger answered sessions");
    supervisor
        .recorded_turns(&print_id)
        .expect_err("an unreadable ledger answered turns");
    block_on(supervisor.close_session(print_id, "finished".to_owned()))
        .expect_err("an unreadable ledger was closed");

    // And a state directory with something other than a ledger directory in it
    // is reported the same way, rather than read as a print with no sessions.
    let blocked = Fixture::new("failures-blocked");
    fs::write(
        blocked.state_dir().join(SESSIONS_DIRECTORY),
        "not a directory",
    )
    .expect("a file where the ledger directory belongs");
    let blocked_watch = Arc::new(Watch::default());
    let blocked_port = port(answering(&blocked), &blocked_watch);
    blocked_port
        .recorded_sessions(&PrintId::new())
        .expect_err("a blocked ledger directory answered sessions");
}

/// A ledger that cannot be written is reported rather than lost.
#[cfg(unix)]
#[test]
fn a_ledger_that_cannot_be_written_is_reported() {
    // Every answer this journey drives is judged by the checked-in assessment
    // schema, so it is held still while the journey reads it.
    let _schemas = schema_read_lock();
    let fixture = Fixture::new("failures-unwritable");
    // A dangling link where the ledger directory belongs: reading a print's
    // ledger finds nothing, and creating the directory to write one cannot
    // succeed.
    std::os::unix::fs::symlink(
        fixture.path("nowhere-at-all"),
        fixture.state_dir().join(SESSIONS_DIRECTORY),
    )
    .expect("a dangling link where the ledger directory belongs");

    let watch = Arc::new(Watch::default());
    let supervisor = port(answering(&fixture), &watch);
    let print_id = PrintId::new();

    let refused = block_on(supervisor.run_turn(turn(print_id, event(print_id, payload()), None)))
        .expect_err("a turn was recorded into a ledger that cannot be written");
    assert!(
        detail(&refused).contains("could not be written"),
        "the unwritable ledger was not named: {}",
        detail(&refused)
    );
    block_on(supervisor.close_session(print_id, "finished".to_owned()))
        .expect_err("a close was recorded into a ledger that cannot be written");
}

/// A ledger a later build wrote is refused rather than read as this build's.
#[test]
fn a_ledger_written_under_another_shape_is_refused() {
    let fixture = Fixture::new("failures-shape");
    let print_id = PrintId::new();
    write_ledger(
        &fixture,
        &print_id,
        &serde_json::json!({
            "schema_version": "2",
            "print_id": print_id,
            "sessions": [],
            "turns": [],
        }),
    );

    let watch = Arc::new(Watch::default());
    let supervisor = port(answering(&fixture), &watch);
    let refused = supervisor
        .recorded_sessions(&print_id)
        .expect_err("a ledger of another shape was read as this build's");
    let said = detail(&refused);
    assert!(
        said.contains("not a ledger this build reads") && said.contains("V1"),
        "the refusal does not say which shape this build writes: {said}"
    );
}

/// A ledger holding another print's sessions is refused rather than continued.
#[test]
fn a_ledger_of_another_print_is_refused() {
    let fixture = Fixture::new("failures-other-print");
    let print_id = PrintId::new();
    let other = PrintId::new();
    write_ledger(
        &fixture,
        &print_id,
        &serde_json::json!({
            "schema_version": "1",
            "print_id": other,
            "sessions": [],
            "turns": [],
        }),
    );

    let watch = Arc::new(Watch::default());
    let supervisor = port(answering(&fixture), &watch);
    let refused = supervisor
        .recorded_turns(&print_id)
        .expect_err("one print's ledger was read as another's");
    let said = detail(&refused);
    assert!(
        said.contains(&other.to_string()) && said.contains(&print_id.to_string()),
        "the refusal names neither print: {said}"
    );
}

/// A harness session store lost under a live ledger opens the conversation
/// again rather than stranding the print.
#[test]
fn a_lost_harness_store_opens_the_conversation_again() {
    // Every answer this journey drives is judged by the checked-in assessment
    // schema, so it is held still while the journey reads it.
    let _schemas = schema_read_lock();
    let fixture = Fixture::new("failures-lost-store");
    let watch = Arc::new(Watch::default());
    let supervisor = port(answering(&fixture), &watch);
    let print_id = PrintId::new();

    let opened = block_on(supervisor.run_turn(turn(print_id, event(print_id, payload()), None)))
        .expect("the first turn runs");
    assert_eq!(opened.phase, SessionPhase::Created);
    assert_eq!(opened.session.session_name, session_name(&print_id, 1));

    let store: &Path = &fixture.state_dir().join(HARNESS_SESSIONS_DIRECTORY);
    fs::remove_dir_all(store).expect("the harness session store is removable");

    let again = block_on(supervisor.run_turn(turn(print_id, event(print_id, payload()), None)))
        .expect("the turn after the store was lost runs");
    assert_eq!(
        again.phase,
        SessionPhase::Created,
        "the report said the conversation continued, with nothing left to continue"
    );
    assert_eq!(again.session.session_name, opened.session.session_name);

    let sessions = supervisor
        .recorded_sessions(&print_id)
        .expect("the ledger is readable");
    assert_eq!(sessions.len(), 1, "the print grew a second session");
    assert!(sessions[0].created_at >= opened.session.created_at);
}
