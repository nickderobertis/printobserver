//! Who this server serves: a caller that presented the credential in force.
//!
//! # What is refused, and what that refusal touches
//!
//! Every declared operation is driven over HTTP with no `Authorization` header,
//! a malformed one and a wrong credential, and each is answered `401` with the
//! operations' own error body and a `WWW-Authenticate: Bearer` header. What
//! makes that a refusal *before any handler runs* rather than a refusal after
//! one is read off the far side of every port: the machine was asked nothing,
//! the agent was run for nothing, the print's history did not move — and a
//! server over a store that fails every read answers the same `401` rather than
//! the store's own failure, which it could only do by never reaching the store.
//!
//! # Where the credential in force comes from
//!
//! `api.credential` when the operator configured one, and otherwise the file
//! the server generated into its state directory. Both are driven through
//! [`Server::start`], the composition root the installed unit's own command
//! reaches: the configured one leaves a stale file untouched and unread, the
//! generated one is reused unchanged by a second start, and every state of that
//! file a person could leave behind refuses the start naming the file and never
//! what it holds.

use std::os::unix::fs::PermissionsExt as _;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use base64::Engine as _;
use printobserver_core::store::{HistoryQuery, Stores};
use printobserver_obico::{ObicoVision, ObicoVisionConfig};
use printobserver_server::{
    API_CREDENTIAL_FILE, ApiCredential, CLIENT_CONFIG_FILE, ConfigField,
    GENERATED_CREDENTIAL_BYTES, MEDIA_TYPE, Method, OPERATIONS, Operation, Ports, REDACTED,
    Running, Server, ServerConfig, StartError, TOKEN_HEADER,
};
use printobserver_types::PrintId;
use printobserver_types::serde_json::Value;
use tempfile::TempDir;

use crate::agent::StandInAgent;
use crate::failing_store::FailingStore;
use crate::printer::RecordingPrinter;
use crate::probes::{base_url, silent_host};
use crate::surface::{body_for, image_id};
use crate::world::{
    SECRET, World, committed_sample, document, generated_credential, manifest_write, presenting,
    set, write,
};

/// A credential an operator chose, spelled so that a search for it finds only
/// a rendering of it.
const CONFIGURED: &str = "an-operators-own-credential-7Hq2vX9mKp4Lw";

/// What a stale file left in the state directory holds.
const STALE: &str = "a-stale-credential-nothing-should-read-3Rt8";

/// The characters unpadded URL-safe base64 is written in.
fn url_safe(character: char) -> bool {
    character.is_ascii_alphanumeric() || character == '-' || character == '_'
}

/// One way a request can fail to present the credential in force.
struct Presenting {
    /// What the journey calls it in a failure message.
    named: &'static str,
    /// Every `Authorization` header it carries, in order.
    headers: Vec<String>,
}

/// Every way a request presents something other than the credential in force.
fn not_the_credential(credential: &str) -> Vec<Presenting> {
    let short = &credential[..credential.len() - 1];
    [
        ("no header", Vec::new()),
        ("the scheme alone", vec!["Bearer".to_owned()]),
        ("the scheme and nothing", vec!["Bearer ".to_owned()]),
        (
            "a wrong credential",
            vec!["Bearer not-the-credential-in-force".to_owned()],
        ),
        (
            "the credential one character short",
            vec![format!("Bearer {short}")],
        ),
        (
            "the credential and one character more",
            vec![format!("Bearer {credential}x")],
        ),
        (
            "the credential under a lowercase scheme",
            vec![format!("bearer {credential}")],
        ),
        (
            "the credential under another scheme",
            vec![format!("Basic {credential}")],
        ),
        ("the credential with no scheme", vec![credential.to_owned()]),
        (
            "the credential after two spaces",
            vec![format!("Bearer  {credential}")],
        ),
        (
            "the credential in two headers",
            vec![
                format!("Bearer {credential}"),
                format!("Bearer {credential}"),
            ],
        ),
        (
            "the ingress shared secret",
            vec![format!("Bearer {SECRET}")],
        ),
    ]
    .into_iter()
    .map(|(named, headers)| Presenting { named, headers })
    .collect()
}

/// One operation's URL on one server, with its identifiers substituted.
fn url_of(address: std::net::SocketAddr, operation: &Operation, print_id: PrintId) -> String {
    format!(
        "http://{address}{}",
        operation
            .full_path()
            .replace("{print_id}", &print_id.to_string())
            .replace("{image_id}", &image_id())
    )
}

/// Ask one operation, carrying exactly the headers given and a body it takes.
async fn ask(
    client: &reqwest::Client,
    address: std::net::SocketAddr,
    operation: &Operation,
    print_id: PrintId,
    authorization: &[String],
) -> reqwest::Response {
    let url = url_of(address, operation, print_id);
    let mut request = match operation.method {
        Method::Get => client.get(&url),
        Method::Post => client.post(&url).json(&body_for(operation)),
        Method::Put => client.put(&url).json(&manifest_write(
            &printobserver_types::serde_json::to_value(
                <printobserver_core::JobManifest as printobserver_types::contract::Sample>::sample_full(),
            )
            .expect("a manifest renders"),
        )),
    };
    for value in authorization {
        request = request.header(reqwest::header::AUTHORIZATION, value);
    }
    request.send().await.expect("the server answers")
}

/// Assert one answer is the refusal a caller that did not authenticate gets.
///
/// Answers the error text the body carried.
async fn assert_refused(response: reqwest::Response, what: &str, credential: &str) -> String {
    assert_eq!(
        response.status(),
        reqwest::StatusCode::UNAUTHORIZED,
        "{what} was not refused as unauthenticated"
    );
    assert_eq!(
        response
            .headers()
            .get(reqwest::header::WWW_AUTHENTICATE)
            .and_then(|value| value.to_str().ok()),
        Some("Bearer"),
        "{what} was refused without saying which scheme to authenticate with"
    );
    let media = response
        .headers()
        .get(reqwest::header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .unwrap_or_default()
        .to_owned();
    assert!(
        media.starts_with(MEDIA_TYPE),
        "{what} was refused under {media:?} rather than the operations' own media type"
    );
    let text = response.text().await.expect("a refusal has a body");
    assert!(
        !text.contains(credential),
        "the refusal of {what} carries the credential in force: {text}"
    );
    let body: Value = printobserver_types::serde_json::from_str(&text)
        .unwrap_or_else(|error| panic!("the refusal of {what} is not JSON ({error}): {text}"));
    body.get("error")
        .and_then(Value::as_str)
        .unwrap_or_else(|| panic!("the refusal of {what} is not the error body: {text}"))
        .to_owned()
}

/// Every event one print holds.
async fn history(stores: &Stores, print_id: PrintId) -> usize {
    stores
        .events
        .history(HistoryQuery {
            print_id,
            kinds: Vec::new(),
            since: None,
            until: None,
            limit: None,
        })
        .await
        .expect("a history reads")
        .len()
}

/// Every operation refuses every request that did not present the credential,
/// and a refused request reaches no port, no agent and no record.
#[tokio::test(flavor = "multi_thread")]
async fn every_operation_refuses_a_caller_that_did_not_present_the_credential() {
    let world = World::open().await;
    let print_id = world.open_print().await;
    let held = history(&world.stores, print_id).await;
    world.printer.forget();
    let anonymous = reqwest::Client::new();

    for operation in OPERATIONS {
        for presenting in not_the_credential(&world.credential) {
            let what = format!("`{}` presenting {}", operation.name, presenting.named);
            let response = ask(
                &anonymous,
                world.server.address(),
                &operation,
                print_id,
                &presenting.headers,
            )
            .await;
            let said = assert_refused(response, &what, &world.credential).await;
            assert!(
                said.contains("Authorization: Bearer"),
                "the refusal of {what} does not say what to present: {said}"
            );
        }
    }

    assert!(
        world.printer.calls().is_empty(),
        "a request that did not authenticate reached the machine: {:?}",
        world.printer.calls()
    );
    assert!(
        world.agent.turns().is_empty(),
        "a request that did not authenticate ran the agent"
    );
    assert_eq!(
        history(&world.stores, print_id).await,
        held,
        "a request that did not authenticate was written into the print's history"
    );

    // The same requests, presenting the credential, are served: each answers
    // something other than the refusal, which is what says the refusal above
    // was about the credential rather than about the request.
    for operation in OPERATIONS {
        let response = ask(
            &anonymous,
            world.server.address(),
            &operation,
            print_id,
            &[format!("Bearer {}", world.credential)],
        )
        .await;
        assert_ne!(
            response.status(),
            reqwest::StatusCode::UNAUTHORIZED,
            "`{}` refused a caller that presented the credential in force",
            operation.name
        );
    }
    world.server.stop().await;
}

/// A path beneath the versioned prefix that no operation serves is refused the
/// same way, and says there is nothing there only to a caller that authenticated.
#[tokio::test(flavor = "multi_thread")]
async fn a_path_nothing_serves_is_refused_before_it_is_found_missing() {
    let world = World::open().await;
    let url = world.url("/prints/reboot");

    let refused = reqwest::Client::new()
        .get(&url)
        .send()
        .await
        .expect("the server answers");
    assert_refused(refused, "a path nothing serves", &world.credential).await;

    let (status, _, _) = world.raw_get(&url).await;
    assert_eq!(status, reqwest::StatusCode::NOT_FOUND);
    world.server.stop().await;
}

/// A refused request reads nothing from the store.
///
/// Over a store that fails every read, a request that reached one would answer
/// the store's own failure. Every operation answers the refusal instead.
#[tokio::test(flavor = "multi_thread")]
async fn a_refused_request_reads_nothing_from_the_store() {
    let root = TempDir::new().expect("a journey's own root");
    let path = write(root.path(), &document(root.path(), "http://127.0.0.1:1"));
    let config = ServerConfig::load(&path).expect("the configuration is accepted");
    let server = Server::start_with(
        config,
        Ports {
            printer: RecordingPrinter::printing()
                as Arc<dyn printobserver_printer_api::PrinterPort>,
            stores: FailingStore::after_starting(),
            vision: Arc::new(
                ObicoVision::new(ObicoVisionConfig::default()).expect("the adapter is built"),
            ),
            agent: StandInAgent::new() as Arc<dyn printobserver_supervisor_api::SupervisorPort>,
        },
    )
    .await
    .expect("a store that fails afterwards lets the server start");
    let credential = generated_credential(&server.config().state_dir);

    for operation in OPERATIONS {
        let response = ask(
            &reqwest::Client::new(),
            server.address(),
            &operation,
            PrintId::new(),
            &[format!("Bearer {SECRET}")],
        )
        .await;
        let said = assert_refused(response, operation.name, &credential).await;
        assert!(
            !said.contains(crate::failing_store::DETAIL),
            "`{}` read the store before refusing: {said}",
            operation.name
        );
    }
    server.stop().await;
}

/// The ingress admits a post by its shared secret alone.
///
/// The API credential does not admit a post there, and the shared secret — in
/// the header or the query the ingress takes it in — does not admit a
/// versioned operation.
#[tokio::test(flavor = "multi_thread")]
async fn the_ingress_and_the_api_each_admit_by_their_own_secret_alone() {
    let world = World::open().await;
    let anonymous = reqwest::Client::new();

    let refused = anonymous
        .post(world.server.ingress_url())
        .header(reqwest::header::CONTENT_TYPE, "application/json")
        .header(
            reqwest::header::AUTHORIZATION,
            format!("Bearer {}", world.credential),
        )
        .body(committed_sample().to_owned())
        .send()
        .await
        .expect("the ingress answers")
        .status();
    assert_eq!(
        refused,
        reqwest::StatusCode::UNAUTHORIZED,
        "the API credential admitted a post to the ingress"
    );

    let mut completions = world.server.completions();
    let accepted = anonymous
        .post(format!("{}?token={SECRET}", world.server.ingress_url()))
        .header(reqwest::header::CONTENT_TYPE, "application/json")
        .body(committed_sample().to_owned())
        .send()
        .await
        .expect("the ingress answers")
        .status();
    assert_eq!(
        accepted,
        reqwest::StatusCode::ACCEPTED,
        "the shared secret alone did not admit a post to the ingress"
    );
    completions.changed().await.expect("the handling finishes");

    let print_id = world.open_print().await;
    let status = printobserver_server::operation("status").expect("status is served");
    let in_the_header = anonymous
        .get(url_of(world.server.address(), status,print_id))
        .header(TOKEN_HEADER, SECRET)
        .send()
        .await
        .expect("the server answers");
    assert_refused(
        in_the_header,
        "a status read carrying the shared secret in the ingress's header",
        &world.credential,
    )
    .await;
    let in_the_query = anonymous
        .get(format!(
            "{}?token={SECRET}",
            url_of(world.server.address(), status,print_id)
        ))
        .send()
        .await
        .expect("the server answers");
    assert_refused(
        in_the_query,
        "a status read carrying the shared secret in the ingress's query",
        &world.credential,
    )
    .await;
    world.server.stop().await;
}

/// One root, and the configuration the real composition root is started under.
struct Rooted {
    /// The journey's root, removed when it is dropped.
    root: TempDir,
    /// The configuration file in it.
    path: PathBuf,
    /// The host answering where the configuration says `OctoPrint` is.
    _host: crate::http_host::Host,
}

impl Rooted {
    /// A root whose configuration is the base one, changed as given.
    async fn with(change: impl FnOnce(&mut toml::Value)) -> Self {
        let host = silent_host().await;
        let root = TempDir::new().expect("a journey's own root");
        let mut configured = document(root.path(), &base_url(&host));
        change(&mut configured);
        let path = write(root.path(), &configured);
        Self {
            root,
            path,
            _host: host,
        }
    }

    /// The state directory the configuration names, created.
    fn state(&self) -> PathBuf {
        let state = self.root.path().join("state");
        std::fs::create_dir_all(&state).expect("a state directory");
        state
    }

    /// The credential file in that state directory.
    fn credential_file(&self) -> PathBuf {
        self.state().join(API_CREDENTIAL_FILE)
    }

    /// Start the real composition root over it.
    async fn start(&self) -> Result<Running, StartError> {
        Server::start(&self.path).await
    }
}

/// The mode of one file, without the file type.
fn mode_of(path: &Path) -> u32 {
    std::fs::metadata(path)
        .unwrap_or_else(|error| panic!("{} is not there: {error}", path.display()))
        .permissions()
        .mode()
        & 0o777
}

/// The client configuration a running server wrote, as a document.
fn client_configuration(state: &Path) -> toml::Value {
    let path = state.join(CLIENT_CONFIG_FILE);
    assert_eq!(
        mode_of(&path),
        0o600,
        "the client configuration carrying the credential is readable by others"
    );
    toml::from_str(&std::fs::read_to_string(&path).expect("the client configuration reads"))
        .expect("the client configuration is a document")
}

/// Whether one server admits one credential, asked through a status read.
async fn admits(server: &Running, credential: &str) -> bool {
    let status = printobserver_server::operation("status").expect("status is served");
    presenting(credential)
        .get(url_of(server.address(), status,PrintId::new()))
        .send()
        .await
        .expect("the server answers")
        .status()
        != reqwest::StatusCode::UNAUTHORIZED
}

/// With no credential configured, a first start generates one and a second
/// reuses it unchanged.
#[tokio::test(flavor = "multi_thread")]
async fn a_generated_credential_is_written_privately_once_and_reused() {
    let rooted = Rooted::with(|_| {}).await;
    let file = rooted.credential_file();
    assert!(
        !file.exists(),
        "the journey's state directory already holds a credential"
    );

    let first = rooted.start().await.expect("the server starts");
    let generated = std::fs::read_to_string(&file).expect("the server wrote a credential");
    assert_eq!(
        mode_of(&file),
        0o600,
        "the generated credential is readable by others"
    );
    assert!(
        generated.chars().all(url_safe) && !generated.contains('='),
        "the generated credential is not unpadded URL-safe base64"
    );
    let decoded = base64::engine::general_purpose::URL_SAFE_NO_PAD
        .decode(&generated)
        .expect("the generated credential decodes");
    assert!(
        decoded.len() >= GENERATED_CREDENTIAL_BYTES && GENERATED_CREDENTIAL_BYTES >= 32,
        "the generated credential carries {} random bytes",
        decoded.len()
    );

    let written = client_configuration(&rooted.state());
    assert_eq!(
        written["client"]["server"].as_str(),
        Some(format!("http://{}", first.address()).as_str()),
        "the client configuration does not name the address this server bound"
    );
    assert_eq!(
        written["client"]["credential"].as_str(),
        Some(generated.as_str()),
        "the client configuration does not carry the credential in force"
    );
    assert!(admits(&first, &generated).await);
    assert!(!admits(&first, STALE).await);
    let modified = std::fs::metadata(&file)
        .and_then(|metadata| metadata.modified())
        .expect("the file has a modification time");
    first.stop().await;

    let second = rooted.start().await.expect("the server starts again");
    assert_eq!(
        std::fs::read_to_string(&file).expect("the credential is still there"),
        generated,
        "a second start replaced the credential the first generated"
    );
    assert_eq!(
        std::fs::metadata(&file)
            .and_then(|metadata| metadata.modified())
            .expect("the file has a modification time"),
        modified,
        "a second start rewrote the credential file"
    );
    assert!(
        admits(&second, &generated).await,
        "a second start does not admit the credential the first generated"
    );
    assert_eq!(
        client_configuration(&rooted.state())["client"]["credential"].as_str(),
        Some(generated.as_str())
    );
    second.stop().await;

    // A second state directory is a second credential: nothing about the draw
    // is fixed.
    let elsewhere = Rooted::with(|_| {}).await;
    let other = elsewhere.start().await.expect("the server starts");
    assert_ne!(
        std::fs::read_to_string(elsewhere.credential_file()).expect("a credential was written"),
        generated,
        "two state directories were given the same credential"
    );
    other.stop().await;
}

/// A configured credential is the one in force, and the state directory's file
/// is neither read nor written.
#[tokio::test(flavor = "multi_thread")]
async fn a_configured_credential_is_in_force_and_the_file_is_left_alone() {
    let rooted = Rooted::with(|configured| {
        let mut api = toml::Table::new();
        api.insert(
            "credential".to_owned(),
            toml::Value::String(CONFIGURED.to_owned()),
        );
        set(configured, "api", toml::Value::Table(api));
    })
    .await;
    let stale = rooted.credential_file();
    std::fs::write(&stale, STALE).expect("a stale file is writable");
    std::fs::set_permissions(&stale, std::fs::Permissions::from_mode(0o640))
        .expect("the stale file's mode is settable");
    let modified = std::fs::metadata(&stale)
        .and_then(|metadata| metadata.modified())
        .expect("the file has a modification time");

    let server = rooted.start().await.expect("the server starts");

    assert!(
        admits(&server, CONFIGURED).await,
        "the configured credential is not in force"
    );
    assert!(
        !admits(&server, STALE).await,
        "the stale file's credential was admitted"
    );
    assert_eq!(
        std::fs::read_to_string(&stale).expect("the stale file is still there"),
        STALE,
        "a server with a configured credential rewrote the state directory's file"
    );
    assert_eq!(
        std::fs::metadata(&stale)
            .and_then(|metadata| metadata.modified())
            .expect("the file has a modification time"),
        modified,
        "a server with a configured credential touched the state directory's file"
    );
    assert_eq!(mode_of(&stale), 0o640, "the stale file's mode was changed");
    assert_eq!(
        client_configuration(&rooted.state())["client"]["credential"].as_str(),
        Some(CONFIGURED),
        "the client configuration does not carry the configured credential"
    );
    let rendered = format!("{:?} {server:?}", server.config());
    assert!(
        !rendered.contains(CONFIGURED) && rendered.contains(REDACTED),
        "a rendering of the configuration carries the configured credential: {rendered}"
    );
    server.stop().await;

    std::fs::remove_file(&stale).expect("the stale file is removable");
    let again = rooted.start().await.expect("the server starts");
    assert!(
        !stale.exists(),
        "a server with a configured credential generated one into the state directory"
    );
    again.stop().await;
}

/// Every state of the credential file a server cannot use refuses the start,
/// naming the file and never what it holds, and leaves the file as it was.
#[tokio::test(flavor = "multi_thread")]
async fn a_credential_file_that_cannot_be_used_refuses_the_start() {
    let held_values: [(&str, &[u8]); 5] = [
        ("empty", b""),
        ("only whitespace", b"  \t \n"),
        (
            "carrying a control character",
            b"qx-distinctive-held\x07value",
        ),
        (
            "carrying a character outside ASCII",
            "qx-distinctive-held-v\u{e4}lue".as_bytes(),
        ),
        ("not text", b"qx-distinctive-held\xff\xfe"),
    ];
    for (what, held) in held_values {
        let rooted = Rooted::with(|_| {}).await;
        let file = rooted.credential_file();
        std::fs::write(&file, held).expect("the file is writable");

        let refusal = rooted
            .start()
            .await
            .err()
            .unwrap_or_else(|| panic!("a credential file {what} was accepted"));

        assert_refused_naming(&refusal, &file, what);
        assert_eq!(
            std::fs::read(&file).expect("the file is still there"),
            held,
            "a credential file {what} was replaced"
        );
        assert!(
            !rooted.state().join(CLIENT_CONFIG_FILE).exists(),
            "a server refused for a credential file {what} got as far as listening"
        );
    }

    // A path that cannot be read at all: a directory where the file would be.
    let rooted = Rooted::with(|_| {}).await;
    let file = rooted.credential_file();
    std::fs::create_dir(&file).expect("a directory is creatable");
    let refusal = rooted
        .start()
        .await
        .err()
        .unwrap_or_else(|| panic!("a credential file that cannot be read was accepted"));
    assert_refused_naming(&refusal, &file, "that cannot be read");
    assert!(file.is_dir(), "an unreadable credential path was replaced");
}

/// One refusal is about the credential file, names it, and quotes nothing of it.
fn assert_refused_naming(refusal: &StartError, file: &Path, what: &str) {
    assert!(
        matches!(refusal, StartError::Credential { .. }),
        "a credential file {what} was refused for something else: {refusal}"
    );
    let said = refusal.to_string();
    assert!(
        said.contains(&file.display().to_string()),
        "the refusal of a credential file {what} does not name it: {said}"
    );
    assert!(
        !said.contains("qx-distinctive-held"),
        "the refusal of a credential file {what} quotes what it holds: {said}"
    );
}

/// A configured credential no header could present refuses the start, naming
/// the field and never the value.
#[tokio::test(flavor = "multi_thread")]
async fn a_configured_credential_no_caller_could_present_is_refused() {
    for (what, value) in [
        ("empty", ""),
        ("only whitespace", " \t "),
        (
            "carrying a control character",
            "qx-distinctive-set\u{1b}value",
        ),
        (
            "carrying a character outside ASCII",
            "qx-distinctive-set-v\u{e4}lue",
        ),
        ("beginning with a space", " qx-distinctive-set-value"),
    ] {
        let rooted = Rooted::with(|configured| {
            let mut api = toml::Table::new();
            api.insert(
                "credential".to_owned(),
                toml::Value::String(value.to_owned()),
            );
            set(configured, "api", toml::Value::Table(api));
        })
        .await;

        let refusal = rooted
            .start()
            .await
            .err()
            .unwrap_or_else(|| panic!("a configured credential {what} was accepted"));

        assert_eq!(
            refusal.field(),
            Some(ConfigField::ApiCredential),
            "a configured credential {what} was refused for something else: {refusal}"
        );
        let said = refusal.to_string();
        assert!(
            said.contains("api.credential"),
            "the refusal of a configured credential {what} does not name the field: {said}"
        );
        assert!(
            !said.contains("qx-distinctive-set"),
            "the refusal of a configured credential {what} quotes it: {said}"
        );
        assert!(
            !rooted.credential_file().exists(),
            "a refused configuration generated a credential anyway"
        );
    }
}

/// Neither rendering of a credential shows it.
#[test]
fn neither_rendering_of_a_credential_shows_it() {
    let credential = ApiCredential::new(CONFIGURED).expect("a credential");
    assert_eq!(credential.to_string(), REDACTED);
    assert!(!format!("{credential:?}").contains(CONFIGURED));
    assert!(credential.admits(CONFIGURED.as_bytes()));
    assert!(!credential.admits(STALE.as_bytes()));
    assert!(!credential.admits(&CONFIGURED.as_bytes()[1..]));
}
