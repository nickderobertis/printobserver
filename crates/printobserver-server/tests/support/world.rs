//! What every journey here drives: a real server, over a real HTTP client.
//!
//! Nothing below stands in for the layer under test. The router, the policy,
//! the store, the ingress and the vision adapter are the real ones; the two
//! things a journey supplies are the external systems on the far side of a
//! port — the machine and the supervising agent — and the real ones are what
//! this crate's integration tier runs against.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use printobserver_obico::{ObicoVision, ObicoVisionConfig};
use printobserver_server::{Ports, Running, Server, ServerConfig};
use printobserver_store_api::StorePort;
use printobserver_store_sqlite::SqliteStore;
use printobserver_types::{PrintId, serde_json};
use tempfile::TempDir;

use crate::agent::StandInAgent;
use crate::printer::RecordingPrinter;

/// The shared secret every journey's ingress requires.
pub const SECRET: &str = "a-shared-secret-nothing-else-knows";

/// The file the configuration is written to under a journey's own root.
pub const CONFIG_FILE: &str = "config.toml";

/// One configuration document, in the shape the file takes.
///
/// Built from values rather than from a string, so a journey that makes one
/// field unacceptable changes that field and nothing else.
#[must_use]
pub fn document(root: &Path, octoprint_url: &str) -> toml::Value {
    let text = format!(
        r#"
state_dir = "{state}"
listen = "127.0.0.1:0"

[octoprint]
url = "{octoprint_url}"
api_key = "a-provisioned-key"
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
    );
    toml::from_str(&text).expect("the base configuration is a document")
}

/// Set one dotted key of a configuration document.
///
/// # Panics
///
/// Panics when the path names no table, which is a journey naming a field the
/// document does not have.
pub fn set(document: &mut toml::Value, dotted: &str, value: toml::Value) {
    let (table, last) = table_of(document, dotted);
    table.insert(last, value);
}

/// Remove one dotted key of a configuration document.
///
/// # Panics
///
/// Panics when the path names no table.
pub fn remove(document: &mut toml::Value, dotted: &str) {
    let (table, last) = table_of(document, dotted);
    table.remove(&last);
}

/// The table one dotted key lives in, and the key's own last segment.
fn table_of<'a>(
    document: &'a mut toml::Value,
    dotted: &str,
) -> (&'a mut toml::Table, String) {
    let mut segments: Vec<&str> = dotted.split('.').collect();
    let last = segments.pop().expect("a key has at least one segment").to_owned();
    let mut here = document;
    for segment in segments {
        here = here
            .get_mut(segment)
            .unwrap_or_else(|| panic!("the document has no `{segment}` table"));
    }
    (
        here.as_table_mut()
            .unwrap_or_else(|| panic!("`{dotted}` does not name a table entry")),
        last,
    )
}

/// Write one configuration document into a root, and answer its path.
pub fn write(root: &Path, document: &toml::Value) -> PathBuf {
    let path = root.join(CONFIG_FILE);
    std::fs::write(
        &path,
        toml::to_string(document).expect("a configuration document renders"),
    )
    .expect("the configuration is writable");
    path
}

/// One journey's own world: a running server, and what it was composed from.
pub struct World {
    /// The journey's root, removed when it is dropped.
    pub root: TempDir,
    /// The server, serving.
    pub server: Running,
    /// The machine it is acting on.
    pub printer: Arc<RecordingPrinter>,
    /// The agent it is supervising through.
    pub agent: Arc<StandInAgent>,
    /// Durable state, as the server holds it.
    pub store: Arc<dyn StorePort>,
    /// The client a journey drives the real surface with.
    pub client: reqwest::Client,
}

impl World {
    /// A server over a fresh root, with a machine part-way through a print.
    pub async fn open() -> Self {
        Self::open_with(RecordingPrinter::printing(), StandInAgent::new()).await
    }

    /// A server over a fresh root, over the machine and agent given.
    pub async fn open_with(
        printer: Arc<RecordingPrinter>,
        agent: Arc<StandInAgent>,
    ) -> Self {
        let root = TempDir::new().expect("a journey's own root");
        let path = write(root.path(), &document(root.path(), "http://127.0.0.1:1"));
        let config = ServerConfig::load(&path).expect("the base configuration is accepted");
        let (server, store) = start(&config, &printer, &agent).await;
        Self {
            root,
            server,
            printer,
            agent,
            store,
            client: reqwest::Client::new(),
        }
    }

    /// Stop this server and start another over the same state directory.
    ///
    /// This is what a restart is: the store and the session directory are the
    /// whole of the state, and nothing else is carried across.
    pub async fn restart(self) -> Self {
        let Self {
            root,
            server,
            printer,
            agent,
            client,
            ..
        } = self;
        let config = server.config().clone();
        server.stop().await;
        let (server, store) = start(&config, &printer, &agent).await;
        Self {
            root,
            server,
            printer,
            agent,
            store,
            client,
        }
    }

    /// The state directory this world's store lives in.
    #[must_use]
    pub fn state_dir(&self) -> PathBuf {
        self.server.config().state_dir.clone()
    }

    /// One URL beneath the versioned prefix.
    #[must_use]
    pub fn url(&self, path: &str) -> String {
        format!("{}{path}", self.server.base_url())
    }

    /// One whole path on this server, prefix included.
    #[must_use]
    pub fn at(&self, whole_path: &str) -> String {
        format!("http://{}{whole_path}", self.server.address())
    }

    /// One operation's URL, with the print substituted into it.
    ///
    /// The path an operation declares already carries the versioned prefix, so
    /// this is taken as a whole path rather than beneath one.
    #[must_use]
    pub fn operation_url(&self, path: &str, print_id: PrintId) -> String {
        self.at(&path.replace("{print_id}", &print_id.to_string()))
    }

    /// Open one print to act on.
    pub async fn open_print(&self) -> PrintId {
        self.store
            .open_print(Some(4211), Some("benchy.gcode".to_owned()))
            .await
            .expect("a print opens")
            .id
    }

    /// Read one answer without asking it to be JSON, for a journey about a
    /// path this server serves nothing at.
    pub async fn raw_get(&self, url: &str) -> (reqwest::StatusCode, String, String) {
        let response = self
            .client
            .get(url)
            .send()
            .await
            .expect("the server answers");
        let status = response.status();
        let media = response
            .headers()
            .get(reqwest::header::CONTENT_TYPE)
            .and_then(|value| value.to_str().ok())
            .unwrap_or_default()
            .to_owned();
        let text = response.text().await.expect("an answer has a body");
        (status, media, text)
    }

    /// Read one JSON answer, and the status it came under.
    pub async fn get(&self, url: &str) -> (reqwest::StatusCode, serde_json::Value) {
        let response = self
            .client
            .get(url)
            .header(reqwest::header::ACCEPT, printobserver_server::MEDIA_TYPE)
            .send()
            .await
            .expect("the server answers");
        read(response).await
    }

    /// Send one JSON body, and read the JSON answer.
    pub async fn post(
        &self,
        url: &str,
        body: &serde_json::Value,
    ) -> (reqwest::StatusCode, serde_json::Value) {
        let response = self
            .client
            .post(url)
            .json(body)
            .send()
            .await
            .expect("the server answers");
        read(response).await
    }

    /// Replace one record with a JSON body, and read the JSON answer.
    pub async fn put(
        &self,
        url: &str,
        body: &serde_json::Value,
    ) -> (reqwest::StatusCode, serde_json::Value) {
        let response = self
            .client
            .put(url)
            .json(body)
            .send()
            .await
            .expect("the server answers");
        read(response).await
    }
}

/// The status, the media type and the body of one answer.
async fn read(response: reqwest::Response) -> (reqwest::StatusCode, serde_json::Value) {
    let status = response.status();
    let media = response
        .headers()
        .get(reqwest::header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .unwrap_or_default()
        .to_owned();
    let text = response.text().await.expect("an answer has a body");
    assert!(
        media.starts_with(printobserver_server::MEDIA_TYPE),
        "the server answered under {media:?} rather than JSON: {text}"
    );
    let value = serde_json::from_str(&text)
        .unwrap_or_else(|error| panic!("the answer is not JSON ({error}): {text}"));
    (status, value)
}

/// Start one server over the ports a journey supplies.
async fn start(
    config: &ServerConfig,
    printer: &Arc<RecordingPrinter>,
    agent: &Arc<StandInAgent>,
) -> (Running, Arc<dyn StorePort>) {
    let store: Arc<dyn StorePort> = Arc::new(
        SqliteStore::open(&config.state_dir).expect("the store opens on the state directory"),
    );
    let vision =
        Arc::new(ObicoVision::new(ObicoVisionConfig::default()).expect("the adapter is built"));
    let server = Server::start_with(
        config.clone(),
        Ports {
            printer: Arc::clone(printer) as Arc<dyn printobserver_printer_api::PrinterPort>,
            store: Arc::clone(&store),
            vision,
            agent: Arc::clone(agent) as Arc<dyn printobserver_supervisor_api::SupervisorPort>,
        },
    )
    .await
    .expect("the server starts");
    (server, store)
}

/// One `Obico` failure alert, as the producer posts it.
///
/// The committed sample is what the ingress journeys post; this is what a
/// journey that needs a *second* alert about the same print, or an alert naming
/// an image host it controls, builds from it.
#[must_use]
pub fn failure_alert(obico_print_id: i64, image_url: &str) -> serde_json::Value {
    let mut sample: serde_json::Value =
        serde_json::from_str(committed_sample()).expect("the committed sample is JSON");
    sample["print"]["id"] = serde_json::json!(obico_print_id);
    sample["img_url"] = serde_json::json!(image_url);
    sample
}

/// The committed `Obico` failure alert sample, read from the contracts' own file.
#[must_use]
pub fn committed_sample() -> &'static str {
    include_str!("../../../printobserver-types/samples/obico/failure-alert.json")
}

/// Every event kind one print's history carries, counted.
#[must_use]
pub fn kinds(events: &serde_json::Value) -> BTreeMap<String, usize> {
    let mut found = BTreeMap::new();
    for event in events.as_array().into_iter().flatten() {
        let kind = event["kind"].as_str().unwrap_or_default().to_owned();
        *found.entry(kind).or_insert(0) += 1;
    }
    found
}
