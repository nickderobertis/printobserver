//! The printer watch, against a real server and the real `OctoPrint` adapter.
//!
//! The printer is the scripted `OctoPrint` the adapter's `standin` feature
//! compiles: a real HTTP server and a real websocket on a loopback socket,
//! answering the REST bodies and the push events the observability spike
//! tabulated for each stop — the ones `OctoPrint`'s virtual printer cannot be
//! made to produce on demand, a firmware reset above all. Everything on this
//! side of that socket is the real program: the adapter, the watch, the
//! policy, the store and the API. The agent is the stand-in harness, so what a
//! journey asserts of a turn is what the harness was handed.
//!
//! What is asserted is what an operator or the agent can observe: the events
//! the printer history answers, the status and context reads, and the turns
//! the harness was given.

use std::sync::Arc;
use std::time::Duration;

use printobserver_core::store::Stores;
use printobserver_obico::{ObicoVision, ObicoVisionConfig};
use printobserver_octoprint::OctoPrintPrinter;
use printobserver_octoprint::standin::{JobScript, STANDIN_API_KEY, ScriptedOctoPrint};
use printobserver_server::{Ports, Running, Server, ServerConfig};
use printobserver_store_sqlite::SqliteStore;
use printobserver_supervisor_api::TurnRequest;
use printobserver_types::serde_json::{Value, json};
use tempfile::TempDir;

use crate::agent::StandInAgent;
use crate::world::{OPERATOR, document, presenting, set, write};

/// The file every scripted print is of, and its size.
const FILE: &str = "tower.gcode";

/// The scripted file's size in bytes.
const SIZE: i64 = 816;

/// How often these journeys' watch reads the machine.
const POLL: Duration = Duration::from_millis(200);

/// How long a journey waits for what the watch should record.
const PATIENCE: Duration = Duration::from_secs(20);

/// A real server watching the scripted `OctoPrint`.
pub struct Watch {
    /// The journey's root.
    _root: TempDir,
    /// The server.
    pub server: Running,
    /// The scripted printer.
    pub octoprint: ScriptedOctoPrint,
    /// The supervising agent.
    pub agent: Arc<StandInAgent>,
    /// The client, presenting the operator's credential.
    pub client: reqwest::Client,
}

impl Watch {
    /// A server watching a fresh scripted printer, its configuration edited
    /// and then its validated form tuned by the journey — a poll below the
    /// five seconds a file admits, so that a journey waits milliseconds for
    /// what it would otherwise wait seconds for, is set there.
    pub async fn start(
        edit: impl FnOnce(&mut toml::Value),
        tune: impl FnOnce(&mut ServerConfig),
    ) -> Self {
        let octoprint = ScriptedOctoPrint::start();
        let root = TempDir::new().expect("a journey's own root");
        let mut configured = document(root.path(), &octoprint.base_url());
        set(
            &mut configured,
            "octoprint.api_key",
            toml::Value::String(STANDIN_API_KEY.to_owned()),
        );
        edit(&mut configured);
        let path = write(root.path(), &configured);
        let mut config = ServerConfig::load(&path).expect("the configuration is accepted");
        tune(&mut config);
        let stores = Stores::of(Arc::new(
            SqliteStore::open(&config.state_dir).expect("the store opens"),
        ));
        let agent = StandInAgent::new();
        let server = Server::start_with(
            config.clone(),
            Ports {
                printer: Arc::new(OctoPrintPrinter::new(config.octoprint.clone())),
                stores,
                vision: Arc::new(ObicoVision::new(ObicoVisionConfig::default()).expect("built")),
                agent: Arc::clone(&agent) as Arc<dyn printobserver_supervisor_api::SupervisorPort>,
            },
        )
        .await
        .expect("the server starts");
        Self {
            _root: root,
            server,
            octoprint,
            agent,
            client: presenting(OPERATOR),
        }
    }

    /// A server watching a fresh scripted printer under the base configuration.
    pub async fn open() -> Self {
        Self::start(|_| {}, |config| config.printer_poll = POLL).await
    }

    /// One JSON read of the versioned surface.
    pub async fn get(&self, path: &str) -> Value {
        let response = self
            .client
            .get(format!("{}{path}", self.server.base_url()))
            .send()
            .await
            .expect("the server answers");
        assert!(
            response.status().is_success(),
            "{path}: {}",
            response.status()
        );
        response.json().await.expect("a JSON answer")
    }

    /// Every event the printer history answers, oldest first.
    pub async fn history(&self) -> Vec<Value> {
        let mut events = self.get("/history?limit=500").await["events"]
            .as_array()
            .cloned()
            .unwrap_or_default();
        events.reverse();
        events
    }

    /// Every event of one kind the printer history answers, oldest first.
    pub async fn of_kind(&self, kind: &str) -> Vec<Value> {
        self.history()
            .await
            .into_iter()
            .filter(|event| event["kind"] == kind)
            .collect()
    }

    /// Wait until the printer history holds `count` events of one kind.
    pub async fn until(&self, kind: &str, count: usize) -> Vec<Value> {
        let deadline = tokio::time::Instant::now() + PATIENCE;
        loop {
            let found = self.of_kind(kind).await;
            if found.len() >= count {
                return found;
            }
            assert!(
                tokio::time::Instant::now() < deadline,
                "only {} of {count} `{kind}` were recorded: {:#}",
                found.len(),
                Value::Array(self.history().await)
            );
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
    }

    /// Wait until the push socket is open.
    pub async fn connected(&self) {
        let deadline = tokio::time::Instant::now() + PATIENCE;
        while self.octoprint.open_sockets() == 0 {
            assert!(
                tokio::time::Instant::now() < deadline,
                "the push socket never opened"
            );
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        // The reader is told the channel is up as it takes the socket.
        tokio::time::sleep(Duration::from_millis(100)).await;
    }

    /// Start a print at the machine and wait until the watch adopted it,
    /// answering its identifier.
    pub async fn printing(&self, filepos: i64) -> String {
        self.octoprint
            .reports("Printing", JobScript::printing(FILE, SIZE, filepos, 60));
        let started = self.until("print_started", 1).await;
        started
            .last()
            .and_then(|event| event["print_id"].as_str())
            .expect("the print started")
            .to_owned()
    }

    /// Wait until `count` turns have been handed to the agent, answering them.
    pub async fn turns(&self, count: usize) -> Vec<TurnRequest> {
        let deadline = tokio::time::Instant::now() + PATIENCE;
        loop {
            let turns = self.agent.turns();
            if turns.len() >= count {
                return turns;
            }
            assert!(
                tokio::time::Instant::now() < deadline,
                "only {} of {count} turns were run",
                turns.len()
            );
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    }
}

/// The one print the history ended, and how.
pub fn ended(events: &[Value], print_id: &str) -> String {
    events
        .iter()
        .find(|event| event["kind"] == "print_ended" && event["print_id"] == print_id)
        .map(|event| {
            event["payload"]["outcome"]
                .as_str()
                .unwrap_or_default()
                .to_owned()
        })
        .expect("the print ended")
}

/// The idle printer a stop that landed it idle reports, its file still
/// selected and its progress cleared.
fn landed_idle() -> JobScript {
    JobScript {
        file: Some((FILE.to_owned(), SIZE)),
        ..JobScript::idle()
    }
}

/// The printer a stop that went offline reports.
fn landed_offline(state: &str, error: Option<&str>) -> JobScript {
    JobScript {
        state: state.to_owned(),
        error: error.map(str::to_owned),
        ..JobScript::idle()
    }
}

/// The turn the agent was handed on one print's ending names the transition.
fn assert_turn_names_the_ending(turns: &[TurnRequest], print_id: &str, outcome: &str) {
    let turn = turns
        .iter()
        .find(|turn| {
            turn.print_id.to_string() == print_id && turn.event.kind().as_str() == "print_ended"
        })
        .unwrap_or_else(|| panic!("no turn was handed the ending of {print_id}: {turns:?}"));
    let noticed = turn.situation.noticed.as_deref().unwrap_or_default();
    assert!(
        noticed.starts_with("print_ended") && noticed.contains(outcome),
        "the turn's situation does not name the transition: {noticed:?}"
    );
}

/// A firmware reset that lands the printer idle, which the push socket says
/// was a reset, ends the print `interrupted`, records the reset during it, and
/// gives its agent a turn naming the transition.
#[tokio::test(flavor = "multi_thread")]
async fn a_reset_landing_idle_ends_the_print_interrupted() {
    let watch = Watch::open().await;
    watch.connected().await;
    let print_id = watch.printing(400).await;

    watch.octoprint.pushes_event("PrinterReset", &json!({}));
    let reset = watch.until("printer_reset", 1).await;
    watch.octoprint.reports("Operational", landed_idle());
    watch.until("print_ended", 1).await;

    assert_eq!(reset[0]["payload"]["during_print"], print_id.as_str());
    assert_eq!(reset[0]["print_id"], Value::Null);
    assert_eq!(ended(&watch.history().await, &print_id), "interrupted");
    let turns = watch.turns(1).await;
    assert_turn_names_the_ending(&turns, &print_id, "interrupted");
}

/// The same return to idle with no reset pushed is a cancel made somewhere
/// else — the printer's menu, `OctoPrint`'s own interface — and ends the print
/// `cancelled`, giving no turn.
#[tokio::test(flavor = "multi_thread")]
async fn a_return_to_idle_with_no_reset_ends_the_print_cancelled() {
    let watch = Watch::open().await;
    watch.connected().await;
    let print_id = watch.printing(400).await;

    watch.octoprint.reports("Operational", landed_idle());
    watch.until("print_ended", 1).await;
    tokio::time::sleep(POLL * 5).await;

    assert_eq!(ended(&watch.history().await, &print_id), "cancelled");
    assert!(
        watch.agent.turns().is_empty(),
        "a cancel gave the agent a turn"
    );
    assert!(watch.of_kind("printer_reset").await.is_empty());
}

/// A reset that lands the printer offline after a resend error, which pushes
/// a reset and an error that is not the printer's own, ends `interrupted`
/// and records the connection going offline — and the printer's refusal to be
/// read while offline is recorded as no error at all.
#[tokio::test(flavor = "multi_thread")]
async fn a_reset_landing_offline_after_a_resend_error_ends_interrupted() {
    let watch = Watch::open().await;
    watch.connected().await;
    let print_id = watch.printing(400).await;

    watch.octoprint.pushes_event("PrinterReset", &json!({}));
    watch.octoprint.pushes_event(
        "Error",
        &json!({ "error": "Should resend line 2", "reason": "resend" }),
    );
    watch.until("printer_reset", 1).await;
    watch.octoprint.reports(
        "Closed",
        landed_offline(
            "Offline after error",
            Some("Should resend line 2 but no sufficient history is available, can't resend"),
        ),
    );
    let changed = watch.until("printer_connection_changed", 1).await;
    watch.until("print_ended", 1).await;

    assert_eq!(ended(&watch.history().await, &print_id), "interrupted");
    assert_eq!(changed[0]["payload"]["from"], "printing");
    assert_eq!(changed[0]["payload"]["to"], "offline");
    assert_eq!(changed[0]["print_id"], Value::Null);
    assert_turn_names_the_ending(&watch.turns(1).await, &print_id, "interrupted");
    let status = watch.get(&format!("/prints/{print_id}/status")).await;
    assert_eq!(status["printer"]["connection"], "offline");
    let history = watch.get(&format!("/prints/{print_id}/history")).await;
    let failures: Vec<&Value> = history["events"]
        .as_array()
        .expect("the print's history")
        .iter()
        .filter(|event| event["kind"] == "port_failure")
        .collect();
    assert!(
        failures.is_empty(),
        "a read of an offline printer was recorded as a failure: {failures:?}"
    );
}

/// An exception's name as the job's error, with no error of the printer's own
/// pushed, is the host losing the line: `interrupted`.
#[tokio::test(flavor = "multi_thread")]
async fn a_serial_timeout_with_no_pushed_error_ends_interrupted() {
    let watch = Watch::open().await;
    watch.connected().await;
    let print_id = watch.printing(400).await;

    watch.octoprint.reports(
        "Closed",
        landed_offline("Offline after error", Some("SerialTimeoutException: ")),
    );
    watch.until("print_ended", 1).await;

    assert_eq!(ended(&watch.history().await, &print_id), "interrupted");
    assert_turn_names_the_ending(&watch.turns(1).await, &print_id, "interrupted");
}

/// With the push socket down, a printer's own message as the job's error is
/// the printer's error: `failed`, the events saying the channel was down; and
/// the socket admitted again is reopened on its own.
#[tokio::test(flavor = "multi_thread")]
async fn a_printers_message_with_the_push_socket_down_ends_failed_and_the_socket_reopens() {
    let watch = Watch::open().await;
    watch.connected().await;
    let print_id = watch.printing(400).await;
    watch.octoprint.refuses_sockets(true);
    watch.octoprint.drops_sockets();
    tokio::time::sleep(POLL * 3).await;

    watch.octoprint.reports(
        "Closed",
        landed_offline(
            "Offline after error",
            Some(
                "Thermal Runaway, system stopped! Heater_ID: bed - Printer halted. kill() called!",
            ),
        ),
    );
    let changed = watch.until("printer_connection_changed", 1).await;
    watch.until("print_ended", 1).await;

    assert_eq!(ended(&watch.history().await, &print_id), "failed");
    let detail = changed[0]["payload"]["detail"].as_str().unwrap_or_default();
    assert!(
        detail.contains(printobserver_core::PUSH_CHANNEL_DOWN),
        "the change does not say the push channel was down: {detail}"
    );
    assert_turn_names_the_ending(&watch.turns(1).await, &print_id, "failed");

    watch.octoprint.refuses_sockets(false);
    let deadline = tokio::time::Instant::now() + PATIENCE;
    while watch.octoprint.open_sockets() == 0 {
        assert!(
            tokio::time::Instant::now() < deadline,
            "the push socket was never reopened"
        );
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
}

/// A finish no poll saw the finishing of reads as completion at the file's
/// size, and ends `completed`, giving no turn.
#[tokio::test(flavor = "multi_thread")]
async fn a_finish_whose_finishing_no_poll_saw_ends_completed() {
    let watch = Watch::open().await;
    watch.connected().await;
    let print_id = watch.printing(700).await;

    watch.octoprint.reports(
        "Operational",
        JobScript {
            completion: Some(100.0),
            filepos: Some(SIZE),
            ..landed_idle()
        },
    );
    watch.until("print_ended", 1).await;
    tokio::time::sleep(POLL * 5).await;

    assert_eq!(ended(&watch.history().await, &print_id), "completed");
    assert!(
        watch.agent.turns().is_empty(),
        "a finished print gave the agent a turn"
    );
}

/// A reset while idle, which nothing a read reports shows, is recorded with
/// no print it happened during.
#[tokio::test(flavor = "multi_thread")]
async fn an_idle_reset_is_recorded_during_no_print() {
    let watch = Watch::open().await;
    watch.connected().await;

    watch.octoprint.pushes_event("PrinterReset", &json!({}));
    let reset = watch.until("printer_reset", 1).await;

    assert_eq!(reset[0]["payload"]["during_print"], Value::Null);
    assert_eq!(reset[0]["print_id"], Value::Null);
}

/// With no print active the watch still reads the machine, at the poll the
/// configuration names: going offline and coming back are each recorded, the
/// printer history answering both.
#[tokio::test(flavor = "multi_thread")]
async fn the_watch_runs_with_no_print_at_the_configured_poll() {
    let watch = Watch::start(
        |document| {
            set(
                document,
                "supervisor.printer_poll_s",
                toml::Value::Integer(5),
            );
        },
        |_| {},
    )
    .await;
    assert_eq!(watch.server.config().printer_poll, Duration::from_secs(5));
    // The first read is one poll after the start, and it is what the change
    // is measured against.
    tokio::time::sleep(Duration::from_secs(6)).await;

    let went = tokio::time::Instant::now();
    watch
        .octoprint
        .reports("Closed", landed_offline("Offline", None));
    let offline = watch.until("printer_connection_changed", 1).await;
    let seen_after = went.elapsed();
    watch.octoprint.reports("Operational", JobScript::idle());
    let back = watch.until("printer_connection_changed", 2).await;

    assert!(
        seen_after <= Duration::from_secs(6),
        "the change was seen {seen_after:?} after it happened, past one poll"
    );
    assert_eq!(offline[0]["payload"]["from"], "operational");
    assert_eq!(offline[0]["payload"]["to"], "offline");
    assert_eq!(back[1]["payload"]["from"], "offline");
    assert_eq!(back[1]["payload"]["to"], "operational");
}

/// A print the operator cancels through printobserver ends `cancelled`.
#[tokio::test(flavor = "multi_thread")]
async fn a_print_cancelled_through_printobserver_ends_cancelled() {
    let watch = Watch::open().await;
    watch.connected().await;
    let print_id = watch.printing(400).await;

    let response = watch
        .client
        .post(format!(
            "{}/prints/{print_id}/actions/cancel",
            watch.server.base_url()
        ))
        .json(&json!({ "reason": "the operator stopped it", "actor": "operator" }))
        .send()
        .await
        .expect("the server answers");
    assert!(response.status().is_success(), "{}", response.status());
    watch.until("print_ended", 1).await;

    assert_eq!(watch.octoprint.job_commands(), vec!["cancel".to_owned()]);
    assert_eq!(ended(&watch.history().await, &print_id), "cancelled");
}

/// End one print `interrupted` with a serial drop, and answer it.
async fn interrupted(watch: &Watch) -> String {
    let print_id = watch.printing(400).await;
    watch.octoprint.reports(
        "Closed",
        landed_offline("Offline after error", Some("SerialTimeoutException: ")),
    );
    watch.until("print_ended", 1).await;
    // The printer reconnects, idle and holding no job.
    watch.octoprint.reports("Operational", JobScript::idle());
    watch.until("printer_connection_changed", 2).await;
    print_id
}

/// Heating with no job running after an interrupted print is recorded once
/// however many polls find it, naming the print, with the head's position
/// read; and its agent is given one turn on it, naming the activity.
#[tokio::test(flavor = "multi_thread")]
async fn heating_after_an_interrupted_print_is_recorded_once_with_one_turn() {
    let watch = Watch::open().await;
    watch.connected().await;
    let print_id = interrupted(&watch).await;

    watch.octoprint.targets(200.0, 0.0);
    let activity = watch.until("unattended_printer_activity", 1).await;
    tokio::time::sleep(POLL * 10).await;

    let activity_events = watch.of_kind("unattended_printer_activity").await;
    assert_eq!(
        activity_events.len(),
        1,
        "the same activity was recorded again"
    );
    let payload = &activity[0]["payload"];
    assert_eq!(payload["last_print_id"], print_id.as_str());
    assert_eq!(payload["tools"], json!([{ "tool": 0, "target_c": 200.0 }]));
    assert_eq!(payload["bed_target_c"], Value::Null);
    assert_eq!(payload["moved"], false);
    let turns = watch.turns(2).await;
    let on_activity: Vec<&TurnRequest> = turns
        .iter()
        .filter(|turn| turn.event.kind().as_str() == "unattended_printer_activity")
        .collect();
    assert_eq!(on_activity.len(), 1, "{turns:?}");
    assert_eq!(on_activity[0].print_id.to_string(), print_id);
    let noticed = on_activity[0]
        .situation
        .noticed
        .as_deref()
        .unwrap_or_default();
    assert!(
        noticed.starts_with("unattended_printer_activity") && noticed.contains("200"),
        "{noticed}"
    );
    // `M114` was sent only while no job ran.
    let commands = watch.octoprint.commands();
    assert!(!commands.is_empty(), "no position was asked for");
    assert!(
        commands
            .iter()
            .all(|(line, printing)| line == "M114" && !printing),
        "{commands:?}"
    );
}

/// Heating after a failed print is recorded too.
#[tokio::test(flavor = "multi_thread")]
async fn heating_after_a_failed_print_is_recorded() {
    let watch = Watch::open().await;
    watch.connected().await;
    let print_id = watch.printing(400).await;
    watch.octoprint.pushes_event(
        "Error",
        &json!({ "error": "Thermal Runaway", "reason": "firmware" }),
    );
    tokio::time::sleep(POLL).await;
    watch.octoprint.reports(
        "Closed",
        landed_offline(
            "Offline after error",
            Some("Thermal Runaway, system stopped!"),
        ),
    );
    watch.until("print_ended", 1).await;
    assert_eq!(ended(&watch.history().await, &print_id), "failed");
    watch.octoprint.reports("Operational", JobScript::idle());

    watch.octoprint.targets(0.0, 55.0);
    let activity = watch.until("unattended_printer_activity", 1).await;

    assert_eq!(activity[0]["payload"]["last_print_id"], print_id.as_str());
    assert_eq!(activity[0]["payload"]["bed_target_c"], 55.0);
    let turns = watch.turns(2).await;
    assert!(
        turns
            .iter()
            .any(|turn| turn.event.kind().as_str() == "unattended_printer_activity"),
        "{turns:?}"
    );
}

/// With the push socket down no position can be read, so the same heating is
/// recorded with `moved` unknown, and no command is sent at all.
#[tokio::test(flavor = "multi_thread")]
async fn heating_with_the_push_socket_down_is_recorded_with_motion_unknown() {
    let watch = Watch::open().await;
    watch.connected().await;
    let print_id = interrupted(&watch).await;
    watch.octoprint.refuses_sockets(true);
    watch.octoprint.drops_sockets();
    tokio::time::sleep(POLL * 3).await;

    watch.octoprint.targets(180.0, 0.0);
    let activity = watch.until("unattended_printer_activity", 1).await;

    assert_eq!(activity[0]["payload"]["last_print_id"], print_id.as_str());
    assert_eq!(activity[0]["payload"]["moved"], Value::Null);
    let detail = activity[0]["payload"]["detail"]
        .as_str()
        .unwrap_or_default();
    assert!(
        detail.contains(printobserver_core::PUSH_CHANNEL_DOWN),
        "{detail}"
    );
}

/// A target changing on every poll is recorded once per change, but its agent
/// is given at most one turn on activity per observation interval; the changes
/// between turns are in the context the next turn reads.
#[tokio::test(flavor = "multi_thread")]
async fn activity_changing_every_poll_gives_at_most_one_turn_per_interval() {
    let interval = Duration::from_secs(3);
    let watch = Watch::start(
        |_| {},
        |config| {
            config.printer_poll = POLL;
            config.observation_interval = interval;
        },
    )
    .await;
    watch.connected().await;
    let print_id = interrupted(&watch).await;

    for (count, target) in [(1, 150.0), (2, 160.0), (3, 170.0)] {
        watch.octoprint.targets(target, 0.0);
        watch.until("unattended_printer_activity", count).await;
    }
    let activity_turns = |turns: &[TurnRequest]| {
        turns
            .iter()
            .filter(|turn| turn.event.kind().as_str() == "unattended_printer_activity")
            .count()
    };
    assert_eq!(
        activity_turns(&watch.agent.turns()),
        1,
        "a change inside the interval gave a turn"
    );

    tokio::time::sleep(interval).await;
    watch.octoprint.targets(180.0, 0.0);
    watch.until("unattended_printer_activity", 4).await;
    let deadline = tokio::time::Instant::now() + PATIENCE;
    while activity_turns(&watch.agent.turns()) < 2 {
        assert!(
            tokio::time::Instant::now() < deadline,
            "no second turn on activity"
        );
        tokio::time::sleep(Duration::from_millis(20)).await;
    }

    let context = watch.get(&format!("/prints/{print_id}/context")).await;
    let recent: Vec<f64> = context["context"]["recent_events"]
        .as_array()
        .expect("recent events")
        .iter()
        .filter(|event| event["kind"] == "unattended_printer_activity")
        .filter_map(|event| event["payload"]["tools"][0]["target_c"].as_f64())
        .collect();
    for target in [150.0, 160.0, 170.0, 180.0] {
        assert!(recent.contains(&target), "{target} is not among {recent:?}");
    }
}

/// Activity outside the window after an interrupted print records nothing.
#[tokio::test(flavor = "multi_thread")]
async fn activity_outside_the_window_records_nothing() {
    let watch = Watch::start(
        |_| {},
        |config| config.unattended_window = Duration::from_secs(1),
    )
    .await;
    watch.connected().await;
    interrupted(&watch).await;
    tokio::time::sleep(Duration::from_secs(2)).await;

    watch.octoprint.targets(200.0, 60.0);
    tokio::time::sleep(POLL * 8).await;

    assert!(
        watch
            .of_kind("unattended_printer_activity")
            .await
            .is_empty()
    );
}

/// Activity after a cancelled or a completed print records nothing.
#[tokio::test(flavor = "multi_thread")]
async fn activity_after_a_cancelled_or_completed_print_records_nothing() {
    let watch = Watch::open().await;
    watch.connected().await;
    watch.printing(400).await;
    watch.octoprint.reports("Operational", landed_idle());
    watch.until("print_ended", 1).await;
    watch.octoprint.targets(200.0, 60.0);
    tokio::time::sleep(POLL * 8).await;
    assert!(
        watch
            .of_kind("unattended_printer_activity")
            .await
            .is_empty()
    );

    watch.octoprint.targets(0.0, 0.0);
    watch.octoprint.reports(
        "Printing",
        JobScript::printing("second.gcode", SIZE, 700, 600),
    );
    watch.until("print_started", 2).await;
    watch.octoprint.reports(
        "Operational",
        JobScript {
            file: Some(("second.gcode".to_owned(), SIZE)),
            completion: Some(100.0),
            filepos: Some(SIZE),
            ..JobScript::idle()
        },
    );
    watch.until("print_ended", 2).await;
    watch.octoprint.targets(200.0, 60.0);
    tokio::time::sleep(POLL * 8).await;
    assert!(
        watch
            .of_kind("unattended_printer_activity")
            .await
            .is_empty()
    );
}

/// What the watch records during a print is in the context the print's next
/// turn reads, the printer's own events among the print's.
#[tokio::test(flavor = "multi_thread")]
async fn what_the_watch_records_during_a_print_is_in_its_next_turns_context() {
    let watch = Watch::open().await;
    watch.connected().await;
    let print_id = watch.printing(400).await;
    watch.octoprint.pushes_event("PrinterReset", &json!({}));
    watch.until("printer_reset", 1).await;
    watch.octoprint.reports(
        "Closed",
        landed_offline("Offline after error", Some("SerialTimeoutException: ")),
    );
    watch.until("print_ended", 1).await;
    watch.turns(1).await;

    let context = watch.get(&format!("/prints/{print_id}/context")).await;
    let kinds: Vec<String> = context["context"]["recent_events"]
        .as_array()
        .expect("recent events")
        .iter()
        .filter_map(|event| event["kind"].as_str().map(str::to_owned))
        .collect();
    for kind in ["printer_reset", "printer_connection_changed", "print_ended"] {
        assert!(
            kinds.iter().any(|held| held == kind),
            "{kind} is not among {kinds:?}"
        );
    }
    assert_eq!(
        context["context"]["detector"],
        json!({ "status": "unchecked", "since": null })
    );
}
