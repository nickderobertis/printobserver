//! The refusals a caller reaches before any request is made.
//!
//! Everything here drives the compiled program as a subprocess, the way a user
//! does. What it is about is the two things that happen on this host — reading
//! the arguments and reading the configuration — so nothing here needs a
//! server, and every journey that does is `tests/journeys.rs`.

use std::net::{TcpListener, TcpStream};
use std::path::Path;
use std::process::Command;

use printobserver::failure::Exit;
use printobserver::surface::{MAX_DURATION_SECONDS, MIN_DURATION_SECONDS};

/// A print this system could have minted.
const A_PRINT: &str = "01a08000-0000-7000-8000-000000000001";

/// One invocation of the built program, under an environment configuring
/// nothing, and everything it said.
fn run(arguments: &[&str]) -> (Option<i32>, String) {
    running(arguments, &[])
}

/// One invocation of the built program under the environment given.
fn running(arguments: &[&str], environment: &[(&str, &str)]) -> (Option<i32>, String) {
    let mut process = Command::new(env!("CARGO_BIN_EXE_printobserver"));
    process
        .args(arguments)
        .env_remove("PRINTOBSERVER_SERVER")
        .env_remove("PRINTOBSERVER_CREDENTIAL");
    for (name, value) in environment {
        process.env(name, value);
    }
    let output = process
        .output()
        .expect("the built `printobserver` binary should be spawnable");
    (
        output.status.code(),
        String::from_utf8_lossy(&output.stdout).into_owned()
            + &String::from_utf8_lossy(&output.stderr),
    )
}

/// An address nothing is listening on.
///
/// A port is bound to learn one that is free and then released, so what a read
/// meets is a refused connection rather than a served refusal.
fn nothing_listening() -> String {
    let listener = TcpListener::bind("127.0.0.1:0").expect("a loopback port");
    let address = listener.local_addr().expect("the bound address");
    format!("http://{address}")
}

/// The program prints its surface and exits zero.
#[test]
fn the_installed_command_runs_and_exits_zero() {
    for arguments in [vec![], vec!["--help"]] {
        let (code, said) = run(&arguments);

        assert_eq!(code, Some(0), "`{arguments:?}` exited {code:?}: {said}");
        assert!(
            said.contains("printobserver server --config") || said.contains("printobserver server"),
            "the surface does not name the command that runs the supervisor: {said}"
        );
        assert!(
            said.contains("--json") && said.contains("--config"),
            "the surface does not name the options every command takes: {said}"
        );
    }
}

/// The program prints its own version and exits zero.
#[test]
fn the_program_prints_its_own_version() {
    let (code, said) = run(&["--version"]);

    assert_eq!(code, Some(0));
    assert!(
        said.contains(env!("CARGO_PKG_VERSION")),
        "the version this program prints is not the one it was built as: {said}"
    );
}

/// An invocation this program does not answer to is refused, naming it.
#[test]
fn an_invocation_this_program_does_not_answer_to_is_refused() {
    for (arguments, named) in [
        (vec!["fly"], "fly"),
        (vec!["server"], "configuration file"),
        (vec!["server", "--fast", "x"], "--fast"),
        (vec!["server", "--config"], "--config"),
        (
            vec!["server", "--config", "one.toml", "--config", "another.toml"],
            "twice",
        ),
        (vec!["status"], "--print-id"),
        (vec!["status", "--print-id"], "takes a value"),
        (vec!["pause", "--print-id", A_PRINT], "--actor"),
        (vec!["context", "--quickly", "x"], "--quickly"),
        (
            vec!["context", "--print-id", A_PRINT, "--print-id", A_PRINT],
            "twice",
        ),
        (
            vec!["context", "--print-id", "a print\r\nGET /elsewhere"],
            "not an identifier this system mints",
        ),
        (
            vec![
                "set-feedrate-factor",
                "--print-id",
                A_PRINT,
                "--actor",
                "operator",
                "--reason",
                "a test is asking",
                "--factor",
                "quickly",
            ],
            "takes a number",
        ),
        (
            vec!["history", "--print-id", A_PRINT, "--limit", "some"],
            "takes a whole number",
        ),
        (
            vec![
                "manifest-set",
                "--print-id",
                A_PRINT,
                "--reason",
                "a test is asking",
                "--manifest-file",
                "/nowhere/at/all.json",
            ],
            "could not be read",
        ),
    ] {
        let (code, said) = run(&arguments);
        assert_eq!(
            code,
            Some(i32::from(Exit::Usage.status())),
            "`{arguments:?}` was accepted: {said}"
        );
        assert!(
            said.contains(named),
            "`{arguments:?}` was refused without naming `{named}`: {said}"
        );
    }
}

/// A duration outside what this program declares is refused where it is asked
/// for.
#[test]
fn a_duration_outside_what_this_program_declares_is_refused() {
    for offered in [
        "0",
        "-1",
        "quickly",
        &(MAX_DURATION_SECONDS + 1).to_string(),
    ] {
        let (code, said) = run(&[
            "set-fan-percent",
            "--print-id",
            A_PRINT,
            "--actor",
            "operator",
            "--reason",
            "a test is asking",
            "--percent",
            "40",
            "--duration-s",
            offered,
        ]);

        assert_eq!(
            code,
            Some(i32::from(Exit::Usage.status())),
            "a duration of `{offered}` was accepted: {said}"
        );
        assert!(
            said.contains("whole number")
                || (said.contains(&MIN_DURATION_SECONDS.to_string())
                    && said.contains(&MAX_DURATION_SECONDS.to_string())),
            "a duration of `{offered}` was refused without saying what is accepted: {said}"
        );
    }
}

/// A program nothing configured says which file to write and what to set.
#[test]
fn a_program_nothing_configured_says_what_to_configure() {
    let (code, said) = run(&["status", "--print-id", A_PRINT]);

    assert_eq!(code, Some(i32::from(Exit::Unconfigured.status())));
    assert!(
        said.contains("PRINTOBSERVER_SERVER") && said.contains("[client]"),
        "the refusal names no way to configure this program: {said}"
    );
}

/// A configuration file this program will not read is refused, naming it.
#[test]
fn a_configuration_file_this_program_will_not_read_is_refused() {
    let root = tempfile::TempDir::new().expect("a journey's own root");
    let unparsable = write(
        root.path(),
        "unparsable.toml",
        "[client]\nthis is not toml\n",
    );
    let addressless = write(
        root.path(),
        "addressless.toml",
        "[client]\ncredential = \"x\"\n",
    );
    let unreachable = write(
        root.path(),
        "unreachable.toml",
        "[client]\nserver = \"http://a-name-rather-than-an-address:1\"\n",
    );
    let empty = write(
        root.path(),
        "empty-credential.toml",
        "[client]\nserver = \"http://127.0.0.1:8420\"\ncredential = \"  \"\n",
    );
    let missing = root.path().join("nowhere.toml");

    for (path, named) in [
        (unparsable, "not a document"),
        (addressless, "has no supervisor to talk to"),
        (unreachable, "is not an address"),
        (empty, "is empty"),
        (missing, "could not be read"),
    ] {
        let (code, said) = run(&[
            "status",
            "--print-id",
            A_PRINT,
            "--config",
            &path.display().to_string(),
        ]);

        assert_eq!(
            code,
            Some(i32::from(Exit::Unconfigured.status())),
            "{} was taken: {said}",
            path.display()
        );
        assert!(
            said.contains(named),
            "{} was refused without saying `{named}`: {said}",
            path.display()
        );
    }
}

/// One configuration file under a root, and its path.
fn write(root: &Path, name: &str, contents: &str) -> std::path::PathBuf {
    let path = root.join(name);
    std::fs::write(&path, contents).expect("a configuration is writable");
    path
}

/// The environment names the server when nothing else does, and wins when
/// something does.
#[test]
fn the_environment_names_the_server_and_wins_over_the_file() {
    let root = tempfile::TempDir::new().expect("a journey's own root");
    let pointed = write(
        root.path(),
        "pointed.toml",
        "[client]\nserver = \"http://127.0.0.1:1\"\n",
    );
    let elsewhere = nothing_listening();

    let (code, said) = running(
        &[
            "status",
            "--print-id",
            A_PRINT,
            "--config",
            &pointed.display().to_string(),
        ],
        &[("PRINTOBSERVER_SERVER", &elsewhere)],
    );

    assert_eq!(code, Some(i32::from(Exit::Unreachable.status())));
    assert!(
        said.contains(&elsewhere),
        "the environment did not win over the file: {said}"
    );
}

/// The server's own configuration file configures the clients beside it.
#[test]
fn the_servers_own_configuration_file_configures_a_client() {
    let root = tempfile::TempDir::new().expect("a journey's own root");
    let address = nothing_listening();
    let served = write(
        root.path(),
        "config.toml",
        &format!(
            "state_dir = \"/var/lib/printobserver\"\nlisten = \"{}\"\n",
            address.trim_start_matches("http://")
        ),
    );

    let (code, said) = run(&[
        "status",
        "--print-id",
        A_PRINT,
        "--config",
        &served.display().to_string(),
    ]);

    assert_eq!(code, Some(i32::from(Exit::Unreachable.status())));
    assert!(
        said.contains(&address),
        "the address the server was told to listen on did not configure this program: {said}"
    );
}

/// A read of a supervisor that is not there says where it looked and what to
/// do.
///
/// Driven over a read that leaves an optional value out as well as one that has
/// none, because what a command asks for is built from the values it was given
/// rather than from the ones it could have been.
#[test]
fn a_read_of_a_supervisor_that_is_not_there_says_where_it_looked() {
    let address = nothing_listening();

    let (code, said) = running(
        &["history", "--print-id", A_PRINT],
        &[("PRINTOBSERVER_SERVER", &address)],
    );
    assert_eq!(code, Some(i32::from(Exit::Unreachable.status())));
    assert!(
        said.contains(&address),
        "the refusal does not say where it looked: {said}"
    );

    let (code, said) = running(
        &["context", "--print-id", A_PRINT],
        &[("PRINTOBSERVER_SERVER", &address)],
    );

    assert_eq!(code, Some(i32::from(Exit::Unreachable.status())));
    assert!(
        said.contains(&address) && said.contains("printobserver server --config"),
        "the refusal does not say where it looked and what to do: {said}"
    );
}

/// An answer this program cannot read is refused rather than printed.
///
/// A truncated answer would hand the supervising agent a document that parses
/// as less than the supervisor said, which is the one thing a read must not do
/// quietly.
#[test]
fn an_answer_this_program_cannot_read_is_refused_rather_than_printed() {
    for (described, answer) in [
        ("an answer with no head at all", "not http at all"),
        (
            "an answer declaring no length",
            "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\n\r\n{}",
        ),
        (
            "an answer declaring more than it sent",
            "HTTP/1.1 200 OK\r\nContent-Length: 400\r\n\r\n{}",
        ),
        (
            "an answer with no status",
            "GARBLED\r\nContent-Length: 2\r\n\r\n{}",
        ),
        (
            "an answer that is not a document",
            "HTTP/1.1 200 OK\r\nContent-Length: 5\r\n\r\nnope!",
        ),
    ] {
        let address = answering_with(answer);

        let (code, said) = running(
            &["context", "--print-id", A_PRINT],
            &[("PRINTOBSERVER_SERVER", &format!("http://{address}"))],
        );

        assert_ne!(code, Some(0), "{described} was taken: {said}");
        assert!(
            said.starts_with("printobserver:"),
            "{described} was refused with something other than this program's own \
             message: {said}"
        );
        assert!(
            !said.contains("panicked"),
            "{described} panicked rather than being refused: {said}"
        );
    }
}

/// A host answering one fixed thing to every request, for as long as this
/// process runs.
fn answering_with(answer: &'static str) -> std::net::SocketAddr {
    use std::io::{Read as _, Write as _};

    let listener = TcpListener::bind("127.0.0.1:0").expect("a loopback port");
    let address = listener.local_addr().expect("the bound address");
    std::thread::spawn(move || {
        for stream in listener.incoming().flatten() {
            let mut stream: TcpStream = stream;
            let mut buffer = [0_u8; 1024];
            let _ = stream.read(&mut buffer);
            let _ = stream.write_all(answer.as_bytes());
        }
    });
    address
}

/// A configuration that is not there refuses the start, naming the path.
#[test]
fn a_configuration_that_is_not_there_refuses_the_start() {
    let root = tempfile::TempDir::new().expect("a journey's own root");
    let missing = root.path().join("nowhere.toml");

    let (code, said) = run(&["server", "--config", &missing.display().to_string()]);

    assert_ne!(code, Some(0), "the server started with no configuration");
    assert!(
        said.contains("nowhere.toml"),
        "the refusal does not name the configuration: {said}"
    );
}
