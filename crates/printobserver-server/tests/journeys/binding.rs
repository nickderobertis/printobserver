//! Every request is the identity its credential authenticated.
//!
//! A supervision turn is issued a credential of its own, bound to the agent
//! class, its session and its print; the operator's credential is the
//! operator. These journeys hold a real turn open — the stand-in agent is
//! issued its credential the way the adapter is, and waits — and drive the
//! real router with that credential and with the operator's, reading what each
//! request left behind off the far side of every port: the machine, the
//! print's history and the action record.

use printobserver_core::store::Stores;
use printobserver_supervisor_api::SupervisorError;
use printobserver_types::PrintId;
use printobserver_types::serde_json::{Value, json};

use crate::agent::{Pass, StandInAgent, session_of};
use crate::authenticating::history;
use crate::printer::RecordingPrinter;
use crate::world::{SECRET, World, failure_alert, presenting, set};

/// The provider identifier the journeys' print carries, which an alert about
/// it names.
const OBICO_PRINT: i64 = 4211;

/// Post one failure alert about the journeys' print, which starts a turn.
async fn alert(world: &World) {
    let status = reqwest::Client::new()
        .post(format!("{}?token={SECRET}", world.server.ingress_url()))
        .header(reqwest::header::CONTENT_TYPE, "application/json")
        .body(failure_alert(OBICO_PRINT, "http://127.0.0.1:1/frame.jpg").to_string())
        .send()
        .await
        .expect("the ingress answers")
        .status();
    assert_eq!(status, reqwest::StatusCode::ACCEPTED);
}

/// A world with a print, and one turn about it held open: what the turn was
/// issued.
async fn held_turn(world: &World) -> (PrintId, Pass) {
    let print_id = world.open_print().await;
    world.agent.hold_turns();
    alert(world).await;
    let pass = world
        .agent
        .issued(1)
        .await
        .into_iter()
        .next()
        .expect("one pass");
    (print_id, pass)
}

/// The body of one action, claiming one actor.
fn claiming(actor: &Value, extra: &[(&str, Value)]) -> Value {
    let mut body = json!({
        "reason": "a journey is asking as somebody",
        "actor": actor,
    });
    for (key, value) in extra {
        body[*key] = value.clone();
    }
    body
}

/// A manifest a start or a manifest write carries.
fn manifest() -> Value {
    printobserver_types::serde_json::to_value(
        <printobserver_core::JobManifest as printobserver_types::contract::Sample>::sample_full(),
    )
    .expect("a manifest renders")
}

/// The actor document of one agent session.
fn agent(session_name: &str) -> Value {
    json!({ "agent": { "session_name": session_name } })
}

/// Send one request and read its status and its error text.
async fn sent(request: reqwest::RequestBuilder) -> (reqwest::StatusCode, Value) {
    let response = request.send().await.expect("the server answers");
    let status = response.status();
    let body: Value = response.json().await.expect("the answer is JSON");
    (status, body)
}

/// One action's URL on one print.
fn action_url(world: &World, print_id: PrintId, action: &str) -> String {
    world.url(&format!("/prints/{print_id}/actions/{action}"))
}

/// How many events one print's history holds: every action requested, decided
/// on and carried out is written there.
async fn record_of(stores: &Stores, print_id: PrintId) -> usize {
    history(stores, print_id).await
}

/// Assert one answer is a refusal naming who authenticated and what was asked.
fn assert_forbidden(answer: &(reqwest::StatusCode, Value), what: &str, naming: &[&str]) {
    assert_eq!(
        answer.0,
        reqwest::StatusCode::FORBIDDEN,
        "{what} was not refused as forbidden: {}",
        answer.1
    );
    let said = answer.1["error"].as_str().unwrap_or_default();
    for named in naming {
        assert!(
            said.contains(named),
            "the refusal of {what} does not name {named:?}: {said}"
        );
    }
}

/// A turn's credential is admitted during its turn for its own print — its
/// reads, the listing beside them and an action its grants allow — and every
/// claim that is not the turn, every other print and both operator-only writes
/// are refused before anything is decided or recorded.
#[tokio::test(flavor = "multi_thread")]
async fn a_turn_credential_is_its_own_session_on_its_own_print_and_nothing_else() {
    let world = World::open().await;
    let (print_id, pass) = held_turn(&world).await;
    let other = world.open_print().await;
    let turn = presenting(&pass.credential);
    let session = session_of(print_id);
    assert_eq!(pass.session_name, session);
    assert_eq!(
        pass.server.as_deref(),
        Some(format!("http://{}", world.server.address()).as_str()),
        "the turn was not told the address this server bound"
    );

    // Its own print's reads, and the listing.
    for path in [
        format!("/prints/{print_id}/status"),
        format!("/prints/{print_id}/context"),
        format!("/prints/{print_id}/history"),
        format!("/prints/{print_id}/manifest"),
        "/prints".to_owned(),
    ] {
        let (status, body) = sent(turn.get(world.url(&path))).await;
        assert_eq!(status, reqwest::StatusCode::OK, "{path}: {body}");
    }
    world.printer.forget();
    let before = record_of(&world.stores, print_id).await;

    // Every claim that is not this turn's own session.
    for (claimed, named) in [
        (json!("operator"), "the operator"),
        (json!("system"), "the system"),
        (agent("print-another-session"), "print-another-session"),
    ] {
        let answer = sent(
            turn.post(action_url(&world, print_id, "cancel"))
                .json(&claiming(&claimed, &[])),
        )
        .await;
        assert_forbidden(
            &answer,
            &format!("a turn claiming {claimed}"),
            &[&session, named],
        );
    }
    // Another print, read or acted on.
    let answer = sent(turn.get(world.url(&format!("/prints/{other}/status")))).await;
    assert_forbidden(
        &answer,
        "a turn reading another print",
        &[&session, &other.to_string()],
    );
    let answer = sent(
        turn.post(action_url(&world, other, "pause"))
            .json(&claiming(&agent(&session), &[])),
    )
    .await;
    assert_forbidden(
        &answer,
        "a turn acting on another print",
        &[&session, &other.to_string()],
    );
    // The two writes that are the operator's alone.
    let answer = sent(
        turn.post(action_url(&world, print_id, "start_print"))
            .json(&claiming(
                &agent(&session),
                &[
                    ("file_name", json!("benchy.gcode")),
                    ("manifest", manifest()),
                ],
            )),
    )
    .await;
    assert_forbidden(
        &answer,
        "a turn starting a print",
        &[&session, "start-print"],
    );
    let answer = sent(
        turn.put(world.url(&format!("/prints/{print_id}/manifest")))
            .json(&json!({"reason": "widening my own bounds", "manifest": manifest()})),
    )
    .await;
    assert_forbidden(
        &answer,
        "a turn replacing a manifest",
        &[&session, "manifest-set"],
    );

    assert!(
        world.printer.calls().is_empty(),
        "a refused request reached the machine: {:?}",
        world.printer.calls()
    );
    assert_eq!(
        record_of(&world.stores, print_id).await,
        before,
        "a refused request was decided on or recorded"
    );

    // And an action its grants allow, as itself, on its own print.
    let (status, body) = sent(
        turn.post(action_url(&world, print_id, "pause"))
            .json(&claiming(&agent(&session), &[])),
    )
    .await;
    assert_eq!(status, reqwest::StatusCode::OK, "{body}");
    assert_eq!(
        body["record"]["request"]["actor"],
        agent(&session),
        "{body}"
    );

    world.agent.release_turns();
    world.server.stop().await;
}

/// The ticket's own request: an agent holding its turn's credential asks to
/// cancel the print as the operator, which its grants exclude.
#[tokio::test(flavor = "multi_thread")]
async fn a_turn_asking_to_cancel_as_the_operator_is_refused() {
    let world = World::open().await;
    let (print_id, pass) = held_turn(&world).await;
    world.printer.forget();

    let answer = sent(
        presenting(&pass.credential)
            .post(action_url(&world, print_id, "cancel"))
            .json(&claiming(&json!("operator"), &[])),
    )
    .await;

    assert_forbidden(
        &answer,
        "a turn cancelling as the operator",
        &["the operator", &session_of(print_id)],
    );
    assert!(world.printer.calls().is_empty());
    world.agent.release_turns();
    world.server.stop().await;
}

/// The operator's credential is the operator: claiming the agent, or the
/// system, is refused rather than overridden.
#[tokio::test(flavor = "multi_thread")]
async fn the_operators_credential_claiming_anybody_else_is_refused() {
    let world = World::open().await;
    let print_id = world.open_print().await;
    let before = record_of(&world.stores, print_id).await;

    for (claimed, named) in [
        (agent(&session_of(print_id)), "the agent"),
        (json!("system"), "the system"),
    ] {
        let answer = sent(
            world
                .client
                .post(action_url(&world, print_id, "pause"))
                .json(&claiming(&claimed, &[])),
        )
        .await;
        assert_forbidden(
            &answer,
            &format!("the operator claiming {claimed}"),
            &["the operator", named],
        );
    }
    assert!(world.printer.calls().is_empty());
    assert_eq!(record_of(&world.stores, print_id).await, before);

    let (status, body) = sent(
        world
            .client
            .post(action_url(&world, print_id, "pause"))
            .json(&claiming(&json!("operator"), &[])),
    )
    .await;
    assert_eq!(status, reqwest::StatusCode::OK, "{body}");
    world.server.stop().await;
}

/// Whether one server admits one credential, asked through the listing.
async fn admitted(world: &World, credential: &str) -> bool {
    presenting(credential)
        .get(world.url("/prints"))
        .send()
        .await
        .expect("the server answers")
        .status()
        != reqwest::StatusCode::UNAUTHORIZED
}

/// A turn's credential stops being admitted when its turn returns and after a
/// restart, two turns are issued two credentials, and none of them is in any
/// file under the state directory.
#[tokio::test(flavor = "multi_thread")]
async fn a_turn_credential_ends_with_its_turn_and_is_written_nowhere() {
    let world = World::open().await;
    let (_, first) = held_turn(&world).await;
    assert!(admitted(&world, &first.credential).await);
    let mut completions = world.server.completions();
    world.agent.release_turns();
    completions.changed().await.expect("the handling finishes");
    assert!(
        !admitted(&world, &first.credential).await,
        "a turn's credential was admitted after its turn returned"
    );

    world.agent.hold_turns();
    alert(&world).await;
    let second = world.agent.issued(2).await[1].clone();
    assert_ne!(
        first.credential, second.credential,
        "two turns shared a credential"
    );
    assert!(admitted(&world, &second.credential).await);

    // What is on disk after a turn ran and another is running.
    let state = world.state_dir();
    for credential in [&first.credential, &second.credential] {
        let carrying = crate::world::files_carrying(&state, credential);
        assert!(
            carrying.is_empty(),
            "a turn's credential is written under the state directory: {carrying:?}"
        );
    }

    // A restart admits nothing the server before it minted.
    world.agent.release_turns();
    let world = world.restart().await;
    assert!(
        !admitted(&world, &second.credential).await,
        "a turn's credential was admitted after a restart"
    );
    world.server.stop().await;
}

/// The agent's minimum interval holds every machine-changing action a turn's
/// credential asks for, and claiming the operator to step around it is refused.
#[tokio::test(flavor = "multi_thread")]
async fn the_agent_interval_holds_every_turn_whatever_it_claims() {
    let world = World::configured(
        RecordingPrinter::printing(),
        StandInAgent::new(),
        |document| {
            set(
                document,
                "safety.agent_min_interval_s",
                toml::Value::Integer(30),
            );
        },
    )
    .await;
    let (print_id, pass) = held_turn(&world).await;
    let turn = presenting(&pass.credential);
    let session = session_of(print_id);

    let (status, body) = sent(
        turn.post(action_url(&world, print_id, "set_fan_percent"))
            .json(&claiming(&agent(&session), &[("percent", json!(40.0))])),
    )
    .await;
    assert_eq!(status, reqwest::StatusCode::OK, "{body}");

    let (status, body) = sent(
        turn.post(action_url(&world, print_id, "set_feedrate_factor"))
            .json(&claiming(&agent(&session), &[("factor", json!(1.1))])),
    )
    .await;
    assert_eq!(status, reqwest::StatusCode::CONFLICT, "{body}");
    assert!(
        body["record"]["decision"]["rejected"]
            .get("min_interval_not_elapsed")
            .is_some(),
        "the second change was not held by the agent's interval: {body}"
    );

    let answer = sent(
        turn.post(action_url(&world, print_id, "set_feedrate_factor"))
            .json(&claiming(&json!("operator"), &[("factor", json!(1.1))])),
    )
    .await;
    assert_forbidden(
        &answer,
        "a turn stepping around its interval as the operator",
        &[&session, "the operator"],
    );
    world.agent.release_turns();
    world.server.stop().await;
}

/// A turn whose run fails, or times out, has its credential revoked when it
/// returns exactly as a turn that answered does.
#[tokio::test(flavor = "multi_thread")]
async fn a_turn_that_fails_or_times_out_is_revoked_when_it_returns() {
    for (what, error) in [
        ("timed out", SupervisorError::TimedOut),
        (
            "failed",
            SupervisorError::Unavailable {
                detail: "the harness could not be reached".to_owned(),
            },
        ),
    ] {
        let world = World::open().await;
        world.agent.fail_turns_with(error);
        let (_, pass) = held_turn(&world).await;
        assert!(
            admitted(&world, &pass.credential).await,
            "the credential of a turn that {what} was not admitted while it ran"
        );

        let mut completions = world.server.completions();
        world.agent.release_turns();
        completions.changed().await.expect("the handling finishes");

        assert!(
            !admitted(&world, &pass.credential).await,
            "the credential of a turn that {what} was admitted after it returned"
        );
        world.server.stop().await;
    }
}

/// A turn moved into a second session — as the adapter moves one whose
/// session the harness refused to continue — is issued that session's
/// credential, and the first session's is refused from that moment: a turn
/// holds one live credential, for the session it is running in.
#[tokio::test(flavor = "multi_thread")]
async fn a_turn_moved_to_a_second_session_holds_only_that_sessions_credential() {
    let world = World::open().await;
    world.agent.move_turns_to_a_second_session();
    let print_id = world.open_print().await;
    world.agent.hold_turns();
    alert(&world).await;
    let passes = world.agent.issued(2).await;
    let (first, second) = (&passes[0], &passes[1]);
    assert_eq!(first.session_name, session_of(print_id));
    assert_eq!(second.session_name, format!("{}-2", session_of(print_id)));

    assert!(
        !admitted(&world, &first.credential).await,
        "the first session's credential is still admitted once the second is issued"
    );
    let turn = presenting(&second.credential);
    let answer = sent(
        turn.post(action_url(&world, print_id, "pause"))
            .json(&claiming(&agent(&first.session_name), &[])),
    )
    .await;
    assert_forbidden(
        &answer,
        "the second session claiming the first",
        &[&first.session_name, &second.session_name],
    );
    let (status, body) = sent(
        turn.post(action_url(&world, print_id, "pause"))
            .json(&claiming(&agent(&second.session_name), &[])),
    )
    .await;
    assert_eq!(status, reqwest::StatusCode::OK, "{body}");
    world.agent.release_turns();
    world.server.stop().await;
}
