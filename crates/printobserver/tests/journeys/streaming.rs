//! `printobserver watch` and `printobserver events`, against a real supervisor.
//!
//! Every journey runs the built program as a subprocess against a supervisor
//! of its own — the built program run as `server`, which the journeys about a
//! stream that loses its server stop and start again. The events a journey is
//! about are put in the record beside it through a second connection to its
//! store — the one way to record an observation with a `received_at` of the
//! journey's choosing — and it is then asked for a look at another print,
//! which it records through its own store: that write is what wakes a waiting
//! read, as any write the supervisor makes does.
//!
//! What a stream printed is held to the record itself: the events in scope, in
//! the order the store stored them, read back through the store's own ordered
//! read once the stream has ended.

use std::io::{BufRead as _, BufReader};
use std::process::{Child, Command, ExitStatus, Stdio};
use std::sync::mpsc;
use std::time::{Duration, Instant};

use printobserver_core::records::PrinterState;
use printobserver_core::store::{EventDraft, EventStore as _, StoredQuery};
use printobserver_core::{
    PeriodicObservationPayload, PrintEndedPayload, PrintOutcome, system_source,
};
use printobserver_types::contract::Sample as _;
use printobserver_types::serde_json::{self, Value, json};
use printobserver_types::{
    EventBody, EventKind, EventPayload as _, EventRecord, PrintId, Timestamp,
};

use super::running;
use crate::proxy::Proxy;
use crate::standalone::Standalone;
use crate::world::{CREDENTIAL, STOOD_IN, World};

/// The kind of the routine observation every journey here records.
const OBSERVATION: &str = "periodic_observation";

/// A kind no crate declares, standing for anything that is not routine.
const NOTABLE: &str = "streaming_notable";

/// A second kind no crate declares, which the filters leave out.
const OTHER: &str = "streaming_other";

/// How long a journey waits for a stream to print what it is waiting for.
const PATIENCE: Duration = Duration::from_secs(30);

/// One observation of a print, received at the instant given.
fn observation(print_id: Option<PrintId>, received_at: Timestamp) -> EventDraft {
    EventDraft {
        print_id,
        source: system_source(),
        received_at,
        body: EventBody::of(&PeriodicObservationPayload::sample_full())
            .expect("an observation renders"),
        raw: None,
    }
}

/// One event of a kind nothing declares, on a print or on none.
fn notable(print_id: Option<PrintId>) -> EventDraft {
    EventDraft {
        print_id,
        source: system_source(),
        received_at: Timestamp::now(),
        body: EventBody {
            kind: EventKind::new(NOTABLE).expect("a kind name"),
            payload: json!({ "said": "something happened" }),
        },
        raw: None,
    }
}

/// One event of a kind nothing declares, received at the instant given.
fn notable_at(print_id: Option<PrintId>, received_at: Timestamp) -> EventDraft {
    EventDraft {
        received_at,
        ..notable(print_id)
    }
}

/// One event of another kind nothing declares, which the filters leave out.
fn other_kind(print_id: Option<PrintId>) -> EventDraft {
    EventDraft {
        body: EventBody {
            kind: EventKind::new(OTHER).expect("a kind name"),
            payload: json!({ "said": "something else happened" }),
        },
        ..notable(print_id)
    }
}

/// One print's end.
fn ended(print_id: PrintId) -> EventDraft {
    ended_as(print_id, PrintOutcome::Completed)
}

/// One print's end, with the outcome given.
fn ended_as(print_id: PrintId, outcome: PrintOutcome) -> EventDraft {
    EventDraft {
        print_id: Some(print_id),
        source: system_source(),
        received_at: Timestamp::now(),
        body: EventBody::of(&PrintEndedPayload {
            outcome,
            state: PrinterState::Operational,
            reason: "it ran to its end".to_owned(),
            last_position: None,
        })
        .expect("an end renders"),
        raw: None,
    }
}

/// One identifier, read as the print it names.
fn print(id: &str) -> PrintId {
    id.parse().expect("a print identifier")
}

/// An instant some seconds after another.
fn after(at: Timestamp, seconds: i64) -> Timestamp {
    at.plus_seconds(seconds).expect("an instant")
}

/// The events in a print's scope, in the order the store stored them, from
/// that print's first event.
fn in_scope(own: &Standalone, print_id: PrintId) -> Vec<EventRecord> {
    let store = own.store();
    let mut found = Vec::new();
    let mut cursor = None;
    loop {
        let page = printobserver_core::block_on(store.stored_after(StoredQuery {
            after: cursor,
            print_id: Some(print_id),
            limit: Some(1_000),
        }))
        .expect("the record reads");
        let Some(last) = page.last() else {
            return found;
        };
        cursor = Some(last.id);
        found.extend(page);
    }
}

/// One stream, running.
struct Watching {
    /// The program, running.
    child: Child,
    /// Each line it printed, as it printed it.
    lines: mpsc::Receiver<String>,
    /// Every line read so far.
    seen: Vec<String>,
    /// Everything it said on standard error.
    said: std::sync::Arc<std::sync::Mutex<String>>,
}

/// Start `watch` with these arguments against a journey's own supervisor.
fn watch(own: &Standalone, arguments: &[&str]) -> Watching {
    watch_with(arguments, &own.client)
}

/// Start `watch` configured by the file given.
fn watch_with(arguments: &[&str], config: &std::path::Path) -> Watching {
    let mut child = Command::new(running::program())
        .arg("watch")
        .args(arguments)
        .arg("--config")
        .arg(config)
        .env_remove("PRINTOBSERVER_SERVER")
        .env_remove("PRINTOBSERVER_CREDENTIAL")
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("the watch starts");
    let mut err = child.stderr.take().expect("the watch's error output");
    let said = std::sync::Arc::new(std::sync::Mutex::new(String::new()));
    let keeping = std::sync::Arc::clone(&said);
    std::thread::spawn(move || {
        let mut text = String::new();
        let _ = std::io::Read::read_to_string(&mut err, &mut text);
        keeping
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .push_str(&text);
    });
    let out = child.stdout.take().expect("the watch's output");
    let (sending, lines) = mpsc::channel();
    std::thread::spawn(move || {
        for line in BufReader::new(out).lines().map_while(Result::ok) {
            if sending.send(line).is_err() {
                return;
            }
        }
    });
    Watching {
        child,
        lines,
        seen: Vec::new(),
        said,
    }
}

impl Watching {
    /// Read lines until what was read satisfies `done`.
    ///
    /// # Panics
    ///
    /// Panics when it does not within [`PATIENCE`], naming what was read.
    fn until(&mut self, what: &str, done: impl Fn(&[String]) -> bool) {
        let deadline = Instant::now() + PATIENCE;
        while !done(&self.seen) {
            let left = deadline.saturating_duration_since(Instant::now());
            match self.lines.recv_timeout(left) {
                Ok(line) => self.seen.push(line),
                Err(error) => panic!(
                    "the stream never printed {what} ({error}): {:#?}",
                    self.seen
                ),
            }
        }
    }

    /// Wait for the stream to end, and answer every line, the exit and what
    /// it said on standard error.
    fn finished_saying(mut self) -> (Vec<String>, ExitStatus, String) {
        let (lines, status) = self.wait_out();
        let said = self.said();
        (lines, status, said)
    }

    /// Wait for the stream to end, and answer every line and the exit.
    fn finished(mut self) -> (Vec<String>, ExitStatus) {
        self.wait_out()
    }

    /// Wait for the stream to end, and answer every line and the exit.
    fn wait_out(&mut self) -> (Vec<String>, ExitStatus) {
        let deadline = Instant::now() + PATIENCE;
        loop {
            if let Some(status) = self.child.try_wait().expect("the watch is waited on") {
                // Whatever it printed before it exited is still in the pipe.
                while let Ok(line) = self.lines.recv_timeout(Duration::from_millis(500)) {
                    self.seen.push(line);
                }
                return (std::mem::take(&mut self.seen), status);
            }
            if let Ok(line) = self.lines.recv_timeout(Duration::from_millis(100)) {
                self.seen.push(line);
            }
            assert!(
                Instant::now() < deadline,
                "the stream did not end: {:#?}",
                self.seen
            );
        }
    }

    /// Everything the stream has said on standard error, once it has ended.
    fn said(&self) -> String {
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            let said = self
                .said
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .clone();
            if !said.is_empty() || Instant::now() > deadline {
                return said;
            }
            std::thread::sleep(Duration::from_millis(50));
        }
    }

    /// Ask the stream to stop with one signal, as a terminal or a service
    /// manager does.
    #[cfg(unix)]
    fn signal(&self, which: &str) {
        let sent = Command::new("kill")
            .arg(format!("-{which}"))
            .arg(self.child.id().to_string())
            .status()
            .expect("the signal is sent");
        assert!(sent.success(), "the signal was not sent");
    }
}

impl Drop for Watching {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

/// Every line, as a document.
fn documents(lines: &[String]) -> Vec<Value> {
    lines
        .iter()
        .map(|line| {
            serde_json::from_str(line)
                .unwrap_or_else(|error| panic!("`{line}` is not one JSON object: {error}"))
        })
        .collect()
}

/// The events a stream printed, by identifier, in the order printed.
fn printed_ids(lines: &[Value]) -> Vec<String> {
    lines
        .iter()
        .filter(|line| line.get("id").is_some())
        .map(|line| line["id"].as_str().expect("an event id").to_owned())
        .collect()
}

/// The stream's last line, which is its ending, and that it is its only one.
fn ending(lines: &[Value]) -> Value {
    let endings: Vec<&Value> = lines
        .iter()
        .filter(|line| line.get("watch_ended").is_some())
        .collect();
    assert_eq!(
        endings.len(),
        1,
        "a stream ended other than once: {lines:#?}"
    );
    let last = lines.last().expect("the stream printed something");
    assert!(
        last.get("watch_ended").is_some(),
        "the ending was not the last line: {lines:#?}"
    );
    last["watch_ended"].clone()
}

/// The events of a print's scope that pass a filter, up to and including the
/// print's end, in the order stored.
fn expected(
    own: &Standalone,
    print_id: PrintId,
    passes: impl Fn(&EventRecord) -> bool,
) -> Vec<String> {
    let mut found = Vec::new();
    for event in in_scope(own, print_id) {
        let is_the_end =
            event.kind().as_str() == PrintEndedPayload::KIND && event.print_id == Some(print_id);
        if passes(&event) {
            found.push(event.id.to_string());
        }
        if is_the_end {
            return found;
        }
    }
    panic!("the record holds no end of print {print_id}")
}

/// The events a stream printed are exactly those of the print's scope that
/// pass, in the order stored, and the stream ended with the print, once.
fn held_to_the_record(
    own: &Standalone,
    print_id: PrintId,
    lines: &[Value],
    status: ExitStatus,
    passes: impl Fn(&EventRecord) -> bool,
) {
    assert_eq!(status.code(), Some(0), "{lines:#?}");
    let ended = ending(lines);
    assert_eq!(ended["reason"], json!("print_ended"), "{lines:#?}");
    assert_eq!(
        printed_ids(lines),
        expected(own, print_id, passes),
        "the stream did not print the print's passing events, once each, in the order stored"
    );
}

/// One answer of a journey's own supervisor, read through this program.
fn read(world: &World, own: &Standalone, arguments: &[&str]) -> Value {
    let mut given: Vec<String> = arguments.iter().map(|word| (*word).to_owned()).collect();
    given.extend([
        "--json".to_owned(),
        "--config".to_owned(),
        own.client.display().to_string(),
    ]);
    let ran = running::with(world, &given, &[]);
    assert_eq!(ran.code, Some(0), "{arguments:?} failed: {}", ran.said());
    serde_json::from_str(&ran.out)
        .unwrap_or_else(|error| panic!("{arguments:?} answered {error}: {}", ran.out))
}

/// The supervisor's own `history --json` and `events --json` for one print,
/// by event identifier.
fn as_served(
    world: &World,
    own: &Standalone,
    print_id: &str,
) -> std::collections::BTreeMap<String, Value> {
    let mut served = std::collections::BTreeMap::new();
    let history = read(
        world,
        own,
        &["history", "--print-id", print_id, "--limit", "1000"],
    );
    let mut cursor: Option<String> = None;
    loop {
        let mut asked = vec!["events", "--print-id", print_id, "--limit", "500"];
        let from;
        if let Some(after) = &cursor {
            from = after.clone();
            asked.extend(["--after", from.as_str()]);
        }
        let page = read(world, own, &asked);
        let events = page["events"].as_array().cloned().unwrap_or_default();
        if events.is_empty() {
            break;
        }
        for event in events {
            served.insert(event["id"].as_str().expect("an id").to_owned(), event);
        }
        cursor = page["cursor"].as_str().map(str::to_owned);
    }
    // A print's own events are held to what its history prints; the printer's,
    // which no print's history carries, to what the events read prints.
    for event in history["events"].as_array().cloned().unwrap_or_default() {
        served.insert(event["id"].as_str().expect("an id").to_owned(), event);
    }
    served
}

/// A stream over a print carries that print's events and the printer's, in
/// the order stored, each exactly as `history --json` prints it, and no other
/// print's; it prints the print's end and then ends with it, once, exit 0.
#[test]
fn a_watch_streams_a_prints_events_and_the_printers_and_ends_with_the_print() {
    let world = World::open(STOOD_IN);
    let own = Standalone::beside(&world, "streaming");
    let watched = own.print_id.clone();
    let mut stream = watch(&own, &["--json", "--print-id", &watched]);
    std::thread::sleep(Duration::from_millis(500));

    let first = own.record(vec![
        notable(Some(print(&watched))),
        notable(None),
        notable(Some(print(&own.elsewhere))),
        observation(Some(print(&watched)), Timestamp::now()),
    ]);
    own.nudge(&world);
    let last_of_batch = first[3].id.to_string();
    stream.until("the first batch", |seen| {
        seen.iter().any(|line| line.contains(&last_of_batch))
    });
    let end = own.record(vec![ended(print(&watched))]);
    own.nudge(&world);
    let (lines, status) = stream.finished();
    let lines = documents(&lines);

    held_to_the_record(&own, print(&watched), &lines, status, |_| true);
    assert!(
        printed_ids(&lines).contains(&first[1].id.to_string()),
        "the printer's own event was not streamed: {lines:#?}"
    );
    assert!(
        !printed_ids(&lines).contains(&first[2].id.to_string()),
        "another print's event was streamed: {lines:#?}"
    );
    assert_eq!(
        printed_ids(&lines).last(),
        Some(&end[0].id.to_string()),
        "the print's end was not the last event printed: {lines:#?}"
    );
    let served = as_served(&world, &own, &watched);
    for line in lines.iter().filter(|line| line.get("id").is_some()) {
        let id = line["id"].as_str().expect("an id");
        assert_eq!(
            Some(line),
            served.get(id),
            "a streamed event is not what the supervisor's own reads print for it"
        );
    }
}

/// `--kind` and `--exclude-kind` pass exactly what they name; and watching a
/// print with an inclusion list that leaves its end out, or with its end
/// excluded, still ends with the print — without printing the end.
#[test]
fn kind_filters_pass_exactly_what_they_name_and_still_end_with_the_print() {
    let world = World::open(STOOD_IN);
    let own = Standalone::beside(&world, "streaming");
    let watched = own.print_id.clone();
    let included = watch(
        &own,
        &["--json", "--print-id", &watched, "--kind", OBSERVATION],
    );
    let mut excluded = watch(
        &own,
        &[
            "--json",
            "--print-id",
            &watched,
            "--exclude-kind",
            PrintEndedPayload::KIND,
        ],
    );
    let mut routine = watch(
        &own,
        &[
            "--json",
            "--print-id",
            &watched,
            "--exclude-kind",
            OBSERVATION,
        ],
    );
    std::thread::sleep(Duration::from_millis(500));
    let now = Timestamp::now();
    let recorded = own.record(vec![
        observation(Some(print(&watched)), now),
        notable(Some(print(&watched))),
        observation(None, after(now, 1)),
        notable(None),
    ]);
    own.nudge(&world);
    let last = recorded[3].id.to_string();
    for stream in [&mut excluded, &mut routine] {
        stream.until("the batch", |seen| {
            seen.iter().any(|line| line.contains(&last))
        });
    }
    // The printer's event stored after the end is in the print's scope and
    // after its end, so no stream over the print prints it.
    own.record(vec![ended(print(&watched)), notable(None)]);
    own.nudge(&world);

    let is = |kind: &'static str| move |event: &EventRecord| event.kind().as_str() == kind;
    let (lines, status) = included.finished();
    held_to_the_record(
        &own,
        print(&watched),
        &documents(&lines),
        status,
        is(OBSERVATION),
    );
    let (lines, status) = excluded.finished();
    held_to_the_record(&own, print(&watched), &documents(&lines), status, |event| {
        event.kind().as_str() != PrintEndedPayload::KIND
    });
    let (lines, status) = routine.finished();
    held_to_the_record(&own, print(&watched), &documents(&lines), status, |event| {
        event.kind().as_str() != OBSERVATION
    });
}

/// `--sample periodic_observation=300` passes the first observation, drops
/// each received inside 300 s of the last one passed, and passes each at or
/// beyond it — so observations keep arriving for the whole print — while
/// every other kind passes untouched.
#[test]
fn a_sampled_kind_passes_one_per_interval_by_received_at_for_the_whole_print() {
    let world = World::open(STOOD_IN);
    let own = Standalone::beside(&world, "streaming");
    let watched = own.print_id.clone();
    // Starts after an event already in the record, so the observations below
    // are the only ones the sampling sees, and the event it starts after is
    // not printed.
    let start = own.record(vec![notable(Some(print(&watched)))])[0]
        .id
        .to_string();
    let stream = watch(
        &own,
        &[
            "--json",
            "--print-id",
            &watched,
            "--after",
            &start,
            "--sample",
            &format!("{OBSERVATION}=300"),
        ],
    );
    let base = Timestamp::now();
    let mut drafts = Vec::new();
    for offset in [0, 100, 299, 300, 450, 600, 601, 1_000, 1_299, 1_300] {
        drafts.push(observation(Some(print(&watched)), after(base, offset)));
    }
    drafts.insert(3, notable(Some(print(&watched))));
    drafts.push(ended(print(&watched)));
    let recorded = own.record(drafts);
    own.nudge(&world);
    let (lines, status) = stream.finished();
    let lines = documents(&lines);

    let at = |index: usize| recorded[index].id.to_string();
    // 0, 100, 299, notable, 300, 450, 600, 601, 1000, 1299, 1300, end.
    assert_eq!(
        printed_ids(&lines),
        vec![at(0), at(3), at(4), at(6), at(8), at(10), at(11)],
        "the sampling did not pass exactly one observation per interval: {lines:#?}"
    );
    assert_eq!(status.code(), Some(0), "{lines:#?}");
    assert_eq!(ending(&lines)["reason"], json!("print_ended"));
}

/// Without `--json` a stream prints one line per passing event and the print's
/// end in words, and exits 0.
#[test]
fn a_watch_in_words_prints_a_line_per_event_and_the_end() {
    let world = World::open(STOOD_IN);
    let own = Standalone::beside(&world, "streaming");
    let watched = own.print_id.clone();
    let stream = watch(
        &own,
        &["--print-id", &watched, "--exclude-kind", OBSERVATION],
    );
    std::thread::sleep(Duration::from_millis(500));
    own.record(vec![
        notable(Some(print(&watched))),
        observation(Some(print(&watched)), Timestamp::now()),
        notable(None),
        ended(print(&watched)),
    ]);
    own.nudge(&world);
    let (lines, status) = stream.finished();

    let expected = expected(&own, print(&watched), |event| {
        event.kind().as_str() != OBSERVATION
    });
    assert_eq!(status.code(), Some(0), "{lines:#?}");
    assert_eq!(lines.len(), expected.len() + 1, "{lines:#?}");
    for (line, id) in lines.iter().zip(&expected) {
        assert!(line.contains(id), "`{line}` is not the line for event {id}");
    }
    let last = lines.last().expect("a last line");
    assert!(
        last.starts_with("watch ended: the print ended.") && last.contains("completed"),
        "the stream did not end in words: {last}"
    );
}

/// A heartbeat line every second while nothing passes, and SIGINT and SIGTERM
/// each end the stream with one `interrupted` line and exit 0.
#[cfg(unix)]
#[test]
fn a_quiet_stream_beats_and_a_signal_ends_it_as_interrupted() {
    let world = World::open(STOOD_IN);
    let own = Standalone::beside(&world, "streaming");
    for signal in ["INT", "TERM"] {
        let mut stream = watch(
            &own,
            &[
                "--json",
                "--heartbeat-s",
                "1",
                "--kind",
                "streaming_never_recorded",
            ],
        );
        stream.until("two heartbeats", |seen| {
            seen.iter()
                .filter(|line| line.contains("watch_heartbeat"))
                .count()
                >= 2
        });
        stream.signal(signal);
        let (lines, status) = stream.finished();
        let lines = documents(&lines);

        for beat in lines.iter().filter_map(|line| line.get("watch_heartbeat")) {
            assert!(beat["at"].is_string(), "{beat}");
            assert!(beat["since"].is_string(), "{beat}");
            assert!(beat.get("cursor").is_some(), "{beat}");
        }
        let ended = ending(&lines);
        assert_eq!(
            ended["reason"],
            json!("interrupted"),
            "SIG{signal}: {lines:#?}"
        );
        assert!(
            ended["detail"]
                .as_str()
                .is_some_and(|detail| detail.contains(&format!("SIG{signal}"))),
            "{ended}"
        );
        assert_eq!(status.code(), Some(0), "SIG{signal}: {lines:#?}");
    }
}

/// A server that stops ends the stream as unreachable, exit 3, once it has not
/// answered for longer than `--reconnect-s`.
#[test]
fn a_server_that_goes_away_ends_the_stream_as_unreachable_after_the_retry() {
    let world = World::open(STOOD_IN);
    let own = Standalone::beside(&world, "streaming");
    let mut stream = watch(
        &own,
        &["--json", "--reconnect-s", "2", "--heartbeat-s", "1"],
    );
    stream.until("a heartbeat", |seen| {
        seen.iter().any(|line| line.contains("watch_heartbeat"))
    });
    let stopped = Instant::now();
    own.stop();
    let (lines, status) = stream.finished();
    let lines = documents(&lines);

    assert!(
        stopped.elapsed() >= Duration::from_secs(2),
        "the stream gave up before its retry was over: {:?}",
        stopped.elapsed()
    );
    assert_eq!(ending(&lines)["reason"], json!("unreachable"), "{lines:#?}");
    assert_eq!(status.code(), Some(3), "{lines:#?}");
}

/// A server restarted inside `--reconnect-s` is followed on from the cursor:
/// what was recorded while it was down is streamed, and nothing is printed
/// twice or left out.
#[test]
fn a_server_restarted_inside_the_retry_is_followed_on_from_the_cursor() {
    let world = World::open(STOOD_IN);
    let own = Standalone::beside(&world, "streaming");
    let watched = own.print_id.clone();
    let mut stream = watch(
        &own,
        &["--json", "--print-id", &watched, "--reconnect-s", "60"],
    );
    std::thread::sleep(Duration::from_millis(500));
    let before = own.record(vec![notable(Some(print(&watched))), notable(None)]);
    own.nudge(&world);
    let seen = before[1].id.to_string();
    stream.until("what was recorded before", |lines| {
        lines.iter().any(|line| line.contains(&seen))
    });

    own.stop();
    own.record(vec![
        notable(Some(print(&watched))),
        observation(Some(print(&watched)), Timestamp::now()),
        notable(None),
    ]);
    own.start();
    own.record(vec![ended(print(&watched))]);
    own.nudge(&world);
    let (lines, status) = stream.finished();

    held_to_the_record(&own, print(&watched), &documents(&lines), status, |_| true);
}

/// A print whose record holds more events than one read answers, followed by
/// a stream that lost the server and came back only after the print ended —
/// and by one started on the print once it had ended: each prints every
/// passing event once, in the order stored, and then ends with the print once.
#[test]
fn a_stream_that_comes_back_after_the_end_or_starts_there_prints_all_of_a_long_print() {
    let world = World::open(STOOD_IN);
    let own = Standalone::beside(&world, "streaming");
    let watched = own.print_id.clone();
    let mut stream = watch(
        &own,
        &[
            "--json",
            "--print-id",
            &watched,
            "--reconnect-s",
            "60",
            "--exclude-kind",
            PrintEndedPayload::KIND,
        ],
    );
    std::thread::sleep(Duration::from_millis(500));
    let before = own.record(vec![notable(Some(print(&watched)))]);
    own.nudge(&world);
    let seen = before[0].id.to_string();
    stream.until("what was recorded before", |lines| {
        lines.iter().any(|line| line.contains(&seen))
    });

    own.stop();
    let base = Timestamp::now();
    let mut long: Vec<EventDraft> = (0..300)
        .map(|second| observation(Some(print(&watched)), after(base, second)))
        .collect();
    long.push(ended(print(&watched)));
    own.record(long);
    own.start();
    let (lines, status) = stream.finished();
    let lines = documents(&lines);
    held_to_the_record(&own, print(&watched), &lines, status, |event| {
        event.kind().as_str() != PrintEndedPayload::KIND
    });
    assert!(
        printed_ids(&lines).len() > 300,
        "the stream did not carry the whole of a long print: {}",
        printed_ids(&lines).len()
    );

    // The same print, watched only once it has ended.
    let (lines, status) = watch(
        &own,
        &["--json", "--print-id", &watched, "--kind", OBSERVATION],
    )
    .finished();
    let lines = documents(&lines);
    held_to_the_record(&own, print(&watched), &lines, status, |event| {
        event.kind().as_str() == OBSERVATION
    });
    assert!(
        printed_ids(&lines).len() >= 300,
        "{}",
        printed_ids(&lines).len()
    );
}

/// A credential the supervisor refuses, and a request it refuses, each end the
/// stream as refused with exit 7 at once rather than being retried until it is
/// unreachable.
#[test]
fn a_refused_credential_or_request_ends_the_stream_as_refused_at_once() {
    let world = World::open(STOOD_IN);
    let own = Standalone::beside(&world, "streaming");
    for (arguments, config) in [
        (vec!["--json"], own.refused_client.clone()),
        (vec!["--json", "--kind", "Not-A-Kind"], own.client.clone()),
    ] {
        let started = Instant::now();
        let mut asked = arguments.clone();
        asked.extend(["--reconnect-s", "60"]);
        let (lines, status) = watch_with(&asked, &config).finished();
        let lines = documents(&lines);

        assert_eq!(
            ending(&lines)["reason"],
            json!("refused"),
            "{arguments:?}: {lines:#?}"
        );
        assert_eq!(status.code(), Some(7), "{arguments:?}: {lines:#?}");
        assert!(
            started.elapsed() < Duration::from_secs(10),
            "{arguments:?} was retried rather than refused at once: {:?}",
            started.elapsed()
        );
    }
}

/// `printobserver events` prints what the route answered: the events, the
/// cursor and, scoped to a print, its end — the end told although the read
/// filtered its event out.
#[test]
fn the_events_command_prints_the_answer_the_route_served() {
    let world = World::open(STOOD_IN);
    let own = Standalone::beside(&world, "streaming");
    let watched = own.print_id.clone();
    // Read through a recording proxy, so what the route served is the bytes
    // that came back over the wire rather than what this program says they
    // were.
    let proxy = Proxy::in_front_of(own.address);
    let through = world.root.path().join("through-the-proxy.toml");
    std::fs::write(
        &through,
        format!(
            "[client]\nserver = \"{}\"\ncredential = \"{CREDENTIAL}\"\n",
            proxy.url()
        ),
    )
    .expect("the client configuration is writable");
    let recorded = own.record(vec![
        notable(Some(print(&watched))),
        notable(None),
        ended(print(&watched)),
    ]);

    let after = recorded[0].id.to_string();
    let ran = running::with(
        &world,
        &[
            "events",
            "--json",
            "--print-id",
            &watched,
            "--after",
            &after,
            "--exclude-kinds",
            PrintEndedPayload::KIND,
            "--config",
            &through.display().to_string(),
        ]
        .map(str::to_owned),
        &[],
    );
    assert_eq!(ran.code, Some(0), "{}", ran.said());
    let said: Value = serde_json::from_str(&ran.out).expect("the command printed JSON");
    let served: Value =
        serde_json::from_str(&proxy.the_one_request().answered).expect("the route answered JSON");

    assert_eq!(
        said, served,
        "the command did not print what the route served"
    );
    assert_eq!(
        said["events"].as_array().map(|events| events
            .iter()
            .map(|event| event["id"].clone())
            .collect::<Vec<_>>()),
        Some(vec![json!(recorded[1].id.to_string())]),
        "{said:#}"
    );
    assert_eq!(
        said["cursor"],
        json!(recorded[2].id.to_string()),
        "{said:#}"
    );
    assert_eq!(
        said["print_ended"]["event_id"],
        json!(recorded[2].id.to_string()),
        "{said:#}"
    );
    assert_eq!(
        said["print_ended"]["outcome"],
        json!("completed"),
        "{said:#}"
    );
}

/// A watch that cannot start prints no line at all: a malformed sampling is
/// refused as usage, and a configuration naming no supervisor as unconfigured,
/// before any stream exists to end.
#[test]
fn a_watch_that_cannot_start_prints_no_line_and_exits_as_every_command_does() {
    let world = World::open(STOOD_IN);
    let own = Standalone::beside(&world, "streaming");
    let serverless = world.serverless_config();
    for (arguments, config, exit) in [
        (
            vec!["--json", "--sample", "periodic_observation"],
            &own.client,
            2,
        ),
        (vec!["--json", "--sample", "Not-A-Kind=300"], &own.client, 2),
        (vec!["--json", "--reconnect-s", "-1"], &own.client, 2),
        (vec!["--json"], &serverless, 4),
    ] {
        let (lines, status) = watch_with(&arguments, config).finished();

        assert!(
            lines.is_empty(),
            "{arguments:?} printed a stream: {lines:#?}"
        );
        assert_eq!(status.code(), Some(exit), "{arguments:?}");
    }
}

/// A heartbeat and a retry take any whole number of seconds the option holds,
/// a day and a second or the largest there is: the watch runs, and ends with
/// the print, rather than refusing either or failing on a timer it cannot set.
#[test]
fn a_heartbeat_and_a_retry_of_any_length_are_taken() {
    let world = World::open(STOOD_IN);
    let own = Standalone::beside(&world, "streaming");
    let watched = own.print_id.clone();
    own.record(vec![ended(print(&watched))]);
    for seconds in ["86401", &i64::MAX.to_string()] {
        let (lines, status) = watch(
            &own,
            &[
                "--json",
                "--print-id",
                &watched,
                "--heartbeat-s",
                seconds,
                "--reconnect-s",
                seconds,
            ],
        )
        .finished();
        let lines = documents(&lines);

        assert_eq!(
            ending(&lines)["reason"],
            json!("print_ended"),
            "{seconds}: {lines:#?}"
        );
        assert_eq!(status.code(), Some(0), "{seconds}: {lines:#?}");
    }
}

/// A supervisor that answers an events read with something that is not one
/// ends the stream as refused, exit 7, at once.
#[test]
fn an_answer_that_is_not_an_events_answer_ends_the_stream_as_refused() {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("a loopback port");
    let address = listener.local_addr().expect("the bound address");
    std::thread::spawn(move || {
        for mut stream in listener.incoming().flatten() {
            let mut asked = [0_u8; 4096];
            let _ = std::io::Read::read(&mut stream, &mut asked);
            let body = r#"{"not":"an events answer"}"#;
            let _ = std::io::Write::write_all(
                &mut stream,
                format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{body}",
                    body.len()
                )
                .as_bytes(),
            );
        }
    });
    let root = tempfile::TempDir::new().expect("a root");
    let config = root.path().join("client.toml");
    std::fs::write(
        &config,
        format!("[client]\nserver = \"http://{address}\"\ncredential = \"{CREDENTIAL}\"\n"),
    )
    .expect("the client configuration is writable");

    let (lines, status) = watch_with(&["--json", "--reconnect-s", "60"], &config).finished();
    let lines = documents(&lines);

    let ended = ending(&lines);
    assert_eq!(ended["reason"], json!("refused"), "{lines:#?}");
    assert!(
        ended["detail"]
            .as_str()
            .is_some_and(|detail| detail.contains("cannot read")),
        "{ended}"
    );
    assert_eq!(status.code(), Some(7), "{lines:#?}");
}

/// A watch asked to start after a print's end prints nothing more and ends
/// with the print at once.
#[test]
fn a_watch_started_after_a_prints_end_ends_with_it_at_once() {
    let world = World::open(STOOD_IN);
    let own = Standalone::beside(&world, "streaming");
    let watched = own.print_id.clone();
    let recorded = own.record(vec![notable(Some(print(&watched))), ended(print(&watched))]);
    let end = recorded[1].id.to_string();

    let (lines, status) =
        watch(&own, &["--json", "--print-id", &watched, "--after", &end]).finished();
    let lines = documents(&lines);

    assert!(printed_ids(&lines).is_empty(), "{lines:#?}");
    let ended = ending(&lines);
    assert_eq!(ended["reason"], json!("print_ended"), "{lines:#?}");
    assert!(
        ended["detail"]
            .as_str()
            .is_some_and(|detail| detail.contains(&end)),
        "{ended}"
    );
    assert_eq!(status.code(), Some(0), "{lines:#?}");
}

/// A watch over a print that stopped with no cancel, no error and no finish
/// ends saying the print was interrupted, as a person reads it.
#[test]
fn a_watch_over_an_interrupted_print_says_it_was_interrupted() {
    let world = World::open(STOOD_IN);
    let own = Standalone::beside(&world, "streaming");
    let watched = own.print_id.clone();
    let recorded = own.record(vec![
        notable(Some(print(&watched))),
        ended_as(print(&watched), PrintOutcome::Interrupted),
    ]);
    let end = recorded[1].id.to_string();

    let (lines, status, err) =
        watch(&own, &["--print-id", &watched, "--after", &end]).finished_saying();

    let said = format!("{}\n{err}", lines.join("\n"));
    assert!(
        said.contains(&format!("print {watched} ended interrupted at ")) && said.contains(&end),
        "the watch did not say the print was interrupted: {said}"
    );
    assert_eq!(status.code(), Some(0), "{said}");
}

/// Without `--print-id`, a stream carries every event stored after it began —
/// every print's and the printer's — and none stored before.
#[test]
fn a_watch_over_no_print_streams_every_event_stored_after_it_began() {
    let world = World::open(STOOD_IN);
    let own = Standalone::beside(&world, "streaming");
    let earlier = own.record(vec![notable(None)]);
    let mut stream = watch(&own, &["--json", "--kind", NOTABLE, "--reconnect-s", "1"]);
    std::thread::sleep(Duration::from_millis(500));
    let later = own.record(vec![
        notable(Some(print(&own.print_id))),
        notable(Some(print(&own.elsewhere))),
        notable(None),
    ]);
    own.nudge(&world);
    let last = later[2].id.to_string();
    stream.until("what was stored after it began", |seen| {
        seen.iter().any(|line| line.contains(&last))
    });
    own.stop();
    let (lines, status) = stream.finished();
    let lines = documents(&lines);

    assert_eq!(
        printed_ids(&lines),
        later
            .iter()
            .map(|event| event.id.to_string())
            .collect::<Vec<_>>(),
        "earlier {}: {lines:#?}",
        earlier[0].id
    );
    assert_eq!(ending(&lines)["reason"], json!("unreachable"), "{lines:#?}");
    assert_eq!(status.code(), Some(3), "{lines:#?}");
}

/// Without `--json`, a quiet stream says so in words every heartbeat, and
/// its ending is in words too.
#[test]
fn a_quiet_watch_in_words_beats_and_ends_in_words() {
    let world = World::open(STOOD_IN);
    let own = Standalone::beside(&world, "streaming");
    let mut stream = watch(&own, &["--heartbeat-s", "1", "--reconnect-s", "1"]);
    stream.until("two heartbeats", |seen| {
        seen.iter()
            .filter(|line| line.contains("still watching"))
            .count()
            >= 2
    });
    own.stop();
    let (lines, status, said) = stream.finished_saying();

    assert!(
        lines.iter().all(|line| line.contains("still watching")),
        "a failure was printed among the stream's lines: {lines:#?}"
    );
    assert!(
        said.starts_with("watch ended: the supervisor could not be reached."),
        "the failure was not said on standard error: {said}"
    );
    assert_eq!(status.code(), Some(3), "{lines:#?}");
}

/// `--kind`, `--exclude-kind` and `--sample` each given twice: every value
/// given rules on the stream.
#[test]
fn every_value_of_a_repeated_option_rules_on_the_stream() {
    let world = World::open(STOOD_IN);
    let own = Standalone::beside(&world, "streaming");
    let watched = own.print_id.clone();
    let observation_every = format!("{OBSERVATION}=300");
    let notable_every = format!("{NOTABLE}=600");
    let included = watch(
        &own,
        &[
            "--json",
            "--print-id",
            &watched,
            "--kind",
            OBSERVATION,
            "--kind",
            NOTABLE,
            "--sample",
            &observation_every,
            "--sample",
            &notable_every,
        ],
    );
    let excluded = watch(
        &own,
        &[
            "--json",
            "--print-id",
            &watched,
            "--exclude-kind",
            OTHER,
            "--exclude-kind",
            OBSERVATION,
        ],
    );
    std::thread::sleep(Duration::from_millis(500));
    let base = Timestamp::now();
    let on = Some(print(&watched));
    let recorded = own.record(vec![
        observation(on, base),
        notable_at(on, base),
        other_kind(on),
        observation(on, after(base, 100)),
        notable_at(on, after(base, 100)),
        observation(on, after(base, 300)),
        notable_at(on, after(base, 300)),
        notable_at(on, after(base, 600)),
        ended(print(&watched)),
    ]);
    own.nudge(&world);
    let at = |index: usize| recorded[index].id.to_string();

    let (lines, status) = included.finished();
    let lines = documents(&lines);
    assert_eq!(
        printed_ids(&lines),
        vec![at(0), at(1), at(5), at(7)],
        "two kinds, each sampled by its own interval: {lines:#?}"
    );
    assert_eq!(status.code(), Some(0), "{lines:#?}");
    assert_eq!(ending(&lines)["reason"], json!("print_ended"));

    let (lines, status) = excluded.finished();
    let lines = documents(&lines);
    held_to_the_record(&own, print(&watched), &lines, status, |event| {
        event.kind().as_str() != OTHER && event.kind().as_str() != OBSERVATION
    });
    for index in [1, 4, 6, 7, 8] {
        assert!(
            printed_ids(&lines).contains(&at(index)),
            "two kinds excluded, and more with them: {lines:#?}"
        );
    }
}

/// A heartbeat comes the interval after the last line and no sooner, and an
/// event printed puts the next one back a whole interval.
///
/// The gap after the event is read off the instants the stream itself wrote,
/// never off when this journey happened to read a line: the heartbeat's `since`,
/// stamped once the stream had written the event line and its timer reset, and
/// its `at`, stamped as it was printed. A reader delayed under load reads both
/// lines late without moving either instant. That `since` is the event line's
/// own is held too: it is no earlier than the event was stored, which was
/// after the first heartbeat, and the event line comes before the heartbeat.
#[test]
fn a_heartbeat_comes_an_interval_after_the_last_line() {
    let world = World::open(STOOD_IN);
    let own = Standalone::beside(&world, "streaming");
    let interval = Duration::from_secs(2);
    let started = Instant::now();
    let mut stream = watch(&own, &["--json", "--kind", NOTABLE, "--heartbeat-s", "2"]);
    let beats = |seen: &[String]| {
        seen.iter()
            .filter(|line| line.contains("watch_heartbeat"))
            .count()
    };
    stream.until("a heartbeat", |seen| beats(seen) == 1);
    assert!(
        started.elapsed() >= interval,
        "a heartbeat came before its interval: {:?}",
        started.elapsed()
    );

    // Halfway to the next heartbeat by the stream's own clock, so a stream
    // that went on counting from the first heartbeat rather than from the
    // event would beat half an interval after the event, and fail below.
    let first_beat = beat_at(&documents(&stream.seen));
    std::thread::sleep(left_until(first_beat, interval / 2));
    let recorded = own.record(vec![notable(None)]);
    own.nudge(&world);
    let printed = recorded[0].id.to_string();
    stream.until("the event", |seen| {
        seen.iter().any(|line| line.contains(&printed))
    });
    stream.until("the next heartbeat", |seen| beats(seen) == 2);
    let lines = stream.seen.clone();
    let documents = documents(&lines);
    let beat = last_beat(&documents);
    let (since, second_beat) = (instant(beat, "since"), instant(beat, "at"));
    assert!(
        since >= recorded[0].received_at && since > first_beat,
        "the heartbeat after an event counted from {since}, not from the event \
         line, stored at {} after the first heartbeat at {first_beat}",
        recorded[0].received_at
    );
    let after_the_event = between(since, second_beat);
    assert!(
        after_the_event >= Duration::from_millis(1_800),
        "the heartbeat after an event came before a whole interval: {after_the_event:?}"
    );
    let event_at = lines.iter().position(|line| line.contains(&printed));
    let second_beat = lines
        .iter()
        .rposition(|line| line.contains("watch_heartbeat"));
    assert!(event_at < second_beat, "{lines:#?}");
}

/// The last heartbeat among `lines`.
fn last_beat(lines: &[Value]) -> &Value {
    lines
        .iter()
        .rev()
        .find_map(|line| line.get("watch_heartbeat"))
        .unwrap_or_else(|| panic!("a heartbeat: {lines:#?}"))
}

/// The instant a heartbeat's `field` names.
fn instant(beat: &Value, field: &str) -> Timestamp {
    beat[field]
        .as_str()
        .unwrap_or_else(|| panic!("a heartbeat carrying `{field}`: {beat}"))
        .parse()
        .unwrap_or_else(|error| panic!("a heartbeat's `{field}` is an instant: {error:?}"))
}

/// When the last heartbeat among `lines` says it was printed.
fn beat_at(lines: &[Value]) -> Timestamp {
    instant(last_beat(lines), "at")
}

/// How long from `earlier` to `later`, or nothing when `later` is not later.
fn between(earlier: Timestamp, later: Timestamp) -> Duration {
    (*later.as_utc() - *earlier.as_utc())
        .to_std()
        .unwrap_or(Duration::ZERO)
}

/// How long from now until `wait` has passed since `since`, by the wall clock.
fn left_until(since: Timestamp, wait: Duration) -> Duration {
    wait.saturating_sub(between(since, Timestamp::now()))
}
