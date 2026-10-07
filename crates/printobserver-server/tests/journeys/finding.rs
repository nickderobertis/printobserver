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

fn prints_url(world: &World) -> String {
    world.at(&printobserver_server::operation("prints")
        .expect("the prints read is served")
        .full_path())
}

async fn prints(world: &World) -> Value {
    let (status, answer) = world.get(&prints_url(world)).await;
    assert_eq!(status, reqwest::StatusCode::OK, "{answer}");
    answer
}

fn listed(answer: &Value) -> Vec<String> {
    answer["prints"]
        .as_array()
        .unwrap_or_else(|| panic!("the prints answer carries no list: {answer}"))
        .iter()
        .map(|print| print["id"].as_str().expect("an identifier").to_owned())
        .collect()
}

/// Two reads of the prints at the same moment, while a job nothing holds a
/// print for is running, open one print for it between them.
///
/// Each reads the stored prints, then the job, and then may open one, so
/// interleaved they could each find none and each open one. The machine here
/// makes the two job reads — each taken after that read's own listing — wait for
/// one another, which is exactly that interleaving when nothing keeps the two
/// reads apart.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn two_reads_at_the_same_moment_open_one_print_for_the_job() {
    let world = World::open().await;
    world.printer.job_reads_meet(2);

    let (first, second) = tokio::join!(prints(&world), prints(&world));

    let after = prints(&world).await;
    assert_eq!(
        listed(&after).len(),
        1,
        "two reads at once each opened a print for the one job: {after}"
    );
    assert_eq!(first["active"], after["active"], "{first}");
    assert_eq!(second["active"], after["active"], "{second}");
    world.server.stop().await;
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

/// The print the listing names with this identifier.
fn print_listed<'a>(answer: &'a Value, id: &Value) -> &'a Value {
    answer["prints"]
        .as_array()
        .and_then(|prints| prints.iter().find(|print| &print["id"] == id))
        .unwrap_or_else(|| panic!("the listing carries no print {id}: {answer}"))
}

/// The instant one field of a listed print names.
fn instant(print: &Value, field: &str) -> Timestamp {
    print[field]
        .as_str()
        .unwrap_or_else(|| panic!("the print carries no {field}: {print}"))
        .parse()
        .expect("an instant")
}

/// Read the prints with the machine running `file` in `state`, `running`
/// seconds into it.
async fn prints_while(
    world: &World,
    file: &str,
    state: PrinterState,
    running: Option<i64>,
) -> Value {
    world.printer.runs_job(file, state, running);
    prints(world).await
}

/// Post one `Obico` alert to the ingress and wait for its handling to finish.
async fn alert(world: &World, body: &Value) {
    let mut completions = world.server.completions();
    let status = world
        .client
        .post(format!("{}?token={SECRET}", world.server.ingress_url()))
        .header(reqwest::header::CONTENT_TYPE, "application/json")
        .body(body.to_string())
        .send()
        .await
        .expect("the ingress answers")
        .status();
    assert_eq!(status, reqwest::StatusCode::ACCEPTED);
    completions.changed().await.expect("the handling finishes");
}

/// An alert about `file`, which `Obico` says started at `started_at`.
fn alert_started(obico_print_id: i64, file: &str, started_at: Timestamp, image: &str) -> Value {
    let mut body = failure_alert(obico_print_id, image);
    body["print"]["filename"] = json!(file);
    body["print"]["started_at"] = json!(printobserver_core::unix_seconds(started_at));
    body
}

/// The running times `OctoPrint` reports for one job read at `t`, `t + 60`,
/// `t + 600` and `t + 630`, paused from `t + 30` to `t + 600`: a pause counts
/// while it lasts and comes back out at the resume.
///
/// This world's reads are moments apart on the wall clock, so what it carries
/// faithfully is the running times; the start the record keeps moving later at
/// the resume is the core journeys', which run on a clock they advance.
const THROUGH_A_LONG_PAUSE: [(PrinterState, i64); 4] = [
    (PrinterState::Printing, 900),
    (PrinterState::Paused, 960),
    (PrinterState::Paused, 1500),
    (PrinterState::Printing, 960),
];

/// Read the prints through [`THROUGH_A_LONG_PAUSE`], requiring every read to
/// name one print active and the listing to hold that print alone; answers
/// the last read.
async fn read_through_a_long_pause(world: &World) -> Value {
    let mut print = None;
    let mut last = Value::Null;
    for (state, running) in THROUGH_A_LONG_PAUSE {
        last = prints_while(world, RUNNING, state.clone(), Some(running)).await;
        let active = print.get_or_insert_with(|| last["active"].clone());
        assert_eq!(
            &last["active"], active,
            "{state:?} at {running} s split it: {last}"
        );
        assert_eq!(listed(&last).len(), 1, "{last}");
    }
    last
}

/// A print paused for longer than the tolerance, with the pause read, and then
/// resumed, keeps one record, and records the printing time the resume
/// reported.
#[tokio::test(flavor = "multi_thread")]
async fn a_long_pause_the_reads_see_then_a_resume_keeps_one_record() {
    let world = World::open().await;

    let after = read_through_a_long_pause(&world).await;

    let held = &after["prints"][0];
    assert!(held.get("ended_at").is_none(), "{after}");
    assert_eq!(held["job_print_time_s"], json!(960), "{after}");
    world.server.stop().await;
}

/// A pause and a resume both between two reads keep one record, whether the
/// next read finds the job printing again or paused once more.
#[tokio::test(flavor = "multi_thread")]
async fn a_pause_and_resume_between_two_reads_keeps_one_record() {
    let world = World::open().await;
    let print =
        prints_while(&world, RUNNING, PrinterState::Printing, Some(900)).await["active"].clone();

    for (state, running) in [
        (PrinterState::Printing, 950),
        (PrinterState::Paused, 950 + 86_400),
        (PrinterState::Printing, 1000),
    ] {
        let read = prints_while(&world, RUNNING, state.clone(), Some(running)).await;
        assert_eq!(
            read["active"], print,
            "{state:?} at {running} s split it: {read}"
        );
        assert_eq!(listed(&read).len(), 1, "{read}");
    }
    world.server.stop().await;
}

/// The sequence `tests/real-prints/spaghetti-floating-slab` recorded — an
/// alert opens the print, the supervisor reads it printing, the print is
/// cancelled at the machine, a job of the same file is started, and the
/// supervisor restarts — leaves two records rather than one adopted at every
/// start: the first ended as replaced, and the running job a print of its own.
#[tokio::test(flavor = "multi_thread")]
async fn the_recorded_cancellation_then_a_same_file_job_yields_two_records() {
    const FILE: &str = "spaghetti-test.gcode";
    let host = image_host(snapshot_bytes()).await;
    let world = World::open().await;
    let started: Timestamp = "2026-09-27T21:25:47.573451042Z"
        .parse()
        .expect("the recorded start");
    world
        .printer
        .runs_job(FILE, PrinterState::Printing, Some(1170));
    alert(&world, &alert_started(1, FILE, started, &host.url())).await;
    let first = prints(&world).await["active"].clone();
    let first_id: PrintId = first
        .as_str()
        .expect("an identifier")
        .parse()
        .expect("a print identifier");
    let (_, status) = world
        .get(&world.operation_url("/v1/prints/{print_id}/status", first_id))
        .await;
    assert_eq!(status["print"]["provider_print_id"], json!(1), "{status}");
    assert_eq!(status["print"]["job_print_time_s"], json!(1170), "{status}");

    // Cancelled at the machine, and the same file started again half a minute
    // before the supervisor next comes up.
    world
        .printer
        .runs_job(FILE, PrinterState::Printing, Some(30));
    let world = world.restart().await;

    let reconciled = world.server.reconciliation();
    assert_eq!(reconciled.closed, vec![first_id], "{reconciled:?}");
    assert!(
        !reconciled.adopted.contains(&first_id),
        "the start adopted the cancelled print again: {reconciled:?}"
    );
    let after = prints(&world).await;
    assert_eq!(listed(&after).len(), 2, "{after}");
    let stale = print_listed(&after, &first);
    assert_eq!(
        stale["end_reason"],
        json!(printobserver_core::listing::REPLACED_REASON),
        "{after}"
    );
    let fresh = &after["active"];
    assert_ne!(fresh, &first, "{after}");
    let running = print_listed(&after, fresh);
    assert!(running.get("ended_at").is_none(), "{after}");
    assert_eq!(running["file_name"], json!(FILE), "{after}");
    assert_eq!(running["job_print_time_s"], json!(30), "{after}");
    world.server.stop().await;
}

/// A job read paused, then cancelled and replaced by the same file before the
/// next read, yields two records where the earlier job had been read printing
/// longer than the later one has, by more than the tolerance.
#[tokio::test(flavor = "multi_thread")]
async fn a_paused_job_replaced_before_the_next_read_yields_two_records() {
    let world = World::open().await;
    let earlier =
        prints_while(&world, RUNNING, PrinterState::Printing, Some(1800)).await["active"].clone();
    let paused = prints_while(&world, RUNNING, PrinterState::Paused, Some(1860)).await;
    assert_eq!(paused["active"], earlier, "{paused}");

    let after = prints_while(&world, RUNNING, PrinterState::Printing, Some(200)).await;

    assert_ne!(after["active"], earlier, "{after}");
    assert_eq!(listed(&after).len(), 2, "{after}");
    assert_eq!(
        print_listed(&after, &earlier)["end_reason"],
        json!(printobserver_core::listing::REPLACED_REASON),
        "{after}"
    );
    world.server.stop().await;
}

/// What the reads cannot tell from one job paused between them is kept as one
/// record: a later job that has printed for as long as the earlier one was
/// last read printing, less the tolerance, or longer; and a later job
/// replacing one that was only ever read paused.
#[tokio::test(flavor = "multi_thread")]
async fn what_the_reads_cannot_tell_from_a_pause_keeps_one_record() {
    let world = World::open().await;
    let print =
        prints_while(&world, RUNNING, PrinterState::Printing, Some(100)).await["active"].clone();
    let after = prints_while(&world, RUNNING, PrinterState::Printing, Some(300)).await;
    assert_eq!(after["active"], print, "{after}");
    assert_eq!(listed(&after).len(), 1, "{after}");
    world.server.stop().await;

    let world = World::open().await;
    let print =
        prints_while(&world, RUNNING, PrinterState::Paused, Some(1800)).await["active"].clone();
    let after = prints_while(&world, RUNNING, PrinterState::Printing, Some(10)).await;
    assert_eq!(after["active"], print, "{after}");
    assert_eq!(listed(&after).len(), 1, "{after}");
    world.server.stop().await;
}

/// A record written with no sighting — before sightings were recorded — is
/// adopted by its file name, and so is a job reporting no running time.
#[tokio::test(flavor = "multi_thread")]
async fn a_legacy_record_and_a_job_with_no_running_time_are_matched_by_file_name() {
    let world = World::open().await;
    let legacy = world
        .stores
        .prints
        .open_print(None, Some(RUNNING.to_owned()))
        .await
        .expect("a print opens");
    assert_eq!(legacy.job_started_at, None);

    let adopted = prints_while(&world, RUNNING, PrinterState::Printing, Some(5000)).await;
    assert_eq!(adopted["active"], json!(legacy.id.to_string()), "{adopted}");
    assert_eq!(listed(&adopted).len(), 1, "{adopted}");

    let unknown = prints_while(&world, RUNNING, PrinterState::Printing, None).await;
    assert_eq!(unknown["active"], json!(legacy.id.to_string()), "{unknown}");
    assert_eq!(listed(&unknown).len(), 1, "{unknown}");
    world.server.stop().await;
}

/// An alert about the job a print was adopted for, after that job was read
/// through a long pause and resumed, is that print's: the alert carries the
/// job's own start, which is the pause earlier than any start a read after the
/// resume puts it at — and earlier is no objection.
#[tokio::test(flavor = "multi_thread")]
async fn an_alert_for_the_same_job_after_a_long_pause_attaches_to_its_print() {
    let host = image_host(snapshot_bytes()).await;
    let world = World::open().await;
    let resumed = read_through_a_long_pause(&world).await;
    let print = resumed["active"].clone();
    let recorded = instant(print_listed(&resumed, &print), "job_started_at");
    let own_start = recorded.plus_seconds(-570).expect("an instant");

    alert(
        &world,
        &alert_started(5150, RUNNING, own_start, &host.url()),
    )
    .await;

    let after = prints(&world).await;
    assert_eq!(
        listed(&after).len(),
        1,
        "the alert opened a second print: {after}"
    );
    assert_eq!(
        print_listed(&after, &print)["provider_print_id"],
        json!(5150)
    );
    world.server.stop().await;
}

/// An alert about a later job of the adopted print's file opens a print of its
/// own, and the next read, finding that later job, closes the earlier print as
/// replaced.
#[tokio::test(flavor = "multi_thread")]
async fn an_alert_for_a_later_same_file_job_opens_its_own_print() {
    let host = image_host(snapshot_bytes()).await;
    let world = World::open().await;
    let earlier =
        prints_while(&world, RUNNING, PrinterState::Printing, Some(900)).await["active"].clone();

    world
        .printer
        .runs_job(RUNNING, PrinterState::Printing, Some(30));
    let later_start = Timestamp::now().plus_seconds(-30).expect("an instant");
    alert(
        &world,
        &alert_started(5150, RUNNING, later_start, &host.url()),
    )
    .await;

    let after = prints(&world).await;
    assert_eq!(listed(&after).len(), 2, "{after}");
    let later = &after["active"];
    assert_ne!(later, &earlier, "{after}");
    assert_eq!(
        print_listed(&after, later)["provider_print_id"],
        json!(5150)
    );
    let stale = print_listed(&after, &earlier);
    assert!(stale.get("provider_print_id").is_none(), "{after}");
    assert_eq!(
        stale["end_reason"],
        json!(printobserver_core::listing::REPLACED_REASON),
        "{after}"
    );
    world.server.stop().await;
}

/// A listing or a status read that finds the machine idle closes the open
/// print through the close-out — the print ended with the state it reached —
/// and one that cannot read the machine closes nothing.
#[tokio::test(flavor = "multi_thread")]
async fn a_read_that_finds_the_machine_idle_closes_the_print_and_an_unread_one_does_not() {
    for read in ["listing", "status"] {
        let world = World::open().await;
        let print_id = world.open_print().await;
        let status_url = world.operation_url("/v1/prints/{print_id}/status", print_id);

        world.printer.unreadable(true);
        let _ = prints(&world).await;
        let _ = world.get(&status_url).await;
        assert!(
            world
                .stores
                .prints
                .print(print_id)
                .await
                .expect("the print reads")
                .is_some_and(|held| held.ended_at.is_none()),
            "{read}: a machine nothing could read closed the print"
        );
        world.printer.unreadable(false);

        world.printer.in_state(PrinterState::Operational);
        let answered = if read == "listing" {
            print_listed(&prints(&world).await, &json!(print_id.to_string())).clone()
        } else {
            world.get(&status_url).await.1["print"].clone()
        };
        assert_eq!(
            answered["end_reason"],
            json!("the print reached Operational"),
            "{read}: {answered}"
        );
        assert_eq!(
            answered["state"],
            json!("operational"),
            "{read}: {answered}"
        );
        world.server.stop().await;
    }
}

/// A status or a context read can be the first to find a later job of its
/// print's file. It closes the print as replaced through the close-out every
/// other read takes — the bounded change it carried expired, and the value it
/// replaced put back — and answers the print as that left it; the next listing
/// gives the running job a print of its own.
#[tokio::test(flavor = "multi_thread")]
async fn a_status_or_context_read_first_to_find_a_later_job_closes_the_print_as_replaced() {
    for (operation, print_at) in [
        ("/v1/prints/{print_id}/status", "/print"),
        ("/v1/prints/{print_id}/context", "/context/print"),
    ] {
        let world = World::open().await;
        let earlier =
            prints_while(&world, RUNNING, PrinterState::Printing, Some(1800)).await["active"]
                .clone();
        let earlier_id: PrintId = earlier
            .as_str()
            .expect("an identifier")
            .parse()
            .expect("a print identifier");
        let (status, adjusted) = world
            .post(
                &world.operation_url(
                    "/v1/prints/{print_id}/actions/set_feedrate_factor",
                    earlier_id,
                ),
                &json!({
                    "reason": "a journey is asking",
                    "actor": "operator",
                    "factor": 1.2,
                    "duration_s": 3600,
                }),
            )
            .await;
        assert_eq!(status, reqwest::StatusCode::OK, "{adjusted}");

        world
            .printer
            .runs_job(RUNNING, PrinterState::Printing, Some(200));
        let (_, read) = world.get(&world.operation_url(operation, earlier_id)).await;

        let answered = read.pointer(print_at).expect("the read carries its print");
        assert_eq!(
            answered["end_reason"],
            json!(printobserver_core::listing::REPLACED_REASON),
            "{operation} answered the print as it was before the read: {read}"
        );
        assert!(answered.get("ended_at").is_some(), "{read}");
        assert!(
            world
                .stores
                .actions
                .active_interventions(earlier_id)
                .await
                .expect("the interventions read")
                .is_empty(),
            "{operation} closed the print and left its bounded change running"
        );
        assert_eq!(
            world
                .printer
                .value_of(printobserver_printer_api::Adjustable::Feedrate),
            Some(1.0),
            "{operation} closed the print without putting its feedrate back"
        );
        let after = prints(&world).await;
        assert_eq!(listed(&after).len(), 2, "{after}");
        assert_ne!(after["active"], earlier, "{after}");
        world.server.stop().await;
    }
}
