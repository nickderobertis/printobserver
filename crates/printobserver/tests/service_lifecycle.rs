//! The sequence a service reports to its manager, driven over a real server.
//!
//! A Windows service reports each state it enters to the service control
//! manager — start pending, running, stop pending, stopped — and takes the
//! manager's stop control as the one graceful shutdown this program has. Only
//! a Windows runner has a manager to hand, but the sequence itself is
//! `printobserver::service`'s and is compiled on every platform, so this tier
//! drives it here: the `printobserver-service-fixture` program runs the real
//! server through that sequence with the manager stood in for by two streams,
//! and what is asserted is what the manager would read — each report in order,
//! the API answering while the service says it is running, and the process gone
//! with status zero after the stop control, which a process killed never is.
//!
//! Nothing here uses an HTTP client crate: the one question is written out over
//! a socket, because the point is what the running service answers.

use std::io::{BufRead as _, BufReader, Read as _, Write as _};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::path::Path;
use std::process::{Child, ChildStdout, Command, Stdio};
use std::time::Duration;

use printobserver::failure::Exit;
use printobserver::service::{START_WAIT_HINT, STOP_WAIT_HINT, ServiceState};
use printobserver_server::CLIENT_CONFIG_FILE;
use tempfile::TempDir;

/// The credential the configuration below puts in force.
const CREDENTIAL: &str = "a-credential-this-tier-configures-8c2f4e1d7a";

/// What every report line the fixture prints begins with.
const REPORTED: &str = "reported ";

/// How long the service is given to start serving, and to stop once told to.
const SETTLES_WITHIN: Duration = Duration::from_secs(60);

/// One report the fixture printed, as the manager would have received it.
#[derive(Debug, PartialEq, Eq)]
struct Reported {
    state: String,
    wait_hint_ms: u128,
    exit: u8,
}

/// A host that answers every request with an empty document, which is all the
/// server asks of its configured `OctoPrint` before it listens.
fn answering_host() -> SocketAddr {
    host_answering(
        b"HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: 2\r\n\
          Connection: close\r\n\r\n{}",
    )
}

/// A host that answers every request with the one response given, for as long
/// as the test runs: the port stays held, so no other test's host or server
/// running beside this one can be handed it.
fn host_answering(response: &'static [u8]) -> SocketAddr {
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
                let _ = stream.write_all(response);
            });
        }
    });
    address
}

/// A configuration the server starts under, over a state directory of the
/// tier's own and a port the operating system chooses.
fn configuration(root: &Path, octoprint: &str) -> std::path::PathBuf {
    let state = root.join("state");
    let path = root.join("config.toml");
    let document = toml::toml! {
        state_dir = (state.display().to_string())
        listen = "127.0.0.1:0"
        [octoprint]
        url = octoprint
        api_key = "a-provisioned-key"
        fan = "commandable"
        [supervisor]
        harness = "claude-code"
        [ingress]
        shared_secret = "a-shared-secret"
        [api]
        credential = CREDENTIAL
        [safety]
        agent_min_interval_s = 30
        [safety.allowed]
        feedrate = { min = 0.5, max = 1.5 }
        [safety.actions]
        operator = ["pause"]
        agent = []
        system = []
    };
    std::fs::write(&path, document.to_string()).expect("the configuration is writable");
    path
}

/// Start the fixture over one configuration, its three streams piped.
fn start(configuration: &Path) -> Child {
    start_refusing(configuration, None)
}

/// Start the fixture with its stand-in manager refusing the report of one state.
fn start_refusing(configuration: &Path, refused: Option<ServiceState>) -> Child {
    let mut command = Command::new(env!("CARGO_BIN_EXE_printobserver-service-fixture"));
    if let Some(state) = refused {
        command.env("PRINTOBSERVER_FIXTURE_REFUSES_REPORT", state.name());
    }
    command
        .arg("--config")
        .arg(configuration)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("the fixture starts")
}

/// The next report the fixture prints, or none if it printed no more.
fn next_report(lines: &mut std::io::Lines<BufReader<ChildStdout>>) -> Option<Reported> {
    let line = lines.next()?.expect("the fixture's output reads");
    let rest = line
        .strip_prefix(REPORTED)
        .unwrap_or_else(|| panic!("the fixture printed a line that is not a report: {line}"));
    let mut words = rest.split_whitespace();
    let state = words.next().expect("a report names a state").to_owned();
    let wait_hint_ms = words
        .next()
        .and_then(|word| word.strip_prefix("wait-hint-ms="))
        .expect("a report carries a wait hint")
        .parse()
        .expect("a wait hint is a number of milliseconds");
    let exit = words
        .next()
        .and_then(|word| word.strip_prefix("exit="))
        .expect("a report carries an exit")
        .parse()
        .expect("an exit is a status");
    Some(Reported {
        state,
        wait_hint_ms,
        exit,
    })
}

/// The address and credential the running server wrote for the clients beside it.
fn where_it_serves(state: &Path) -> (String, String) {
    let written: toml::Value = toml::from_str(
        &std::fs::read_to_string(state.join(CLIENT_CONFIG_FILE))
            .expect("the running server wrote the configuration its clients read"),
    )
    .expect("the configuration the server wrote is a document");
    let server = written["client"]["server"]
        .as_str()
        .expect("the client configuration names the server")
        .to_owned();
    let credential = written["client"]["credential"]
        .as_str()
        .expect("the client configuration carries the credential")
        .to_owned();
    (server.trim_start_matches("http://").to_owned(), credential)
}

/// One question to the API, written out over a socket, and the whole answer.
fn ask(address: &str, credential: &str) -> String {
    let mut stream = TcpStream::connect(address).expect("the running service accepts a connection");
    write!(
        stream,
        "GET /v1/prints HTTP/1.1\r\nHost: {address}\r\nAccept: application/json\r\n\
         Authorization: Bearer {credential}\r\nConnection: close\r\n\r\n"
    )
    .expect("the question is written");
    let mut answer = String::new();
    stream
        .read_to_string(&mut answer)
        .expect("the answer is read");
    answer
}

/// Whether anything still accepts a connection at the address.
fn still_listening(address: &str) -> bool {
    address
        .parse::<SocketAddr>()
        .is_ok_and(|parsed| TcpStream::connect_timeout(&parsed, Duration::from_secs(5)).is_ok())
}

/// Wait for the process to exit, for as long as a stop is given.
fn exits_within(child: &mut Child, within: Duration) -> std::process::ExitStatus {
    let started = std::time::Instant::now();
    loop {
        if let Some(status) = child.try_wait().expect("the fixture can be waited on") {
            return status;
        }
        assert!(
            started.elapsed() < within,
            "the service did not stop within {within:?} of the stop control"
        );
        std::thread::sleep(Duration::from_millis(50));
    }
}

/// The address and credential, once the server has written them whole: with
/// the running report refused, nothing else says when it is serving, and the
/// file is written in place rather than renamed into it.
fn once_serving(state: &Path) -> (String, String) {
    let started = std::time::Instant::now();
    loop {
        let written = std::fs::read_to_string(state.join(CLIENT_CONFIG_FILE))
            .ok()
            .and_then(|text| toml::from_str::<toml::Value>(&text).ok());
        let field = |name: &str| {
            written
                .as_ref()
                .and_then(|document| document.get("client")?.get(name)?.as_str())
                .map(str::to_owned)
        };
        if let (Some(server), Some(credential)) = (field("server"), field("credential")) {
            return (server.trim_start_matches("http://").to_owned(), credential);
        }
        assert!(
            started.elapsed() < SETTLES_WITHIN,
            "the service never wrote the configuration its clients read"
        );
        std::thread::sleep(Duration::from_millis(50));
    }
}

/// Everything the fixture said on standard error.
fn said(child: &mut Child) -> String {
    let mut said = String::new();
    if let Some(mut stderr) = child.stderr.take() {
        let _ = stderr.read_to_string(&mut said);
    }
    said
}

/// The service reports start pending, then running, answers the API, and on
/// the stop control reports stop pending and then stopped with status zero,
/// exiting the same way.
#[test]
fn the_service_reports_each_state_answers_while_running_and_stops_cleanly() {
    let root = TempDir::new().expect("the tier's own root");
    let octoprint = format!("http://{}", answering_host());
    let configuration = configuration(root.path(), &octoprint);
    let mut child = start(&configuration);
    let mut reports = BufReader::new(child.stdout.take().expect("the fixture's reports")).lines();

    assert_eq!(
        next_report(&mut reports),
        Some(Reported {
            state: ServiceState::StartPending.name().to_owned(),
            wait_hint_ms: START_WAIT_HINT.as_millis(),
            exit: 0,
        }),
        "the first report is not that the service is starting"
    );
    assert_eq!(
        next_report(&mut reports),
        Some(Reported {
            state: ServiceState::Running.name().to_owned(),
            wait_hint_ms: 0,
            exit: 0,
        }),
        "the second report is not that the service is running: {}",
        said(&mut child)
    );

    // Running is a claim about the API, so the API is asked.
    let (address, credential) = where_it_serves(&root.path().join("state"));
    let answer = ask(&address, &credential);
    assert!(
        answer.contains("HTTP/1.1 200") && answer.contains("application/json"),
        "the service said it was running and its API did not answer:\n{answer}"
    );

    let mut stdin = child.stdin.take().expect("the fixture's stop control");
    writeln!(stdin, "stop").expect("the stop control is sent");
    drop(stdin);

    assert_eq!(
        next_report(&mut reports),
        Some(Reported {
            state: ServiceState::StopPending.name().to_owned(),
            wait_hint_ms: STOP_WAIT_HINT.as_millis(),
            exit: 0,
        }),
        "the report after the stop control is not that the service is stopping"
    );
    assert_eq!(
        next_report(&mut reports),
        Some(Reported {
            state: ServiceState::Stopped.name().to_owned(),
            wait_hint_ms: 0,
            exit: Exit::Success.status(),
        }),
        "the last report is not that the service stopped cleanly"
    );
    assert_eq!(
        next_report(&mut reports),
        None,
        "the service reported after stopping"
    );

    let status = exits_within(&mut child, SETTLES_WITHIN);
    assert_eq!(
        status.code(),
        Some(i32::from(Exit::Success.status())),
        "the service did not exit with status zero once stopped: {status}"
    );
    assert!(
        !still_listening(&address),
        "{address} still answers after the service reported it stopped"
    );
}

/// A service whose server will not start reports start pending and then stopped
/// with the exit the console form gives the same refusal, and never running.
#[test]
fn a_service_that_will_not_start_reports_stopped_with_the_refusal_and_never_running() {
    let root = TempDir::new().expect("the tier's own root");
    // An `OctoPrint` that refuses the configured key: the server refuses to
    // start. A port freed for nothing to answer at would do the same, until a
    // test running beside this one was handed it and answered in its place.
    let refusing = format!(
        "http://{}",
        host_answering(b"HTTP/1.1 403 Forbidden\r\nContent-Length: 0\r\nConnection: close\r\n\r\n")
    );
    let configuration = configuration(root.path(), &refusing);
    let mut child = start(&configuration);
    let mut reports = BufReader::new(child.stdout.take().expect("the fixture's reports")).lines();

    assert_eq!(
        next_report(&mut reports).map(|report| report.state),
        Some(ServiceState::StartPending.name().to_owned()),
        "the first report is not that the service is starting"
    );
    assert_eq!(
        next_report(&mut reports),
        Some(Reported {
            state: ServiceState::Stopped.name().to_owned(),
            wait_hint_ms: 0,
            exit: Exit::Unconfigured.status(),
        }),
        "a service that cannot start did not report itself stopped with the refusal"
    );
    assert_eq!(
        next_report(&mut reports),
        None,
        "the service reported after stopping"
    );

    let status = exits_within(&mut child, SETTLES_WITHIN);
    assert_eq!(
        status.code(),
        Some(i32::from(Exit::Unconfigured.status())),
        "the process did not exit with the refusal it reported: {status}"
    );
    let said = said(&mut child);
    assert!(
        said.contains("will not start"),
        "the refusal was not said in the console form's own words:\n{said}"
    );
}

/// The sentence the service says when the manager refuses the report of `state`.
fn refusal_said(state: ServiceState) -> String {
    format!(
        "printobserver could not report that it is {name}: \
         the stand-in manager refused the {name} report",
        name = state.name()
    )
}

/// A manager that refuses any one report does not stop the sequence: the
/// refusal is said on standard error naming that state, every other report
/// still arrives in order, the service goes on serving, and the stop control
/// still takes it through the one graceful shutdown to a clean exit.
#[test]
fn a_report_the_manager_refuses_is_said_and_the_service_goes_on() {
    let sequence = ServiceState::ALL;
    for refused in sequence {
        let root = TempDir::new().expect("the tier's own root");
        let octoprint = format!("http://{}", answering_host());
        let configuration = configuration(root.path(), &octoprint);
        let mut child = start_refusing(&configuration, Some(refused));
        let mut reports =
            BufReader::new(child.stdout.take().expect("the fixture's reports")).lines();
        let expected = |states: &[ServiceState]| -> Vec<String> {
            states
                .iter()
                .filter(|state| **state != refused)
                .map(|state| state.name().to_owned())
                .collect()
        };

        let mut before_stop = Vec::new();
        for _ in expected(&sequence[..2]) {
            before_stop.push(next_report(&mut reports).expect("a report").state);
        }
        assert_eq!(
            before_stop,
            expected(&sequence[..2]),
            "with the {} report refused, the reports before the stop control were not the rest in order",
            refused.name()
        );

        let (address, credential) = once_serving(&root.path().join("state"));
        let answer = ask(&address, &credential);
        assert!(
            answer.contains("HTTP/1.1 200"),
            "with the {} report refused, the service did not serve:\n{answer}",
            refused.name()
        );

        let mut stdin = child.stdin.take().expect("the fixture's stop control");
        writeln!(stdin, "stop").expect("the stop control is sent");
        drop(stdin);

        let mut after_stop = Vec::new();
        while let Some(report) = next_report(&mut reports) {
            after_stop.push(report.state);
        }
        assert_eq!(
            after_stop,
            expected(&sequence[2..]),
            "with the {} report refused, the reports after the stop control were not the rest in order",
            refused.name()
        );
        let status = exits_within(&mut child, SETTLES_WITHIN);
        assert_eq!(
            status.code(),
            Some(i32::from(Exit::Success.status())),
            "with the {} report refused, the service did not exit cleanly: {status}",
            refused.name()
        );
        let said = said(&mut child);
        assert!(
            said.contains(&refusal_said(refused)),
            "the refused {} report was not said on standard error:\n{said}",
            refused.name()
        );
    }
}

/// A service that will not start, whose manager refuses the report that it
/// stopped, still says why it would not start and exits with that refusal.
#[test]
fn a_refused_stopped_report_after_a_failed_start_still_exits_with_the_refusal() {
    let root = TempDir::new().expect("the tier's own root");
    let refusing = format!(
        "http://{}",
        host_answering(b"HTTP/1.1 403 Forbidden\r\nContent-Length: 0\r\nConnection: close\r\n\r\n")
    );
    let configuration = configuration(root.path(), &refusing);
    let mut child = start_refusing(&configuration, Some(ServiceState::Stopped));
    let mut reports = BufReader::new(child.stdout.take().expect("the fixture's reports")).lines();

    assert_eq!(
        next_report(&mut reports).map(|report| report.state),
        Some(ServiceState::StartPending.name().to_owned()),
        "the first report is not that the service is starting"
    );
    assert_eq!(
        next_report(&mut reports),
        None,
        "a report arrived that the manager refused"
    );
    let status = exits_within(&mut child, SETTLES_WITHIN);
    assert_eq!(
        status.code(),
        Some(i32::from(Exit::Unconfigured.status())),
        "the process did not exit with the refusal it could not report: {status}"
    );
    let said = said(&mut child);
    assert!(
        said.contains("will not start") && said.contains(&refusal_said(ServiceState::Stopped)),
        "the failed start and the refused report were not both said:\n{said}"
    );
}

/// A refusal naming no state is refused, rather than read as refusing nothing
/// and letting a tier pass that asked for a refusal it never got.
#[test]
fn the_fixture_refuses_a_refusal_naming_no_state() {
    let root = TempDir::new().expect("the tier's own root");
    let configuration = configuration(root.path(), "http://127.0.0.1:9");
    let output = Command::new(env!("CARGO_BIN_EXE_printobserver-service-fixture"))
        .env("PRINTOBSERVER_FIXTURE_REFUSES_REPORT", "runnning")
        .arg("--config")
        .arg(&configuration)
        .stdin(Stdio::null())
        .output()
        .expect("the fixture runs");

    assert_eq!(
        output.status.code(),
        Some(2),
        "the fixture ran over a refusal naming no state"
    );
    assert!(
        output.stdout.is_empty(),
        "the fixture reported over a refusal naming no state"
    );
    let said = String::from_utf8_lossy(&output.stderr);
    assert!(
        said.contains("`runnning`, which names no state a service reports"),
        "the fixture did not say which value named no state:\n{said}"
    );
}
