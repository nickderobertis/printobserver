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
use printobserver_server::CredentialVerifier;
use tempfile::TempDir;

/// The operator credential this tier presents: the configuration below names
/// its verifier, as an operator who issued it configures one.
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

/// A loopback address nothing is listening on now: bound to learn a free port,
/// then released for the server to take.
fn free_address() -> String {
    let listener = TcpListener::bind("127.0.0.1:0").expect("a loopback port");
    listener
        .local_addr()
        .expect("the bound address")
        .to_string()
}

/// A configuration the server starts under, over a state directory of the
/// tier's own and a free port it is told to listen on — which is how this tier
/// knows where it serves, since the server writes that down nowhere.
fn configuration(root: &Path, octoprint: &str) -> std::path::PathBuf {
    let listen = free_address();
    let verifier = CredentialVerifier::of(CREDENTIAL).to_string();
    let state = root.join("state");
    let path = root.join("config.toml");
    // The committed skill, which is what `gh skill install` puts on a host.
    let skill = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../skills/printobserver/SKILL.md")
        .display()
        .to_string();
    let document = toml::toml! {
        state_dir = (state.display().to_string())
        listen = listen
        [octoprint]
        url = octoprint
        api_key = "a-provisioned-key"
        fan = "commandable"
        [supervisor]
        harness = "claude-code"
        skill_path = skill
        [ingress]
        shared_secret = "a-shared-secret"
        [api]
        credential_verifier = verifier
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

/// Where the server under one state directory was told to listen, read off
/// the configuration beside that directory, and the credential it admits.
fn where_it_serves(state: &Path) -> (String, String) {
    let configured: toml::Value = toml::from_str(
        &std::fs::read_to_string(
            state
                .parent()
                .expect("the state directory is in the tier's root")
                .join("config.toml"),
        )
        .expect("the configuration reads"),
    )
    .expect("the configuration is a document");
    let listen = configured["listen"]
        .as_str()
        .expect("the configuration names where to listen")
        .to_owned();
    (listen, CREDENTIAL.to_owned())
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

/// The address and credential, once the server is listening: with the running
/// report refused, nothing else says when it is serving.
fn once_serving(state: &Path) -> (String, String) {
    let (address, credential) = where_it_serves(state);
    let started = std::time::Instant::now();
    while !still_listening(&address) {
        assert!(
            started.elapsed() < SETTLES_WITHIN,
            "the service never listened at {address}"
        );
        std::thread::sleep(Duration::from_millis(50));
    }
    (address, credential)
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

/// Put the operator's credential into one configuration in plaintext, as
/// `api.credential`, in place of its verifier: what an installation from before
/// verifiers carries, and what every start under it warns about.
fn with_a_plaintext_credential(configuration: &Path) {
    let mut document: toml::Table =
        toml::from_str(&std::fs::read_to_string(configuration).expect("the configuration reads"))
            .expect("the configuration is a document");
    let mut api = toml::Table::new();
    api.insert(
        "credential".to_owned(),
        toml::Value::String(CREDENTIAL.to_owned()),
    );
    document.insert("api".to_owned(), toml::Value::Table(api));
    std::fs::write(configuration, document.to_string()).expect("the configuration is writable");
}

/// A service started under a plaintext credential says so before it reports
/// running, as the console form does, and then serves under that credential.
#[test]
fn a_service_under_a_plaintext_credential_warns_and_serves() {
    let root = TempDir::new().expect("the tier's own root");
    let octoprint = format!("http://{}", answering_host());
    let configuration = configuration(root.path(), &octoprint);
    with_a_plaintext_credential(&configuration);
    let mut child = start(&configuration);
    let mut reports = BufReader::new(child.stdout.take().expect("the fixture's reports")).lines();

    assert_eq!(
        next_report(&mut reports).map(|report| report.state),
        Some(ServiceState::StartPending.name().to_owned())
    );
    assert_eq!(
        next_report(&mut reports).map(|report| report.state),
        Some(ServiceState::Running.name().to_owned()),
        "the service did not report running: {}",
        said(&mut child)
    );
    let (address, credential) = where_it_serves(&root.path().join("state"));
    let answer = ask(&address, &credential);
    assert!(
        answer.contains("HTTP/1.1 200"),
        "the service did not admit the plaintext credential it was configured with:\n{answer}"
    );

    let mut stdin = child.stdin.take().expect("the fixture's stop control");
    writeln!(stdin, "stop").expect("the stop control is sent");
    drop(stdin);
    let status = exits_within(&mut child, SETTLES_WITHIN);
    let said = said(&mut child);
    assert_eq!(
        status.code(),
        Some(i32::from(Exit::Success.status())),
        "{said}"
    );
    assert!(
        said.contains("printobserver: warning:") && said.contains("`api.credential`"),
        "the start under a plaintext credential did not warn: {said}"
    );
}

/// What a start that could not read the printer's per-file tallies says, which
/// is that it skipped accounting the prints nobody watched — said before it
/// reports running, and the service then serves.
///
/// The machine here answers every request with an empty document, which the
/// printer's file list cannot be read from; the next start that can read it
/// accounts what this one skipped.
#[test]
fn a_service_that_cannot_read_the_printers_tallies_says_it_skipped_them_and_serves() {
    let root = TempDir::new().expect("the tier's own root");
    let octoprint = format!("http://{}", answering_host());
    let configuration = configuration(root.path(), &octoprint);
    let mut child = start(&configuration);
    let mut reports = BufReader::new(child.stdout.take().expect("the fixture's reports")).lines();

    assert_eq!(
        next_report(&mut reports).map(|report| report.state),
        Some(ServiceState::StartPending.name().to_owned())
    );
    assert_eq!(
        next_report(&mut reports).map(|report| report.state),
        Some(ServiceState::Running.name().to_owned()),
        "the service did not report running: {}",
        said(&mut child)
    );
    let (address, credential) = where_it_serves(&root.path().join("state"));
    assert!(ask(&address, &credential).contains("HTTP/1.1 200"));

    let mut stdin = child.stdin.take().expect("the fixture's stop control");
    writeln!(stdin, "stop").expect("the stop control is sent");
    drop(stdin);
    exits_within(&mut child, SETTLES_WITHIN);
    let said = said(&mut child);
    assert!(
        said.contains("printobserver: the printer could not be read")
            && said.contains("were not accounted on this start"),
        "the start that could not read the tallies did not say it skipped them: {said}"
    );
}

/// The console form of `printobserver server` on Windows — what runs whenever
/// the service control manager did not start it — stops the way a console
/// process is asked to: on `CTRL_BREAK`, it shuts the server down and exits
/// with status zero, which a process ended outright never does.
///
/// Windows has no signals, and a console control event reaches only the
/// processes attached to the console it is generated on. So the server is
/// started in a console and a process group of its own, and the event is
/// generated by a PowerShell that attaches to that console first — which works
/// whether or not this test was itself given a console. Its plaintext
/// `api.credential` makes the start warn, and the warning is asserted: what
/// the console form says before it serves is part of what it does.
#[cfg(windows)]
#[test]
fn the_console_server_stops_cleanly_on_a_console_break() {
    use std::os::windows::process::CommandExt as _;

    /// A console of the process's own, rather than this test's.
    const CREATE_NEW_CONSOLE: u32 = 0x0000_0010;
    /// A process group of its own, which a console control event can address.
    const CREATE_NEW_PROCESS_GROUP: u32 = 0x0000_0200;

    let root = TempDir::new().expect("the tier's own root");
    let octoprint = format!("http://{}", answering_host());
    let configuration = configuration(root.path(), &octoprint);
    with_a_plaintext_credential(&configuration);

    let mut server = Command::new(env!("CARGO_BIN_EXE_printobserver"))
        .arg("server")
        .arg("--config")
        .arg(&configuration)
        .creation_flags(CREATE_NEW_CONSOLE | CREATE_NEW_PROCESS_GROUP)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .expect("the server starts");
    let (address, credential) = once_serving(&root.path().join("state"));
    let answer = ask(&address, &credential);
    assert!(
        answer.contains("HTTP/1.1 200") && answer.contains("application/json"),
        "the console server did not answer the API:\n{answer}"
    );

    // Written to a file rather than passed as an argument, so that nothing in
    // it is reinterpreted by a command line's own quoting.
    let pid = server.id();
    let sender = root.path().join("send-ctrl-break.ps1");
    std::fs::write(
        &sender,
        format!(
            "$ErrorActionPreference = 'Stop'\r\n\
             $signature = @'\r\n\
             [DllImport(\"kernel32.dll\", SetLastError = true)]\r\n\
             public static extern bool FreeConsole();\r\n\
             [DllImport(\"kernel32.dll\", SetLastError = true)]\r\n\
             public static extern bool AttachConsole(uint dwProcessId);\r\n\
             [DllImport(\"kernel32.dll\", SetLastError = true)]\r\n\
             public static extern bool GenerateConsoleCtrlEvent(uint dwCtrlEvent, uint dwProcessGroupId);\r\n\
             '@\r\n\
             Add-Type -Namespace PrintObserverJourney -Name Console -MemberDefinition $signature\r\n\
             [void][PrintObserverJourney.Console]::FreeConsole()\r\n\
             if (-not [PrintObserverJourney.Console]::AttachConsole({pid})) {{ exit 2 }}\r\n\
             if (-not [PrintObserverJourney.Console]::GenerateConsoleCtrlEvent(1, {pid})) {{ exit 3 }}\r\n\
             exit 0\r\n"
        ),
    )
    .expect("the sender's script is writable");
    let sent = Command::new("powershell")
        .args([
            "-NoProfile",
            "-NonInteractive",
            "-ExecutionPolicy",
            "Bypass",
            "-File",
        ])
        .arg(&sender)
        .stdin(Stdio::null())
        .output()
        .expect("PowerShell runs");
    assert!(
        sent.status.success(),
        "CTRL_BREAK was not sent to the server's console ({}): {}{}",
        sent.status,
        String::from_utf8_lossy(&sent.stdout),
        String::from_utf8_lossy(&sent.stderr)
    );

    let status = exits_within(&mut server, SETTLES_WITHIN);
    let said = said(&mut server);
    assert_eq!(
        status.code(),
        Some(i32::from(Exit::Success.status())),
        "the server did not exit with status zero on CTRL_BREAK: {status}\n{said}"
    );
    assert!(
        !still_listening(&address),
        "{address} still answers after the server stopped"
    );
    assert!(
        said.contains("printobserver: warning:") && said.contains("`api.credential`"),
        "the start under a plaintext credential did not warn: {said}"
    );
}

/// The same program under the real service control manager.
///
/// Everything above stands the manager in, because only a Windows host has
/// one; on Windows the manager itself starts the program. Each case here
/// registers the crate's own `printobserver` as a service of its own name,
/// starts it through `sc.exe`, and reads back what the manager says — so the
/// connection to the manager, the call back into the service, the handler
/// registration and every report reaching the manager's handle are this
/// program's, driven the way the installed service is.
///
/// The manager starts the program with an environment of its own rather than
/// this test's, so the coverage profile this run is writing to is handed to it
/// through the service's own registry environment: without that, what the
/// service ran would be measured nowhere.
#[cfg(windows)]
mod under_the_manager {
    use std::path::Path;
    use std::process::Command;
    use std::time::{Duration, Instant};

    use printobserver::failure::Exit;
    use tempfile::TempDir;

    use super::{
        SETTLES_WITHIN, answering_host, ask, configuration, host_answering, where_it_serves,
    };

    /// What `sc.exe` answers a caller the manager will not let register a
    /// service: `ERROR_ACCESS_DENIED`.
    const ACCESS_DENIED: i32 = 5;

    /// The variable the gate's cells set so that a case whose prerequisite is
    /// missing fails rather than skips — here, a shell that may register one.
    const REQUIRED: &str = "PRINTOBSERVER_SERVICE_JOURNEY";

    /// What the manager records for a service that stopped with an exit of its
    /// own: `ERROR_SERVICE_SPECIFIC_ERROR`, with that exit beside it.
    const SERVICE_SPECIFIC: &str = "1066";

    /// One `sc.exe` invocation's exit and everything it said.
    fn sc(arguments: &[&str]) -> (Option<i32>, String) {
        let output = Command::new("sc.exe")
            .args(arguments)
            .output()
            .expect("`sc.exe` runs");
        let mut said = String::from_utf8_lossy(&output.stdout).into_owned();
        said.push_str(&String::from_utf8_lossy(&output.stderr));
        (output.status.code(), said)
    }

    /// The first word of `FIELD : value` in `sc.exe`'s own listing.
    fn field(listing: &str, name: &str) -> Option<String> {
        listing.lines().find_map(|line| {
            let (head, value) = line.split_once(':')?;
            (head.trim() == name).then(|| value.split_whitespace().next().unwrap_or("").to_owned())
        })
    }

    /// The service's state by the manager's name for it, as `sc.exe query` reads it.
    fn state(name: &str) -> (String, String) {
        let (_, listing) = sc(&["query", name]);
        let state = listing
            .lines()
            .find_map(|line| {
                let (head, value) = line.split_once(':')?;
                (head.trim() == "STATE")
                    .then(|| value.split_whitespace().nth(1).unwrap_or("").to_owned())
            })
            .unwrap_or_default();
        (state, listing)
    }

    /// Wait until the manager reports the service in `wanted`, and answer its listing.
    fn reaches(name: &str, wanted: &str) -> String {
        let started = Instant::now();
        loop {
            let (state, listing) = state(name);
            if state == wanted {
                return listing;
            }
            assert!(
                started.elapsed() < SETTLES_WITHIN,
                "the manager never reported `{name}` {wanted}:\n{listing}"
            );
            std::thread::sleep(Duration::from_millis(200));
        }
    }

    /// A service this case registered, removed again however the case ends.
    struct Registered {
        name: String,
    }

    impl Registered {
        /// Register the crate's own program as the service `name`, serving
        /// under `configuration`; `None` where this shell may not register
        /// one and nothing requires that it can.
        fn register(name: &str, configuration: &Path) -> Option<Self> {
            let program = env!("CARGO_BIN_EXE_printobserver");
            let command_line = format!(
                "\"{program}\" server --config \"{}\"",
                configuration.display()
            );
            let (code, said) = sc(&[
                "create",
                name,
                "binPath=",
                &command_line,
                "start=",
                "demand",
            ]);
            if code == Some(ACCESS_DENIED) && std::env::var(REQUIRED).as_deref() != Ok("required") {
                eprintln!("skipped: this shell may not register a service ({said})");
                return None;
            }
            assert_eq!(code, Some(0), "`sc.exe create {name}` failed:\n{said}");
            let registered = Self {
                name: name.to_owned(),
            };
            if let Ok(profile) = std::env::var("LLVM_PROFILE_FILE") {
                let key = format!(r"HKLM\SYSTEM\CurrentControlSet\Services\{name}");
                let entry = format!("LLVM_PROFILE_FILE={profile}");
                // `|` separates a multi-string's entries rather than reg.exe's
                // default `\0`, which a Windows path could carry.
                let output = Command::new("reg.exe")
                    .args([
                        "add",
                        &key,
                        "/v",
                        "Environment",
                        "/t",
                        "REG_MULTI_SZ",
                        "/s",
                        "|",
                    ])
                    .args(["/d", &entry, "/f"])
                    .output()
                    .expect("`reg.exe` runs");
                assert!(
                    output.status.success(),
                    "the service's environment was not written: {output:?}"
                );
            }
            Some(registered)
        }

        /// Ask the manager to start it.
        fn start(&self) -> (Option<i32>, String) {
            sc(&["start", &self.name])
        }
    }

    impl Drop for Registered {
        fn drop(&mut self) {
            let _ = sc(&["stop", &self.name]);
            let started = Instant::now();
            while state(&self.name).0 != "STOPPED" && started.elapsed() < SETTLES_WITHIN {
                std::thread::sleep(Duration::from_millis(200));
            }
            let _ = sc(&["delete", &self.name]);
        }
    }

    /// A service name of this case's own, so no two cases share one.
    fn own_name(case: &str) -> String {
        format!("printobserver-tier-{case}-{}", std::process::id())
    }

    /// Started by the manager, the program reports itself running and answers
    /// the API; stopped through the manager, it is recorded as a clean exit.
    #[test]
    fn the_manager_starts_it_it_answers_and_a_stop_is_a_clean_exit() {
        let root = TempDir::new().expect("the tier's own root");
        let octoprint = format!("http://{}", answering_host());
        let configuration = configuration(root.path(), &octoprint);
        let name = own_name("clean");
        let Some(service) = Registered::register(&name, &configuration) else {
            return;
        };

        let (code, said) = service.start();
        assert_eq!(code, Some(0), "`sc.exe start {name}` failed:\n{said}");
        reaches(&name, "RUNNING");

        let (address, credential) = where_it_serves(&root.path().join("state"));
        let answer = ask(&address, &credential);
        assert!(
            answer.contains("HTTP/1.1 200") && answer.contains("application/json"),
            "the manager said the service was running and its API did not answer:\n{answer}"
        );

        let (code, said) = sc(&["stop", &name]);
        assert_eq!(code, Some(0), "`sc.exe stop {name}` failed:\n{said}");
        let listing = reaches(&name, "STOPPED");
        assert_eq!(
            field(&listing, "WIN32_EXIT_CODE").as_deref(),
            Some("0"),
            "{listing}"
        );
        assert_eq!(
            field(&listing, "SERVICE_EXIT_CODE").as_deref(),
            Some("0"),
            "{listing}"
        );
    }

    /// A service whose server will not start is recorded by the manager as
    /// stopped with this program's own refusal, never as running.
    #[test]
    fn a_service_that_will_not_start_is_recorded_stopped_with_the_refusal() {
        let root = TempDir::new().expect("the tier's own root");
        let refusing = format!(
            "http://{}",
            host_answering(
                b"HTTP/1.1 403 Forbidden\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
            )
        );
        let configuration = configuration(root.path(), &refusing);
        let name = own_name("refused");
        let Some(service) = Registered::register(&name, &configuration) else {
            return;
        };

        // The start is taken whether or not the server then refuses, so what
        // `sc.exe start` answers says nothing here; the manager's record does.
        let _ = service.start();
        let listing = reaches(&name, "STOPPED");
        assert_eq!(
            field(&listing, "WIN32_EXIT_CODE").as_deref(),
            Some(SERVICE_SPECIFIC),
            "{listing}"
        );
        assert_eq!(
            field(&listing, "SERVICE_EXIT_CODE"),
            Some(Exit::Unconfigured.status().to_string()),
            "{listing}"
        );
    }
}
