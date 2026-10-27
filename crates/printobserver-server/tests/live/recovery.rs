//! The agent's granted recoveries, against `OctoPrint`'s own virtual printer.
//!
//! A real server over the real instance, configured to grant the agent a lift
//! and a cool and nothing else. The agent holds a real supervision turn — the
//! stand-in harness is issued its credential the way the adapter is, and
//! waits — and asks for each through the real API with that credential. What
//! is asserted is what the machine itself reports: its position over the push
//! socket and its heater targets. A recovery it is not granted is refused and
//! leaves both as they were.
//!
//! It puts the hold print back running, as `just octoprint-up` left it.

use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use printobserver_core::store::{PrintStore as _, Stores};
use printobserver_obico::{ObicoVision, ObicoVisionConfig};
use printobserver_octoprint::{OctoPrintConfig, OctoPrintPrinter};
use printobserver_printer_api::{PrinterPort as _, PrinterState};
use printobserver_server::{Ports, Running, Server, ServerConfig};
use printobserver_store_sqlite::SqliteStore;
use printobserver_supervisor_api::{
    AgentAssessment, CREDENTIAL_ENV, Confidence, SessionPhase, SupervisionSession, SupervisorError,
    SupervisorPort, TurnAccess, TurnOutcome, TurnRequest, TurnSession,
};
use printobserver_types::serde_json::{Value, json};
use tempfile::TempDir;

use crate::minted::minted;
use crate::scripted::Scripted;

/// An agent that is issued its turn's credential, as the harness adapter is,
/// and holds the turn open until it is released, so the credential stays
/// admitted while the journey asks with it.
#[derive(Default)]
struct Holding {
    /// The session and credential the one turn was issued.
    pass: Mutex<Option<(String, String)>>,
    /// Raised to let the held turn return.
    released: tokio::sync::Notify,
}

impl Holding {
    /// Wait until a turn was issued its credential, answering its session and
    /// credential.
    async fn issued(&self) -> (String, String) {
        let deadline = tokio::time::Instant::now() + PATIENCE;
        loop {
            if let Some(pass) = self.pass.lock().expect("the agent holds").clone() {
                return pass;
            }
            assert!(
                tokio::time::Instant::now() < deadline,
                "no turn was issued a credential"
            );
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
    }
}

impl SupervisorPort for Holding {
    fn run_turn(
        &self,
        request: TurnRequest,
        access: Arc<dyn TurnAccess>,
    ) -> printobserver_supervisor_api::BoxFuture<'_, Result<TurnOutcome, SupervisorError>> {
        let session_name = format!("recovery-{}", request.print_id);
        let issued = match TurnSession::new(session_name.as_str())
            .and_then(|session| access.issue(&session))
        {
            Ok(issued) => issued,
            Err(error) => return Box::pin(async move { Err(error) }),
        };
        let credential = issued
            .environment()
            .iter()
            .find(|(named, _)| *named == CREDENTIAL_ENV)
            .map(|(_, value)| value.clone())
            .unwrap_or_default();
        *self.pass.lock().expect("the agent holds") = Some((session_name.clone(), credential));
        let now = printobserver_types::Timestamp::now();
        let session = SupervisionSession {
            print_id: request.print_id,
            session_name,
            harness_identity: "claude-code".to_owned(),
            created_at: now,
            last_turn_at: now,
            closed_at: None,
            close_reason: None,
        };
        Box::pin(async move {
            // Bounded, so a journey that failed before releasing it does not
            // hold the runtime open behind it.
            let _ = tokio::time::timeout(PATIENCE, self.released.notified()).await;
            Ok(TurnOutcome {
                session,
                phase: SessionPhase::Created,
                assessment: AgentAssessment {
                    summary: "the nozzle is free of the part".to_owned(),
                    confidence: Confidence::High,
                    should_continue: false,
                    did: "lifted the nozzle and cooled the heaters".to_owned(),
                    why: "the print had stopped with the nozzle on the part".to_owned(),
                    escalating: false,
                },
            })
        })
    }

    fn close_session(
        &self,
        _print_id: printobserver_types::PrintId,
        _close_reason: String,
    ) -> printobserver_supervisor_api::BoxFuture<'_, Result<(), SupervisorError>> {
        Box::pin(async { Ok(()) })
    }
}

/// How long one step waits for the machine.
const PATIENCE: Duration = Duration::from_secs(60);

/// The file the scripted environment holds printing.
const HOLD_FILE: &str = "hold.gcode";

/// The provider identifier the held print carries, which the alert names.
const OBICO_PRINT: i64 = 4211;

/// One request to the API with one credential, answering its status and body.
async fn asked(
    client: &reqwest::Client,
    url: String,
    body: Option<&Value>,
) -> (reqwest::StatusCode, Value) {
    let request = match body {
        Some(body) => client.post(url).json(body),
        None => client.get(url),
    };
    let response = request.send().await.expect("the server answers");
    let status = response.status();
    (status, response.json().await.unwrap_or(Value::Null))
}

/// A client presenting one credential.
fn presenting(credential: &str) -> reqwest::Client {
    let mut headers = reqwest::header::HeaderMap::new();
    headers.insert(
        reqwest::header::AUTHORIZATION,
        reqwest::header::HeaderValue::from_str(&format!("Bearer {credential}"))
            .expect("a header value"),
    );
    reqwest::Client::builder()
        .default_headers(headers)
        .build()
        .expect("a client")
}

/// Where the machine reports the head.
async fn head(printer: &OctoPrintPrinter) -> [f64; 3] {
    let at = printer.read_position().await.expect("the position reads");
    [at.x, at.y, at.z]
}

/// The machine's tool and bed targets.
async fn targets(printer: &OctoPrintPrinter) -> (Option<f64>, Option<f64>) {
    let snapshot = printer.snapshot().await.expect("the machine reads");
    (
        snapshot
            .tools
            .first()
            .and_then(|tool| tool.target_c)
            .map(|target| target.value()),
        snapshot
            .bed
            .and_then(|bed| bed.target_c)
            .map(|target| target.value()),
    )
}

/// Wait until one read of the machine answers what is wanted.
async fn until<T: PartialEq + core::fmt::Debug>(
    describing: &str,
    wanted: T,
    mut read: impl AsyncFnMut() -> T,
) {
    let deadline = tokio::time::Instant::now() + PATIENCE;
    loop {
        let found = read().await;
        if found == wanted {
            return;
        }
        assert!(
            tokio::time::Instant::now() < deadline,
            "{describing}: the machine reports {found:?}, not {wanted:?}"
        );
        tokio::time::sleep(Duration::from_millis(250)).await;
    }
}

/// Wait until the machine's job reads one state.
async fn job_is(printer: &OctoPrintPrinter, wanted: PrinterState) {
    until(
        &format!("the job reading {wanted:?}"),
        Some(wanted),
        async || printer.job().await.ok().map(|job| job.state),
    )
    .await;
}

/// Start the hold print on an idle machine again.
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

/// A real server over the instance, granting the agent a lift and a cool,
/// with the print the alert is about open in its store: its root, the
/// server, the agent and the ingress secret.
async fn started(instance: &Scripted) -> (TempDir, Running, Arc<Holding>, String) {
    let root = TempDir::new().expect("a root");
    let credential = minted("operator-credential");
    let secret = minted("ingress-secret");
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
shared_secret = "{secret}"
[api]
credential_verifier = "{verifier}"
[safety]
agent_min_interval_s = 0
[safety.allowed]
"tool_target:0" = {{ min = 0.0, max = 260.0 }}
[safety.actions]
operator = ["cancel"]
agent = []
system = []
[recovery]
away_from_bed = "z_plus"
max_z_mm = 200.0
park_xy = [10.0, 200.0]
z_home_xy = [100.0, 100.0]
z_home_sweeps_bed = false
[policy.recovery]
agent = ["recover_lift", "recover_cool"]
"#,
        state = toml::Value::String(root.path().join("state").display().to_string()),
        url = instance.url,
        key = instance.api_key,
        skill = toml::Value::String(skill.display().to_string()),
        verifier = printobserver_server::CredentialVerifier::of(&credential),
    );
    let path = root.path().join("config.toml");
    std::fs::write(&path, text).expect("the configuration is writable");
    let config = ServerConfig::load(&path).expect("the configuration is accepted");
    let store = Arc::new(SqliteStore::open(&config.state_dir).expect("the store opens"));
    // The print the alert is about, which the turn is held for.
    store
        .open_print(Some(OBICO_PRINT), Some(HOLD_FILE.to_owned()))
        .await
        .expect("a print opens");
    let agent = Arc::new(Holding::default());
    let server = Server::start_with(
        config.clone(),
        Ports {
            printer: Arc::new(OctoPrintPrinter::new(config.octoprint.clone())),
            stores: Stores::of(store),
            vision: Arc::new(ObicoVision::new(ObicoVisionConfig::default()).expect("built")),
            agent: Arc::clone(&agent) as Arc<dyn printobserver_supervisor_api::SupervisorPort>,
        },
    )
    .await
    .expect("the server starts");
    (root, server, agent, secret)
}

/// The agent, granted a lift and a cool, lifts the nozzle and cools the
/// heaters of the real machine, and is refused a heat it is not granted.
pub async fn walk(instance: &Scripted) {
    let (_root, server, agent, secret) = started(instance).await;
    let printer = OctoPrintPrinter::new(
        OctoPrintConfig::new(&instance.url, instance.api_key.clone())
            .expect("the instance's configuration"),
    );

    // A turn, held open: its credential is the agent's.
    job_is(&printer, PrinterState::Printing).await;
    let mut alert: Value = printobserver_types::serde_json::from_str(include_str!(
        "../../../printobserver-obico/samples/obico/failure-alert.json"
    ))
    .expect("the committed sample is JSON");
    alert["print"]["id"] = json!(OBICO_PRINT);
    alert["img_url"] = json!("http://127.0.0.1:1/frame.jpg");
    let posted = reqwest::Client::new()
        .post(format!("{}?token={secret}", server.ingress_url()))
        .json(&alert)
        .send()
        .await
        .expect("the ingress answers")
        .status();
    assert_eq!(posted, reqwest::StatusCode::ACCEPTED);
    let (session_name, credential) = agent.issued().await;
    let turn = presenting(&credential);
    let actor = json!({ "agent": { "session_name": session_name } });
    let recovery = |action: &str| format!("{}/printer/recovery/{action}", server.base_url());

    // Recovery is asked of an idle machine.
    printer.cancel().await.expect("the hold print cancels");
    job_is(&printer, PrinterState::Operational).await;

    let before = head(&printer).await;
    let (status, answer) = asked(
        &turn,
        recovery("recover_lift"),
        Some(&json!({ "mm": 5.0, "actor": actor, "reason": "free the nozzle" })),
    )
    .await;
    assert_eq!(status, reqwest::StatusCode::OK, "{answer:#}");
    until(
        "the agent's lift",
        [before[0], before[1], before[2] + 5.0],
        async || head(&printer).await,
    )
    .await;

    printer
        .set_tool_target_c(0, 150.0)
        .await
        .expect("the tool heats");
    printer.set_bed_target_c(50.0).await.expect("the bed heats");
    until("the heaters set", (Some(150.0), Some(50.0)), async || {
        targets(&printer).await
    })
    .await;
    let (status, answer) = asked(
        &turn,
        recovery("recover_cool"),
        Some(&json!({ "actor": actor, "reason": "the part is free" })),
    )
    .await;
    assert_eq!(status, reqwest::StatusCode::OK, "{answer:#}");
    until("the agent's cool", (Some(0.0), Some(0.0)), async || {
        targets(&printer).await
    })
    .await;

    // A heat the agent is not granted reaches nothing.
    let at = head(&printer).await;
    let (status, answer) = asked(
        &turn,
        recovery("recover_heat"),
        Some(&json!({
            "tool": 0, "target_c": 150.0, "clearance_asserted": true,
            "actor": actor, "reason": "warm the nozzle",
        })),
    )
    .await;
    assert_eq!(status, reqwest::StatusCode::CONFLICT, "{answer:#}");
    assert_eq!(
        answer["rejection"],
        json!({ "actor_may_not_request": { "action": "recover_heat" } })
    );
    until(
        "the targets after the refusal",
        (Some(0.0), Some(0.0)),
        async || targets(&printer).await,
    )
    .await;
    until("the head after the refusal", at, async || {
        head(&printer).await
    })
    .await;

    agent.released.notify_one();
    server.stop().await;
    hold_printing(&printer).await;
}
