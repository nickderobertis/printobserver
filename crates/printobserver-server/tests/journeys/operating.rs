//! Every operation the declared list names, proven by its own effect.
//!
//! The walk here is over [`OPERATIONS`] rather than over a chosen few, and what
//! it asserts of a mutating operation is four things a status code alone would
//! not reach:
//!
//! 1. An accepted request has **the effect that operation names** — read back
//!    out of the machine's own state where the machine reports it, and out of
//!    what the machine received where it does not. An operation served as a
//!    successful no-op fails here, which is what asserting the effect is for.
//! 2. A request the policy rejects answers **the policy's own rejection** — its
//!    reason, the value asked for and the range allowed where it carries one —
//!    and leaves the machine's state exactly as it was.
//! 3. A request carrying **no reason** is refused, with nothing reaching the
//!    machine and nothing written into the record.
//! 4. An adjustment **applies the optional duration**: the intervention it
//!    opens carries an expiry derived from the duration asked for, and one with
//!    no duration opens no intervention at all.
//!
//! Every operation gets a world of its own, because several of them move the
//! machine into a state the next one is not valid from — cancelling in
//! particular — and a walk whose order was load-bearing would be proving the
//! order rather than the operations.

use printobserver_types::{ActionKind, Adjustable, PrinterState, serde_json::Value, serde_json::json};

use printobserver_server::{Effect, OPERATIONS, Operation};

use crate::printer::Call;
use crate::world::World;

/// How long a bounded adjustment stands for, in a journey that bounds one.
const DURATION_S: i64 = 120;

/// What one operation asks for, and what asking for it should do.
struct Asked {
    /// A body the policy accepts.
    accepted: Value,
    /// A body the policy rejects, of this operation's own kind.
    rejected: Value,
    /// The state the machine has to be in for the accepted body to be valid.
    from: PrinterState,
    /// What the machine should have been asked once the accepted body is taken.
    call: Option<Call>,
    /// The adjustable this operation changes, when it changes one.
    adjustable: Option<(Adjustable, f64)>,
}

/// One body carrying a reason and an actor, plus whatever else is given.
fn body(actor: Value, extra: &[(&str, Value)]) -> Value {
    let mut body = json!({ "reason": "a journey is asking", "actor": actor });
    let object = body.as_object_mut().expect("the body is an object");
    for (name, value) in extra {
        object.insert((*name).to_owned(), value.clone());
    }
    body
}

/// A manifest a start is bounded by.
fn manifest() -> Value {
    printobserver_types::serde_json::to_value(
        <printobserver_types::JobManifest as printobserver_types::contract::Sample>::sample_full(),
    )
    .expect("a manifest renders")
}

/// What one mutating operation is asked, and what it should do.
///
/// The rejected body is of the operation's own kind: a value outside the
/// effective bounds for an adjustment, and an actor class the envelope does not
/// grant the action to — or a state it is not valid from — for the rest.
fn asked(kind: ActionKind) -> Asked {
    let operator = json!("operator");
    // The safety envelope these journeys run under grants the agent `pause`,
    // `set_feedrate_factor` and `set_fan_percent` and nothing else, so an agent
    // asking for anything else is a rejection of that request's own kind.
    let agent = json!({ "agent": { "session_name": "watch-1" } });
    match kind {
        ActionKind::Pause => Asked {
            accepted: body(operator, &[]),
            // Pausing is not valid from a machine that is not printing.
            rejected: body(json!("operator"), &[]),
            from: PrinterState::Printing,
            call: Some(Call::Pause),
            adjustable: None,
        },
        ActionKind::Resume => Asked {
            accepted: body(operator, &[]),
            rejected: body(agent, &[]),
            from: PrinterState::Paused,
            call: Some(Call::Resume),
            adjustable: None,
        },
        ActionKind::Cancel => Asked {
            accepted: body(operator, &[]),
            rejected: body(agent, &[]),
            from: PrinterState::Printing,
            call: Some(Call::Cancel),
            adjustable: None,
        },
        ActionKind::StartPrint => Asked {
            accepted: body(
                operator,
                &[
                    ("file_name", json!("benchy.gcode")),
                    ("manifest", manifest()),
                ],
            ),
            rejected: body(
                agent,
                &[
                    ("file_name", json!("benchy.gcode")),
                    ("manifest", manifest()),
                ],
            ),
            from: PrinterState::Operational,
            call: Some(Call::Start(
                printobserver_types::FileName::new("benchy.gcode").expect("a file name"),
            )),
            adjustable: None,
        },
        ActionKind::SetFeedrateFactor => Asked {
            accepted: body(operator, &[("factor", json!(1.2))]),
            rejected: body(json!("operator"), &[("factor", json!(9.0))]),
            from: PrinterState::Printing,
            call: Some(Call::Feedrate(1.2)),
            adjustable: Some((Adjustable::Feedrate, 1.2)),
        },
        ActionKind::SetFlowrateFactor => Asked {
            accepted: body(operator, &[("factor", json!(1.05))]),
            rejected: body(json!("operator"), &[("factor", json!(4.0))]),
            from: PrinterState::Printing,
            call: Some(Call::Flowrate(1.05)),
            adjustable: Some((Adjustable::Flowrate, 1.05)),
        },
        ActionKind::SetToolTargetC => Asked {
            accepted: body(operator, &[("tool", json!(0)), ("target_c", json!(220.0))]),
            rejected: body(
                json!("operator"),
                &[("tool", json!(0)), ("target_c", json!(900.0))],
            ),
            from: PrinterState::Printing,
            call: Some(Call::ToolTarget(0, 220.0)),
            adjustable: Some((Adjustable::ToolTarget { tool: 0 }, 220.0)),
        },
        ActionKind::SetBedTargetC => Asked {
            accepted: body(operator, &[("target_c", json!(65.0))]),
            rejected: body(json!("operator"), &[("target_c", json!(400.0))]),
            from: PrinterState::Printing,
            call: Some(Call::BedTarget(65.0)),
            adjustable: Some((Adjustable::BedTarget, 65.0)),
        },
        ActionKind::SetFanPercent => Asked {
            accepted: body(operator, &[("percent", json!(80.0))]),
            rejected: body(json!("operator"), &[("percent", json!(500.0))]),
            from: PrinterState::Printing,
            call: Some(Call::Fan(80.0)),
            adjustable: Some((Adjustable::Fan, 80.0)),
        },
        ActionKind::AcknowledgeFailure => Asked {
            accepted: body(
                operator,
                &[
                    ("event_id", json!(printobserver_types::EventId::new())),
                    ("disposition", json!("stop")),
                ],
            ),
            rejected: body(
                agent,
                &[
                    ("event_id", json!(printobserver_types::EventId::new())),
                    ("disposition", json!("continue")),
                ],
            ),
            from: PrinterState::Printing,
            // Stopping is what an acknowledgement asks of the machine, and
            // cancelling is how a print is stopped.
            call: Some(Call::Cancel),
            adjustable: None,
        },
    }
}

/// Every mutating operation has the effect it names, and only when it may.
#[tokio::test(flavor = "multi_thread")]
async fn every_mutating_operation_has_its_own_effect_and_its_own_rejection() {
    for operation in OPERATIONS {
        let Effect::Mutating(kind) = operation.effect else {
            continue;
        };
        let plan = asked(kind);

        rejects(&operation, &plan).await;
        refuses_a_request_with_no_reason(&operation).await;
        accepts(&operation, &plan).await;
    }
}

/// The accepted body has the effect the operation names.
async fn accepts(operation: &Operation, plan: &Asked) {
    let world = World::open().await;
    world.printer.in_state(plan.from.clone());
    let print_id = world.open_print().await;
    let url = world.operation_url(&operation.full_path(), print_id);

    let (status, answer) = world.post(&url, &plan.accepted).await;

    assert_eq!(
        status,
        reqwest::StatusCode::OK,
        "`{}` was not accepted: {answer}",
        operation.name
    );
    assert_eq!(
        answer["record"]["decision"], json!("accepted"),
        "`{}` answered a decision that is not acceptance: {answer}",
        operation.name
    );
    if let Some(call) = &plan.call {
        assert!(
            world.printer.calls().contains(call),
            "`{}` was accepted and the machine was never asked {call:?}; it received {:?}",
            operation.name,
            world.printer.calls()
        );
    }
    if let Some((adjustable, value)) = plan.adjustable {
        assert_eq!(
            world.printer.value_of(adjustable),
            Some(value),
            "`{}` was accepted and {adjustable} did not move to {value}",
            operation.name
        );
    }
    world.server.stop().await;
}

/// The rejected body answers the policy's own rejection and moves nothing.
async fn rejects(operation: &Operation, plan: &Asked) {
    let world = World::open().await;
    world.printer.in_state(plan.from.clone());
    let print_id = world.open_print().await;
    // Pausing is rejected from a state it is not valid from, which is a
    // rejection of its own kind and needs the machine in that state.
    if operation.name == "pause" {
        world.printer.in_state(PrinterState::Operational);
    }
    let before = world.printer.snapshot();
    world.printer.forget();
    let url = world.operation_url(&operation.full_path(), print_id);

    let (status, answer) = world.post(&url, &plan.rejected).await;

    assert_eq!(
        status,
        reqwest::StatusCode::CONFLICT,
        "`{}` was not rejected: {answer}",
        operation.name
    );
    let rejection = &answer["record"]["decision"]["rejected"];
    assert!(
        !rejection.is_null(),
        "`{}` answered no rejection the caller can act on: {answer}",
        operation.name
    );
    if let Some((adjustable, _)) = plan.adjustable {
        let bounds = &rejection["out_of_bounds"];
        assert_eq!(
            bounds["adjustable"],
            json!(adjustable.to_string()),
            "`{}` was rejected without naming what was out of bounds: {answer}",
            operation.name
        );
        assert!(
            !bounds["requested"].is_null() && !bounds["allowed"].is_null(),
            "`{}` was rejected without the value asked for and the range allowed: {answer}",
            operation.name
        );
    }
    assert_eq!(
        world.printer.snapshot(),
        before,
        "`{}` was rejected and the machine moved anyway",
        operation.name
    );
    assert!(
        world.printer.calls().is_empty(),
        "`{}` was rejected and the machine was asked {:?}",
        operation.name,
        world.printer.calls()
    );
    world.server.stop().await;
}

/// A request carrying no reason reaches neither the machine nor the record.
async fn refuses_a_request_with_no_reason(operation: &Operation) {
    let world = World::open().await;
    let print_id = world.open_print().await;
    let history = world.operation_url(
        &printobserver_server::operation("history")
            .expect("history is served")
            .full_path(),
        print_id,
    );
    let (_, before) = world.get(&history).await;
    let url = world.operation_url(&operation.full_path(), print_id);

    let mut reasonless = asked(
        operation
            .action_kind()
            .expect("this is a mutating operation"),
    )
    .accepted;
    reasonless
        .as_object_mut()
        .expect("the body is an object")
        .remove("reason");

    let (status, answer) = world.post(&url, &reasonless).await;

    assert_eq!(
        status,
        reqwest::StatusCode::BAD_REQUEST,
        "`{}` took a request carrying no reason: {answer}",
        operation.name
    );
    assert!(
        answer["error"]
            .as_str()
            .unwrap_or_default()
            .contains("reason"),
        "`{}` refused a reasonless request without saying so: {answer}",
        operation.name
    );
    assert!(
        world.printer.calls().is_empty(),
        "`{}` carried a reasonless request to the machine: {:?}",
        operation.name,
        world.printer.calls()
    );
    let (_, after) = world.get(&history).await;
    assert_eq!(
        before, after,
        "`{}` wrote a record for a request it refused",
        operation.name
    );
    world.server.stop().await;
}

/// Every adjustment applies the duration it is given, and opens nothing without
/// one.
#[tokio::test(flavor = "multi_thread")]
async fn every_adjustment_applies_the_duration_it_is_given() {
    for operation in OPERATIONS {
        let Some(kind) = operation.action_kind() else {
            continue;
        };
        let plan = asked(kind);
        let Some((adjustable, _)) = plan.adjustable else {
            continue;
        };

        let world = World::open().await;
        let print_id = world.open_print().await;
        let url = world.operation_url(&operation.full_path(), print_id);

        let mut bounded = plan.accepted.clone();
        bounded
            .as_object_mut()
            .expect("the body is an object")
            .insert("duration_s".to_owned(), json!(DURATION_S));
        let (status, answer) = world.post(&url, &bounded).await;
        assert_eq!(status, reqwest::StatusCode::OK, "{answer}");

        let intervention = &answer["intervention"];
        assert!(
            !intervention.is_null(),
            "`{}` was given a duration and opened no intervention: {answer}",
            operation.name
        );
        assert_eq!(
            intervention["adjustable"],
            json!(adjustable.to_string()),
            "`{}` opened an intervention about something else: {answer}",
            operation.name
        );
        let applied = instant(&intervention["applied_at"]);
        let expires = instant(&intervention["expires_at"]);
        assert_eq!(
            expires - applied,
            DURATION_S,
            "`{}` opened an intervention whose expiry is not the duration asked for: {answer}",
            operation.name
        );

        let (status, answer) = world.post(&url, &plan.accepted).await;
        assert_eq!(status, reqwest::StatusCode::OK, "{answer}");
        assert!(
            answer["intervention"].is_null(),
            "`{}` was given no duration and opened an intervention anyway: {answer}",
            operation.name
        );
        world.server.stop().await;
    }
}

/// Every read answers the record it names, read back out of the store.
#[tokio::test(flavor = "multi_thread")]
async fn every_read_answers_the_record_it_names() {
    let world = World::open().await;
    let print_id = world.open_print().await;
    let manifest = manifest();

    let (status, written) = world
        .put(&world.operation_url(&path("manifest_set"), print_id), &manifest)
        .await;
    assert_eq!(status, reqwest::StatusCode::OK, "{written}");

    let (status, read) = world
        .get(&world.operation_url(&path("manifest_get"), print_id))
        .await;
    assert_eq!(status, reqwest::StatusCode::OK);
    assert_eq!(
        read["manifest"], manifest,
        "the manifest read back is not the one written"
    );

    let (status, status_answer) = world
        .get(&world.operation_url(&path("status"), print_id))
        .await;
    assert_eq!(status, reqwest::StatusCode::OK);
    assert_eq!(status_answer["print"]["id"], json!(print_id.to_string()));
    assert_eq!(status_answer["printer"]["connection"], json!("printing"));

    let (status, context) = world
        .get(&world.operation_url(&path("context"), print_id))
        .await;
    assert_eq!(status, reqwest::StatusCode::OK);
    assert_eq!(
        context["context"]["manifest"], manifest,
        "the context does not carry the manifest that was written"
    );

    // One action, so the history has something of this print's own in it.
    let (status, _) = world
        .post(
            &world.operation_url(&path("pause"), print_id),
            &body(json!("operator"), &[]),
        )
        .await;
    assert_eq!(status, reqwest::StatusCode::OK);

    let (status, history) = world
        .get(&world.operation_url(&path("history"), print_id))
        .await;
    assert_eq!(status, reqwest::StatusCode::OK);
    assert!(
        history["events"].is_array(),
        "the history answered no events at all: {history}"
    );

    world.server.stop().await;
}

/// A history read asking for more than the store's own maximum is refused.
#[tokio::test(flavor = "multi_thread")]
async fn a_history_read_above_the_stores_maximum_is_refused() {
    let world = World::open().await;
    let print_id = world.open_print().await;
    let url = format!(
        "{}?limit={}",
        world.operation_url(&path("history"), print_id),
        printobserver_store_api::MAX_HISTORY_LIMIT + 1
    );

    let (status, answer) = world.get(&url).await;

    assert_eq!(status, reqwest::StatusCode::BAD_REQUEST, "{answer}");
    world.server.stop().await;
}

/// A read of a print nothing holds is refused naming it.
#[tokio::test(flavor = "multi_thread")]
async fn a_read_of_a_print_nothing_holds_is_refused() {
    let world = World::open().await;
    let absent = printobserver_types::PrintId::new();

    let (status, answer) = world
        .get(&world.operation_url(&path("status"), absent))
        .await;

    assert_eq!(status, reqwest::StatusCode::NOT_FOUND, "{answer}");
    assert!(
        answer["error"]
            .as_str()
            .unwrap_or_default()
            .contains(&absent.to_string()),
        "the refusal does not name the print: {answer}"
    );
    world.server.stop().await;
}

/// One operation's whole path, by its name.
fn path(name: &str) -> String {
    printobserver_server::operation(name)
        .unwrap_or_else(|| panic!("`{name}` is served"))
        .full_path()
}

/// One instant of an answer, as whole seconds after the epoch.
fn instant(value: &Value) -> i64 {
    let text = value.as_str().expect("an instant is a string");
    let parsed: printobserver_types::Timestamp =
        text.parse().expect("an instant is RFC 3339");
    parsed.as_utc().timestamp()
}
