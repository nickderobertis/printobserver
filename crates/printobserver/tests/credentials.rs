//! The operator's credential: issued by this program, held by the operator,
//! verified by the server.
//!
//! Everything here drives the compiled program as a subprocess, the way an
//! operator does. `credential issue` writes the operator's own client
//! configuration under their configuration home and prints the verifier line
//! the server's configuration takes; `credential verifier` prints the verifier
//! of a credential handed to it on standard input. A real server is then
//! started with that line in its configuration, and every way a client command
//! finds its configuration is driven against it: the operator's own file,
//! `--config`, and the two variables.
//!
//! The configuration home is pointed at a directory of the journey's own
//! through the variable this platform reads it from — `XDG_CONFIG_HOME` on
//! Linux and every other Unix, `HOME` on macOS, `APPDATA` on Windows — and
//! through nothing else.
//!
//! One failure is not driven here: the operating system's random source
//! refusing `credential issue` a draw, which nothing a test does to its own
//! process makes the kernel do. `credential.rs`'s own tests drive that branch
//! over a draw that refuses instead.

#[path = "support/announced.rs"]
mod announced;

use std::io::{Read as _, Write as _};
use std::net::{SocketAddr, TcpListener};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Output, Stdio};

use printobserver::failure::Exit;
use printobserver_server::CredentialVerifier;
use tempfile::TempDir;

/// The variable this platform reads a user's configuration home from.
#[cfg(windows)]
const HOME_VARIABLE: &str = "APPDATA";

/// The variable this platform reads a user's configuration home from.
#[cfg(target_os = "macos")]
const HOME_VARIABLE: &str = "HOME";

/// The variable this platform reads a user's configuration home from.
#[cfg(not(any(windows, target_os = "macos")))]
const HOME_VARIABLE: &str = "XDG_CONFIG_HOME";

/// Every variable any platform reads a configuration home from: all but this
/// platform's own are taken away from a run, so the one that configures it is
/// the one this journey set.
const HOME_VARIABLES: [&str; 3] = ["XDG_CONFIG_HOME", "HOME", "APPDATA"];

/// The SHA-256 of `abc`, as FIPS 180-2 gives it.
const VERIFIER_OF_ABC: &str =
    "sha256:ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad";

/// Where `credential issue` writes the operator's configuration under one
/// configuration home, as this platform lays it out.
fn issued_at(home: &Path) -> PathBuf {
    let base = if cfg!(target_os = "macos") {
        home.join("Library").join("Application Support")
    } else {
        home.to_path_buf()
    };
    base.join("printobserver").join("client.toml")
}

/// One run of the program with its configuration home at `home`, the
/// variables given, and `input` on its standard input.
fn run(home: &Path, arguments: &[&str], environment: &[(&str, &str)], input: &[u8]) -> Output {
    running(Some(home), arguments, environment, input)
}

/// The same, with no configuration home at all when `home` is none.
fn running(
    home: Option<&Path>,
    arguments: &[&str],
    environment: &[(&str, &str)],
    input: &[u8],
) -> Output {
    let mut command = Command::new(env!("CARGO_BIN_EXE_printobserver"));
    command
        .args(arguments)
        .env_remove("PRINTOBSERVER_SERVER")
        .env_remove("PRINTOBSERVER_CREDENTIAL")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    for name in HOME_VARIABLES {
        command.env_remove(name);
    }
    if let Some(home) = home {
        command.env(HOME_VARIABLE, home);
    }
    for (name, value) in environment {
        command.env(name, value);
    }
    let mut child = command.spawn().expect("the program runs");
    child
        .stdin
        .take()
        .expect("standard input is piped")
        .write_all(input)
        .expect("standard input is written");
    child.wait_with_output().expect("the program exits")
}

/// Everything a run printed, on either stream.
fn said(output: &Output) -> String {
    String::from_utf8_lossy(&output.stdout).into_owned() + &String::from_utf8_lossy(&output.stderr)
}

/// The `[client]` table of the operator's issued configuration.
fn issued(home: &Path) -> toml::Value {
    toml::from_str::<toml::Value>(
        &std::fs::read_to_string(issued_at(home)).expect("the issued configuration reads"),
    )
    .expect("the issued configuration is a document")["client"]
        .clone()
}

/// The verifier line one run of `credential issue` or `credential verifier`
/// printed.
fn verifier_line(output: &Output) -> String {
    String::from_utf8_lossy(&output.stdout)
        .lines()
        .find(|line| line.starts_with("api.credential_verifier = \""))
        .unwrap_or_else(|| panic!("no verifier line was printed: {}", said(output)))
        .to_owned()
}

/// The mode of one file, without the file type.
#[cfg(unix)]
fn mode_of(path: &Path) -> u32 {
    use std::os::unix::fs::PermissionsExt as _;

    std::fs::metadata(path)
        .expect("the file is there")
        .permissions()
        .mode()
        & 0o777
}

/// `credential issue` writes the operator's own configuration, private to
/// them, naming the server and a credential drawn from 32 random bytes; prints
/// that credential's verifier as the line the server's configuration takes and
/// never the credential; refuses to replace it without `--replace`; and names
/// the server `PRINTOBSERVER_SERVER` names when it names one.
#[test]
fn credential_issue_writes_the_operators_own_configuration_and_prints_only_its_verifier() {
    use base64::Engine as _;

    let home = TempDir::new().expect("an operator's own home");

    let first = run(home.path(), &["credential", "issue"], &[], b"");
    assert_eq!(first.status.code(), Some(0), "{}", said(&first));
    let client = issued(home.path());
    assert_eq!(client["server"].as_str(), Some("http://127.0.0.1:8420"));
    let credential = client["credential"]
        .as_str()
        .expect("a credential was issued")
        .to_owned();
    let drawn = base64::engine::general_purpose::URL_SAFE_NO_PAD
        .decode(&credential)
        .expect("the credential is unpadded URL-safe base64");
    assert_eq!(drawn.len(), 32, "the credential is not 32 drawn bytes");
    assert_eq!(
        verifier_line(&first),
        format!(
            "api.credential_verifier = \"{}\"",
            CredentialVerifier::of(&credential)
        ),
        "the printed verifier is not the credential's SHA-256"
    );
    assert!(
        !said(&first).contains(&credential),
        "`credential issue` printed the credential: {}",
        said(&first)
    );
    assert!(
        said(&first).contains("restart the service"),
        "`credential issue` did not say to restart the service: {}",
        said(&first)
    );
    #[cfg(unix)]
    {
        let issued = issued_at(home.path());
        assert_eq!(mode_of(&issued), 0o600);
        assert_eq!(mode_of(issued.parent().expect("a directory")), 0o700);
    }

    let again = run(home.path(), &["credential", "issue"], &[], b"");
    assert_eq!(
        again.status.code(),
        Some(i32::from(Exit::Unconfigured.status())),
        "{}",
        said(&again)
    );
    assert!(said(&again).contains("--replace"), "{}", said(&again));
    assert_eq!(
        issued(home.path())["credential"].as_str(),
        Some(credential.as_str()),
        "a refused issue replaced the configuration"
    );

    let replaced = run(
        home.path(),
        &["credential", "issue", "--replace"],
        &[("PRINTOBSERVER_SERVER", "http://127.0.0.1:9417")],
        b"",
    );
    assert_eq!(replaced.status.code(), Some(0), "{}", said(&replaced));
    let client = issued(home.path());
    assert_eq!(client["server"].as_str(), Some("http://127.0.0.1:9417"));
    assert_ne!(
        client["credential"].as_str(),
        Some(credential.as_str()),
        "a replaced credential is the one it replaced"
    );
    assert_ne!(verifier_line(&replaced), verifier_line(&first));
    #[cfg(unix)]
    assert_eq!(mode_of(&issued_at(home.path())), 0o600);

    let machine = run(
        home.path(),
        &["credential", "issue", "--replace", "--json"],
        &[],
        b"",
    );
    let document: printobserver_types::serde_json::Value =
        printobserver_types::serde_json::from_slice(&machine.stdout)
            .unwrap_or_else(|error| panic!("{error}: {}", said(&machine)));
    assert_eq!(
        document["credential_verifier"].as_str().map(str::to_owned),
        Some(
            CredentialVerifier::of(
                issued(home.path())["credential"]
                    .as_str()
                    .unwrap_or_default()
            )
            .to_string()
        )
    );
    assert_eq!(
        document["client_config"].as_str(),
        Some(issued_at(home.path()).display().to_string().as_str()),
        "the answer names a file other than the one the credential was written to"
    );
}

/// `credential verifier` prints the verifier of the one credential standard
/// input carries, and never echoes it; standard input carrying no credential is
/// refused saying so and quoting nothing.
#[test]
fn credential_verifier_reads_standard_input_and_never_echoes_it() {
    let home = TempDir::new().expect("an operator's own home");

    for input in [&b"abc"[..], b"abc\n", b"abc\r\n"] {
        let printed = run(home.path(), &["credential", "verifier"], &[], input);
        assert_eq!(printed.status.code(), Some(0), "{}", said(&printed));
        assert_eq!(
            verifier_line(&printed),
            format!("api.credential_verifier = \"{VERIFIER_OF_ABC}\"")
        );
    }

    let secret = "qx-an-operators-own-long-random-value-7Fd2";
    let printed = run(
        home.path(),
        &["credential", "verifier"],
        &[],
        secret.as_bytes(),
    );
    assert!(!said(&printed).contains(secret), "{}", said(&printed));

    for (refused, what) in [
        (&b""[..], "nothing"),
        (b"two\nlines\n", "two lines"),
        (b"qx-distinctive\x07held", "a control character"),
    ] {
        let printed = run(home.path(), &["credential", "verifier"], &[], refused);
        assert_eq!(
            printed.status.code(),
            Some(i32::from(Exit::Usage.status())),
            "standard input carrying {what} was accepted: {}",
            said(&printed)
        );
        assert!(
            said(&printed).contains("not a credential"),
            "{}",
            said(&printed)
        );
        assert!(
            !said(&printed).contains("qx-distinctive"),
            "{}",
            said(&printed)
        );
    }
    assert!(
        !issued_at(home.path()).exists(),
        "`credential verifier` wrote a configuration"
    );
}

/// A host that answers every request with an empty document, which is all the
/// server asks of its configured `OctoPrint` before it listens.
fn answering_host() -> SocketAddr {
    let listener = TcpListener::bind("127.0.0.1:0").expect("a loopback port");
    let address = listener.local_addr().expect("the bound address");
    std::thread::spawn(move || {
        for mut stream in listener.incoming().flatten() {
            std::thread::spawn(move || {
                let mut request = Vec::new();
                let mut buffer = [0_u8; 1024];
                while !request.windows(4).any(|window| window == b"\r\n\r\n") {
                    match stream.read(&mut buffer) {
                        Ok(0) | Err(_) => return,
                        Ok(read) => request.extend_from_slice(&buffer[..read]),
                    }
                }
                let _ = stream.write_all(
                    b"HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: 2\r\n\
                      Connection: close\r\n\r\n{}",
                );
            });
        }
    });
    address
}

/// A server, started with the line `credential issue` printed at the head of
/// its configuration.
struct Serving {
    /// The server.
    child: Child,
    /// Where it serves, as a client names it.
    url: String,
    /// Everything it printed before it said where it serves.
    log: String,
}

impl Drop for Serving {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

/// Start the server under `root`, listening where an issued configuration
/// names, with `line` at the head of its configuration — the verifier line
/// `credential issue` printed, a legacy `api.credential`, or nothing.
fn serving(root: &Path, listen: &str, line: &str) -> Serving {
    let octoprint = answering_host();
    let skill = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../skills/printobserver/SKILL.md")
        .display()
        .to_string();
    let state = root.join("state");
    let document = toml::toml! {
        state_dir = (state.display().to_string())
        listen = listen
        [octoprint]
        url = (format!("http://{octoprint}"))
        api_key = "a-provisioned-key"
        fan = "commandable"
        [supervisor]
        harness = "claude-code"
        skill_path = skill
        [ingress]
        shared_secret = "a-shared-secret"
        [safety]
        agent_min_interval_s = 30
        [safety.allowed]
        feedrate = { min = 0.5, max = 1.5 }
        [safety.actions]
        operator = ["pause"]
        agent = []
        system = []
    };
    let configuration = root.join("server.toml");
    std::fs::write(&configuration, format!("{line}\n{document}"))
        .expect("the configuration is writable");
    let mut child = Command::new(env!("CARGO_BIN_EXE_printobserver"))
        .arg("server")
        .arg("--config")
        .arg(&configuration)
        .stderr(Stdio::piped())
        .spawn()
        .expect("the server starts");
    let (address, stream) = announced::serving(&mut child, "the server under the issued verifier");
    Serving {
        child,
        url: format!("http://{address}"),
        log: stream.printed,
    }
}

/// A loopback address nothing listens on now, for the server to take.
fn free_address() -> String {
    let listener = TcpListener::bind("127.0.0.1:0").expect("a loopback port");
    listener
        .local_addr()
        .expect("the bound address")
        .to_string()
}

/// Whether one run was admitted: a listing that answered.
fn admitted(output: &Output) -> bool {
    output.status.code() == Some(0) && String::from_utf8_lossy(&output.stdout).contains("prints")
}

/// An operator's issued credential and the server it was issued for.
struct Issued {
    /// The operator's own configuration home.
    home: PathBuf,
    /// Where the server serves, as a client names it.
    url: String,
    /// The credential that was issued.
    credential: String,
    /// The server, configured with its verifier.
    server: Serving,
}

/// Issue the operator a credential naming a free address, and start the
/// server there with the verifier `credential issue` printed.
fn issued_and_serving(root: &Path) -> Issued {
    let home = root.join("operator-home");
    std::fs::create_dir_all(&home).expect("the operator's own home");
    let listen = free_address();
    let url = format!("http://{listen}");
    let issued_run = run(
        &home,
        &["credential", "issue"],
        &[("PRINTOBSERVER_SERVER", &url)],
        b"",
    );
    assert_eq!(issued_run.status.code(), Some(0), "{}", said(&issued_run));
    let credential = issued(&home)["credential"]
        .as_str()
        .expect("a credential")
        .to_owned();
    let server = serving(root, &listen, &verifier_line(&issued_run));
    assert_eq!(server.url, url);
    Issued {
        home,
        url,
        credential,
        server,
    }
}

/// An operator who issued a credential and put its verifier in the server's
/// configuration is admitted by every command with no `--config` and no
/// variable: their own configuration is read first. `--config` reads only the
/// file it names; the two variables win over every file; with both set and no
/// `--config` no file is read at all; and a variable naming the server beside
/// an operator file naming only the credential is the two of them together.
#[test]
fn an_issued_credential_configures_every_command_by_the_documented_order() {
    let root = TempDir::new().expect("a journey's own root");
    let Issued {
        home,
        url,
        credential,
        server,
    } = issued_and_serving(root.path());

    // Their own configuration, and nothing else.
    let own = run(&home, &["prints", "--json"], &[], b"");
    assert!(
        admitted(&own),
        "the operator's own configuration was not read: {}",
        said(&own)
    );

    // `--config` reads only the file it names, the operator's own included.
    let other = root.path().join("other.toml");
    std::fs::write(
        &other,
        format!("[client]\nserver = \"{url}\"\ncredential = \"not-the-operators\"\n"),
    )
    .expect("writable");
    let named = run(
        &home,
        &["prints", "--json", "--config", &other.display().to_string()],
        &[],
        b"",
    );
    assert_eq!(
        named.status.code(),
        Some(i32::from(Exit::Unconfigured.status())),
        "a named file was not the only one read: {}",
        said(&named)
    );

    // The variables win over the operator's own file.
    let overridden = run(
        &home,
        &["prints", "--json"],
        &[
            ("PRINTOBSERVER_SERVER", &url),
            ("PRINTOBSERVER_CREDENTIAL", "not-the-operators"),
        ],
        b"",
    );
    assert_eq!(
        overridden.status.code(),
        Some(i32::from(Exit::Unconfigured.status())),
        "the variables did not win over the operator's own file: {}",
        said(&overridden)
    );

    // An operator file naming the credential alone takes the server from the
    // variable.
    std::fs::write(
        issued_at(&home),
        format!("[client]\ncredential = \"{credential}\"\n"),
    )
    .expect("writable");
    let together = run(
        &home,
        &["prints", "--json"],
        &[("PRINTOBSERVER_SERVER", &url)],
        b"",
    );
    assert!(
        admitted(&together),
        "the operator's credential and the variable's server were not taken together: {}",
        said(&together)
    );

    // An operator file that is not a document is read first, and refused
    // naming it — unless both variables are set, when no file is read at all.
    std::fs::write(issued_at(&home), "[client]\nthis is not a document\n").expect("writable");
    let unreadable = run(&home, &["prints", "--json"], &[], b"");
    assert_eq!(
        unreadable.status.code(),
        Some(i32::from(Exit::Unconfigured.status())),
        "{}",
        said(&unreadable)
    );
    assert!(
        said(&unreadable).contains("client.toml"),
        "the operator's own file was not the one read: {}",
        said(&unreadable)
    );
    let from_the_environment = run(
        &home,
        &["prints", "--json"],
        &[
            ("PRINTOBSERVER_SERVER", &url),
            ("PRINTOBSERVER_CREDENTIAL", &credential),
        ],
        b"",
    );
    assert!(
        admitted(&from_the_environment),
        "with both variables set a file was read anyway: {}",
        said(&from_the_environment)
    );
    drop(server);
}

/// An operator who lost the credential issues a new one with `--replace`, puts
/// its verifier in the configuration and restarts the server: the new one is
/// admitted, and the old one is refused.
#[test]
fn a_replaced_credential_is_admitted_and_the_old_one_is_refused() {
    let root = TempDir::new().expect("a journey's own root");
    let home = root.path().join("operator-home");
    std::fs::create_dir_all(&home).expect("the operator's own home");
    let listen = free_address();
    let url = format!("http://{listen}");
    let server_env = [("PRINTOBSERVER_SERVER", url.as_str())];

    let first = run(&home, &["credential", "issue"], &server_env, b"");
    let old = issued(&home)["credential"]
        .as_str()
        .expect("a credential")
        .to_owned();
    let server = serving(root.path(), &listen, &verifier_line(&first));
    assert!(admitted(&run(&home, &["prints", "--json"], &[], b"")));
    drop(server);

    let replaced = run(
        &home,
        &["credential", "issue", "--replace"],
        &server_env,
        b"",
    );
    assert_eq!(replaced.status.code(), Some(0), "{}", said(&replaced));
    let server = serving(root.path(), &listen, &verifier_line(&replaced));
    assert!(
        admitted(&run(&home, &["prints", "--json"], &[], b"")),
        "the replaced credential was not admitted"
    );
    let stale = run(
        &home,
        &["prints", "--json"],
        &[
            ("PRINTOBSERVER_SERVER", url.as_str()),
            ("PRINTOBSERVER_CREDENTIAL", old.as_str()),
        ],
        b"",
    );
    assert_eq!(
        stale.status.code(),
        Some(i32::from(Exit::Unconfigured.status())),
        "the credential that was replaced is still admitted: {}",
        said(&stale)
    );
    drop(server);
}

/// The layout printobserver v0.3.0 left a state directory in: the credential it
/// generated, and the client configuration carrying it beside the address,
/// both private to the service.
fn legacy_layout(state: &Path, credential: &str, url: &str) {
    std::fs::create_dir_all(state).expect("a state directory");
    let generated = state.join("api-credential");
    let client = state.join("client.toml");
    std::fs::write(&generated, credential).expect("writable");
    std::fs::write(
        &client,
        format!("[client]\nserver = \"{url}\"\ncredential = \"{credential}\"\n"),
    )
    .expect("writable");
    #[cfg(unix)]
    for path in [&generated, &client] {
        std::fs::set_permissions(path, std::os::unix::fs::PermissionsExt::from_mode(0o600))
            .expect("private");
    }
}

/// An installation printobserver v0.3.0 left is converted on its first start:
/// the generated credential becomes its verifier, private to the service, and
/// both plaintext files are removed — and the credential its operator already
/// holds keeps authenticating, through the two variables and through their own
/// `--config` file.
#[test]
fn a_legacy_installation_keeps_its_operators_credential_through_every_route() {
    const LEGACY: &str = "Zq9Xr2Lk7Vb4Nw1Hc8Td5Ms3Pf6Jy0GaUe2Qo9Ri4Ex";
    let root = TempDir::new().expect("a journey's own root");
    let home = root.path().join("operator-home");
    std::fs::create_dir_all(&home).expect("an empty home");
    let listen = free_address();
    let url = format!("http://{listen}");
    let state = root.path().join("state");
    legacy_layout(&state, LEGACY, &url);

    let server = serving(root.path(), &listen, "");

    assert!(
        !state.join("api-credential").exists(),
        "the plaintext was left"
    );
    assert!(
        !state.join("client.toml").exists(),
        "the client configuration was left"
    );
    let verifier = state.join("api-credential.verifier");
    assert_eq!(
        std::fs::read_to_string(&verifier).expect("a verifier was written"),
        format!("{}\n", CredentialVerifier::of(LEGACY))
    );
    #[cfg(unix)]
    assert_eq!(mode_of(&verifier), 0o600);

    let by_the_variables = run(
        &home,
        &["prints", "--json"],
        &[
            ("PRINTOBSERVER_SERVER", url.as_str()),
            ("PRINTOBSERVER_CREDENTIAL", LEGACY),
        ],
        b"",
    );
    assert!(admitted(&by_the_variables), "{}", said(&by_the_variables));
    let own = root.path().join("their-own.toml");
    std::fs::write(
        &own,
        format!("[client]\nserver = \"{url}\"\ncredential = \"{LEGACY}\"\n"),
    )
    .expect("writable");
    let by_their_own_file = run(
        &home,
        &["prints", "--json", "--config", &own.display().to_string()],
        &[],
        b"",
    );
    assert!(admitted(&by_their_own_file), "{}", said(&by_their_own_file));
    assert!(
        !server.log.contains(LEGACY),
        "the server printed the legacy credential"
    );
    drop(server);
}

/// A plaintext `api.credential` an operator wrote does not stop the server: it
/// is admitted, and every start writes a warning to the server's log naming the
/// key, its replacement and the command that computes one, saying a supervision
/// turn can read it — and never quoting it.
#[test]
fn a_plaintext_api_credential_is_admitted_and_warned_about_in_the_log_on_every_start() {
    const PLAINTEXT: &str = "qx-an-operators-plaintext-credential-4Rk8";
    let root = TempDir::new().expect("a journey's own root");
    let home = root.path().join("operator-home");
    std::fs::create_dir_all(&home).expect("an empty home");
    let listen = free_address();
    let url = format!("http://{listen}");

    for start in ["the first start", "the second start"] {
        let server = serving(
            root.path(),
            &listen,
            &format!("api.credential = \"{PLAINTEXT}\""),
        );
        let warning = server
            .log
            .lines()
            .find(|line| line.contains("warning") && line.contains("`api.credential`"))
            .unwrap_or_else(|| panic!("{start} logged no warning: {}", server.log));
        for named in [
            "api.credential_verifier",
            "printobserver credential verifier",
            "supervision turn",
        ] {
            assert!(
                warning.contains(named),
                "{start}'s warning names no {named}: {warning}"
            );
        }
        assert!(
            !server.log.contains("qx-an-operators"),
            "{start} quoted it: {}",
            server.log
        );
        let admitted_run = run(
            &home,
            &["prints", "--json"],
            &[
                ("PRINTOBSERVER_SERVER", url.as_str()),
                ("PRINTOBSERVER_CREDENTIAL", PLAINTEXT),
            ],
            b"",
        );
        assert!(admitted(&admitted_run), "{start}: {}", said(&admitted_run));
        drop(server);
    }
}

/// With no configuration home there is nowhere to keep a credential: refused,
/// naming the variable to set, and nothing written.
#[test]
fn credential_issue_with_no_configuration_home_says_which_variable_to_set() {
    let refused = running(None, &["credential", "issue"], &[], b"");

    assert_eq!(
        refused.status.code(),
        Some(i32::from(Exit::Unconfigured.status())),
        "{}",
        said(&refused)
    );
    assert!(said(&refused).contains(HOME_VARIABLE), "{}", said(&refused));
    assert!(!said(&refused).contains("api.credential_verifier"));
}

/// `PRINTOBSERVER_SERVER` naming no address is refused before anything is
/// written; a bare `host:port` is written as the address a client reads.
#[test]
fn credential_issue_holds_the_server_it_writes_to_an_address() {
    let home = TempDir::new().expect("an operator's own home");

    let refused = run(
        home.path(),
        &["credential", "issue"],
        &[("PRINTOBSERVER_SERVER", "the printer room")],
        b"",
    );
    assert_eq!(
        refused.status.code(),
        Some(i32::from(Exit::Unconfigured.status())),
        "{}",
        said(&refused)
    );
    assert!(
        !issued_at(home.path()).exists(),
        "a refused issue wrote a configuration"
    );

    let bare = run(
        home.path(),
        &["credential", "issue"],
        &[("PRINTOBSERVER_SERVER", "127.0.0.1:9418")],
        b"",
    );
    assert_eq!(bare.status.code(), Some(0), "{}", said(&bare));
    assert_eq!(
        issued(home.path())["server"].as_str(),
        Some("http://127.0.0.1:9418")
    );
}

/// A configuration that cannot be written is refused naming it, and one that
/// was already there is left exactly as it was.
#[test]
fn credential_issue_that_cannot_write_leaves_what_was_there() {
    let home = TempDir::new().expect("an operator's own home");
    let target = issued_at(home.path());
    let directory = target.parent().expect("the configuration has a directory");
    std::fs::create_dir_all(directory.parent().expect("a home above it")).expect("writable");
    std::fs::write(directory, b"a file where the directory would be").expect("writable");

    let refused = run(home.path(), &["credential", "issue"], &[], b"");
    assert_eq!(
        refused.status.code(),
        Some(i32::from(Exit::Unconfigured.status())),
        "{}",
        said(&refused)
    );
    assert!(
        said(&refused).contains("could not be written"),
        "{}",
        said(&refused)
    );

    // A configuration that cannot be moved into place — a directory stands
    // where it would go — is refused after the credential was staged, and the
    // staged copy is removed rather than left holding a credential.
    let home = TempDir::new().expect("an operator's own home");
    let target = issued_at(home.path());
    std::fs::create_dir_all(target.join("held")).expect("a directory where the file would be");
    let unmoved = run(home.path(), &["credential", "issue", "--replace"], &[], b"");
    assert_eq!(
        unmoved.status.code(),
        Some(i32::from(Exit::Unconfigured.status())),
        "{}",
        said(&unmoved)
    );
    assert!(target.join("held").is_dir(), "what was there was changed");
    let left: Vec<_> = std::fs::read_dir(target.parent().expect("a directory"))
        .expect("readable")
        .map(|entry| entry.expect("an entry").file_name())
        .collect();
    assert_eq!(
        left,
        vec![std::ffi::OsString::from("client.toml")],
        "a staged credential was left"
    );

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt as _;

        let home = TempDir::new().expect("an operator's own home");
        let first = run(home.path(), &["credential", "issue"], &[], b"");
        assert_eq!(first.status.code(), Some(0), "{}", said(&first));
        let held = std::fs::read(issued_at(home.path())).expect("issued");
        let directory = issued_at(home.path())
            .parent()
            .expect("a directory")
            .to_path_buf();
        std::fs::set_permissions(&directory, std::fs::Permissions::from_mode(0o500))
            .expect("the directory is made read-only");
        let replaced = run(home.path(), &["credential", "issue", "--replace"], &[], b"");
        std::fs::set_permissions(&directory, std::fs::Permissions::from_mode(0o700))
            .expect("the directory is made writable again");
        assert_eq!(
            replaced.status.code(),
            Some(i32::from(Exit::Unconfigured.status())),
            "{}",
            said(&replaced)
        );
        assert_eq!(
            std::fs::read(issued_at(home.path())).expect("still there"),
            held,
            "a replacement that could not be written changed the configuration"
        );
    }
}

/// `credential verifier --json` answers a document carrying the verifier
/// alone, and standard input that is not text is refused quoting nothing.
#[test]
fn credential_verifier_answers_a_document_and_refuses_what_is_not_text() {
    let home = TempDir::new().expect("an operator's own home");

    let machine = run(
        home.path(),
        &["credential", "verifier", "--json"],
        &[],
        b"abc\n",
    );
    let document: printobserver_types::serde_json::Value =
        printobserver_types::serde_json::from_slice(&machine.stdout)
            .unwrap_or_else(|error| panic!("{error}: {}", said(&machine)));
    assert_eq!(
        document,
        printobserver_types::serde_json::json!({ "credential_verifier": VERIFIER_OF_ABC })
    );

    let refused = run(home.path(), &["credential", "verifier"], &[], b"qx\xff\xfe");
    assert_eq!(
        refused.status.code(),
        Some(i32::from(Exit::Usage.status())),
        "{}",
        said(&refused)
    );
    assert!(said(&refused).contains("not text"), "{}", said(&refused));
}

/// Standard input that cannot be read — here a directory, which every Unix
/// refuses a read of — is refused as usage, saying so and printing no verifier.
#[cfg(unix)]
#[test]
fn credential_verifier_refuses_standard_input_it_cannot_read() {
    let home = TempDir::new().expect("an operator's own home");
    let mut command = Command::new(env!("CARGO_BIN_EXE_printobserver"));
    command
        .args(["credential", "verifier"])
        .env_remove("PRINTOBSERVER_SERVER")
        .env_remove("PRINTOBSERVER_CREDENTIAL")
        .env(HOME_VARIABLE, home.path())
        .stdin(std::fs::File::open(home.path()).expect("the directory opens"));

    let refused = command.output().expect("the program exits");

    assert_eq!(
        refused.status.code(),
        Some(i32::from(Exit::Usage.status())),
        "{}",
        said(&refused)
    );
    assert!(
        said(&refused).contains("could not be read"),
        "{}",
        said(&refused)
    );
    assert!(
        !said(&refused).contains("sha256:"),
        "a verifier was printed: {}",
        said(&refused)
    );
}

/// How many `credential issue` runs contend for one configuration home at once.
const CONTENDERS: usize = 8;

/// How many times a contest is held: one race can happen to serialise, so the
/// journeys below hold several before concluding nothing overlapped.
const CONTESTS: usize = 12;

/// `credential issue` run by every contender against one configuration home at
/// once, each with its standard input closed, and what each printed.
fn contested(home: &Path, arguments: &[&str]) -> Vec<Output> {
    let children: Vec<Child> = (0..CONTENDERS)
        .map(|_| {
            let mut command = Command::new(env!("CARGO_BIN_EXE_printobserver"));
            command
                .args(arguments)
                .env_remove("PRINTOBSERVER_SERVER")
                .env_remove("PRINTOBSERVER_CREDENTIAL")
                .stdin(Stdio::null())
                .stdout(Stdio::piped())
                .stderr(Stdio::piped());
            for name in HOME_VARIABLES {
                command.env_remove(name);
            }
            command.env(HOME_VARIABLE, home);
            command.spawn().expect("the program runs")
        })
        .collect();
    children
        .into_iter()
        .map(|child| child.wait_with_output().expect("the program exits"))
        .collect()
}

/// The verifier line the configuration `credential issue` left would need.
fn stored_verifier_line(home: &Path) -> String {
    let credential = issued(home)["credential"]
        .as_str()
        .expect("a credential is stored")
        .to_owned();
    format!(
        "api.credential_verifier = \"{}\"",
        CredentialVerifier::of(&credential)
    )
}

/// What the configuration directory holds once every contender has exited:
/// the configuration alone, and no copy staged on the way to it.
fn assert_nothing_staged(home: &Path) {
    let directory = issued_at(home).parent().expect("a directory").to_path_buf();
    let mut left: Vec<String> = std::fs::read_dir(&directory)
        .expect("readable")
        .map(|entry| {
            entry
                .expect("an entry")
                .file_name()
                .to_string_lossy()
                .into_owned()
        })
        .collect();
    left.sort();
    assert_eq!(
        left,
        vec!["client.toml".to_owned()],
        "a staged copy was left"
    );
}

/// Issues racing for a configuration home with none in it publish exactly one
/// configuration: one run succeeds, and the verifier it printed is the one the
/// stored credential needs; every other run is refused for finding a
/// configuration already there, prints no verifier, and leaves the winner's
/// file as it was.
#[test]
fn racing_issues_publish_exactly_one_configuration_and_print_its_verifier() {
    for contest in 0..CONTESTS {
        let home = TempDir::new().expect("an operator's own home");

        let outputs = contested(home.path(), &["credential", "issue"]);

        let (won, lost): (Vec<&Output>, Vec<&Output>) =
            outputs.iter().partition(|output| output.status.success());
        assert_eq!(
            won.len(),
            1,
            "contest {contest}: {} issues succeeded without `--replace`: {}",
            won.len(),
            outputs.iter().map(said).collect::<Vec<_>>().join("\n---\n")
        );
        assert_eq!(
            verifier_line(won[0]),
            stored_verifier_line(home.path()),
            "contest {contest}: the winner printed a verifier the stored credential does not have"
        );
        for loser in lost {
            assert_eq!(
                loser.status.code(),
                Some(i32::from(Exit::Unconfigured.status())),
                "contest {contest}: {}",
                said(loser)
            );
            assert!(
                said(loser).contains("already there"),
                "contest {contest}: a loser was refused for something else: {}",
                said(loser)
            );
            assert!(
                !String::from_utf8_lossy(&loser.stdout).contains("api.credential_verifier"),
                "contest {contest}: a loser printed a verifier: {}",
                said(loser)
            );
        }
        assert_nothing_staged(home.path());
    }
}

/// Replacements racing over one configuration each succeed, and what is left
/// is whole: the stored credential is one a contender issued and printed the
/// verifier of, and no staged copy is left beside it.
#[test]
fn racing_replacements_each_succeed_and_leave_one_whole_configuration() {
    for contest in 0..CONTESTS {
        let home = TempDir::new().expect("an operator's own home");
        let first = run(home.path(), &["credential", "issue"], &[], b"");
        assert_eq!(first.status.code(), Some(0), "{}", said(&first));

        let outputs = contested(home.path(), &["credential", "issue", "--replace"]);

        for output in &outputs {
            assert_eq!(
                output.status.code(),
                Some(0),
                "contest {contest}: a replacement failed: {}",
                said(output)
            );
        }
        let stored = stored_verifier_line(home.path());
        assert!(
            outputs.iter().any(|output| verifier_line(output) == stored),
            "contest {contest}: the stored credential is one no contender printed the verifier of"
        );
        assert_nothing_staged(home.path());
    }
}

/// A staged file another issue left behind is stepped past rather than
/// written into or removed: the issue stages under the next free name,
/// publishes, and leaves the stale file exactly as it was. With every name
/// taken, it is refused saying so and writes nothing.
#[test]
fn credential_issue_steps_past_staged_files_it_does_not_own() {
    let home = TempDir::new().expect("an operator's own home");
    let directory = issued_at(home.path())
        .parent()
        .expect("a directory")
        .to_path_buf();
    std::fs::create_dir_all(&directory).expect("writable");
    let stale = directory.join(".client.toml.0.new");
    std::fs::write(&stale, b"another issue's staged credential").expect("writable");

    let stepped = run(home.path(), &["credential", "issue"], &[], b"");

    assert_eq!(stepped.status.code(), Some(0), "{}", said(&stepped));
    assert_eq!(verifier_line(&stepped), stored_verifier_line(home.path()));
    assert_eq!(
        std::fs::read(&stale).expect("still there"),
        b"another issue's staged credential",
        "another issue's staged file was written into or removed"
    );

    let crowded = TempDir::new().expect("an operator's own home");
    let directory = issued_at(crowded.path())
        .parent()
        .expect("a directory")
        .to_path_buf();
    std::fs::create_dir_all(&directory).expect("writable");
    for name in 0..printobserver::credential::STAGING_NAMES {
        std::fs::write(directory.join(format!(".client.toml.{name}.new")), b"taken")
            .expect("writable");
    }

    let refused = run(crowded.path(), &["credential", "issue"], &[], b"");

    assert_eq!(
        refused.status.code(),
        Some(i32::from(Exit::Unconfigured.status())),
        "{}",
        said(&refused)
    );
    assert!(
        said(&refused).contains("is taken") && said(&refused).contains(".client.toml.*.new"),
        "the refusal does not say the staging names are taken: {}",
        said(&refused)
    );
    assert!(
        !issued_at(crowded.path()).exists(),
        "a configuration was written"
    );
    assert!(
        !String::from_utf8_lossy(&refused.stdout).contains("api.credential_verifier"),
        "a verifier was printed: {}",
        said(&refused)
    );
}

/// With `XDG_CONFIG_HOME` unset or relative, the operator's configuration is
/// kept under `$HOME/.config`: `credential issue` writes it there, and every
/// command after reads it from there and is admitted.
#[cfg(all(unix, not(target_os = "macos")))]
#[test]
fn an_operator_with_no_rooted_xdg_config_home_is_kept_under_home() {
    let root = TempDir::new().expect("a journey's own root");
    let home = root.path().join("home");
    std::fs::create_dir_all(&home).expect("the operator's own home");
    let listen = free_address();
    let url = format!("http://{listen}");
    let under_home = |arguments: &[&str], xdg: Option<&str>, server: Option<&str>| {
        let mut command = Command::new(env!("CARGO_BIN_EXE_printobserver"));
        command
            .args(arguments)
            .current_dir(root.path())
            .env_remove("PRINTOBSERVER_SERVER")
            .env_remove("PRINTOBSERVER_CREDENTIAL")
            .stdin(Stdio::null());
        for name in HOME_VARIABLES {
            command.env_remove(name);
        }
        command.env("HOME", &home);
        if let Some(xdg) = xdg {
            command.env("XDG_CONFIG_HOME", xdg);
        }
        if let Some(server) = server {
            command.env("PRINTOBSERVER_SERVER", server);
        }
        command.output().expect("the program runs")
    };

    let issued_run = under_home(&["credential", "issue"], None, Some(&url));

    assert_eq!(issued_run.status.code(), Some(0), "{}", said(&issued_run));
    let kept = home
        .join(".config")
        .join("printobserver")
        .join("client.toml");
    assert!(
        kept.is_file(),
        "nothing was kept under $HOME/.config: {}",
        said(&issued_run)
    );
    let _server = serving(root.path(), &listen, &verifier_line(&issued_run));
    for xdg in [None, Some("relative/config")] {
        let answered = under_home(&["prints", "--json"], xdg, None);
        assert!(
            admitted(&answered),
            "with XDG_CONFIG_HOME {xdg:?} the configuration under $HOME was not read: {}",
            said(&answered)
        );
    }
    assert!(
        !root.path().join("relative").exists(),
        "a relative XDG_CONFIG_HOME was written to"
    );
}

/// The credential commands read no configuration, so `--config` is refused
/// by both rather than taken as a file to write; and a credential command
/// named without its second word, with an unknown one, or with `--replace`
/// twice is refused naming what is wrong.
#[test]
fn the_credential_commands_refuse_what_they_do_not_take() {
    let home = TempDir::new().expect("an operator's own home");
    let elsewhere = home.path().join("shared.toml");
    let named = elsewhere.display().to_string();

    for (arguments, naming) in [
        (vec!["credential", "issue", "--config", &named], "--config"),
        (
            vec!["credential", "verifier", "--config", &named],
            "--config",
        ),
        (vec!["credential"], "`issue` or `verifier`"),
        (vec!["credential", "rotate"], "credential rotate"),
        (
            vec!["credential", "issue", "--replace", "--replace"],
            "--replace",
        ),
    ] {
        let refused = run(home.path(), &arguments, &[], b"abc");
        assert_eq!(
            refused.status.code(),
            Some(i32::from(Exit::Usage.status())),
            "{arguments:?}: {}",
            said(&refused)
        );
        assert!(
            said(&refused).contains(naming),
            "{arguments:?}: {}",
            said(&refused)
        );
    }
    assert!(!elsewhere.exists(), "a named file was written");
    assert!(
        !issued_at(home.path()).exists(),
        "a refused command wrote a configuration"
    );
}
