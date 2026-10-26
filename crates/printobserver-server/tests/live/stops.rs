//! How a print stops, against `OctoPrint`'s own virtual printer.
//!
//! Each stop is caused the way the observability spike caused it, through the
//! virtual printer's own triggers typed at the real instance's terminal — a
//! serial drop, a Marlin fatal error, a cancel from the printer's menu — or
//! through `OctoPrint`'s own job endpoint, or by a print run to its end; and
//! one is the operator's cancel through this server. A real server watches the
//! real instance through the real adapter, with its watch reading the machine
//! every second, and what is asserted is what the printer history answers:
//! how each print ended, the connection going offline where it went, no error
//! recorded for the printer read the instance refuses while offline, and —
//! from the instance's own serial log — no `M114` sent while a job ran. After
//! the serial drop, a move typed while no job runs and every heater target is
//! zero is recorded as the printer moving on its own.
//!
//! Every stop is put back: an offline printer is reconnected, and the walk
//! ends with the hold print running again, as `just octoprint-up` left it.

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use printobserver_core::store::Stores;
use printobserver_obico::{ObicoVision, ObicoVisionConfig};
use printobserver_octoprint::standin::Terminal;
use printobserver_octoprint::{OctoPrintConfig, OctoPrintPrinter};
use printobserver_printer_api::{PrinterPort as _, PrinterState};
use printobserver_server::{Ports, Running, Server, ServerConfig};
use printobserver_store_sqlite::SqliteStore;
use printobserver_types::serde_json::{Value, json};
use tempfile::TempDir;

use crate::scripted::Scripted;

/// A harness that runs no turn: what this walk proves is what the server
/// records of the machine, and a turn it claimed is recorded as the harness
/// refusing it rather than run.
struct Absent;

impl printobserver_supervisor_api::SupervisorPort for Absent {
    fn run_turn(
        &self,
        _request: printobserver_supervisor_api::TurnRequest,
        _access: Arc<dyn printobserver_supervisor_api::TurnAccess>,
    ) -> printobserver_supervisor_api::BoxFuture<
        '_,
        Result<
            printobserver_supervisor_api::TurnOutcome,
            printobserver_supervisor_api::SupervisorError,
        >,
    > {
        Box::pin(async {
            Err(printobserver_supervisor_api::SupervisorError::Unavailable {
                detail: "this walk runs no turn".to_owned(),
            })
        })
    }

    fn close_session(
        &self,
        _print_id: printobserver_types::PrintId,
        _close_reason: String,
    ) -> printobserver_supervisor_api::BoxFuture<
        '_,
        Result<(), printobserver_supervisor_api::SupervisorError>,
    > {
        Box::pin(async { Ok(()) })
    }
}

/// How often this walk's watch reads the machine.
const POLL: Duration = Duration::from_secs(1);

/// How long one step waits for what it caused.
const PATIENCE: Duration = Duration::from_secs(60);

/// The file the scripted environment uploads, which the walk holds printing.
const HOLD_FILE: &str = "hold.gcode";

/// The short file a print is run to its end with.
const FINISH_FILE: &str = "finish.gcode";

/// A real server watching the real instance.
struct Watching {
    /// Its root.
    _root: TempDir,
    /// The server.
    server: Running,
    /// The client, presenting the operator's credential.
    client: reqwest::Client,
}

/// The operator credential this walk's server admits.
const CREDENTIAL: &str = "a-stop-walks-own-operator-credential-Q8w3";

impl Watching {
    /// A server over a fresh root watching the instance.
    async fn open(instance: &Scripted) -> Self {
        let root = TempDir::new().expect("a root");
        let skill =
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../skills/printobserver/SKILL.md");
        let text = format!(
            r#"
state_dir = {state}
listen = "127.0.0.1:0"
[octoprint]
url = "{url}"
api_key = "{key}"
[supervisor]
harness = "claude-code"
skill_path = {skill}
[ingress]
shared_secret = "a-stop-walks-own-secret"
[api]
credential_verifier = "{verifier}"
[safety]
agent_min_interval_s = 0
[safety.allowed]
feedrate = {{ min = 0.5, max = 1.5 }}
[safety.actions]
operator = ["cancel"]
agent = []
system = []
"#,
            state = toml::Value::String(root.path().join("state").display().to_string()),
            url = instance.url,
            key = instance.api_key,
            skill = toml::Value::String(skill.display().to_string()),
            verifier = printobserver_server::CredentialVerifier::of(CREDENTIAL),
        );
        let path = root.path().join("config.toml");
        std::fs::write(&path, text).expect("the configuration is writable");
        let mut config = ServerConfig::load(&path).expect("the configuration is accepted");
        config.printer_poll = POLL;
        let stores = Stores::of(Arc::new(
            SqliteStore::open(&config.state_dir).expect("the store opens"),
        ));
        let server = Server::start_with(
            config.clone(),
            Ports {
                printer: Arc::new(OctoPrintPrinter::new(config.octoprint.clone())),
                stores,
                vision: Arc::new(ObicoVision::new(ObicoVisionConfig::default()).expect("built")),
                agent: Arc::new(Absent),
            },
        )
        .await
        .expect("the server starts");
        let mut headers = reqwest::header::HeaderMap::new();
        headers.insert(
            reqwest::header::AUTHORIZATION,
            reqwest::header::HeaderValue::from_str(&format!("Bearer {CREDENTIAL}"))
                .expect("a header value"),
        );
        Self {
            _root: root,
            server,
            client: reqwest::Client::builder()
                .default_headers(headers)
                .build()
                .expect("a client"),
        }
    }

    /// One JSON read of the versioned surface.
    async fn get(&self, path: &str) -> Value {
        self.client
            .get(format!("{}{path}", self.server.base_url()))
            .send()
            .await
            .expect("the server answers")
            .json()
            .await
            .expect("a JSON answer")
    }

    /// Every event the printer history answers, oldest first.
    async fn history(&self) -> Vec<Value> {
        let mut events = self.get("/history?limit=500").await["events"]
            .as_array()
            .cloned()
            .unwrap_or_default();
        events.reverse();
        events
    }

    /// Wait until the history holds an event the predicate accepts, and
    /// answer it.
    async fn until(&self, what: &str, accepts: impl Fn(&Value) -> bool) -> Value {
        let deadline = tokio::time::Instant::now() + PATIENCE;
        loop {
            if let Some(found) = self
                .history()
                .await
                .into_iter()
                .find(|event| accepts(event))
            {
                return found;
            }
            assert!(
                tokio::time::Instant::now() < deadline,
                "{what} was never recorded: {:#}",
                Value::Array(self.history().await)
            );
            tokio::time::sleep(Duration::from_millis(250)).await;
        }
    }

    /// Wait for the next print the watch adopts after `after` prints, and
    /// answer its identifier.
    async fn adopted(&self, after: usize) -> String {
        let deadline = tokio::time::Instant::now() + PATIENCE;
        loop {
            let started: Vec<Value> = self
                .history()
                .await
                .into_iter()
                .filter(|event| event["kind"] == "print_started")
                .collect();
            if started.len() > after {
                return started[after]["print_id"]
                    .as_str()
                    .expect("a print")
                    .to_owned();
            }
            assert!(
                tokio::time::Instant::now() < deadline,
                "no print was adopted"
            );
            tokio::time::sleep(Duration::from_millis(250)).await;
        }
    }

    /// Wait for one print to end, and answer how.
    async fn ending(&self, print_id: &str) -> String {
        let ended = self
            .until(&format!("the end of {print_id}"), |event| {
                event["kind"] == "print_ended" && event["print_id"] == print_id
            })
            .await;
        ended["payload"]["outcome"]
            .as_str()
            .unwrap_or_default()
            .to_owned()
    }

    /// Every failure reading the printer the history holds for one print.
    async fn read_failures_of(&self, print_id: &str) -> Vec<Value> {
        self.get(&format!("/prints/{print_id}/history")).await["events"]
            .as_array()
            .cloned()
            .unwrap_or_default()
            .into_iter()
            .filter(|event| {
                event["kind"] == "port_failure"
                    && ["printer_snapshot", "printer_job"]
                        .contains(&event["payload"]["site"].as_str().unwrap_or_default())
            })
            .collect()
    }

    /// How many times the history records the printer going offline.
    async fn offline_count(&self) -> usize {
        self.history()
            .await
            .iter()
            .filter(|event| {
                event["kind"] == "printer_connection_changed" && event["payload"]["to"] == "offline"
            })
            .count()
    }

    /// Wait until the printer has gone offline more than `before` times.
    async fn went_offline_after(&self, before: usize) {
        let deadline = tokio::time::Instant::now() + PATIENCE;
        while self.offline_count().await <= before {
            assert!(
                tokio::time::Instant::now() < deadline,
                "going offline was never recorded"
            );
            tokio::time::sleep(Duration::from_millis(250)).await;
        }
    }
}

/// The adapter and the terminal for the instance itself.
fn reach(instance: &Scripted) -> (OctoPrintPrinter, Terminal) {
    let config = OctoPrintConfig::new(&instance.url, instance.api_key.clone())
        .expect("the instance's configuration");
    (OctoPrintPrinter::new(config.clone()), Terminal::new(config))
}

/// Wait until the machine's job reads one state.
async fn until_job(printer: &OctoPrintPrinter, wanted: &PrinterState) {
    let deadline = tokio::time::Instant::now() + PATIENCE;
    loop {
        if printer.job().await.is_ok_and(|job| job.state == *wanted) {
            return;
        }
        assert!(
            tokio::time::Instant::now() < deadline,
            "the machine never reported {wanted:?}"
        );
        tokio::time::sleep(Duration::from_millis(250)).await;
    }
}

/// Wait until the machine has finished stopping whatever it was stopping.
async fn settled(printer: &OctoPrintPrinter) {
    let deadline = tokio::time::Instant::now() + PATIENCE;
    while printer
        .job()
        .await
        .is_ok_and(|job| job.state == PrinterState::Cancelling)
    {
        assert!(
            tokio::time::Instant::now() < deadline,
            "the machine never finished cancelling"
        );
        tokio::time::sleep(Duration::from_millis(250)).await;
    }
}

/// Start the hold print on an idle machine, waiting out the moment after a
/// cancel in which `OctoPrint` still refuses a start as already printing.
async fn hold_printing(printer: &OctoPrintPrinter) {
    let deadline = tokio::time::Instant::now() + PATIENCE;
    while !printer
        .job()
        .await
        .is_ok_and(|job| job.state == PrinterState::Printing)
    {
        match printer
            .start(printobserver_types::FileName::new(HOLD_FILE).expect("a file name"))
            .await
        {
            Ok(()) | Err(printobserver_printer_api::PrinterError::StateConflict { .. }) => {}
            Err(error) => panic!("the hold print would not start: {error}"),
        }
        assert!(
            tokio::time::Instant::now() < deadline,
            "the hold print never started"
        );
        tokio::time::sleep(Duration::from_millis(500)).await;
    }
}

/// Bring a printer that went offline back, idle.
async fn reconnected(printer: &OctoPrintPrinter, terminal: &Terminal) {
    terminal.connect().expect("the printer reconnects");
    until_job(printer, &PrinterState::Operational).await;
}

/// The instance's own serial log.
fn serial_log() -> String {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../.octoprint-env/instance/logs/serial.log");
    std::fs::read_to_string(path).unwrap_or_default()
}

/// The `M114`s the serial log records sent after `from` bytes of it.
fn m114_sent_after(from: usize) -> Vec<String> {
    serial_log()
        .get(from..)
        .unwrap_or_default()
        .lines()
        .filter(|line| line.contains("Send:") && line.contains("M114"))
        .map(str::to_owned)
        .collect()
}

/// Upload the short file a print is run to its end with.
fn upload_finish_file() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    let file = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/finish.gcode");
    let status = std::process::Command::new("uv")
        .args([
            "run",
            "-q",
            "python",
            "tools/octoprint-env/octoprint_env.py",
            "upload",
        ])
        .arg("--file")
        .arg(&file)
        .current_dir(&root)
        .status()
        .expect("the environment's upload runs");
    assert!(status.success(), "the finish file was not uploaded");
}

/// One stop, caused while the hold print runs: the print ends as `outcome`,
/// no `M114` was sent while it ran, and nothing was recorded failing — the
/// printer read the instance refuses while offline included.
async fn stop(
    watch: &Watching,
    instance: &Scripted,
    adopted: usize,
    cause: impl AsyncFnOnce(),
    outcome: &str,
) -> String {
    let (printer, terminal) = reach(instance);
    settled(&printer).await;
    if printer
        .job()
        .await
        .is_ok_and(|job| job.state == PrinterState::Offline)
    {
        reconnected(&printer, &terminal).await;
    }
    hold_printing(&printer).await;
    let print_id = watch.adopted(adopted).await;
    // From the moment the print is known running to the moment its end is
    // read, the only commands the watch could have sent are ones sent while a
    // job ran: none of these stops leaves the printer idle in the window.
    let log_before = serial_log().len();
    cause().await;
    let ended = watch.ending(&print_id).await;
    let sent = m114_sent_after(log_before);
    assert_eq!(ended, outcome, "print {print_id} ended otherwise");
    assert!(sent.is_empty(), "M114 was sent while a job ran: {sent:?}");
    assert_eq!(watch.read_failures_of(&print_id).await, Vec::<Value>::new());
    print_id
}

/// Every stop, in the one order a shared machine admits.
pub async fn walk(instance: &Scripted) {
    let watch = Watching::open(instance).await;
    the_stops_that_go_offline(&watch, instance).await;
    the_stops_that_land_idle(&watch, instance).await;
    // Put back as `just octoprint-up` left it: the hold print running.
    let (printer, _) = reach(instance);
    settled(&printer).await;
    hold_printing(&printer).await;
}

/// A serial drop ends `interrupted` and a printer error `failed`, each going
/// offline; between them, a move typed while no job runs after the drop is
/// the printer moving on its own.
async fn the_stops_that_go_offline(watch: &Watching, instance: &Scripted) {
    let (printer, terminal) = reach(instance);
    let dropped = stop(
        watch,
        instance,
        0,
        async || terminal.send("!!DEBUG:drop_connection").expect("sent"),
        "interrupted",
    )
    .await;
    watch.went_offline_after(0).await;

    // With the print interrupted and the printer back, idle and cold, a move
    // typed at its terminal is the printer moving on its own.
    reconnected(&printer, &terminal).await;
    terminal.send("M104 S0").expect("sent");
    terminal.send("M140 S0").expect("sent");
    tokio::time::sleep(POLL * 4).await;
    terminal.send("G1 X50 Y60 Z20 F6000").expect("sent");
    let moved = watch
        .until("the printer moving on its own", |event| {
            event["kind"] == "unattended_printer_activity" && event["payload"]["moved"] == true
        })
        .await;
    assert_eq!(moved["payload"]["last_print_id"], dropped.as_str());
    assert_eq!(moved["payload"]["tools"], json!([]));

    let before = watch.offline_count().await;
    stop(
        watch,
        instance,
        1,
        async || {
            terminal
                .send("!!DEBUG:trigger_fatal_error_marlin")
                .expect("sent");
        },
        "failed",
    )
    .await;
    watch.went_offline_after(before).await;
}

/// A cancel from the printer's menu and one through `OctoPrint`'s own job
/// endpoint end `cancelled`; a print run to its end `completed`; and a print
/// the operator cancels through printobserver `cancelled`.
async fn the_stops_that_land_idle(watch: &Watching, instance: &Scripted) {
    let (printer, terminal) = reach(instance);
    stop(
        watch,
        instance,
        2,
        async || terminal.send("!!DEBUG:action_custom cancel").expect("sent"),
        "cancelled",
    )
    .await;
    stop(
        watch,
        instance,
        3,
        async || printer.cancel().await.expect("OctoPrint cancels its job"),
        "cancelled",
    )
    .await;

    settled(&printer).await;
    until_job(&printer, &PrinterState::Operational).await;
    upload_finish_file();
    printer
        .start(printobserver_types::FileName::new(FINISH_FILE).expect("a file name"))
        .await
        .expect("the finish file starts");
    let finished = watch.adopted(4).await;
    assert_eq!(watch.ending(&finished).await, "completed");

    hold_printing(&printer).await;
    let operated = watch.adopted(5).await;
    let answer = watch
        .client
        .post(format!(
            "{}/prints/{operated}/actions/cancel",
            watch.server.base_url()
        ))
        .json(&json!({ "reason": "the operator stopped it", "actor": "operator" }))
        .send()
        .await
        .expect("the server answers");
    assert!(answer.status().is_success(), "{}", answer.status());
    assert_eq!(watch.ending(&operated).await, "cancelled");
}
