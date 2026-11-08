//! The detector an installer's configuration names is the one the server
//! asks: each of the four committed install templates, its commented
//! `[camera]` and `[obico]` examples taken up, run through the real server.
//!
//! Each template is read out of its committed installer as that installer
//! writes it — the shell ones' `CONFIG` here-document and the PowerShell ones'
//! here-string — with the values each interpolates given a root of this
//! journey's own. The examples are uncommented line by line, as an operator
//! would, and only the addresses they name are pointed at this journey's own
//! loopback hosts: a scripted `OctoPrint` printing a job, the detector's
//! `ml_api` and `Obico`'s REST API. What is asserted is what `GET` of the
//! print's status answers about the detector, and that the `ml_api` host
//! received the health check.

use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use printobserver_core::store::Stores;
use printobserver_obico::{ObicoVision, ObicoVisionConfig};
use printobserver_octoprint::OctoPrintPrinter;
use printobserver_octoprint::standin::{JobScript, ScriptedOctoPrint};
use printobserver_server::{Ports, Server, ServerConfig};
use printobserver_store_sqlite::SqliteStore;
use printobserver_types::serde_json::Value;
use tempfile::TempDir;

use crate::agent::StandInAgent;
use crate::minted::minted;
use crate::obico_api::Host;
use crate::world::{OPERATOR, SECRET, operator_verifier, presenting, set};

/// How a journey waits for what the check should find.
const PATIENCE: Duration = Duration::from_secs(30);

/// The four committed installers whose configuration templates carry the
/// examples.
const INSTALLERS: [&str; 4] = [
    "scripts/install.sh",
    "scripts/install-service.sh",
    "scripts/install.ps1",
    "scripts/install-service.ps1",
];

/// The commented example lines an operator takes up.
const EXAMPLES: [&str; 6] = [
    "# [camera]",
    "# snapshot_url = ",
    "# [obico]",
    "# url = ",
    "# access_token = ",
    "# ml_api_url = ",
];

/// The configuration one committed installer writes, with what it
/// interpolates given this root's state directory.
fn template(installer: &str, state: &Path) -> String {
    let script = std::fs::read_to_string(
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../..")
            .join(installer),
    )
    .expect("the committed installer reads");
    let shell = Path::new(installer)
        .extension()
        .is_some_and(|extension| extension.eq_ignore_ascii_case("sh"));
    let (opened, closed) = if shell {
        ("<<CONFIG", "\nCONFIG\n")
    } else {
        ("$configuration = @\"", "\n\"@")
    };
    let at = script
        .find(opened)
        .expect("the installer writes a configuration");
    let start = at + script[at..].find('\n').expect("the opening ends its line") + 1;
    let length = script[start..]
        .find(closed)
        .expect("the configuration closes");
    let skills = state.join("skills");
    let skill = skills.join("printobserver").join("SKILL.md");
    script[start..start + length]
        .replace("$RUNTIME_STATE", &state.display().to_string())
        .replace("$RUNTIME_CONFIG", "config.toml")
        .replace("$INTERVAL_LINE", "# observation_interval_s = 120")
        .replace("$InstalledSkills", &skills.display().to_string())
        .replace("$InstalledSkill", &skill.display().to_string())
        .replace("$InstalledState", &state.display().to_string())
        .replace("$Interval", "# observation_interval_s = 120")
}

/// The template with every commented example taken up, the token filled in.
fn with_the_examples_taken_up(installer: &str, template: &str, obico_token: &str) -> String {
    let mut taken = 0;
    let lines: Vec<String> = template
        .lines()
        .map(|line| {
            if EXAMPLES.iter().any(|marker| line.starts_with(marker)) {
                taken += 1;
                line.trim_start_matches("# ").replace(
                    "access_token = \"\"",
                    &format!("access_token = \"{obico_token}\""),
                )
            } else {
                line.to_owned()
            }
        })
        .collect();
    assert_eq!(
        taken,
        EXAMPLES.len(),
        "{installer}'s template does not carry every example"
    );
    lines.join("\n")
}

/// Wait until the active print's status answers the detector as other than
/// unchecked, and the `ml_api` host has been asked.
async fn until_probed(client: &reqwest::Client, base: &str, ml_api: &Host) -> Value {
    let deadline = tokio::time::Instant::now() + PATIENCE;
    loop {
        let listed: Value = client
            .get(format!("{base}/prints"))
            .send()
            .await
            .expect("the server answers")
            .json()
            .await
            .expect("a listing");
        if let Some(print_id) = listed["active"].as_str() {
            let status: Value = client
                .get(format!("{base}/prints/{print_id}/status"))
                .send()
                .await
                .expect("the server answers")
                .json()
                .await
                .expect("a status");
            let probed = ml_api
                .received()
                .iter()
                .any(|request| request.line.starts_with("GET /hc/ "));
            if probed && status["detector"]["status"] != "unchecked" {
                return status["detector"].clone();
            }
        }
        assert!(
            tokio::time::Instant::now() < deadline,
            "the detector was never probed at the configured ml_api_url"
        );
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
}

/// Each committed install template, its examples taken up, configures a server
/// that sends its detector health check to the `ml_api_url` the example names,
/// and whose status no longer answers the detector unchecked.
#[tokio::test(flavor = "multi_thread")]
async fn every_install_templates_ml_api_url_is_the_detector_the_server_probes() {
    for installer in INSTALLERS {
        let root = TempDir::new().expect("a journey's own root");
        let state = root.path().join("state");
        let skill = state.join("skills").join("printobserver").join("SKILL.md");
        std::fs::create_dir_all(skill.parent().expect("the skill's directory"))
            .expect("the skill's directory is creatable");
        std::fs::write(&skill, "# A skill\n").expect("the skill is writable");
        let octoprint = ScriptedOctoPrint::start();
        octoprint.reports(
            "Printing",
            JobScript::printing("benchy.gcode", 816, 400, 60),
        );
        let token = minted("obico-token");
        let ml_api = Host::ml_api().await;
        let rest = Host::rest(&token).await;
        let camera = crate::http_host::image_host(
            std::fs::read(
                Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/recorded-frame.jpg"),
            )
            .expect("the recorded frame reads"),
        )
        .await;

        let taken_up = with_the_examples_taken_up(installer, &template(installer, &state), &token);
        let mut document: toml::Value =
            toml::from_str(&taken_up).expect("the template is a document");
        let named = document["obico"]["ml_api_url"]
            .as_str()
            .expect("the example names the detector service")
            .to_owned();
        assert!(named.starts_with("http://"), "{installer}: {named}");
        let mut fill = |dotted: &str, value: String| {
            set(&mut document, dotted, toml::Value::String(value));
        };
        fill("listen", "127.0.0.1:0".to_owned());
        fill("octoprint.url", octoprint.base_url());
        fill("octoprint.api_key", octoprint.api_key());
        fill("ingress.shared_secret", SECRET.to_owned());
        fill("camera.snapshot_url", camera.url());
        fill("obico.url", rest.url());
        fill("obico.ml_api_url", ml_api.url());
        document
            .as_table_mut()
            .expect("the configuration is a table")
            .insert(
                "api".to_owned(),
                toml::Value::Table(toml::Table::from_iter([(
                    "credential_verifier".to_owned(),
                    toml::Value::String(operator_verifier()),
                )])),
            );
        set(
            &mut document,
            "supervisor.observation_interval_s",
            toml::Value::Integer(1),
        );
        let path = root.path().join("config.toml");
        std::fs::write(
            &path,
            toml::to_string(&document).expect("the configuration renders"),
        )
        .expect("the configuration is writable");
        let config = ServerConfig::load(&path).expect("the template's configuration reads");
        let server = Server::start_with(
            config.clone(),
            Ports {
                printer: Arc::new(OctoPrintPrinter::new(config.octoprint.clone())),
                stores: Stores::of(Arc::new(
                    SqliteStore::open(&config.state_dir).expect("the store opens"),
                )),
                vision: Arc::new(
                    ObicoVision::new(ObicoVisionConfig::default())
                        .expect("built")
                        .with_api(config.obico_api.clone()),
                ),
                agent: StandInAgent::new() as Arc<dyn printobserver_supervisor_api::SupervisorPort>,
            },
        )
        .await
        .expect("the server starts");

        let detector = until_probed(&presenting(OPERATOR), &server.base_url(), &ml_api).await;
        assert_eq!(detector["status"], "available", "{installer}: {detector}");
        assert!(detector["since"].is_string(), "{installer}: {detector}");
    }
}
