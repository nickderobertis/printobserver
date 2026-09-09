//! The server this tier drives, composed from the real four.
//!
//! Everything here is the implementation the composition root chooses, with two
//! differences a tier is entitled to: the `OctoPrint` instance is the scripted
//! one rather than a printer in a room, and `OneHarness` is pointed at the
//! deterministic responder it itself publishes rather than at a paid provider.
//! Nothing else is replaced — the engine, the session store, the schema
//! validation and the re-prompt loop are the real ones.

use core::time::Duration;
use std::path::PathBuf;
use std::sync::Arc;

use printobserver_obico::{ObicoVision, ObicoVisionConfig};
use printobserver_octoprint::{OctoPrintConfig, OctoPrintPrinter};
use printobserver_oneharness::{
    AssessmentSchema, EnvAssignment, HarnessIdentity, OneharnessSupervisor, SupervisorConfig,
    TurnTimeout,
};
use printobserver_server::{Ports, Running, Server, ServerConfig};
use printobserver_store_api::StorePort;
use printobserver_store_sqlite::SqliteStore;
use printobserver_types::serde_json::json;
use tempfile::TempDir;

use crate::scripted::Scripted;

/// The shared secret this tier's ingress requires.
pub const SECRET: &str = "a-shared-secret-this-tier-configures";

/// The session the responder reports, which is what a continued session is
/// continued under.
pub const SESSION: &str = "watch-integration";

/// How long one request to the instance may take.
///
/// Longer than the adapter's own default, because a virtual printer part-way
/// through a ten-second dwell answers when the dwell does.
const TIMEOUT: Duration = Duration::from_secs(30);

/// The server this tier drives, and everything it was composed from.
pub struct Composed {
    /// The state directory, removed when this is dropped.
    pub root: TempDir,
    /// The server, serving.
    pub server: Running,
    /// Durable state, as the server holds it.
    pub store: Arc<dyn StorePort>,
    /// The client this tier drives the real surface with.
    pub client: reqwest::Client,
    /// The configuration it is running under.
    config: ServerConfig,
}

impl Composed {
    /// A server over a fresh state directory, against the scripted instance.
    pub async fn open(instance: &Scripted) -> Self {
        let root = TempDir::new().expect("this tier's own root");
        let config = configuration(root.path(), instance);
        let (server, store) = start(&config).await;
        Self {
            root,
            server,
            store,
            client: reqwest::Client::new(),
            config,
        }
    }

    /// Stop this server and start another over the same state directory.
    pub async fn restart(self) -> Self {
        let Self {
            root,
            server,
            client,
            config,
            ..
        } = self;
        server.stop().await;
        let (server, store) = start(&config).await;
        Self {
            root,
            server,
            store,
            client,
            config,
        }
    }

    /// One operation's URL, with the print substituted into it.
    pub fn operation_url(&self, name: &str, print_id: printobserver_types::PrintId) -> String {
        let operation =
            printobserver_server::operation(name).unwrap_or_else(|| panic!("`{name}` is served"));
        format!(
            "http://{}{}",
            self.server.address(),
            operation
                .full_path()
                .replace("{print_id}", &print_id.to_string())
        )
    }

    /// Read one JSON answer, and the status it came under.
    pub async fn get(
        &self,
        url: &str,
    ) -> (reqwest::StatusCode, printobserver_types::serde_json::Value) {
        let response = self
            .client
            .get(url)
            .send()
            .await
            .expect("the server answers");
        read(response).await
    }

    /// Ask for one action, and read the JSON answer.
    pub async fn post(
        &self,
        url: &str,
        body: &printobserver_types::serde_json::Value,
    ) -> (reqwest::StatusCode, printobserver_types::serde_json::Value) {
        let response = self
            .client
            .post(url)
            .json(body)
            .send()
            .await
            .expect("the server answers");
        read(response).await
    }
}

/// The status and body of one answer, which must be JSON.
async fn read(
    response: reqwest::Response,
) -> (reqwest::StatusCode, printobserver_types::serde_json::Value) {
    let status = response.status();
    let text = response.text().await.expect("an answer has a body");
    let value = printobserver_types::serde_json::from_str(&text)
        .unwrap_or_else(|error| panic!("the answer is not JSON ({error}): {text}"));
    (status, value)
}

/// The configuration this tier runs under, written into its own root.
fn configuration(root: &std::path::Path, instance: &Scripted) -> ServerConfig {
    let document = format!(
        r#"
state_dir = "{state}"
listen = "127.0.0.1:0"

[octoprint]
url = "{url}"
api_key = "{key}"
fan = "commandable"

[supervisor]
harness = "claude-code"

[ingress]
shared_secret = "{SECRET}"
answer_bound_ms = 1000

[safety]
agent_min_interval_s = 0

[safety.allowed]
feedrate = {{ min = 0.5, max = 1.5 }}
flowrate = {{ min = 0.9, max = 1.1 }}
fan = {{ min = 0.0, max = 100.0 }}
bed_target = {{ min = 0.0, max = 110.0 }}
"tool_target:0" = {{ min = 0.0, max = 260.0 }}

[safety.actions]
operator = ["pause", "resume", "cancel", "start_print", "set_feedrate_factor",
            "set_flowrate_factor", "set_tool_target_c", "set_bed_target_c",
            "set_fan_percent", "acknowledge_failure"]
agent = ["pause", "set_feedrate_factor", "set_fan_percent"]
system = ["set_feedrate_factor", "set_flowrate_factor", "set_tool_target_c",
          "set_bed_target_c", "set_fan_percent"]
"#,
        state = root.join("state").display(),
        url = instance.url,
        key = instance.api_key,
    );
    let path = root.join("config.toml");
    std::fs::write(&path, document).expect("the configuration is writable");
    ServerConfig::load(&path).expect("this tier's configuration is accepted")
}

/// Write one of the agent's committed assets into the state directory.
fn materialize(directory: &std::path::Path, name: &str, contents: &str) -> PathBuf {
    std::fs::create_dir_all(directory).expect("an assets directory");
    let path = directory.join(name);
    std::fs::write(&path, contents).expect("an asset is writable");
    path
}

/// The real supervising agent, with only the provider process replaced.
fn agent(config: &ServerConfig) -> OneharnessSupervisor {
    let assets = config.assets_dir();
    let skill = materialize(
        &assets,
        "printobserver-skill.md",
        printobserver_oneharness::DEFAULT_SKILL,
    );
    let template = materialize(
        &assets,
        "turn-prompt.md",
        printobserver_oneharness::DEFAULT_TURN_PROMPT,
    );
    // The generated assessment schema, as this program materializes it: read
    // from the contracts' own type rather than from a copy of the artifact, so
    // this tier constrains an answer by exactly what the server does.
    let schema = materialize(
        &assets,
        "assessment-schema.json",
        &printobserver_types::serde_json::to_string_pretty(
            &printobserver_types::contract::schema_of::<printobserver_types::AgentAssessment>(),
        )
        .expect("the schema renders"),
    );
    let answer = json!({
        "summary": "the first layer is down and the walls are clean",
        "confidence": "high",
        "should_continue": true,
        "did": "read the event, the picture and the print's context",
        "why": "nothing in the picture is coming away from the bed",
        "escalating": false,
    })
    .to_string();
    let document = json!({
        "result": answer,
        "session_id": SESSION,
        "thread_id": SESSION,
    })
    .to_string();
    OneharnessSupervisor::open(SupervisorConfig {
        state_dir: config.state_dir.clone(),
        skill_path: skill,
        prompt_template_path: template,
        assessment_schema: AssessmentSchema::at(&schema).expect("the schema constrains an answer"),
        harness: HarnessIdentity::new("claude-code").expect("a harness identity"),
        model: None,
        working_dir: config.state_dir.clone(),
        turn_timeout: TurnTimeout::DEFAULT,
        harness_bin: Some(PathBuf::from(env!(
            "CARGO_BIN_EXE_printobserver-server-responder"
        ))),
        harness_env: vec![
            EnvAssignment::new(&format!("MOCK_STDOUT={document}")).expect("an assignment"),
        ],
    })
    .expect("the supervising agent is built")
}

/// Start one server over the real four.
async fn start(config: &ServerConfig) -> (Running, Arc<dyn StorePort>) {
    let store: Arc<dyn StorePort> =
        Arc::new(SqliteStore::open(&config.state_dir).expect("the store opens"));
    let printer = Arc::new(OctoPrintPrinter::new(
        OctoPrintConfig::new(&config.octoprint_url, config.octoprint_api_key.clone())
            .expect("the scripted instance is a configuration")
            .with_timeout(TIMEOUT)
            .with_fan(config.octoprint_fan),
    ));
    let server = Server::start_with(
        config.clone(),
        Ports {
            printer: printer as Arc<dyn printobserver_printer_api::PrinterPort>,
            store: Arc::clone(&store),
            vision: Arc::new(
                ObicoVision::new(ObicoVisionConfig::default()).expect("the adapter is built"),
            ),
            agent: Arc::new(agent(config)) as Arc<dyn printobserver_supervisor_api::SupervisorPort>,
        },
    )
    .await
    .expect("the server starts against the scripted instance");
    (server, store)
}
