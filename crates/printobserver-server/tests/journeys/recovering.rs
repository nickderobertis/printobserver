//! Recovery, against a real server and the real `OctoPrint` adapter.
//!
//! The printer is the scripted `OctoPrint` the adapter's `standin` feature
//! compiles — a real HTTP server and a real websocket on a loopback socket —
//! because these are the states `OctoPrint`'s own virtual printer cannot be
//! held in or made to produce on demand: a job running or cancelling for as
//! long as a journey needs, a printer offline or in error, a reset while idle
//! that changes nothing a read reports. Everything on this side of that socket
//! is the real program: the adapter, the watch, the policy, the store and the
//! API. The virtual-printer journeys are the integration tier's.
//!
//! What is asserted is what an operator or the agent can observe — the answer,
//! the recovery read and the printer history — and what the machine was
//! asked: every print-head request, every terminal line and every heater
//! target the stand-in received. Each refusal leaves all three as they were.

use std::sync::Arc;
use std::time::Duration;

use printobserver_core::store::{PrintStore as _, Stores};
use printobserver_obico::{ObicoVision, ObicoVisionConfig};
use printobserver_octoprint::OctoPrintPrinter;
use printobserver_octoprint::standin::{JobScript, ScriptedOctoPrint};
use printobserver_printer_api::{
    BoxFuture, JobSnapshot, PrinterError, PrinterPort, PrinterSnapshot,
};
use printobserver_server::{Ports, Running, Server, ServerConfig};
use printobserver_store_sqlite::SqliteStore;
use printobserver_types::FileName;
use printobserver_types::serde_json::{Value, json};
use reqwest::StatusCode;
use tempfile::TempDir;

use crate::agent::StandInAgent;
use crate::world::{OPERATOR, SECRET, document, failure_alert, presenting, set, write};

/// How often these journeys' watch reads the machine.
const POLL: Duration = Duration::from_millis(200);

/// How long a journey waits for what the watch should record: a fifth of a
/// second's poll needs far less, and the rest is room for a loaded host.
const PATIENCE: Duration = Duration::from_secs(60);

/// The file a scripted print is of.
const FILE: &str = "part.gcode";

/// The provider identifier the journeys' held print carries.
const OBICO_PRINT: i64 = 4211;

/// The recovery facts of a machine this walk drives.
fn facts() -> toml::Value {
    toml::from_str(
        "away_from_bed = \"z_plus\"\nmax_z_mm = 200.0\nmax_lift_mm = 50.0\n\
         clearance_margin_mm = 5.0\npark_xy = [10.0, 200.0]\nz_home_xy = [100.0, 100.0]\n\
         z_home_sweeps_bed = true\n",
    )
    .expect("the recovery facts are a table")
}

/// A configuration stating the recovery facts, then edited by the journey.
fn with_recovery(edit: impl FnOnce(&mut toml::Value)) -> impl FnOnce(&mut toml::Value) {
    move |document| {
        set(document, "recovery", facts());
        edit(document);
    }
}

/// A printer port that implements every required method and overrides none
/// of the optional ones: no command, no position read and no extent read, as
/// an adapter that has not grown them answers each.
struct Unmoving(OctoPrintPrinter);

impl PrinterPort for Unmoving {
    fn snapshot(&self) -> BoxFuture<'_, Result<PrinterSnapshot, PrinterError>> {
        self.0.snapshot()
    }

    fn job(&self) -> BoxFuture<'_, Result<JobSnapshot, PrinterError>> {
        self.0.job()
    }

    fn start(&self, file_name: FileName) -> BoxFuture<'_, Result<(), PrinterError>> {
        self.0.start(file_name)
    }

    fn pause(&self) -> BoxFuture<'_, Result<(), PrinterError>> {
        self.0.pause()
    }

    fn resume(&self) -> BoxFuture<'_, Result<(), PrinterError>> {
        self.0.resume()
    }

    fn cancel(&self) -> BoxFuture<'_, Result<(), PrinterError>> {
        self.0.cancel()
    }

    fn set_feedrate_factor(&self, factor: f64) -> BoxFuture<'_, Result<(), PrinterError>> {
        self.0.set_feedrate_factor(factor)
    }

    fn set_flowrate_factor(&self, factor: f64) -> BoxFuture<'_, Result<(), PrinterError>> {
        self.0.set_flowrate_factor(factor)
    }

    fn set_tool_target_c(
        &self,
        tool: i64,
        target_c: f64,
    ) -> BoxFuture<'_, Result<(), PrinterError>> {
        self.0.set_tool_target_c(tool, target_c)
    }

    fn set_bed_target_c(&self, target_c: f64) -> BoxFuture<'_, Result<(), PrinterError>> {
        self.0.set_bed_target_c(target_c)
    }

    fn set_fan_percent(&self, percent: f64) -> BoxFuture<'_, Result<(), PrinterError>> {
        self.0.set_fan_percent(percent)
    }
}

/// A real server over the scripted printer, asked for recovery.
struct Recovering {
    /// The journey's root, which a restart keeps.
    root: TempDir,
    /// The server.
    server: Running,
    /// The scripted printer.
    octoprint: ScriptedOctoPrint,
    /// The supervising agent, whose turns a journey can hold open.
    agent: Arc<StandInAgent>,
    /// The client, presenting the operator's credential.
    client: reqwest::Client,
}

impl Recovering {
    /// A server over a fresh scripted printer, its configuration edited.
    async fn start(edit: impl FnOnce(&mut toml::Value)) -> Self {
        let octoprint = ScriptedOctoPrint::start();
        let root = TempDir::new().expect("a journey's own root");
        Self::over(root, octoprint, edit, false).await
    }

    /// The same, over a printer adapter that has grown no command, no
    /// position read and no extent read.
    async fn unmoving(edit: impl FnOnce(&mut toml::Value)) -> Self {
        let octoprint = ScriptedOctoPrint::start();
        let root = TempDir::new().expect("a journey's own root");
        Self::over(root, octoprint, edit, true).await
    }

    /// A server over a root and a printer.
    async fn over(
        root: TempDir,
        octoprint: ScriptedOctoPrint,
        edit: impl FnOnce(&mut toml::Value),
        unmoving: bool,
    ) -> Self {
        let mut configured = document(root.path(), &octoprint.base_url());
        set(
            &mut configured,
            "octoprint.api_key",
            toml::Value::String(octoprint.api_key()),
        );
        edit(&mut configured);
        let path = write(root.path(), &configured);
        let mut config = ServerConfig::load(&path).expect("the configuration is accepted");
        config.printer_poll = POLL;
        let stores = Stores::of(Arc::new(
            SqliteStore::open(&config.state_dir).expect("the store opens"),
        ));
        let adapter = OctoPrintPrinter::new(config.octoprint.clone());
        let printer: Arc<dyn PrinterPort> = if unmoving {
            Arc::new(Unmoving(adapter))
        } else {
            Arc::new(adapter)
        };
        let agent = StandInAgent::new();
        let server = Server::start_with(
            config.clone(),
            Ports {
                printer,
                stores,
                vision: Arc::new(ObicoVision::new(ObicoVisionConfig::default()).expect("built")),
                agent: Arc::clone(&agent) as Arc<dyn printobserver_supervisor_api::SupervisorPort>,
            },
        )
        .await
        .expect("the server starts");
        Self {
            root,
            server,
            octoprint,
            agent,
            client: presenting(OPERATOR),
        }
    }

    /// The same server started again, over the same store and printer.
    async fn restarted(self, edit: impl FnOnce(&mut toml::Value)) -> Self {
        self.server.stop().await;
        Self::over(self.root, self.octoprint, edit, false).await
    }

    /// Ask for one recovery action as the operator.
    async fn ask(&self, action: &str, fields: Value) -> (StatusCode, Value) {
        self.ask_as(&self.client, action, operator_body(fields))
            .await
    }

    /// Ask for one recovery action with one client and body.
    async fn ask_as(
        &self,
        client: &reqwest::Client,
        action: &str,
        body: Value,
    ) -> (StatusCode, Value) {
        let response = client
            .post(format!(
                "{}/printer/recovery/{action}",
                self.server.base_url()
            ))
            .json(&body)
            .send()
            .await
            .expect("the server answers");
        let status = response.status();
        (status, response.json().await.unwrap_or(Value::Null))
    }

    /// The recovery read, as the operator or as one client.
    async fn status_as(&self, client: &reqwest::Client) -> Value {
        let response = client
            .get(format!("{}/printer/recovery", self.server.base_url()))
            .send()
            .await
            .expect("the server answers");
        assert_eq!(response.status(), StatusCode::OK);
        response.json().await.expect("a JSON answer")
    }

    /// The recovery read, as the operator.
    async fn status(&self) -> Value {
        self.status_as(&self.client).await
    }

    /// Every event of the printer history, oldest first.
    async fn history(&self) -> Vec<Value> {
        let mut events: Vec<Value> = self
            .client
            .get(format!("{}/history?limit=500", self.server.base_url()))
            .send()
            .await
            .expect("the server answers")
            .json::<Value>()
            .await
            .expect("a JSON answer")["events"]
            .as_array()
            .cloned()
            .unwrap_or_default();
        events.reverse();
        events
    }

    /// Every recovery event the printer history holds, as its kind and the
    /// action it names, oldest first.
    async fn recorded(&self) -> Vec<(String, String)> {
        self.history()
            .await
            .into_iter()
            .filter(|event| {
                event["kind"]
                    .as_str()
                    .is_some_and(|kind| kind.starts_with("recovery_"))
            })
            .map(|event| {
                (
                    event["kind"].as_str().unwrap_or_default().to_owned(),
                    event["payload"]["action"]["action"]
                        .as_str()
                        .unwrap_or_default()
                        .to_owned(),
                )
            })
            .collect()
    }

    /// Wait until the printer history holds `count` events of one kind.
    async fn until(&self, kind: &str, count: usize) {
        let deadline = tokio::time::Instant::now() + PATIENCE;
        loop {
            let found = self
                .history()
                .await
                .into_iter()
                .filter(|event| event["kind"] == kind)
                .count();
            if found >= count {
                return;
            }
            assert!(
                tokio::time::Instant::now() < deadline,
                "only {found} of {count} `{kind}` were recorded: {:#}",
                Value::Array(self.history().await)
            );
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
    }

    /// Wait until the recovery read reports the printer in one state.
    async fn reads(&self, state: &str) {
        self.reads_as(&json!(state)).await;
    }

    /// Wait until the recovery read answers this state, as it is sent.
    async fn reads_as(&self, state: &Value) {
        let deadline = tokio::time::Instant::now() + PATIENCE;
        while &self.status().await["state"] != state {
            assert!(
                tokio::time::Instant::now() < deadline,
                "the printer never read {state}"
            );
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
    }

    /// Wait until the push socket is open, so a push reaches the server.
    async fn connected(&self) {
        let deadline = tokio::time::Instant::now() + PATIENCE;
        while self.octoprint.open_sockets() == 0 {
            assert!(
                tokio::time::Instant::now() < deadline,
                "the push socket never opened"
            );
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }

    /// Home Z as the operator with the bed asserted clear, and wait until Z
    /// reads trusted.
    async fn home_z(&self) {
        let (status, answer) = self
            .ask("recover_home_z", json!({ "bed_clear_asserted": true }))
            .await;
        assert_eq!(status, StatusCode::OK, "{answer:#}");
        assert!(
            self.status().await["z_trust"]
                .get("trusted_since")
                .is_some(),
            "a Z home the printer took left Z untrusted"
        );
    }

    /// What the machine was asked to move, heat or cool, and the lines it was
    /// sent but the position report, which is a read.
    fn asked_of_the_machine(&self) -> (Vec<Value>, (f64, f64), Vec<String>) {
        (
            self.octoprint.printhead_requests(),
            self.octoprint.reported_targets(),
            self.octoprint
                .commands()
                .into_iter()
                .map(|(line, _)| line)
                .filter(|line| line != "M114")
                .collect(),
        )
    }

    /// Hold one supervision turn open about a print of its own, answering
    /// the agent's credential and its actor.
    async fn agent_turn(&self) -> (reqwest::Client, Value) {
        let state = self.root.path().join("state");
        let store = SqliteStore::open(&state).expect("the store opens");
        store
            .open_print(Some(OBICO_PRINT), Some(FILE.to_owned()))
            .await
            .expect("a print opens");
        // A turn is given about a print the machine is running; once its
        // credential is issued the turn holds it, whatever the print does.
        self.octoprint
            .reports("Printing", JobScript::printing(FILE, 816, 0, 5));
        self.agent.hold_turns();
        let status = reqwest::Client::new()
            .post(format!("{}?token={SECRET}", self.server.ingress_url()))
            .header(reqwest::header::CONTENT_TYPE, "application/json")
            .body(failure_alert(OBICO_PRINT, "http://127.0.0.1:1/frame.jpg").to_string())
            .send()
            .await
            .expect("the ingress answers")
            .status();
        assert_eq!(status, StatusCode::ACCEPTED);
        let pass = self
            .agent
            .issued(1)
            .await
            .into_iter()
            .next()
            .expect("one pass");
        self.octoprint.reports("Operational", JobScript::idle());
        self.reads("operational").await;
        (
            presenting(&pass.credential),
            json!({ "agent": { "session_name": pass.session_name } }),
        )
    }
}

/// A job that reads as the printer down, in `OctoPrint`'s own word for it.
fn down(word: &str) -> JobScript {
    JobScript {
        state: word.to_owned(),
        ..JobScript::idle()
    }
}

/// One operator request's body: the fields, an actor and a reason.
fn operator_body(fields: Value) -> Value {
    with_actor(fields, json!("operator"))
}

/// One request's body, claiming one actor.
fn with_actor(fields: Value, actor: Value) -> Value {
    let mut body = fields;
    body["actor"] = actor;
    body["reason"] = json!("free the nozzle from the part");
    body
}

/// Every recovery action, with a body its own bounds accept.
fn every_action() -> Vec<(&'static str, Value)> {
    vec![
        ("recover_lift", json!({ "mm": 5.0 })),
        ("recover_cool", json!({})),
        ("recover_fan", json!({ "percent": 50.0 })),
        (
            "recover_heat",
            json!({ "tool": 0, "target_c": 150.0, "clearance_asserted": true }),
        ),
        ("recover_park", json!({ "clearance_asserted": true })),
        ("recover_home_xy", json!({ "clearance_asserted": true })),
        ("recover_home_z", json!({ "bed_clear_asserted": true })),
    ]
}

/// The one refusal an answer carries, by its name.
fn refused_as(answer: &Value) -> String {
    match &answer["decision"]["rejected"] {
        Value::String(named) => named.clone(),
        Value::Object(named) => named.keys().next().cloned().unwrap_or_default(),
        other => panic!("the answer carries no refusal: {other:#}"),
    }
}

/// Ask for every action and assert each is refused as `expected`, recorded,
/// and reaches nothing at the machine.
async fn every_action_refused(world: &Recovering, expected: &str) {
    for (action, fields) in every_action() {
        let before = world.asked_of_the_machine();
        let recorded = world.recorded().await.len();
        let (status, answer) = world.ask(action, fields).await;
        assert_eq!(status, StatusCode::CONFLICT, "{action}: {answer:#}");
        assert_eq!(refused_as(&answer), expected, "{action}: {answer:#}");
        assert_eq!(
            world.asked_of_the_machine(),
            before,
            "{action} reached the machine"
        );
        let after = world.recorded().await;
        assert_eq!(
            after[recorded..].to_vec(),
            vec![
                ("recovery_requested".to_owned(), action.to_owned()),
                ("recovery_rejected".to_owned(), action.to_owned()),
            ],
            "{action} was recorded otherwise"
        );
    }
}

/// While a print runs, and while one is being cancelled, every action is
/// refused `print_running`, naming the state, and recorded — and while the
/// printer is offline, `printer_unreachable`.
#[tokio::test(flavor = "multi_thread")]
async fn every_action_is_refused_under_a_running_print_and_an_offline_printer() {
    let world = Recovering::start(with_recovery(|_| {})).await;
    let printing = JobScript::printing(FILE, 816, 400, 60);
    world.octoprint.reports("Printing", printing.clone());
    world.reads("printing").await;
    every_action_refused(&world, "print_running").await;

    world.octoprint.reports(
        "Cancelling",
        JobScript {
            state: "Cancelling".to_owned(),
            ..printing
        },
    );
    world.reads("cancelling").await;
    let (_, answer) = world.ask("recover_lift", json!({ "mm": 5.0 })).await;
    assert_eq!(
        answer["decision"]["rejected"],
        json!({ "print_running": { "state": "cancelling" } })
    );
    every_action_refused(&world, "print_running").await;

    world.octoprint.reports("Offline", down("Offline"));
    world.reads("offline").await;
    every_action_refused(&world, "printer_unreachable").await;

    // A state this program has no word for is one something may be happening
    // in: a serial connection still being opened, which `OctoPrint` flags as
    // neither operational nor closed, is refused as a running print.
    world.octoprint.reports(
        "Connecting",
        JobScript {
            state: "Connecting".to_owned(),
            ..JobScript::idle()
        },
    );
    world.reads_as(&json!({ "unknown": "Connecting" })).await;
    let (_, answer) = world.ask("recover_lift", json!({ "mm": 5.0 })).await;
    assert_eq!(
        answer["decision"]["rejected"],
        json!({ "print_running": { "state": { "unknown": "Connecting" } } })
    );
    every_action_refused(&world, "print_running").await;
}

/// A value outside its bound is refused naming the field, the value and the
/// range — a lift of nothing, a negative one, one past `max_lift_mm`, one
/// that would pass `max_z_mm` with Z trusted, a fan below 0 or above 100, and
/// a heat outside the envelope — and none of them reaches the machine.
#[tokio::test(flavor = "multi_thread")]
async fn a_value_outside_its_bound_is_refused_and_reaches_nothing() {
    let world = Recovering::start(with_recovery(|_| {})).await;
    world.connected().await;
    for (action, fields, field, allowed) in [
        ("recover_lift", json!({ "mm": 0.0 }), "mm", (0.0, 50.0)),
        ("recover_lift", json!({ "mm": -2.0 }), "mm", (0.0, 50.0)),
        ("recover_lift", json!({ "mm": 51.0 }), "mm", (0.0, 50.0)),
        (
            "recover_fan",
            json!({ "percent": -1.0 }),
            "percent",
            (0.0, 100.0),
        ),
        (
            "recover_fan",
            json!({ "percent": 101.0 }),
            "percent",
            (0.0, 100.0),
        ),
        (
            "recover_heat",
            json!({ "tool": 0, "target_c": 300.0, "clearance_asserted": true }),
            "target_c",
            (0.0, 260.0),
        ),
        // A tool the envelope bounds no target for is no tool a heat may
        // name; the range answered is the tools it does bound.
        (
            "recover_heat",
            json!({ "tool": 3, "target_c": 200.0, "clearance_asserted": true }),
            "tool",
            (0.0, 0.0),
        ),
    ] {
        let before = world.asked_of_the_machine();
        let (status, answer) = world.ask(action, fields.clone()).await;
        assert_eq!(
            status,
            StatusCode::CONFLICT,
            "{action} {fields}: {answer:#}"
        );
        let bounds = &answer["decision"]["rejected"]["out_of_bounds"];
        assert_eq!(bounds["field"], field, "{answer:#}");
        assert_eq!(
            bounds["allowed"],
            json!({ "min": allowed.0, "max": allowed.1 })
        );
        assert_eq!(world.asked_of_the_machine(), before, "{action} {fields}");
    }

    // With Z trusted the head's own height bounds a lift: from Z 190 of a
    // 200 mm machine, 10 mm is all the room there is.
    world.home_z().await;
    world.octoprint.moves_head_to([0.0, 0.0, 190.0]);
    let before = world.asked_of_the_machine();
    let (status, answer) = world.ask("recover_lift", json!({ "mm": 20.0 })).await;
    assert_eq!(status, StatusCode::CONFLICT, "{answer:#}");
    assert_eq!(
        answer["decision"]["rejected"]["out_of_bounds"],
        json!({ "field": "mm", "requested": 20.0, "allowed": { "min": 0.0, "max": 10.0 } })
    );
    assert_eq!(world.asked_of_the_machine(), before);
    let head = world.octoprint.head();
    assert!(
        head.iter()
            .zip([0.0, 0.0, 190.0])
            .all(|(at, was)| (at - was).abs() < 1e-9),
        "the refused lift moved the head to {head:?}"
    );
}

/// On a machine whose bed rises to meet the nozzle, away from the bed is
/// toward Z 0, so the room a lift has is the head's own height: from Z 8,
/// 20 mm is refused naming 8 as all there is, and the head stays where it
/// was.
#[tokio::test(flavor = "multi_thread")]
async fn a_lift_on_a_z_minus_machine_is_bounded_by_the_height_left_to_zero() {
    let world = Recovering::start(with_recovery(|document| {
        set(
            document,
            "recovery.away_from_bed",
            toml::Value::String("z_minus".to_owned()),
        );
    }))
    .await;
    world.connected().await;
    world.home_z().await;
    world.octoprint.moves_head_to([0.0, 0.0, 8.0]);

    let before = world.asked_of_the_machine();
    let (status, answer) = world.ask("recover_lift", json!({ "mm": 20.0 })).await;

    assert_eq!(status, StatusCode::CONFLICT, "{answer:#}");
    assert_eq!(
        answer["decision"]["rejected"]["out_of_bounds"],
        json!({ "field": "mm", "requested": 20.0, "allowed": { "min": 0.0, "max": 8.0 } })
    );
    assert_eq!(world.asked_of_the_machine(), before);
    let head = world.octoprint.head();
    assert!(
        head.iter()
            .zip([0.0, 0.0, 8.0])
            .all(|(at, was)| (at - was).abs() < 1e-9),
        "the refused lift moved the head to {head:?}"
    );
}

/// A `[recovery]` table leaving out the two facts with defaults takes them:
/// a lift of at most 50 mm, or the machine's own height when it is shorter,
/// and a clearance margin of 5 mm.
#[tokio::test(flavor = "multi_thread")]
async fn a_lift_and_a_margin_left_out_take_their_defaults() {
    let defaulted = |max_z_mm: f64| {
        with_recovery(move |document| {
            let facts = document
                .get_mut("recovery")
                .and_then(toml::Value::as_table_mut)
                .expect("the recovery facts are a table");
            facts.remove("max_lift_mm");
            facts.remove("clearance_margin_mm");
            facts.insert("max_z_mm".to_owned(), toml::Value::Float(max_z_mm));
        })
    };
    for (max_z_mm, lift) in [(200.0, 50.0), (30.0, 30.0)] {
        let world = Recovering::start(defaulted(max_z_mm)).await;
        world.reads("operational").await;
        let (status, answer) = world.ask("recover_lift", json!({ "mm": lift + 1.0 })).await;
        assert_eq!(status, StatusCode::CONFLICT, "{answer:#}");
        assert_eq!(
            answer["decision"]["rejected"]["out_of_bounds"]["allowed"],
            json!({ "min": 0.0, "max": lift }),
            "a machine {max_z_mm} mm tall: {answer:#}"
        );
    }

    // The margin: with nothing printed, a heat with the nozzle 4 mm above the
    // bed is refused naming 5.
    let world = Recovering::start(defaulted(200.0)).await;
    world.connected().await;
    world.home_z().await;
    world.octoprint.moves_head_to([0.0, 0.0, 4.0]);
    let (status, answer) = world
        .ask(
            "recover_heat",
            json!({ "tool": 0, "target_c": 200.0, "clearance_asserted": false }),
        )
        .await;
    assert_eq!(status, StatusCode::CONFLICT, "{answer:#}");
    assert_eq!(
        answer["decision"]["rejected"]["insufficient_clearance"]["margin_mm"],
        json!(5.0),
        "{answer:#}"
    );
}

/// Homing Z without the operator's word that the bed is clear is refused
/// naming the assertion, and saying what homing Z does on this machine.
#[tokio::test(flavor = "multi_thread")]
async fn homing_z_without_the_bed_asserted_clear_is_refused() {
    let world = Recovering::start(with_recovery(|_| {})).await;
    let before = world.asked_of_the_machine();

    let (status, answer) = world.ask("recover_home_z", json!({})).await;

    assert_eq!(status, StatusCode::CONFLICT, "{answer:#}");
    assert_eq!(
        answer["decision"]["rejected"],
        json!({ "assertion_required": { "field": "bed_clear_asserted" } })
    );
    assert!(
        answer["detail"]
            .as_str()
            .is_some_and(|said| said.contains("gantry down over the whole bed")),
        "{answer:#}"
    );
    assert_eq!(world.asked_of_the_machine(), before);
    assert!(world.status().await["z_trust"].get("untrusted").is_some());
}

/// With no `[recovery]` section every action is refused as not configured,
/// and the recovery read says so of each.
#[tokio::test(flavor = "multi_thread")]
async fn with_no_recovery_facts_every_action_is_refused_as_not_configured() {
    let world = Recovering::start(|_| {}).await;
    every_action_refused(&world, "recovery_not_configured").await;
    let status = world.status().await;
    assert_eq!(status["z_home"], Value::Null);
    for entry in status["actions"].as_array().expect("every action") {
        assert_eq!(entry["allowed"], false, "{entry}");
        assert_eq!(entry["refusal"], "recovery_not_configured", "{entry}");
    }
}

/// The agent is granted no recovery by default, and is refused each as an
/// actor that may not ask for it; granted a heat, its own word that the
/// nozzle is clear never counts as clearance.
#[tokio::test(flavor = "multi_thread")]
async fn the_agent_is_granted_nothing_and_its_word_is_never_clearance() {
    let world = Recovering::start(with_recovery(|_| {})).await;
    let (turn, actor) = world.agent_turn().await;
    for (action, fields) in every_action() {
        let before = world.asked_of_the_machine();
        let (status, answer) = world
            .ask_as(&turn, action, with_actor(fields, actor.clone()))
            .await;
        assert_eq!(status, StatusCode::CONFLICT, "{action}: {answer:#}");
        assert_eq!(
            answer["decision"]["rejected"],
            json!({ "actor_may_not_request": { "action": action } })
        );
        assert_eq!(world.asked_of_the_machine(), before, "{action}");
    }
    let agents = world.status_as(&turn).await;
    for entry in agents["actions"].as_array().expect("every action") {
        assert_eq!(
            entry["refusal"]["actor_may_not_request"]["action"],
            entry["action"]
        );
    }

    let granted = world
        .restarted(with_recovery(|document| {
            set(
                document,
                "policy",
                toml::from_str("[recovery]\nagent = [\"recover_heat\"]\n").expect("a policy table"),
            );
        }))
        .await;
    granted.connected().await;
    granted.home_z().await;
    let (turn, actor) = granted.agent_turn().await;
    let before = granted.asked_of_the_machine();
    let (status, answer) = granted
        .ask_as(
            &turn,
            "recover_heat",
            with_actor(
                json!({ "tool": 0, "target_c": 150.0, "clearance_asserted": true }),
                actor,
            ),
        )
        .await;
    assert_eq!(status, StatusCode::CONFLICT, "{answer:#}");
    assert_eq!(
        answer["decision"]["rejected"],
        json!({ "insufficient_clearance": {
            "nozzle_z_mm": 0.0, "tallest_mm": 0.0, "margin_mm": 5.0,
        } })
    );
    assert_eq!(granted.asked_of_the_machine(), before);
    let recorded = granted.recorded().await;
    assert_eq!(
        recorded.last(),
        Some(&("recovery_rejected".to_owned(), "recover_heat".to_owned()))
    );
}

/// Z reads untrusted after one event, and a park is refused for it.
async fn untrusted(world: &Recovering, after: &str) {
    let status = world.status().await;
    assert!(
        status["z_trust"].get("untrusted").is_some(),
        "Z was still trusted after {after}: {status:#}"
    );
    let before = world.asked_of_the_machine();
    let (status, answer) = world.ask("recover_park", json!({})).await;
    assert_eq!(status, StatusCode::CONFLICT, "{after}: {answer:#}");
    assert_eq!(answer["decision"]["rejected"], "z_untrusted", "{after}");
    assert_eq!(world.asked_of_the_machine(), before, "{after}");
}

/// Z trust is lost after the printer goes offline and comes back, after it
/// reports an error, after a reset while idle that changes nothing a read
/// reports, after a print ends `interrupted` and after one ends `failed`, and
/// after the server starts again — each read as untrusted Z and a park
/// refused for it.
#[tokio::test(flavor = "multi_thread")]
async fn z_trust_is_lost_to_every_event_that_could_have_moved_z() {
    let mut world = Recovering::start(with_recovery(|_| {})).await;
    world.connected().await;

    world.home_z().await;
    world.octoprint.reports("Offline", down("Offline"));
    world.until("printer_connection_changed", 1).await;
    world.octoprint.reports("Operational", JobScript::idle());
    world.until("printer_connection_changed", 2).await;
    untrusted(&world, "going offline and coming back").await;

    world.home_z().await;
    world.octoprint.reports("Error", down("Error"));
    world.until("printer_connection_changed", 3).await;
    world.octoprint.reports("Operational", JobScript::idle());
    world.until("printer_connection_changed", 4).await;
    untrusted(&world, "an error").await;

    world.home_z().await;
    world.octoprint.pushes_event("PrinterReset", &json!({}));
    world.until("printer_reset", 1).await;
    untrusted(&world, "a reset while idle").await;

    // A print that a reset stops lands idle, which ends it interrupted.
    world.home_z().await;
    world
        .octoprint
        .reports("Printing", JobScript::printing(FILE, 816, 400, 60));
    world.until("print_started", 1).await;
    world.octoprint.pushes_event("PrinterReset", &json!({}));
    world.octoprint.reports("Operational", JobScript::idle());
    world.until("print_ended", 1).await;
    untrusted(&world, "a print ending interrupted").await;

    // A print the firmware stops lands offline after an error, which ends it
    // failed.
    world.home_z().await;
    world
        .octoprint
        .reports("Printing", JobScript::printing(FILE, 816, 400, 60));
    world.until("print_started", 2).await;
    world
        .octoprint
        .pushes_event("Error", &json!({ "reason": "firmware" }));
    world.octoprint.reports(
        "Offline after error",
        JobScript {
            state: "Offline after error".to_owned(),
            error: Some("Printer halted. kill() called!".to_owned()),
            ..JobScript::idle()
        },
    );
    world.until("print_ended", 2).await;
    world.octoprint.reports("Operational", JobScript::idle());
    world.until("printer_connection_changed", 6).await;
    untrusted(&world, "a print ending failed").await;
    let endings: Vec<Value> = world
        .history()
        .await
        .into_iter()
        .filter(|event| event["kind"] == "print_ended")
        .map(|event| event["payload"]["outcome"].clone())
        .collect();
    assert_eq!(endings, vec![json!("interrupted"), json!("failed")]);

    world.home_z().await;
    world = world.restarted(with_recovery(|_| {})).await;
    untrusted(&world, "the server starting again").await;
}

/// A request no recovery action can represent — an action this program does
/// not have, a field no action carries such as raw G-code, or no reason — is
/// refused as a request, with no recovery event and nothing sent.
#[tokio::test(flavor = "multi_thread")]
async fn a_request_no_recovery_action_represents_is_refused_and_recorded_nowhere() {
    let world = Recovering::start(with_recovery(|_| {})).await;
    let before = world.asked_of_the_machine();
    for (action, body, status) in [
        (
            "recover_teleport",
            operator_body(json!({})),
            StatusCode::NOT_FOUND,
        ),
        (
            "recover_lift",
            operator_body(json!({ "mm": 5.0, "gcode": "G28 Z" })),
            StatusCode::BAD_REQUEST,
        ),
        (
            "recover_home_z",
            operator_body(json!({ "bed_clear_asserted": true, "action": "recover_lift" })),
            StatusCode::BAD_REQUEST,
        ),
        (
            "recover_cool",
            json!({ "actor": "operator", "reason": "" }),
            StatusCode::BAD_REQUEST,
        ),
        (
            "recover_cool",
            json!({ "actor": "operator", "reason": "   " }),
            StatusCode::BAD_REQUEST,
        ),
    ] {
        let (answered, answer) = world.ask_as(&world.client, action, body.clone()).await;
        assert_eq!(answered, status, "{action} {body}: {answer:#}");
    }
    assert_eq!(world.recorded().await, Vec::<(String, String)>::new());
    assert_eq!(world.asked_of_the_machine(), before);
}

/// Against an adapter that has grown no command, every move is refused as one
/// the printer cannot make — recorded, sending nothing, the server still
/// answering — and against one that cannot read the nozzle's position, a move
/// over the part is refused as clearance it cannot measure.
#[tokio::test(flavor = "multi_thread")]
async fn a_move_the_printer_cannot_make_is_refused_as_such() {
    let world = Recovering::unmoving(with_recovery(|_| {})).await;
    for (action, fields) in [
        ("recover_lift", json!({ "mm": 5.0 })),
        ("recover_home_z", json!({ "bed_clear_asserted": true })),
    ] {
        let before = world.asked_of_the_machine();
        let (status, answer) = world.ask(action, fields).await;
        assert_eq!(status, StatusCode::CONFLICT, "{action}: {answer:#}");
        assert_eq!(
            answer["decision"]["rejected"]["printer_cannot"]["action"], action,
            "{answer:#}"
        );
        assert_eq!(world.asked_of_the_machine(), before, "{action}");
        assert_eq!(
            world.recorded().await.last(),
            Some(&("recovery_rejected".to_owned(), action.to_owned()))
        );
    }
    // Nothing the printer could not do moved Z, so it is not trusted.
    assert!(world.status().await["z_trust"].get("untrusted").is_some());

    let reading = Recovering::start(with_recovery(|_| {})).await;
    reading.connected().await;
    reading.home_z().await;
    reading.octoprint.answers_position(false);
    let before = reading.asked_of_the_machine();
    let (status, answer) = reading.ask("recover_park", json!({})).await;
    assert_eq!(status, StatusCode::CONFLICT, "{answer:#}");
    let cannot = &answer["decision"]["rejected"]["printer_cannot"];
    assert_eq!(cannot["action"], "recover_park");
    assert!(
        cannot["detail"]
            .as_str()
            .is_some_and(|said| said.contains("position")),
        "{answer:#}"
    );
    assert_eq!(reading.asked_of_the_machine(), before);
    assert_eq!(reading.status().await["state"], "operational");
}

/// The recovery read answers the printer's state, heaters and facts, and each
/// action's allowance; a cool sets every heater's target to zero, and is
/// recorded beside its request.
#[tokio::test(flavor = "multi_thread")]
async fn the_recovery_read_answers_each_allowance_and_a_cool_turns_every_heater_off() {
    let world = Recovering::start(with_recovery(|_| {})).await;
    world.octoprint.targets(215.0, 60.0);
    world.reads("operational").await;
    let status = world.status().await;
    assert_eq!(
        status["z_home"],
        json!({ "x": 100.0, "y": 100.0, "sweeps_bed": true })
    );
    assert_eq!(status["temperatures"]["tools"][0]["target_c"], 215.0);
    assert_eq!(status["temperatures"]["bed"]["target_c"], 60.0);
    assert_eq!(status["position"], Value::Null);
    let allowance: Vec<(String, Value)> = status["actions"]
        .as_array()
        .expect("every action")
        .iter()
        .map(|entry| {
            (
                entry["action"].as_str().unwrap_or_default().to_owned(),
                entry.get("refusal").cloned().unwrap_or(Value::Null),
            )
        })
        .collect();
    assert_eq!(
        allowance,
        vec![
            ("recover_lift".to_owned(), Value::Null),
            ("recover_cool".to_owned(), Value::Null),
            ("recover_fan".to_owned(), Value::Null),
            ("recover_heat".to_owned(), json!("z_untrusted")),
            ("recover_park".to_owned(), json!("z_untrusted")),
            ("recover_home_xy".to_owned(), json!("z_untrusted")),
            (
                "recover_home_z".to_owned(),
                json!({ "assertion_required": { "field": "bed_clear_asserted" } })
            ),
        ]
    );

    let (status, answer) = world.ask("recover_cool", json!({})).await;
    assert_eq!(status, StatusCode::OK, "{answer:#}");
    assert_eq!(answer["decision"]["executed"], "succeeded");
    assert_eq!(world.octoprint.reported_targets(), (0.0, 0.0));
    let kinds: Vec<&str> = answer["events"]
        .as_array()
        .expect("the events")
        .iter()
        .filter_map(|event| event["kind"].as_str())
        .collect();
    assert_eq!(kinds, vec!["recovery_requested", "recovery_executed"]);
    assert!(
        answer["events"]
            .as_array()
            .expect("the events")
            .iter()
            .all(|event| event["print_id"].is_null()),
        "a recovery event named a print"
    );
}

/// A cool turns off every tool the printer reports and the bed; a printer that
/// reports no tool still has tool 0's heater turned off, because a read that
/// names none is no evidence there is none.
#[tokio::test(flavor = "multi_thread")]
async fn a_cool_turns_off_every_tool_the_printer_reports_and_tool_zero_when_it_reports_none() {
    let world = Recovering::start(with_recovery(|_| {})).await;
    world.octoprint.has_tools(2, 215.0);
    world.octoprint.targets(215.0, 60.0);
    world.reads("operational").await;

    let (status, answer) = world.ask("recover_cool", json!({})).await;
    assert_eq!(status, StatusCode::OK, "{answer:#}");
    assert_eq!(world.octoprint.tool_targets(), vec![0.0, 0.0]);
    assert_eq!(world.octoprint.reported_targets(), (0.0, 0.0));

    world.octoprint.has_tools(0, 0.0);
    world.octoprint.targets(0.0, 60.0);
    let before = world.octoprint.tool_target_requests();
    let (status, answer) = world.ask("recover_cool", json!({})).await;
    assert_eq!(status, StatusCode::OK, "{answer:#}");
    assert_eq!(
        world.octoprint.tool_target_requests(),
        before + 1,
        "tool 0 was not cooled"
    );
    assert_eq!(world.octoprint.reported_targets(), (0.0, 0.0));
}

/// A cool the printer refuses partway — every tool taken, the bed refused —
/// is answered as the printer's refusal and recorded as executed and failed,
/// not as a refusal of the policy's: the tools it did turn off stay off.
#[tokio::test(flavor = "multi_thread")]
async fn a_cool_the_printer_refuses_partway_is_recorded_as_failed_at_the_printer() {
    let world = Recovering::start(with_recovery(|_| {})).await;
    world.octoprint.has_tools(2, 215.0);
    world.octoprint.targets(215.0, 60.0);
    world.octoprint.refuses_bed_targets(true);
    world.reads("operational").await;

    let (status, answer) = world.ask("recover_cool", json!({})).await;

    assert_eq!(status, StatusCode::BAD_GATEWAY, "{answer:#}");
    assert!(
        answer["decision"]["executed"]["failed"]["reason"]
            .as_str()
            .is_some_and(|said| said.contains("Bed heater is faulted")),
        "{answer:#}"
    );
    assert_eq!(world.octoprint.tool_targets(), vec![0.0, 0.0]);
    assert_eq!(world.octoprint.reported_targets(), (0.0, 60.0));
    assert_eq!(
        world.recorded().await,
        vec![
            ("recovery_requested".to_owned(), "recover_cool".to_owned()),
            ("recovery_executed".to_owned(), "recover_cool".to_owned()),
        ]
    );
}

/// A recovery the printer itself refuses is answered as that — `502`, the
/// printer's own words, and recorded as executed with a failed outcome —
/// rather than as a refusal of the policy's.
#[tokio::test(flavor = "multi_thread")]
async fn a_recovery_the_printer_refuses_is_recorded_as_failed_at_the_printer() {
    let world = Recovering::start(with_recovery(|_| {})).await;
    world.reads("operational").await;
    world.octoprint.refuses_commands(true);

    let (status, answer) = world.ask("recover_fan", json!({ "percent": 50.0 })).await;

    assert_eq!(status, StatusCode::BAD_GATEWAY, "{answer:#}");
    assert!(
        answer["decision"]["executed"]["failed"]["reason"]
            .as_str()
            .is_some_and(|said| said.contains("Printer is not operational")),
        "{answer:#}"
    );
    assert!(
        answer["detail"]
            .as_str()
            .is_some_and(|said| said.contains("which refused it")),
        "{answer:#}"
    );
    assert_eq!(
        world.recorded().await,
        vec![
            ("recovery_requested".to_owned(), "recover_fan".to_owned()),
            ("recovery_executed".to_owned(), "recover_fan".to_owned()),
        ]
    );
}

/// What the last print laid down that cannot be read is clearance that cannot
/// be measured: a move over the part is refused as one the printer cannot
/// make, and reaches nothing.
#[tokio::test(flavor = "multi_thread")]
async fn a_part_whose_file_cannot_be_read_refuses_a_move_over_it() {
    let world = Recovering::start(with_recovery(|_| {})).await;
    world.connected().await;
    // A print the watch reads past its first byte, whose file the printer
    // then cannot answer for.
    world
        .octoprint
        .reports("Printing", JobScript::printing(FILE, 816, 400, 60));
    world.until("print_started", 1).await;
    world.octoprint.reports("Operational", JobScript::idle());
    world.until("print_ended", 1).await;
    world.home_z().await;
    let before = world.asked_of_the_machine();

    let (status, answer) = world.ask("recover_park", json!({})).await;

    assert_eq!(status, StatusCode::CONFLICT, "{answer:#}");
    let cannot = &answer["decision"]["rejected"]["printer_cannot"];
    assert_eq!(cannot["action"], "recover_park", "{answer:#}");
    assert!(
        cannot["detail"]
            .as_str()
            .is_some_and(|said| said.contains("laid down could not be read")),
        "{answer:#}"
    );
    assert_eq!(world.asked_of_the_machine(), before);
    assert_eq!(world.status().await["last_print"], Value::Null);
}

/// A print a recovery moved while it was paused stays not resumable after the
/// server starts again: its resume is refused naming that recovery, and the
/// machine is asked for nothing.
#[tokio::test(flavor = "multi_thread")]
async fn a_print_a_recovery_moved_stays_not_resumable_across_a_restart() {
    let world = Recovering::start(with_recovery(|_| {})).await;
    world.octoprint.reports(
        "Paused",
        JobScript {
            state: "Paused".to_owned(),
            ..JobScript::printing(FILE, 816, 400, 60)
        },
    );
    world.until("print_started", 1).await;
    world.reads("paused").await;
    let (status, cooled) = world.ask("recover_cool", json!({})).await;
    assert_eq!(status, StatusCode::OK, "{cooled:#}");
    let moved = cooled["events"][1]["id"].clone();
    let print_id = world.status().await["paused_print"]["print_id"]
        .as_str()
        .expect("the paused print is named")
        .to_owned();

    let world = world.restarted(with_recovery(|_| {})).await;
    world.reads("paused").await;
    assert_eq!(
        world.status().await["paused_print"],
        json!({ "print_id": print_id, "resumable": false })
    );
    let jobs = world.octoprint.job_commands();
    let resumed: Value = world
        .client
        .post(format!(
            "{}/prints/{print_id}/actions/resume",
            world.server.base_url()
        ))
        .json(&json!({ "actor": "operator", "reason": "carry on printing" }))
        .send()
        .await
        .expect("the server answers")
        .json()
        .await
        .expect("a JSON answer");
    assert_eq!(
        resumed["record"]["decision"]["rejected"],
        json!({ "print_not_resumable": { "recovery_event_id": moved } })
    );
    assert_eq!(
        world.octoprint.job_commands(),
        jobs,
        "the resume reached the machine"
    );
}
