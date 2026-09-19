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

use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use base64::Engine as _;
use printobserver_core::store::{HistoryQuery, Stores};
use printobserver_obico::{ObicoVision, ObicoVisionConfig};
use printobserver_server::{
    API_CREDENTIAL_FILE, ApiCredential, BODY_BOUND, CLIENT_CONFIG_FILE, ConfigField, DRAIN_BOUND,
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
/// a rendering of it — and carrying both characters a TOML string escapes, so
/// the client configuration the server writes is proven to carry it as it is.
const CONFIGURED: &str = r#"an-operators-own-"credential"-7Hq2\vX9mKp4Lw"#;

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
pub async fn history(stores: &Stores, print_id: PrintId) -> usize {
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

/// One refused request over one plain connection, sent the way the responder
/// sends one: the head, then `sent` of a body declared as `declared` bytes
/// long, then the answer read to the end of the connection.
///
/// What comes back is the answer and the socket's own record of what followed
/// it: a reset the server sends after its end reaches this side a moment after
/// the end does, and is recorded on the socket rather than read.
fn refused_exchange(
    address: std::net::SocketAddr,
    path: &str,
    declared: usize,
    sent: &str,
) -> std::io::Result<(String, Option<std::io::Error>)> {
    let mut stream = std::net::TcpStream::connect(address)?;
    stream.set_read_timeout(Some(Duration::from_secs(30)))?;
    stream.set_write_timeout(Some(Duration::from_secs(30)))?;
    write!(
        stream,
        "POST {path} HTTP/1.1\r\nHost: {address}\r\nContent-Type: {MEDIA_TYPE}\r\n\
         Content-Length: {declared}\r\nAuthorization: Bearer {SECRET}\r\nConnection: close\r\n\r\n"
    )?;
    stream.write_all(sent.as_bytes())?;
    let mut answer = String::new();
    stream.read_to_string(&mut answer)?;
    std::thread::sleep(Duration::from_millis(200));
    Ok((answer, stream.take_error()?))
}

/// The action's own body, padded with the whitespace JSON allows to `length`.
fn padded_action(length: usize) -> String {
    let operation = printobserver_server::operation("set_feedrate_factor")
        .expect("the feedrate operation is declared");
    let mut body = printobserver_types::serde_json::to_string(&body_for(operation))
        .expect("an action body renders");
    body.push_str(&" ".repeat(length.saturating_sub(body.len())));
    body
}

/// The path the feedrate action of one print is asked at.
fn feedrate_path(print_id: PrintId) -> String {
    printobserver_server::operation("set_feedrate_factor")
        .expect("the feedrate operation is declared")
        .full_path()
        .replace("{print_id}", &print_id.to_string())
}

/// Assert one answer read back is the refusal, whole.
fn assert_the_refusal_was_read(answer: &str) {
    assert!(
        answer.starts_with("HTTP/1.1 401 "),
        "the answer read back was not the refusal: {answer:?}"
    );
    assert!(
        answer.contains("presented as `Authorization: Bearer <credential>`"),
        "the refusal read back carried no body: {answer:?}"
    );
}

/// A refused request is answered whole even when its body is still arriving.
///
/// The refusal is decided on the head alone. A server that then closed the
/// connection with the body still unread on it would close it with a reset
/// rather than an end, and a caller on Windows — where a reset discards
/// everything received and not yet read — would read the abort in place of
/// the `401`; Linux hands over what was queued first, so the same race was
/// only ever seen there. So the body is drained before the refusal goes out.
///
/// Sent with a body larger than the server reads before it decides, so that a
/// server which did not drain it closes on a body still unread. The server
/// ends its side before it closes, so on Linux the end is read before the
/// reset arrives and the read itself stays clean; the reset is still recorded
/// on the socket, and that record is what is read here. The body stays under
/// the bound the server drains a refused body to.
#[tokio::test(flavor = "multi_thread")]
async fn a_refused_request_is_answered_whole_while_its_body_is_still_arriving() {
    let world = World::open().await;
    let print_id = world.open_print().await;
    world.printer.forget();
    let path = feedrate_path(print_id);
    let address = world.server.address();
    let body = padded_action(BODY_BOUND / 2);

    let exchange =
        tokio::task::spawn_blocking(move || refused_exchange(address, &path, body.len(), &body))
            .await
            .expect("the exchange finishes");

    let (answer, reset) = exchange.expect("the request is sent whole and the refusal read whole");
    assert!(
        reset.is_none(),
        "the connection was reset after the refusal, which a Windows caller reads in place \
         of it: {reset:?}"
    );
    assert_the_refusal_was_read(&answer);
    assert!(
        world.printer.calls().is_empty(),
        "a refused action reached the machine: {:?}",
        world.printer.calls()
    );
    world.server.stop().await;
}

/// A refused request whose body is past the bound is still answered the refusal.
///
/// The drain reads to the bound and no further, so the refusal goes out over
/// a body still arriving — which is the case the drain does not cover, and
/// the one thing asked here is that the refusal is still what comes back,
/// whole, rather than the drain's own limit or nothing at all. What the
/// caller then meets on the connection is its own doing and is not asserted.
#[tokio::test(flavor = "multi_thread")]
async fn a_refused_request_past_the_body_bound_is_still_answered_the_refusal() {
    let world = World::open().await;
    let print_id = world.open_print().await;
    world.printer.forget();
    let path = feedrate_path(print_id);
    let address = world.server.address();
    let body = padded_action(BODY_BOUND + 1);

    let exchange =
        tokio::task::spawn_blocking(move || refused_exchange(address, &path, body.len(), &body))
            .await
            .expect("the exchange finishes");

    let (answer, _) = exchange.expect("the request is sent and the refusal read");
    assert_the_refusal_was_read(&answer);
    assert!(
        world.printer.calls().is_empty(),
        "a refused action reached the machine: {:?}",
        world.printer.calls()
    );
    world.server.stop().await;
}

/// A refused request whose body never arrives is answered inside the drain's bound.
///
/// A caller that sends a head declaring a body and then nothing would otherwise
/// hold its refusal open for as long as it liked. The refusal comes back after
/// `DRAIN_BOUND` rather than never, and inside the caller's own patience.
#[tokio::test(flavor = "multi_thread")]
async fn a_refused_request_whose_body_never_arrives_is_answered_inside_the_bound() {
    let world = World::open().await;
    let print_id = world.open_print().await;
    world.printer.forget();
    let path = feedrate_path(print_id);
    let address = world.server.address();
    let started = std::time::Instant::now();

    let exchange = tokio::task::spawn_blocking(move || refused_exchange(address, &path, 64, ""))
        .await
        .expect("the exchange finishes");

    let (answer, _) = exchange.expect("the refusal is read though no body was sent");
    let waited = started.elapsed();
    assert_the_refusal_was_read(&answer);
    assert!(
        waited >= DRAIN_BOUND && waited < DRAIN_BOUND + Duration::from_secs(10),
        "the refusal over a body that never arrived came after {waited:?}, and the drain's \
         bound is {DRAIN_BOUND:?}"
    );
    assert!(
        world.printer.calls().is_empty(),
        "a refused action reached the machine: {:?}",
        world.printer.calls()
    );
    world.server.stop().await;
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
        .get(url_of(world.server.address(), status, print_id))
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
            url_of(world.server.address(), status, print_id)
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
#[cfg(unix)]
fn mode_of(path: &Path) -> u32 {
    use std::os::unix::fs::PermissionsExt as _;

    std::fs::metadata(path)
        .unwrap_or_else(|error| panic!("{} is not there: {error}", path.display()))
        .permissions()
        .mode()
        & 0o777
}

/// Hold one file to a mode, where this platform has modes.
///
/// Windows has none: a file there carries the access its directory grants, so
/// what these journeys say about a file's mode they say on Unix alone, and
/// everything else they say everywhere.
fn assert_mode(path: &Path, expected: u32, why: &str) {
    #[cfg(unix)]
    assert_eq!(mode_of(path), expected, "{why}");
    #[cfg(not(unix))]
    let _ = (path, expected, why);
}

/// Put one file at a mode, where this platform has modes.
fn set_mode(path: &Path, mode: u32) {
    #[cfg(unix)]
    std::fs::set_permissions(path, std::os::unix::fs::PermissionsExt::from_mode(mode))
        .expect("the file's mode is settable");
    #[cfg(not(unix))]
    let _ = (path, mode);
}

/// The client configuration a running server wrote, as a document.
fn client_configuration(state: &Path) -> toml::Value {
    let path = state.join(CLIENT_CONFIG_FILE);
    assert_mode(
        &path,
        0o600,
        "the client configuration carrying the credential is readable by others",
    );
    toml::from_str(&std::fs::read_to_string(&path).expect("the client configuration reads"))
        .expect("the client configuration is a document")
}

/// Whether one server admits one credential, asked through a status read.
async fn admits(server: &Running, credential: &str) -> bool {
    let status = printobserver_server::operation("status").expect("status is served");
    presenting(credential)
        .get(url_of(server.address(), status, PrintId::new()))
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
    assert_mode(
        &file,
        0o600,
        "the generated credential is readable by others",
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
    set_mode(&stale, 0o640);
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
    assert_mode(&stale, 0o640, "the stale file's mode was changed");
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

/// A credential file a person wrote with a shell or an editor ends in one line
/// terminator, and the credential in force is the text before it — while the
/// file itself is left exactly as that person wrote it, by every start over it.
#[tokio::test(flavor = "multi_thread")]
async fn a_credential_file_ending_in_one_line_terminator_is_the_text_before_it() {
    const HELD: &str = "an-operators-hand-written-credential-5Vn9";
    for (what, terminator) in [
        ("one line feed", "\n"),
        ("a carriage return and line feed", "\r\n"),
    ] {
        let rooted = Rooted::with(|_| {}).await;
        let file = rooted.credential_file();
        let written = format!("{HELD}{terminator}").into_bytes();
        std::fs::write(&file, &written).expect("the file is writable");
        set_mode(&file, 0o600);
        let modified = || {
            std::fs::metadata(&file)
                .and_then(|metadata| metadata.modified())
                .expect("the file has a modification time")
        };
        let as_written = modified();

        for start in ["a first start", "a second start"] {
            let server = rooted.start().await.unwrap_or_else(|error| {
                panic!("{start} over a credential file ending in {what} refused: {error}")
            });

            assert!(
                admits(&server, HELD).await,
                "after {start}, a credential file ending in {what} is not in force as the \
                 text before it"
            );
            assert_eq!(
                client_configuration(&rooted.state())["client"]["credential"].as_str(),
                Some(HELD),
                "after {start}, the client configuration does not carry the text before {what}"
            );
            server.stop().await;
            assert_eq!(
                std::fs::read(&file).expect("the file is still there"),
                written,
                "{start} changed the bytes of a credential file ending in {what}"
            );
            assert_eq!(
                modified(),
                as_written,
                "{start} rewrote a credential file ending in {what}"
            );
        }
    }
}

/// Every state of the credential file a server cannot use refuses the start,
/// naming the file and never what it holds, and leaves the file as it was.
#[tokio::test(flavor = "multi_thread")]
async fn a_credential_file_that_cannot_be_used_refuses_the_start() {
    let held_values: [(&str, &[u8]); 10] = [
        ("empty", b""),
        ("only whitespace", b"  \t \n"),
        (
            "carrying a control character",
            b"qx-distinctive-held\x07value",
        ),
        ("ending in two line feeds", b"qx-distinctive-held-value\n\n"),
        (
            "carrying a line feed mid-text",
            b"qx-distinctive-held\nvalue\n",
        ),
        ("carrying a tab", b"qx-distinctive-held\tvalue"),
        (
            "ending in a carriage return not paired with a line feed",
            b"qx-distinctive-held-value\r",
        ),
        ("carrying a NUL", b"qx-distinctive-held\0value\n"),
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
    let normalized_said = said.replace('\\', "/");
    assert!(
        normalized_said.contains(
            file.file_name()
                .and_then(std::ffi::OsStr::to_str)
                .expect("the credential file has a UTF-8 name"),
        ),
        "the refusal of a credential file {what} does not name it: {said}"
    );
    assert!(
        !said.contains("qx-distinctive-held"),
        "the refusal of a credential file {what} quotes what it holds: {said}"
    );
}

/// Credentials no `Authorization` header carries intact, one for every clause
/// of [`ApiCredential::new`]'s rule.
///
/// One list, because the SDK smoke checks each restate that rule where no copy
/// of this crate is reachable: the installed-client tier holds each of them to
/// every entry here, as this journey holds the server to them.
const UNPRESENTABLE: &str = include_str!("../fixtures/unpresentable-credentials.json");

/// A configured credential no header could present refuses the start, naming
/// the field and never the value.
#[tokio::test(flavor = "multi_thread")]
async fn a_configured_credential_no_caller_could_present_is_refused() {
    let unpresentable: Vec<Value> = printobserver_types::serde_json::from_str(UNPRESENTABLE)
        .expect("the unpresentable credentials are a JSON list");
    assert!(
        unpresentable.len() >= 8,
        "the unpresentable credentials lost a clause"
    );
    for entry in &unpresentable {
        let what = entry["what"].as_str().expect("each entry names itself");
        let value = entry["credential"]
            .as_str()
            .expect("each entry carries a credential");
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
            !said.contains("qx-distinctive"),
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
