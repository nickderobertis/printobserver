//! A turn's model calls are recorded, and a turn attaching its check's image is
//! held to the assessment schema by `OneHarness`, beside that image.
//!
//! The run hands the engine the schema and the image together, and the engine
//! asks again, in the same session, for an answer that does not conform. What
//! the harness reported of each model call it made, every attempt's in order,
//! is written down with the turn.

use std::fs;
use std::sync::Arc;

use printobserver_oneharness::{CallUsage, EnvAssignment, SESSIONS_DIRECTORY, SessionName};
use printobserver_supervisor_api::{SupervisorError, SupervisorPort as _};
use printobserver_types::{PrintId, serde_json};

use crate::support::{
    Fixture, HARNESS, Watch, assessment, assignment, block_on, config, event, failure_alert,
    generated_assessment_schema, port, schema_read_lock, turn,
};

/// One model call's usage as Claude Code streams it: input, cache writes,
/// cache reads and output.
type Streamed = (u64, u64, u64, u64);

/// Claude Code's stream-json output for one run: one assistant message per
/// call, carrying that call's usage, and the result carrying `answer`.
fn streamed(session_id: &str, answer: &str, calls: &[Streamed]) -> String {
    let mut lines: Vec<String> = calls
        .iter()
        .enumerate()
        .map(|(index, (input, written, read, output))| {
            serde_json::json!({
                "type": "assistant",
                "session_id": session_id,
                "message": {
                    "id": format!("msg_{index}"),
                    "model": "claude-test",
                    "content": [{ "type": "text", "text": "judging the frame" }],
                    "usage": {
                        "input_tokens": input,
                        "cache_creation_input_tokens": written,
                        "cache_read_input_tokens": read,
                        "output_tokens": output,
                    },
                },
            })
            .to_string()
        })
        .collect();
    lines.push(
        serde_json::json!({
            "type": "result",
            "subtype": "success",
            "is_error": false,
            "result": answer,
            "session_id": session_id,
        })
        .to_string(),
    );
    lines.join("\n")
}

/// The responder scripted to answer each run in turn, and the last from then on.
fn runs_answering(counter: &std::path::Path, documents: &[String]) -> Vec<EnvAssignment> {
    let mut env = vec![assignment(&format!(
        "MOCK_ATTEMPT_FILE={}",
        counter.display()
    ))];
    for (index, document) in documents.iter().enumerate() {
        env.push(assignment(&format!("MOCK_STDOUT_{}={document}", index + 1)));
    }
    env.push(assignment(&format!(
        "MOCK_STDOUT={}",
        documents.last().cloned().unwrap_or_default()
    )));
    env
}

/// What one streamed call is recorded as.
const fn recorded((input, written, read, output): Streamed) -> CallUsage {
    CallUsage {
        input_tokens: Some(input),
        output_tokens: Some(output),
        cache_read_tokens: Some(read),
        cache_write_tokens: Some(written),
    }
}

/// A scratch frame, a JPEG by its leading bytes.
fn frame(fixture: &Fixture) -> std::path::PathBuf {
    let path = fixture.path("frame.jpg");
    fs::write(&path, b"\xff\xd8\xff\xe0 a frame").expect("a scratch frame");
    path
}

/// An image turn's conforming answer is its assessment, from one run, and each
/// model call that run made is recorded with the turn.
#[test]
fn an_image_turns_conforming_answer_is_taken_and_its_calls_recorded() {
    let schemas = schema_read_lock();
    let fixture = Fixture::new("lean-conforming");
    let calls = [(3, 4_000, 0, 120), (2, 300, 4_000, 80)];
    let watch = Arc::new(Watch::default());
    let supervisor = port(
        config(
            &schemas,
            &fixture,
            HARNESS,
            &generated_assessment_schema(),
            runs_answering(
                &fixture.path("attempts"),
                &[streamed(
                    "SID-LEAN",
                    &assessment("the walls are clean", "high"),
                    &calls,
                )],
            ),
        ),
        &watch,
    );
    let print_id = PrintId::new();
    let image = frame(&fixture);
    let outcome = block_on(supervisor.run_turn(
        turn(
            print_id,
            event(print_id, failure_alert(7, None)),
            Some(image.clone()),
        ),
        crate::support::access(),
    ))
    .expect("the turn's answer conforms");

    assert_eq!(outcome.assessment.summary, "the walls are clean");
    let requests = watch.requests();
    assert_eq!(requests.len(), 1, "a conforming answer was asked for again");
    assert_eq!(requests[0].images, [image]);
    assert_eq!(requests[0].schema, Some(generated_assessment_schema()));
    assert_eq!(requests[0].replace_system, Some(true));
    let turns = supervisor
        .recorded_turns(&print_id)
        .expect("the ledger reads");
    assert_eq!(turns.len(), 1);
    assert_eq!(turns[0].calls, calls.map(recorded));
}

/// An image turn's answer out of the schema is asked for again by `OneHarness`
/// under the turn's one request; the turn answers what the second attempt
/// answered, and records both attempts' calls in order.
#[test]
fn an_image_turn_answering_out_of_the_schema_is_asked_again_in_its_session() {
    let schemas = schema_read_lock();
    let fixture = Fixture::new("lean-again");
    let first = [(3, 4_000, 0, 150)];
    let second = [(1, 200, 4_100, 60)];
    let watch = Arc::new(Watch::default());
    let supervisor = port(
        config(
            &schemas,
            &fixture,
            HARNESS,
            &generated_assessment_schema(),
            runs_answering(
                &fixture.path("attempts"),
                &[
                    streamed(
                        "SID-AGAIN",
                        &serde_json::json!({ "summary": "no disposition" }).to_string(),
                        &first,
                    ),
                    streamed(
                        "SID-AGAIN",
                        &assessment("the print is fine", "medium"),
                        &second,
                    ),
                ],
            ),
        ),
        &watch,
    );
    let print_id = PrintId::new();
    let outcome = block_on(supervisor.run_turn(
        turn(
            print_id,
            event(print_id, failure_alert(7, None)),
            Some(frame(&fixture)),
        ),
        crate::support::access(),
    ))
    .expect("the second answer conforms");

    assert_eq!(outcome.assessment.summary, "the print is fine");
    assert_eq!(
        watch.requests().len(),
        1,
        "the port asked again itself rather than OneHarness"
    );
    assert_eq!(
        watch.programs().len(),
        2,
        "the answer out of schema was not asked again"
    );
    let turns = supervisor
        .recorded_turns(&print_id)
        .expect("the ledger reads");
    assert_eq!(turns.len(), 1);
    assert_eq!(turns[0].failure, None);
    assert_eq!(
        turns[0].calls,
        [first.map(recorded), second.map(recorded)].concat()
    );
}

/// An image turn whose answer never conforms is asked for again by `OneHarness`
/// as often as it asks, under the turn's one request, and is then refused as an
/// answer that failed validation, recorded against the turn with every
/// attempt's calls.
#[test]
fn an_image_turn_never_conforming_fails_as_an_invalid_answer() {
    let schemas = schema_read_lock();
    let fixture = Fixture::new("lean-exhausted");
    let call = (3, 1_000, 2_000, 40);
    let watch = Arc::new(Watch::default());
    let supervisor = port(
        config(
            &schemas,
            &fixture,
            HARNESS,
            &generated_assessment_schema(),
            runs_answering(
                &fixture.path("attempts"),
                &[streamed(
                    "SID-NEVER",
                    &serde_json::json!({ "summary": "no disposition" }).to_string(),
                    &[call],
                )],
            ),
        ),
        &watch,
    );
    let print_id = PrintId::new();
    let refused = block_on(supervisor.run_turn(
        turn(
            print_id,
            event(print_id, failure_alert(7, None)),
            Some(frame(&fixture)),
        ),
        crate::support::access(),
    ));
    let Err(SupervisorError::InvalidAnswer { detail }) = refused else {
        panic!("an answer that never conformed was not refused as one: {refused:?}");
    };
    assert!(detail.contains("disposition"), "{detail}");
    assert_eq!(watch.requests().len(), 1, "the port asked again itself");
    let turns = supervisor
        .recorded_turns(&print_id)
        .expect("the ledger reads");
    assert_eq!(turns.len(), 1);
    assert!(
        turns[0].failure.is_some(),
        "the failed turn was not recorded as one"
    );
    // The engine's own default: two more attempts after the first.
    assert_eq!(turns[0].calls, vec![recorded(call); 3]);
}

/// A ledger written in the earlier shape, its session and turn carrying no
/// calls, is read as this build's and written back as it, the earlier turn kept
/// as it was beside the new one's calls.
#[test]
fn a_ledger_of_the_earlier_shape_is_read_and_written_back_as_this_one() {
    let schemas = schema_read_lock();
    let fixture = Fixture::new("lean-earlier-ledger");
    let print_id = PrintId::new();
    let session = SessionName::of(&print_id, 1).to_string();
    let earlier_turn = serde_json::json!({
        "session_name": session,
        "ran_at": "2026-01-01T00:00:00Z",
    });
    let ledger = fixture
        .state_dir()
        .join(SESSIONS_DIRECTORY)
        .join(format!("{print_id}.json"));
    fs::create_dir_all(ledger.parent().expect("a directory")).expect("the ledger directory");
    fs::write(
        &ledger,
        serde_json::json!({
            "schema_version": "1",
            "print_id": print_id,
            "sessions": [{
                "print_id": print_id,
                "session_name": session,
                "harness_identity": HARNESS,
                "created_at": "2026-01-01T00:00:00Z",
                "last_turn_at": "2026-01-01T00:00:00Z",
            }],
            "turns": [earlier_turn],
        })
        .to_string(),
    )
    .expect("an earlier ledger");
    let calls = [(5, 3_000, 0, 90)];
    let watch = Arc::new(Watch::default());
    let supervisor = port(
        config(
            &schemas,
            &fixture,
            HARNESS,
            &generated_assessment_schema(),
            runs_answering(
                &fixture.path("attempts"),
                &[streamed("SID-EARLIER", &assessment("fine", "high"), &calls)],
            ),
        ),
        &watch,
    );
    block_on(supervisor.run_turn(
        turn(
            print_id,
            event(print_id, failure_alert(7, None)),
            Some(frame(&fixture)),
        ),
        crate::support::access(),
    ))
    .expect("the turn runs over the earlier ledger");

    let written: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(&ledger).expect("the ledger reads"))
            .expect("the ledger is JSON");
    assert_eq!(written["schema_version"], "3");
    assert_eq!(written["sessions"][0]["session_name"], session);
    let turns = written["turns"].as_array().expect("the turns");
    assert_eq!(turns.len(), 2, "the earlier turn was not kept: {written}");
    assert_eq!(
        turns[0], earlier_turn,
        "the earlier turn, which made no calls this build knows of, was rewritten"
    );
    assert_eq!(turns[1]["calls"][0]["cache_write_tokens"], 3_000);
}

/// A probe runs one prompt through the turn's own configuration in a fresh
/// session of its own, with no image, held to the schema as a turn is, and
/// answers the engine's result with each call's usage.
#[test]
fn a_probe_runs_the_turn_configuration_alone_and_answers_its_calls() {
    let schemas = schema_read_lock();
    let fixture = Fixture::new("lean-probe");
    let calls = [(2, 5_000, 3_000, 8)];
    let watch = Arc::new(Watch::default());
    let supervisor = port(
        config(
            &schemas,
            &fixture,
            HARNESS,
            &generated_assessment_schema(),
            runs_answering(
                &fixture.path("attempts"),
                &[streamed("SID-PROBE", &assessment("fine", "high"), &calls)],
            ),
        ),
        &watch,
    );
    let result = supervisor
        .probe(
            "one",
            "Answer with the assessment for a print that is printing normally.",
        )
        .expect("the probe runs");
    let usage = result.calls.expect("the probe reports its calls");
    assert_eq!(usage.len(), 1);
    assert_eq!(usage[0].cache_write_tokens, Some(5_000));
    let request = watch
        .requests()
        .pop()
        .expect("the probe built a run request");
    assert_eq!(request.session.as_deref(), Some("probe-one"));
    assert!(request.images.is_empty());
    assert!(
        request.prompt[0]
            .starts_with("Answer with the assessment for a print that is printing normally.")
            && !request.prompt[0].contains("JSON Schema"),
        "{:?}",
        request.prompt
    );
}

/// The harness-only probe is the text probe's run with none of the port's
/// system text: the same session store, prompt, tools and schema, and the
/// engine's per-call usage answered back.
#[test]
fn a_probe_without_system_text_is_the_text_probe_less_the_skill() {
    let schemas = schema_read_lock();
    let fixture = Fixture::new("lean-probe-bare");
    let calls = [(2, 2_500, 0, 8)];
    let watch = Arc::new(Watch::default());
    let supervisor = port(
        config(
            &schemas,
            &fixture,
            HARNESS,
            &generated_assessment_schema(),
            runs_answering(
                &fixture.path("attempts"),
                &[
                    streamed("SID-TEXT", &assessment("fine", "high"), &calls),
                    streamed("SID-BARE", &assessment("fine", "high"), &calls),
                ],
            ),
        ),
        &watch,
    );
    let prompt = "Answer with the assessment for a print that is printing normally.";
    supervisor
        .probe("text", prompt)
        .expect("the text probe runs");
    let result = supervisor
        .probe_without_system("bare", prompt)
        .expect("the harness-only probe runs");
    let usage = result.calls.expect("the probe reports its calls");
    assert_eq!(usage[0].cache_write_tokens, Some(2_500));
    let requests = watch.requests();
    let [text, bare] = requests.as_slice() else {
        panic!("two probes built {} requests", requests.len());
    };
    assert_eq!(bare.session.as_deref(), Some("probe-bare"));
    assert!(
        text.system
            .as_deref()
            .is_some_and(|system| !system.is_empty()),
        "the text probe carried no system text"
    );
    assert!(
        bare.system.as_deref().is_none_or(str::is_empty),
        "the harness-only probe carried system text: {:?}",
        bare.system
    );
    assert_eq!(bare.prompt, text.prompt);
    assert_eq!(bare.tool_set, text.tool_set);
    assert_eq!(bare.schema, text.schema);
    assert!(bare.images.is_empty());
}

/// A probe whose harness did not complete its run measured nothing, and is
/// refused naming why rather than answered as a measurement.
#[test]
fn a_probe_the_harness_did_not_complete_is_refused() {
    let schemas = schema_read_lock();
    let fixture = Fixture::new("lean-probe-failed");
    let mut env = runs_answering(
        &fixture.path("attempts"),
        &[streamed(
            "SID-PROBE",
            &assessment("fine", "high"),
            &[(2, 1, 1, 1)],
        )],
    );
    env.push(assignment("MOCK_EXIT=3"));
    let watch = Arc::new(Watch::default());
    let supervisor = port(
        config(
            &schemas,
            &fixture,
            HARNESS,
            &generated_assessment_schema(),
            env,
        ),
        &watch,
    );
    let refused = supervisor
        .probe(
            "two",
            "Answer with the assessment for a print that is printing normally.",
        )
        .expect_err("a run the harness did not complete was answered");
    assert!(
        refused.to_string().contains("did not complete"),
        "{refused}"
    );
}

/// Issues the first run its pass and refuses every later one, as a turn that
/// returned while a run was being asked for would.
#[derive(Default)]
struct IssuesOnce(std::sync::atomic::AtomicUsize);

impl printobserver_supervisor_api::TurnAccess for IssuesOnce {
    fn issue(
        &self,
        _session: &printobserver_supervisor_api::TurnSession,
    ) -> Result<printobserver_supervisor_api::TurnPass, SupervisorError> {
        if self.0.fetch_add(1, std::sync::atomic::Ordering::SeqCst) == 0 {
            printobserver_supervisor_api::TurnPass::new(
                Some("127.0.0.1:9".parse().expect("an address")),
                "a-first-credential".to_owned(),
            )
        } else {
            Err(SupervisorError::Unavailable {
                detail: "the turn this run belongs to has already returned".to_owned(),
            })
        }
    }
}

/// A turn answering `stop` on a printing print it asked no pause for is
/// refused once in its session; when that refusal cannot be run, the turn
/// keeps the answer it had and is written down with the calls it made.
#[test]
fn a_stop_that_cannot_be_asked_again_keeps_its_answer_and_its_calls() {
    let schemas = schema_read_lock();
    let fixture = Fixture::new("lean-cannot-ask-again");
    let call = (3, 1_000, 2_000, 40);
    let mut stopping: serde_json::Value =
        serde_json::from_str(&assessment("the part came loose", "high")).expect("JSON");
    stopping["disposition"] = "stop".into();
    let stopping = stopping.to_string();
    let watch = Arc::new(Watch::default());
    let supervisor = port(
        config(
            &schemas,
            &fixture,
            HARNESS,
            &generated_assessment_schema(),
            runs_answering(
                &fixture.path("attempts"),
                &[streamed("SID-ONCE", &stopping, &[call])],
            ),
        ),
        &watch,
    );
    let print_id = PrintId::new();
    let mut request = turn(
        print_id,
        event(print_id, failure_alert(7, None)),
        Some(frame(&fixture)),
    );
    request.situation.printer_state = Some(printobserver_types::PrinterState::Printing);
    let outcome = block_on(supervisor.run_turn(request, Arc::new(IssuesOnce::default())))
        .expect("the turn keeps the answer it had");
    assert_eq!(outcome.assessment.summary, "the part came loose");
    assert_eq!(
        watch.requests().len(),
        1,
        "a run was built for a refusal no pass was issued for"
    );
    let turns = supervisor
        .recorded_turns(&print_id)
        .expect("the ledger reads");
    assert_eq!(turns.len(), 1, "the turn was not written down");
    assert_eq!(turns[0].failure, None);
    assert_eq!(turns[0].calls, vec![recorded(call)]);
}
