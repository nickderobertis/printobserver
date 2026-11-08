//! The operator's word that the bed is clear, recorded once and honoured by
//! recovery until the next print starts — through the real server, over the
//! scripted `OctoPrint` the adapter's `standin` feature compiles.
//!
//! The scene is the one the word exists for: a tall print cancelled, the
//! nozzle left below its top, and Z trusted. A park there is refused for
//! clearance; once the bed is said clear, a Z home, a park, a heat and an XY
//! home are each carried out with no assertion of their own, each naming the
//! word in its audit — after a restart too, because the word is read back out
//! of the event log. A print starting, through printobserver or observed at
//! the printer, ends it; a policy counting no operator assertion never
//! honours it; and nobody but the operator may give it.

use core::fmt::Write as _;
use std::sync::Arc;
use std::time::Duration;

use printobserver_core::store::{PrintStore as _, Stores};
use printobserver_obico::{ObicoVision, ObicoVisionConfig};
use printobserver_octoprint::OctoPrintPrinter;
use printobserver_octoprint::standin::{JobScript, ScriptedOctoPrint};
use printobserver_server::{Ports, Running, Server, ServerConfig};
use printobserver_store_sqlite::SqliteStore;
use printobserver_types::serde_json::{Value, json};
use reqwest::StatusCode;
use tempfile::TempDir;

use crate::agent::StandInAgent;
use crate::world::{OPERATOR, SECRET, document, failure_alert, presenting, set, write};

/// The operations beside the vocabularies these journeys drive.
pub const DRIVEN_THERE: [&str; 1] = [printobserver_server::BED_CLEAR];

/// How often these journeys' watch reads the machine.
const POLL: Duration = Duration::from_millis(200);

/// How long a journey waits for what the watch should record.
const PATIENCE: Duration = Duration::from_secs(60);

/// The tall print a cancel leaves on the bed.
const TALL: &str = "tall-tower.gcode";

/// The height that print reaches, in millimetres.
const TALL_MM: f64 = 15.0;

/// The print a journey starts after the word was given.
const NEXT: &str = "next-part.gcode";

/// The provider identifier an agent's turn is about.
const OBICO_PRINT: i64 = 4211;

/// Why the operator says the bed is clear.
const REASON: &str = "the cancelled tower is off the bed and nothing else is on it";

/// A tower of 0.2 mm layers up to [`TALL_MM`], each a short extruded wall.
fn tall_gcode() -> Vec<u8> {
    let mut gcode = String::from("G90\nM82\nG28\nG92 E0\n");
    let mut extruded = 0.0;
    for layer in 1..=75_u32 {
        let z = f64::from(layer) * 0.2;
        extruded += 1.0;
        let _ = write!(
            gcode,
            "G1 Z{z:.1}\nG1 X100 Y100 E{extruded:.1}\nG1 X110 Y110 E{:.1}\n",
            extruded + 0.5
        );
        extruded += 0.5;
    }
    gcode.into_bytes()
}

/// The recovery facts of the scripted machine.
fn facts() -> toml::Value {
    toml::from_str(
        "away_from_bed = \"z_plus\"\nmax_z_mm = 200.0\nmax_lift_mm = 50.0\n\
         clearance_margin_mm = 5.0\npark_xy = [10.0, 200.0]\nz_home_xy = [100.0, 100.0]\n\
         z_home_sweeps_bed = true\n",
    )
    .expect("the recovery facts are a table")
}

/// A real server over the scripted printer.
struct Bench {
    /// The journey's root, which a restart keeps.
    root: TempDir,
    /// The server.
    server: Running,
    /// The scripted printer.
    octoprint: ScriptedOctoPrint,
    /// The supervising agent.
    agent: Arc<StandInAgent>,
    /// The client, presenting the operator's credential.
    client: reqwest::Client,
}

impl Bench {
    /// A server over a fresh scripted printer holding the tall print, its
    /// configuration stating the recovery facts and edited by the journey.
    async fn start(edit: fn(&mut toml::Value)) -> Self {
        let octoprint = ScriptedOctoPrint::start();
        octoprint.holds_file(TALL, tall_gcode(), 1_000.0);
        octoprint.holds_file(NEXT, tall_gcode(), 1_000.0);
        let root = TempDir::new().expect("a journey's own root");
        Self::over(root, octoprint, edit).await
    }

    /// A server over a root and a printer.
    async fn over(root: TempDir, octoprint: ScriptedOctoPrint, edit: fn(&mut toml::Value)) -> Self {
        let mut configured = document(root.path(), &octoprint.base_url());
        set(
            &mut configured,
            "octoprint.api_key",
            toml::Value::String(octoprint.api_key()),
        );
        set(&mut configured, "recovery", facts());
        set(
            &mut configured,
            "spools",
            toml::from_str("preflight = \"off\"\n").expect("a spools table"),
        );
        edit(&mut configured);
        let path = write(root.path(), &configured);
        let mut config = ServerConfig::load(&path).expect("the configuration is accepted");
        config.printer_poll = POLL;
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
        let bench = Self {
            root,
            server,
            octoprint,
            agent,
            client: presenting(OPERATOR),
        };
        bench.connected().await;
        bench
    }

    /// The same server started again, over the same store and printer.
    async fn restarted(self, edit: fn(&mut toml::Value)) -> Self {
        self.server.stop().await;
        Self::over(self.root, self.octoprint, edit).await
    }

    /// Post one body as one client to one path beneath the version prefix.
    async fn post_as(
        &self,
        client: &reqwest::Client,
        path: &str,
        body: &Value,
    ) -> (StatusCode, Value) {
        let response = client
            .post(format!("{}{path}", self.server.base_url()))
            .json(body)
            .send()
            .await
            .expect("the server answers");
        let status = response.status();
        (status, response.json().await.unwrap_or(Value::Null))
    }

    /// Say the bed is clear, as one client claiming one actor.
    async fn bed_clear_as(&self, client: &reqwest::Client, actor: Value) -> (StatusCode, Value) {
        self.post_as(
            client,
            "/printer/bed_clear",
            &json!({ "actor": actor, "reason": REASON }),
        )
        .await
    }

    /// Say the bed is clear as the operator, and answer the event it was
    /// recorded as.
    async fn bed_clear(&self) -> Value {
        let (status, answer) = self.bed_clear_as(&self.client, json!("operator")).await;
        assert_eq!(status, StatusCode::OK, "{answer:#}");
        answer["event"].clone()
    }

    /// Ask for one recovery as the operator, asserting nothing.
    async fn recover(&self, action: &str, fields: Value) -> (StatusCode, Value) {
        let mut body = fields;
        body["actor"] = json!("operator");
        body["reason"] = json!("put the machine somewhere safe");
        self.post_as(&self.client, &format!("/printer/recovery/{action}"), &body)
            .await
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

    /// Every event of one kind the printer history holds, oldest first.
    async fn of_kind(&self, kind: &str) -> Vec<Value> {
        self.history()
            .await
            .into_iter()
            .filter(|event| event["kind"] == kind)
            .collect()
    }

    /// Wait until the printer history holds `count` events of one kind.
    async fn until(&self, kind: &str, count: usize) {
        let deadline = tokio::time::Instant::now() + PATIENCE;
        while self.of_kind(kind).await.len() < count {
            assert!(
                tokio::time::Instant::now() < deadline,
                "fewer than {count} `{kind}` were recorded: {:#}",
                Value::Array(self.history().await)
            );
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
    }

    /// Wait until the push socket is open.
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

    /// The printer runs one file to its last byte, and is then cancelled at the
    /// printer: the watch observes the print start, and records its end.
    async fn a_print_observed_and_cancelled(&self, file: &str) {
        let starts = self.of_kind("print_started").await.len();
        let ends = self.of_kind("print_ended").await.len();
        let size = i64::try_from(tall_gcode().len()).expect("a small file");
        self.octoprint
            .reports("Printing", JobScript::printing(file, size, size, 600));
        self.until("print_started", starts + 1).await;
        // The watch has read how far into the file the print got.
        tokio::time::sleep(POLL * 3).await;
        self.octoprint.reports("Operational", JobScript::idle());
        self.until("print_ended", ends + 1).await;
    }

    /// Start one file through printobserver, as the operator: a start this
    /// supervisor makes is one it trusts Z from.
    async fn a_print_started(&self, file: &str) {
        let starts = self.of_kind("print_started").await.len();
        let print = printobserver_types::PrintId::new();
        let (status, answer) = self
            .post_as(
                &self.client,
                &format!("/prints/{print}/actions/start_print"),
                &json!({
                    "file_name": file,
                    "manifest": {
                        "file_name": file,
                        "material": "PLA",
                        "sheet": "smooth-pei",
                        "nozzle_diameter_mm": 0.4,
                        "slicer_profile": "0.20mm SPEED",
                        "allowed": {},
                        "metadata": {},
                    },
                    "reason": "the next print in the queue",
                    "actor": "operator",
                }),
            )
            .await;
        assert_eq!(status, StatusCode::OK, "{answer:#}");
        self.until("print_started", starts + 1).await;
    }

    /// The print printobserver started runs its file to its last byte and is
    /// then cancelled at the printer.
    async fn and_cancelled(&self, file: &str) {
        let ends = self.of_kind("print_ended").await.len();
        let size = i64::try_from(tall_gcode().len()).expect("a small file");
        self.octoprint
            .reports("Printing", JobScript::printing(file, size, size, 600));
        tokio::time::sleep(POLL * 5).await;
        self.octoprint.reports("Operational", JobScript::idle());
        self.until("print_ended", ends + 1).await;
    }

    /// Home Z on the operator's own word, so Z is trusted.
    async fn home_z_asserted(&self) {
        let (status, answer) = self
            .recover("recover_home_z", json!({ "bed_clear_asserted": true }))
            .await;
        assert_eq!(status, StatusCode::OK, "{answer:#}");
    }

    /// Hold one supervision turn open about a print of its own, answering the
    /// turn's credential and the actor it claims.
    async fn agent_turn(&self) -> (reqwest::Client, Value) {
        let store = SqliteStore::open(self.root.path().join("state")).expect("the store opens");
        store
            .open_print(Some(OBICO_PRINT), Some(TALL.to_owned()))
            .await
            .expect("a print opens");
        let size = i64::try_from(tall_gcode().len()).expect("a small file");
        self.octoprint
            .reports("Printing", JobScript::printing(TALL, size, 0, 5));
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
        (
            presenting(&pass.credential),
            json!({ "agent": { "session_name": pass.session_name } }),
        )
    }
}

/// The recoveries the word stands in for, each asserting nothing of its own.
fn unasserted() -> [(&'static str, Value); 4] {
    [
        ("recover_home_z", json!({})),
        ("recover_park", json!({})),
        ("recover_heat", json!({ "tool": 0, "target_c": 150.0 })),
        ("recover_home_xy", json!({})),
    ]
}

/// The name of the one refusal an answer carries.
fn refused_as(answer: &Value) -> String {
    match &answer["decision"]["rejected"] {
        Value::String(named) => named.clone(),
        Value::Object(named) => named.keys().next().cloned().unwrap_or_default(),
        other => panic!("the answer carries no refusal: {other:#}"),
    }
}

/// Each recovery the word stands in for is carried out with no assertion of
/// its own, naming the word in both its audit events.
async fn every_one_is_carried_out_on_the_word(bench: &Bench, word: &Value) {
    for (action, fields) in unasserted() {
        let (status, answer) = bench.recover(action, fields).await;
        assert_eq!(status, StatusCode::OK, "{action}: {answer:#}");
        assert_eq!(answer["decision"]["executed"], "succeeded", "{action}");
        assert_eq!(answer["events"][0]["kind"], "recovery_requested");
        assert_eq!(answer["events"][1]["kind"], "recovery_executed");
        for event in answer["events"].as_array().expect("the audit") {
            assert_eq!(
                event["payload"]["bed_clear"], word["id"],
                "{action} does not name the word it was carried out on: {event:#}"
            );
        }
    }
}

/// Each recovery the word stood in for is refused as it is with no word at
/// all, and names none.
async fn every_one_is_refused_without_it(bench: &Bench, after: &str) {
    for (action, fields) in unasserted() {
        let (status, answer) = bench.recover(action, fields).await;
        assert_eq!(
            status,
            StatusCode::CONFLICT,
            "{after}: {action}: {answer:#}"
        );
        for event in answer["events"].as_array().expect("the audit") {
            assert!(
                event["payload"].get("bed_clear").is_none(),
                "{after}: {action} named a word that no longer stands: {event:#}"
            );
        }
    }
}

/// After a tall print is cancelled with the nozzle below its top, the bed said
/// clear once lets a Z home, a park, a heat and an XY home go ahead with no
/// assertion of their own, each naming the word — and across a restart; the
/// word is recorded with its actor and reason.
#[tokio::test(flavor = "multi_thread")]
async fn the_bed_said_clear_once_lets_every_recovery_needing_it_go_ahead() {
    let bench = Bench::start(|_| {}).await;
    bench.a_print_observed_and_cancelled(TALL).await;
    bench.home_z_asserted().await;
    bench.octoprint.moves_head_to([50.0, 50.0, 4.0]);
    let (status, answer) = bench.recover("recover_park", json!({})).await;
    assert_eq!(status, StatusCode::CONFLICT, "{answer:#}");
    assert_eq!(
        answer["decision"]["rejected"]["insufficient_clearance"]["tallest_mm"],
        json!(TALL_MM),
        "the scene is not a nozzle below the cancelled print's top: {answer:#}"
    );

    let word = bench.bed_clear().await;
    assert_eq!(word["kind"], "bed_clear_asserted");
    assert_eq!(word["print_id"], Value::Null);
    assert_eq!(
        word["payload"],
        json!({ "actor": "operator", "reason": REASON })
    );
    let recorded = bench.of_kind("bed_clear_asserted").await;
    assert_eq!(recorded.len(), 1);
    assert_eq!(recorded[0]["id"], word["id"]);
    assert_eq!(recorded[0]["payload"], word["payload"]);

    bench.octoprint.moves_head_to([50.0, 50.0, 4.0]);
    every_one_is_carried_out_on_the_word(&bench, &word).await;
    let recovery = bench
        .client
        .get(format!("{}/printer/recovery", bench.server.base_url()))
        .send()
        .await
        .expect("the server answers")
        .json::<Value>()
        .await
        .expect("a JSON answer");
    for entry in recovery["actions"].as_array().expect("every action") {
        assert_eq!(entry["allowed"], true, "{entry}");
    }

    let restarted = bench.restarted(|_| {}).await;
    restarted.octoprint.moves_head_to([50.0, 50.0, 4.0]);
    every_one_is_carried_out_on_the_word(&restarted, &word).await;
}

/// A print observed starting at the printer ends the word: every recovery it
/// stood in for is refused again.
#[tokio::test(flavor = "multi_thread")]
async fn a_print_observed_starting_ends_the_word() {
    let bench = Bench::start(|_| {}).await;
    bench.a_print_observed_and_cancelled(TALL).await;
    bench.home_z_asserted().await;
    let word = bench.bed_clear().await;
    every_one_is_carried_out_on_the_word(&bench, &word).await;

    bench.a_print_observed_and_cancelled(NEXT).await;
    bench.octoprint.moves_head_to([50.0, 50.0, 4.0]);
    every_one_is_refused_without_it(&bench, "a print observed starting").await;
}

/// A print started through printobserver ends the word just the same.
#[tokio::test(flavor = "multi_thread")]
async fn a_print_started_through_printobserver_ends_the_word() {
    let bench = Bench::start(|_| {}).await;
    bench.a_print_observed_and_cancelled(TALL).await;
    bench.home_z_asserted().await;
    let word = bench.bed_clear().await;
    every_one_is_carried_out_on_the_word(&bench, &word).await;

    bench.a_print_started(NEXT).await;
    bench.octoprint.reports("Operational", JobScript::idle());
    bench.octoprint.moves_head_to([50.0, 50.0, 4.0]);
    every_one_is_refused_without_it(&bench, "a print started through printobserver").await;
}

/// Under a policy counting no operator assertion, the word is recorded and
/// never honoured: with Z trusted from a print this supervisor started and the
/// nozzle below that print's top, a Z home is refused for want of the
/// assertion and the three moves over the part for clearance.
#[tokio::test(flavor = "multi_thread")]
async fn a_policy_counting_no_operator_assertion_never_honours_the_word() {
    let counting_none = |document: &mut toml::Value| {
        set(
            document,
            "policy",
            toml::from_str("[recovery]\nasserting = []\n").expect("a policy table"),
        );
    };
    let bench = Bench::start(counting_none).await;
    bench.a_print_started(TALL).await;
    bench.and_cancelled(TALL).await;
    bench.bed_clear().await;
    bench.octoprint.moves_head_to([50.0, 50.0, 4.0]);
    for (action, fields) in unasserted() {
        let (status, answer) = bench.recover(action, fields).await;
        assert_eq!(status, StatusCode::CONFLICT, "{action}: {answer:#}");
        let expected = if action == "recover_home_z" {
            "assertion_required"
        } else {
            "insufficient_clearance"
        };
        assert_eq!(refused_as(&answer), expected, "{action}: {answer:#}");
    }
}

/// Nobody but the operator gives the word: the agent claiming its own actor,
/// a turn claiming the operator, the operator claiming the agent, and anybody
/// claiming the system are each refused, and nothing is recorded; a reason
/// that is blank is refused too.
#[tokio::test(flavor = "multi_thread")]
async fn nobody_but_the_operator_gives_the_word() {
    let bench = Bench::start(|_| {}).await;
    let (turn, agent) = bench.agent_turn().await;
    for (client, actor, who) in [
        (&turn, agent.clone(), "the agent as itself"),
        (&turn, json!("operator"), "a turn as the operator"),
        (&bench.client, agent.clone(), "the operator as the agent"),
        (&bench.client, json!("system"), "the operator as the system"),
    ] {
        let (status, answer) = bench.bed_clear_as(client, actor).await;
        assert_eq!(status, StatusCode::FORBIDDEN, "{who}: {answer:#}");
        assert!(answer["error"].is_string(), "{who}: {answer:#}");
    }
    let (status, answer) = bench
        .post_as(
            &bench.client,
            "/printer/bed_clear",
            &json!({ "actor": "operator", "reason": "  " }),
        )
        .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{answer:#}");
    assert!(bench.of_kind("bed_clear_asserted").await.is_empty());
    bench.agent.release_turns();
}
