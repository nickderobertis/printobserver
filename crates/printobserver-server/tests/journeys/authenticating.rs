//! Who this server serves: a caller that presented the operator's credential
//! or a live supervision turn's.
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
//! # What the operator's credential is checked against
//!
//! Its verifier, and nothing the server could be read for: `api.credential_verifier`,
//! or `api-credential.verifier` in the state directory. A fresh install has
//! neither, supervises anyway, and refuses every operator naming the command
//! that issues one. Every layout an earlier version left is driven through
//! [`Server::start`], the composition root the installed unit's own command
//! reaches, built here as that version left it: the generated plaintext is
//! converted into its verifier and removed, the client configuration it wrote
//! is removed, and a plaintext `api.credential` keeps working with a warning on
//! every start that never quotes it.

use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use printobserver_core::store::{HistoryQuery, Stores};
use printobserver_obico::{ObicoVision, ObicoVisionConfig};
use printobserver_server::{
    API_CREDENTIAL_FILE, API_CREDENTIAL_VERIFIER_FILE, ApiCredential, BODY_BOUND,
    CLIENT_CONFIG_FILE, ConfigField, CredentialVerifier, DRAIN_BOUND, MEDIA_TYPE, Method,
    OPERATIONS, Operation, Ports, REDACTED, Running, Server, ServerConfig, StartError,
    TOKEN_HEADER,
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
    OPERATOR, SECRET, World, committed_sample, document, files_carrying, manifest_write,
    presenting, remove, set, write,
};

/// A plaintext credential an operator wrote into `api.credential`, spelled so
/// that a search for it finds only a rendering of it — and carrying both
/// characters a TOML string escapes.
const CONFIGURED: &str = r#"an-operators-own-"credential"-7Hq2\vX9mKp4Lw"#;

/// What a credential nobody configured is.
const STALE: &str = "a-stale-credential-nothing-should-read-3Rt8";

/// The credential printobserver v0.3.0 generated into its state directory, as
/// a legacy layout carries it: unpadded URL-safe base64 of 32 random bytes.
const LEGACY: &str = "Zq9Xr2Lk7Vb4Nw1Hc8Td5Ms3Pf6Jy0GaUe2Qo9Ri4Ex";

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
    let credential = OPERATOR.to_owned();

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

    /// A root whose configuration names no operator verifier at all: what a
    /// fresh install, and every installation before verifiers, carries.
    async fn unverified() -> Self {
        Self::with(|configured| remove(configured, "api.credential_verifier")).await
    }

    /// The state directory the configuration names, created.
    fn state(&self) -> PathBuf {
        let state = self.root.path().join("state");
        std::fs::create_dir_all(&state).expect("a state directory");
        state
    }

    /// The legacy plaintext credential file in that state directory.
    fn credential_file(&self) -> PathBuf {
        self.state().join(API_CREDENTIAL_FILE)
    }

    /// The verifier file in that state directory.
    fn verifier_file(&self) -> PathBuf {
        self.state().join(API_CREDENTIAL_VERIFIER_FILE)
    }

    /// Lay the state directory out as printobserver v0.3.0 left it: the
    /// credential it generated, and the client configuration carrying it, both
    /// private to the service.
    fn legacy_layout(&self, credential: &str) {
        std::fs::write(self.credential_file(), credential).expect("the legacy file is writable");
        set_mode(&self.credential_file(), 0o600);
        let client = self.state().join(CLIENT_CONFIG_FILE);
        std::fs::write(
            &client,
            format!(
                "[client]\nserver = \"http://127.0.0.1:8420\"\ncredential = \"{credential}\"\n"
            ),
        )
        .expect("the legacy client configuration is writable");
        set_mode(&client, 0o600);
    }

    /// Start the real composition root over it.
    async fn start(&self) -> Result<Running, StartError> {
        Server::start(&self.path).await
    }

    /// Every file under the root — the state directory and the configuration
    /// — whose bytes carry `credential`.
    fn carrying(&self, credential: &str) -> Vec<PathBuf> {
        files_carrying(self.root.path(), credential)
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

/// Whether one server admits one credential, asked through a status read.
async fn admits(server: &Running, credential: &str) -> bool {
    status_of(server, credential).await.0 != reqwest::StatusCode::UNAUTHORIZED
}

/// What one server answers a status read presenting one credential.
async fn status_of(server: &Running, credential: &str) -> (reqwest::StatusCode, String) {
    let status = printobserver_server::operation("status").expect("status is served");
    let response = presenting(credential)
        .get(url_of(server.address(), status, PrintId::new()))
        .send()
        .await
        .expect("the server answers");
    let code = response.status();
    (code, response.text().await.expect("an answer has a body"))
}

/// A fresh install starts and supervises, writes no credential of its own
/// anywhere, and refuses every operator request naming the command that issues
/// one.
#[tokio::test(flavor = "multi_thread")]
async fn a_fresh_install_supervises_and_refuses_every_operator_until_one_is_issued() {
    let world = World::configured(
        crate::printer::RecordingPrinter::printing(),
        StandInAgent::new(),
        |configured| remove(configured, "api.credential_verifier"),
    )
    .await;
    let state = world.state_dir();
    for absent in [
        API_CREDENTIAL_FILE,
        CLIENT_CONFIG_FILE,
        API_CREDENTIAL_VERIFIER_FILE,
    ] {
        assert!(
            !state.join(absent).exists(),
            "a fresh install wrote {absent} into its state directory"
        );
    }

    for presented in [OPERATOR, STALE] {
        let (status, said) = status_of(&world.server, presented).await;
        assert_eq!(status, reqwest::StatusCode::UNAUTHORIZED, "{said}");
        assert!(
            said.contains("printobserver credential issue")
                && said.contains("api.credential_verifier"),
            "the refusal does not say how an operator gets a credential: {said}"
        );
    }

    // It supervises: an alert is taken, a turn runs, and the turn is issued a
    // credential of its own.
    world.open_print().await;
    let mut completions = world.server.completions();
    let accepted = reqwest::Client::new()
        .post(format!("{}?token={SECRET}", world.server.ingress_url()))
        .header(reqwest::header::CONTENT_TYPE, "application/json")
        .body(crate::world::failure_alert(4211, "http://127.0.0.1:1/frame.jpg").to_string())
        .send()
        .await
        .expect("the ingress answers")
        .status();
    assert_eq!(accepted, reqwest::StatusCode::ACCEPTED);
    completions.changed().await.expect("the handling finishes");
    assert_eq!(world.agent.turns().len(), 1, "no turn ran");
    assert_eq!(
        world.agent.passes().len(),
        1,
        "the turn was issued no credential"
    );
    world.server.stop().await;
}

/// The operator is admitted by the verifier the configuration names, a
/// credential it does not verify is refused, and nothing the server holds or
/// writes carries the operator's credential.
#[tokio::test(flavor = "multi_thread")]
async fn the_operator_is_admitted_by_its_verifier_and_the_server_holds_no_plaintext() {
    let rooted = Rooted::with(|_| {}).await;

    let server = rooted.start().await.expect("the server starts");

    assert!(
        admits(&server, OPERATOR).await,
        "the verified credential was refused"
    );
    let (status, said) = status_of(&server, STALE).await;
    assert_eq!(status, reqwest::StatusCode::UNAUTHORIZED, "{said}");
    for absent in [
        API_CREDENTIAL_FILE,
        CLIENT_CONFIG_FILE,
        API_CREDENTIAL_VERIFIER_FILE,
    ] {
        assert!(
            !rooted.state().join(absent).exists(),
            "a server with a configured verifier wrote {absent}"
        );
    }
    assert!(
        rooted.carrying(OPERATOR).is_empty(),
        "the operator's credential is written somewhere: {:?}",
        rooted.carrying(OPERATOR)
    );
    assert!(server.warnings().is_empty(), "{:?}", server.warnings());
    server.stop().await;
}

/// With no verifier configured, the one in the state directory is in force.
#[tokio::test(flavor = "multi_thread")]
async fn the_state_directorys_verifier_is_in_force_when_none_is_configured() {
    let rooted = Rooted::unverified().await;
    std::fs::write(
        rooted.verifier_file(),
        format!("{}\n", CredentialVerifier::of(OPERATOR)),
    )
    .expect("the verifier file is writable");

    let server = rooted.start().await.expect("the server starts");

    assert!(admits(&server, OPERATOR).await);
    assert!(!admits(&server, STALE).await);
    server.stop().await;
}

/// The layout printobserver v0.3.0 left — the credential it generated and the
/// client configuration carrying it — is converted on the first start: the
/// credential into its verifier, private to the service, and both plaintext
/// files removed. The same credential keeps authenticating, on that start and
/// every one after it.
#[tokio::test(flavor = "multi_thread")]
async fn the_legacy_layout_is_converted_into_a_verifier_and_its_plaintext_removed() {
    let rooted = Rooted::unverified().await;
    rooted.legacy_layout(LEGACY);

    let first = rooted
        .start()
        .await
        .expect("the server starts over the legacy layout");

    assert!(
        !rooted.credential_file().exists(),
        "the generated plaintext was left in the state directory"
    );
    assert!(
        !rooted.state().join(CLIENT_CONFIG_FILE).exists(),
        "the legacy client configuration was left in the state directory"
    );
    assert_eq!(
        std::fs::read_to_string(rooted.verifier_file()).expect("a verifier was written"),
        format!("{}\n", CredentialVerifier::of(LEGACY)),
    );
    assert_mode(
        &rooted.verifier_file(),
        0o600,
        "the verifier file is readable by others",
    );
    assert!(
        admits(&first, LEGACY).await,
        "the legacy credential stopped working"
    );
    assert!(!admits(&first, STALE).await);
    assert!(
        rooted.carrying(LEGACY).is_empty(),
        "the legacy credential is still written somewhere: {:?}",
        rooted.carrying(LEGACY)
    );
    first.stop().await;

    let second = rooted.start().await.expect("the server starts again");
    assert!(
        admits(&second, LEGACY).await,
        "a second start lost the converted credential"
    );
    assert!(!rooted.credential_file().exists());
    second.stop().await;
}

/// A client configuration an earlier version wrote is removed whenever a start
/// finds one, whatever else is configured.
#[tokio::test(flavor = "multi_thread")]
async fn a_legacy_client_configuration_is_removed_at_every_start() {
    let rooted = Rooted::with(|_| {}).await;
    let client = rooted.state().join(CLIENT_CONFIG_FILE);
    std::fs::write(
        &client,
        format!("[client]\nserver = \"http://127.0.0.1:8420\"\ncredential = \"{OPERATOR}\"\n"),
    )
    .expect("the file is writable");

    let server = rooted.start().await.expect("the server starts");

    assert!(!client.exists(), "the legacy client configuration was left");
    server.stop().await;
}

/// A legacy credential file ending in one line terminator, as a person's shell
/// or editor ends one, converts as the text before it.
#[tokio::test(flavor = "multi_thread")]
async fn a_legacy_credential_ending_in_one_line_terminator_converts_as_the_text_before_it() {
    for (what, terminator) in [
        ("one line feed", "\n"),
        ("a carriage return and line feed", "\r\n"),
    ] {
        let rooted = Rooted::unverified().await;
        std::fs::write(rooted.credential_file(), format!("{LEGACY}{terminator}"))
            .expect("the file is writable");

        let server = rooted.start().await.unwrap_or_else(|error| {
            panic!("a start over a legacy credential ending in {what} refused: {error}")
        });

        assert!(
            admits(&server, LEGACY).await,
            "a legacy credential ending in {what} did not convert as the text before it"
        );
        server.stop().await;
    }
}

/// A plaintext `api.credential` does not stop the server: it is admitted, and
/// every start warns that it is there, naming its replacement and the command
/// that computes one, and never quoting it.
#[tokio::test(flavor = "multi_thread")]
async fn a_plaintext_credential_is_admitted_and_warned_about_on_every_start() {
    let rooted = Rooted::with(|configured| {
        remove(configured, "api.credential_verifier");
        set(
            configured,
            "api.credential",
            toml::Value::String(CONFIGURED.to_owned()),
        );
    })
    .await;

    for start in ["a first start", "a second start"] {
        let server = rooted.start().await.expect("the server starts");
        assert!(
            admits(&server, CONFIGURED).await,
            "after {start}, the plaintext credential is not admitted"
        );
        assert_plaintext_warned(&server, start);
        server.stop().await;
    }
}

/// With both keys, the verifier wins: the credential it verifies is the
/// operator, the plaintext one is refused, and the warning still fires.
#[tokio::test(flavor = "multi_thread")]
async fn a_verifier_beside_a_plaintext_credential_wins_and_still_warns() {
    let rooted = Rooted::with(|configured| {
        set(
            configured,
            "api.credential",
            toml::Value::String(CONFIGURED.to_owned()),
        );
    })
    .await;

    let server = rooted.start().await.expect("the server starts");

    assert!(
        admits(&server, OPERATOR).await,
        "the verified credential was refused"
    );
    assert!(
        !admits(&server, CONFIGURED).await,
        "the plaintext credential was admitted beside a verifier"
    );
    assert_plaintext_warned(&server, "a start under both keys");
    server.stop().await;
}

/// One start warned about a plaintext `api.credential`, in words that name what
/// to do and quote nothing of it.
fn assert_plaintext_warned(server: &Running, start: &str) {
    let warnings = server.warnings();
    let warning = warnings
        .iter()
        .find(|warning| warning.contains("`api.credential`"))
        .unwrap_or_else(|| panic!("{start} gave no warning about the plaintext: {warnings:?}"));
    for named in [
        "api.credential_verifier",
        "printobserver credential verifier",
        "supervision turn",
    ] {
        assert!(
            warning.contains(named),
            "{start}'s warning does not name {named:?}: {warning}"
        );
    }
    assert!(
        !warning.contains(CONFIGURED) && !warning.contains("7Hq2"),
        "{start}'s warning quotes the credential: {warning}"
    );
}

/// Every state of the legacy credential file a server cannot convert refuses
/// the start, naming the file and never what it holds, and leaves the file as
/// it was with no verifier written beside it.
#[tokio::test(flavor = "multi_thread")]
async fn a_legacy_credential_that_cannot_be_converted_refuses_the_start() {
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
        let rooted = Rooted::unverified().await;
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
            !rooted.verifier_file().exists(),
            "a verifier was written for a credential file {what}"
        );
    }

    // A path that cannot be read at all: a directory where the file would be.
    let rooted = Rooted::unverified().await;
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

/// A verifier file that is not one refuses the start, naming the file.
#[tokio::test(flavor = "multi_thread")]
async fn a_verifier_file_that_is_not_one_refuses_the_start() {
    let rooted = Rooted::unverified().await;
    let file = rooted.verifier_file();
    std::fs::write(&file, b"qx-distinctive-held-not-a-digest\n").expect("writable");

    let refusal = rooted
        .start()
        .await
        .err()
        .unwrap_or_else(|| panic!("a verifier file that is not one was accepted"));

    assert_refused_naming(&refusal, &file, "that is not a verifier");
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
            !rooted.credential_file().exists() && !rooted.verifier_file().exists(),
            "a refused configuration wrote a credential file anyway"
        );
    }
}

/// Neither rendering of a credential shows it, and a rendering of a
/// configuration carrying one shows only that something is redacted.
#[tokio::test(flavor = "multi_thread")]
async fn neither_rendering_of_a_credential_shows_it() {
    let credential = ApiCredential::new(CONFIGURED).expect("a credential");
    assert_eq!(credential.to_string(), REDACTED);
    assert!(!format!("{credential:?}").contains(CONFIGURED));
    assert!(credential.admits(CONFIGURED.as_bytes()));
    assert!(!credential.admits(STALE.as_bytes()));
    assert!(!credential.admits(&CONFIGURED.as_bytes()[1..]));

    let rooted = Rooted::with(|configured| {
        set(
            configured,
            "api.credential",
            toml::Value::String(CONFIGURED.to_owned()),
        );
    })
    .await;
    let server = rooted.start().await.expect("the server starts");
    let rendered = format!("{:?} {server:?}", server.config());
    assert!(
        !rendered.contains(CONFIGURED) && rendered.contains(REDACTED),
        "a rendering of the configuration carries the configured credential: {rendered}"
    );
    server.stop().await;
}
