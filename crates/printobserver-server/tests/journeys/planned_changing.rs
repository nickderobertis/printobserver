//! A planned colour change, reached through the real webhook ingress.
//!
//! The server is the real composition over the durable store, its printer
//! watch polling a recording machine that holds the committed two-change file
//! in `PrusaSlicer` 2.9.6's syntax. `Obico`'s `filament_change` notification is
//! posted to the real ingress exactly as its plugin posts it, while the machine
//! reports the job paused at the first change's offset; what the server made
//! of it is read back over its own API: the print's history, its status and
//! its context, a refused pause, and the turns the agent was handed.

use core::time::Duration;

use printobserver_printer_api::PrinterState;
use printobserver_types::serde_json::{Value, json};
use printobserver_types::{PrintId, Timestamp};

use crate::agent::StandInAgent;
use crate::ledger_restarts::{FILE, filament_change, look_at_the_job};
use crate::printer::RecordingPrinter;
use crate::world::{SECRET, World};

/// The committed file: a change to red at layer 3 and to blue at layer 5.
const TWO_COLOUR: &str =
    include_str!("../../../printobserver-core/tests/fixtures/planned-changes/two-colour.gcode");

/// How long a journey waits for something the server does on its own.
const PATIENCE: Duration = Duration::from_secs(20);

/// The offset of the first byte of the `nth` `M600` line.
fn change_offset(nth: usize) -> i64 {
    let mut at = 0;
    let mut seen = 0;
    for line in TWO_COLOUR.split_inclusive('\n') {
        if line.trim_end() == "M600" {
            if seen == nth {
                return i64::try_from(at).expect("a small offset");
            }
            seen += 1;
        }
        at += line.len();
    }
    panic!("the file has no M600 number {nth}");
}

/// The print's events of one kind, oldest first, read through the API.
async fn history_of(world: &World, print_id: PrintId, kind: &str) -> Vec<Value> {
    let (status, answer) = world
        .get(&format!(
            "{}?limit=500",
            world.operation_url("/v1/prints/{print_id}/history", print_id)
        ))
        .await;
    assert_eq!(status, reqwest::StatusCode::OK, "{answer}");
    let mut events: Vec<Value> = answer["events"]
        .as_array()
        .expect("a history answers its events")
        .iter()
        .filter(|event| event["kind"] == json!(kind))
        .cloned()
        .collect();
    events.reverse();
    events
}

/// Wait until the print's history holds an event of one kind.
async fn recorded(world: &World, print_id: PrintId, kind: &str) -> Vec<Value> {
    let deadline = tokio::time::Instant::now() + PATIENCE;
    loop {
        let events = history_of(world, print_id, kind).await;
        if !events.is_empty() {
            return events;
        }
        assert!(
            tokio::time::Instant::now() < deadline,
            "the history never held {kind}"
        );
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
}

/// Post `Obico`'s `filament_change` notification to the ingress, as its plugin
/// does, and wait until the server has handled it.
async fn notify_a_filament_change(world: &World, job_started: i64) {
    let mut completions = world.server.completions();
    let status = reqwest::Client::new()
        .post(format!("{}?token={SECRET}", world.server.ingress_url()))
        .header(reqwest::header::CONTENT_TYPE, "application/json")
        .body(filament_change(job_started).to_string())
        .send()
        .await
        .expect("the ingress answers")
        .status();
    assert_eq!(status, reqwest::StatusCode::ACCEPTED);
    completions.changed().await.expect("the handling finishes");
}

/// `Obico`'s notification posted while the machine is paused at the first
/// change reaches that change once, by the signal, and starts no turn; posted
/// again it changes nothing. While the machine waits the status answers the
/// plan and the change waited at, the context names it, and a pause is
/// refused naming it. Once the job moves past it the server records it
/// resumed and checks it once, naming the colour now expected, its layer and
/// the colour before.
#[tokio::test(flavor = "multi_thread")]
async fn obicos_notification_at_a_planned_change_holds_the_print_until_it_carries_on() {
    let printer = RecordingPrinter::printing();
    printer.holds_gcode(FILE, TWO_COLOUR.as_bytes());
    printer.runs_job(FILE, PrinterState::Printing, Some(900));
    printer.at_byte(change_offset(0) - 200);
    let agent = StandInAgent::new();
    let world = World::tuned(
        printer,
        agent,
        |_| {},
        |config| config.printer_poll = Duration::from_millis(200),
        |stores| stores,
    )
    .await;
    let job_started = printobserver_core::unix_seconds(Timestamp::now()) - 900;
    let listing = look_at_the_job(&world).await;
    let print_id: PrintId = listing["active"]
        .as_str()
        .and_then(|id| id.parse().ok())
        .expect("the running job is a print");
    let context = world
        .get(&world.operation_url("/v1/prints/{print_id}/context", print_id))
        .await
        .1;
    assert_eq!(
        context["context"]["slicer"]["next_change"]["index"],
        json!(0),
        "{context}"
    );

    world
        .printer
        .runs_job(FILE, PrinterState::Paused, Some(900));
    world.printer.at_byte(change_offset(0));
    // Several polls read the job paused at the change before the signal
    // arrives, as on a slow host: the signal still reaches it.
    tokio::time::sleep(Duration::from_secs(1)).await;
    notify_a_filament_change(&world, job_started).await;
    notify_a_filament_change(&world, job_started).await;

    let reached = history_of(&world, print_id, "filament_change_planned_reached").await;
    assert_eq!(reached.len(), 1, "{reached:#?}");
    let payload = &reached[0]["payload"];
    assert_eq!(payload["trigger"], json!("signal"));
    assert_eq!(payload["change"]["color"], json!("#FF0000"));
    assert_eq!(payload["change"]["layer"], json!(3));
    assert_eq!(payload["previous_color"], json!("#FFFFFF"));
    let signalled = history_of(&world, print_id, "filament_change_signalled").await;
    assert!(
        signalled
            .iter()
            .all(|event| event["payload"]["planned_change"] == json!(0)),
        "{signalled:#?}"
    );
    assert!(
        world.agent.turns().is_empty(),
        "a turn ran on a planned change"
    );

    the_wait_is_answered_and_refuses_a_pause(&world, print_id).await;

    world
        .printer
        .runs_job(FILE, PrinterState::Printing, Some(900));
    world.printer.at_byte(change_offset(0) + 4096 + 100);
    let resumed = recorded(&world, print_id, "filament_change_planned_resumed").await;
    assert_eq!(resumed.len(), 1, "{resumed:#?}");
    let deadline = tokio::time::Instant::now() + PATIENCE;
    while world.agent.turns().is_empty() {
        assert!(
            tokio::time::Instant::now() < deadline,
            "carrying on was never checked"
        );
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    let turns = world.agent.turns();
    assert_eq!(turns.len(), 1, "{turns:#?}");
    let noticed = turns[0].situation.noticed.clone().unwrap_or_default();
    for named in ["#FF0000", "layer 3", "#FFFFFF"] {
        assert!(noticed.contains(named), "{noticed} does not name {named}");
    }
    world.server.stop().await;
}

/// While the machine waits at the first change, the status answers the plan
/// and the change waited at, the context names it, and an operator's pause is
/// refused naming it.
async fn the_wait_is_answered_and_refuses_a_pause(world: &World, print_id: PrintId) {
    let (status, answer) = world
        .get(&world.operation_url("/v1/prints/{print_id}/status", print_id))
        .await;
    assert_eq!(status, reqwest::StatusCode::OK, "{answer}");
    let planned = &answer["planned_changes"];
    assert_eq!(planned["waiting"], json!(0), "{answer}");
    assert_eq!(planned["expected_color"], json!("#FFFFFF"), "{answer}");
    assert_eq!(planned["changes"].as_array().map(Vec::len), Some(2));
    assert_eq!(planned["segments"].as_array().map(Vec::len), Some(3));
    let context = world
        .get(&world.operation_url("/v1/prints/{print_id}/context", print_id))
        .await
        .1;
    assert_eq!(
        context["context"]["slicer"]["change_waiting"]["index"],
        json!(0),
        "{context}"
    );
    let (_, paused) = world
        .post(
            &world.url(&format!("/prints/{print_id}/actions/pause")),
            &json!({
                "reason": "the operator wants it stopped",
                "actor": "operator",
            }),
        )
        .await;
    assert_eq!(
        paused["record"]["decision"]["rejected"]["planned_change_waiting"]["change_index"],
        json!(0),
        "{paused}"
    );
}
