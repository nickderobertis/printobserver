//! Finding the print to act on, over the real surface.
//!
//! The prints read is the one operation that takes no identifier, because it is
//! where a caller gets one. What it has to do beyond listing is **adopt** the
//! job the machine is running when nothing holds a print for it — once, and
//! without asking the machine for anything but what it is doing — so that a
//! print started at the printer can be read and acted on before any alert about
//! it has arrived. And the first alert about that job has to land on the print
//! that was found, not open a second one beside it.

use printobserver_obico::ObicoFailureAlertPayload;
use printobserver_printer_api::PrinterState;
use printobserver_types::serde_json::{Value, json};
use printobserver_types::{EventPayload as _, PrintId, Timestamp};

use crate::http_host::image_host;
use crate::ingress::snapshot_bytes;
use crate::world::{SECRET, World, failure_alert};

/// The file the machine these journeys drive reports it is running.
const RUNNING: &str = "benchy.gcode";

/// The prints read's own URL.
fn prints_url(world: &World) -> String {
    world.at(&printobserver_server::operation("prints")
        .expect("the prints read is served")
        .full_path())
}

/// One prints read, which must be answered.
async fn prints(world: &World) -> Value {
    let (status, answer) = world.get(&prints_url(world)).await;
    assert_eq!(status, reqwest::StatusCode::OK, "{answer}");
    answer
}

/// Every identifier a prints answer lists, in the order it lists them.
fn listed(answer: &Value) -> Vec<String> {
    answer["prints"]
        .as_array()
        .unwrap_or_else(|| panic!("the prints answer carries no list: {answer}"))
        .iter()
        .map(|print| print["id"].as_str().expect("an identifier").to_owned())
        .collect()
}

/// A running job nothing holds a print for is adopted by the first read and
/// named active, the listing is newest first with ended prints in it, and a
/// second read of the same job opens nothing and moves nothing at the machine.
#[tokio::test(flavor = "multi_thread")]
async fn a_running_job_is_adopted_once_and_listed_newest_first() {
    let world = World::open().await;
    let finished = world
        .stores
        .prints
        .open_print(Some(17), Some("finished.gcode".to_owned()))
        .await
        .expect("a print opens");
    world
        .stores
        .prints
        .end_print(
            finished.id,
            PrinterState::Operational,
            Timestamp::now(),
            "it finished".to_owned(),
        )
        .await
        .expect("the print ends");

    let first = prints(&world).await;

    let ids = listed(&first);
    assert_eq!(ids.len(), 2, "the read did not open one print: {first}");
    assert_eq!(ids[1], finished.id.to_string(), "not newest first: {first}");
    let adopted = &first["prints"][0];
    assert_eq!(first["active"], adopted["id"], "{first}");
    assert_eq!(adopted["file_name"], json!(RUNNING), "{first}");
    assert!(
        adopted.get("provider_print_id").is_none(),
        "an adopted print carries an identifier nothing reported: {first}"
    );
    assert!(adopted.get("ended_at").is_none(), "{first}");
    assert_eq!(adopted["state"], json!("printing"), "{first}");

    let second = prints(&world).await;
    assert_eq!(
        second, first,
        "a second read of the same job answered something else"
    );
    assert_eq!(
        world.printer.calls(),
        Vec::new(),
        "reading the prints asked the machine to do something"
    );

    let active: PrintId = first["active"]
        .as_str()
        .expect("an identifier")
        .parse()
        .expect("a print identifier");
    let (status, context) = world
        .get(&world.operation_url("/v1/prints/{print_id}/context", active))
        .await;
    assert_eq!(
        status,
        reqwest::StatusCode::OK,
        "the adopted print cannot be read by the identifier the listing gave: {context}"
    );
    world.server.stop().await;
}

/// A print already open under the job's file is active — the most recently
/// opened of them — and the read writes nothing.
#[tokio::test(flavor = "multi_thread")]
async fn an_open_print_of_the_running_file_is_active_and_nothing_is_opened() {
    let world = World::open().await;
    let older = world.open_print().await;
    let newer = world.open_print().await;

    let answer = prints(&world).await;

    assert_eq!(answer["active"], json!(newer.to_string()), "{answer}");
    assert_eq!(
        listed(&answer),
        vec![newer.to_string(), older.to_string()],
        "the read opened a print beside the ones open for the job: {answer}"
    );
    world.server.stop().await;
}

/// A machine with no job in progress names nothing active and opens nothing.
#[tokio::test(flavor = "multi_thread")]
async fn a_machine_with_no_job_in_progress_names_nothing_active() {
    let world = World::open().await;
    world.printer.in_state(PrinterState::Operational);

    let answer = prints(&world).await;

    assert_eq!(answer, json!({ "prints": [] }), "{answer}");
    world.server.stop().await;
}

/// A machine that cannot be read still answers the stored prints, with nothing
/// active, rather than failing the read.
#[tokio::test(flavor = "multi_thread")]
async fn a_printer_that_cannot_be_read_still_answers_the_stored_prints() {
    let world = World::open().await;
    let held = world.open_print().await;
    world.printer.unreadable(true);

    let answer = prints(&world).await;

    assert_eq!(listed(&answer), vec![held.to_string()], "{answer}");
    assert!(
        answer.get("active").is_none(),
        "a machine nothing could read had a job named active: {answer}"
    );

    world.printer.unreadable(false);
    let recovered = prints(&world).await;
    assert_eq!(
        recovered["active"],
        json!(held.to_string()),
        "once the machine answers again the job is named: {recovered}"
    );
    world.server.stop().await;
}

/// The first `Obico` alert about an adopted job, posted to the real ingress,
/// attaches its identifier to the adopted print and is handled against it; the
/// listing afterwards shows one print for the job.
#[tokio::test(flavor = "multi_thread")]
async fn an_alert_about_an_adopted_job_attaches_to_the_adopted_print() {
    let host = image_host(snapshot_bytes()).await;
    let world = World::open().await;
    let adopted = prints(&world).await["active"].clone();
    assert!(adopted.is_string(), "nothing was adopted");

    let mut completions = world.server.completions();
    let status = world
        .client
        .post(format!("{}?token={SECRET}", world.server.ingress_url()))
        .header(reqwest::header::CONTENT_TYPE, "application/json")
        .body(failure_alert(5150, &host.url()).to_string())
        .send()
        .await
        .expect("the ingress answers")
        .status();
    assert_eq!(status, reqwest::StatusCode::ACCEPTED);
    completions.changed().await.expect("the handling finishes");

    let after = prints(&world).await;
    assert_eq!(
        listed(&after),
        vec![adopted.as_str().expect("an identifier").to_owned()],
        "the alert opened a second print for the job: {after}"
    );
    assert_eq!(
        after["prints"][0]["provider_print_id"],
        json!(5150),
        "{after}"
    );
    assert_eq!(after["active"], adopted, "{after}");

    let print_id: PrintId = adopted
        .as_str()
        .expect("an identifier")
        .parse()
        .expect("a print identifier");
    let (status, history) = world
        .get(&world.operation_url("/v1/prints/{print_id}/history", print_id))
        .await;
    assert_eq!(status, reqwest::StatusCode::OK, "{history}");
    let kind = printobserver_types::serde_json::to_value(ObicoFailureAlertPayload::kind())
        .expect("a kind renders");
    assert!(
        history["events"]
            .as_array()
            .expect("a history is a list")
            .iter()
            .any(|event| event["kind"] == kind),
        "the alert was not handled against the adopted print: {history}"
    );
    assert_eq!(
        world.agent.turns().first().map(|turn| turn.print_id),
        Some(print_id),
        "the turn the alert prompted is about another print"
    );
    world.server.stop().await;
}
