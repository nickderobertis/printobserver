//! Every endpoint one invocation connected to, read from its own process tree.
//!
//! # Why the observation is made here rather than at a listener
//!
//! The supervisor legitimately holds connections to the machine and to the
//! failure detector, so a connection seen at one of those far ends says nothing
//! about which process opened it. What this program is held to is a property of
//! **its own** process and every descendant of it, so the observation is made
//! by tracing the invocation: every `connect` any process in that tree makes,
//! whatever crate, socket or copied protocol code made it.
//!
//! That is what a manifest check cannot carry. A crate naming neither adapter
//! can still open a connection through a generic transport, and the manifest
//! reads identically either way.
//!
//! # What each platform's observation sees, and what it cannot
//!
//! The observation is chosen by target, and [`traced`] answers the same [`Ran`]
//! on each.
//!
//! **Linux** runs the invocation under `strace -f`, recording every `connect`
//! system call any process of the tree makes — TCP or UDP, to any address,
//! whether or not anything listens there and however briefly — and collects
//! the internet endpoints among them. It cannot see a connection a process of
//! the tree hands to another process to make, which nothing here does.
//!
//! **macOS** has no `strace`, and its own tracer needs System Integrity
//! Protection off, which no hosted runner has. There the tree is observed by
//! the [`interposer`] loaded into it instead: a library every process of the
//! tree inherits, recording each `connect` at the moment it is made, to any
//! address, whether or not anything listens there. It cannot see a process the
//! loader refuses the library to — a binary SIP protects, or one signed with
//! the hardened runtime — and the program under test starts none: the one place
//! it opens a connection is its own transport. A run whose own process did not
//! record the load is refused rather than read as one that did nothing.
//!
//! **Windows** shares one native kernel ETW controller across each test process.
//! Before returning, it flushes and switches the log file, then synchronously
//! decodes the closed chunk with `ProcessTrace`. Process start/end lifetimes
//! determine ancestry; TCP request events include unsuccessful attempts. Lost
//! buffers, missing lifetimes and incomplete decoding are refused. A host-wide
//! named mutex is independent of TEMP routing, and the controller recovers its
//! reserved logger after a crashed worker. The test-only C# bridge is compiled
//! once by PowerShell 7, keeping Rust's unsafe-code prohibition intact. It
//! cannot see UDP `connect`; this program speaks HTTP over TCP and resolves no
//! names. Starting the kernel session requires an administrator.
//!
//! Only the launcher differs between Linux and macOS: [`connections_in`] reads
//! either recording into the same set of endpoints, and every assertion over a
//! [`Ran`] is shared by all three.

#[cfg(not(windows))]
#[path = "interposer.rs"]
pub mod interposer;

#[path = "spike.rs"]
mod spike;

use std::collections::BTreeSet;
use std::net::SocketAddr;
use std::path::Path;
#[cfg(not(windows))]
use std::process::{Command, Output};
#[cfg(not(any(target_os = "macos", windows)))]
use std::sync::atomic::{AtomicU64, Ordering};

/// What the spike's timing sink calls one observed invocation here.
#[cfg(not(any(target_os = "macos", windows)))]
const OBSERVED_KIND: &str = "traced:strace-run";
#[cfg(target_os = "macos")]
const OBSERVED_KIND: &str = "traced:interposed-run";

/// The tracer this tier reads a process tree's connections out of.
#[cfg(not(any(target_os = "macos", windows)))]
const TRACER: &str = "strace";

/// How one traced invocation's own output file is named apart from every other.
#[cfg(not(any(target_os = "macos", windows)))]
static TRACED: AtomicU64 = AtomicU64::new(0);

#[cfg(windows)]
#[path = "windows_trace.rs"]
mod windows_trace;

/// One invocation, and everything it did.
#[derive(Debug, Clone)]
pub struct Ran {
    /// What the caller's shell saw.
    pub code: Option<i32>,
    /// What it wrote to standard output.
    pub out: String,
    /// What it wrote to standard error.
    pub err: String,
    /// Every endpoint any process of its tree connected to.
    pub connected: BTreeSet<SocketAddr>,
    /// The arguments it was given, for a failure message that says which run.
    pub arguments: Vec<String>,
}

impl Ran {
    /// Everything it said, on either stream.
    pub fn said(&self) -> String {
        format!("{}{}", self.out, self.err)
    }
}

/// The variables every platform reads a user's configuration home from.
pub const CONFIG_HOME_VARIABLES: [&str; 3] = ["XDG_CONFIG_HOME", "HOME", "APPDATA"];

/// The environment one invocation runs under, with its configuration home a
/// directory of its own holding nothing — unless the journey named one.
///
/// With no file named, the program reads its operator's own client
/// configuration from that home first, so a run that inherited this host's
/// would be configured by whatever this host's user last issued.
#[must_use]
pub fn with_its_own_config_home(
    environment: &[(String, String)],
    scratch: &Path,
) -> Vec<(String, String)> {
    let mut given = environment.to_vec();
    if !given
        .iter()
        .any(|(name, _)| CONFIG_HOME_VARIABLES.contains(&name.as_str()))
    {
        let home = scratch.join("no-operator-configuration");
        std::fs::create_dir_all(&home).expect("an empty configuration home");
        for name in CONFIG_HOME_VARIABLES {
            given.push((name.to_owned(), home.display().to_string()));
        }
    }
    given
}

/// Run one invocation under this platform's observation, and answer what it did.
///
/// # Panics
///
/// Panics when the observation cannot be made on this host — `strace` is not
/// there, or the interposer cannot be built or loaded — which is a toolchain
/// that cannot make the observation this tier's whole claim rests on, rather
/// than a walk that quietly stops making it.
#[cfg(not(windows))]
pub fn traced(
    program: &Path,
    arguments: &[String],
    environment: &[(String, String)],
    scratch: &Path,
) -> Ran {
    let environment = &with_its_own_config_home(environment, scratch);
    let (output, recorded) = spike::timed(OBSERVED_KIND, || {
        observed(program, arguments, environment, scratch)
    });
    Ran {
        code: output.status.code(),
        out: String::from_utf8_lossy(&output.stdout).into_owned(),
        err: String::from_utf8_lossy(&output.stderr).into_owned(),
        connected: connections_in(&recorded),
        arguments: arguments.to_vec(),
    }
}

/// One invocation under `strace -f`, and the trace it wrote.
#[cfg(not(any(target_os = "macos", windows)))]
fn observed(
    program: &Path,
    arguments: &[String],
    environment: &[(String, String)],
    scratch: &Path,
) -> (Output, String) {
    let at = scratch.join(format!(
        "connections-{}-{}.log",
        std::process::id(),
        TRACED.fetch_add(1, Ordering::Relaxed)
    ));
    let mut process = Command::new(TRACER);
    process
        .arg("-f")
        .arg("-qq")
        .arg("-e")
        .arg("trace=connect")
        .arg("-o")
        .arg(&at)
        .arg("--")
        .arg(program)
        .args(arguments)
        .env_remove("PRINTOBSERVER_SERVER")
        .env_remove("PRINTOBSERVER_CREDENTIAL");
    for (name, value) in environment {
        process.env(name, value);
    }
    let output = process.output().unwrap_or_else(|error| {
        panic!(
            "`{TRACER}` could not run, and this tier's whole claim about which endpoints an \
             invocation reaches rests on it: {error}"
        )
    });
    let traced = std::fs::read_to_string(&at)
        .unwrap_or_else(|error| panic!("`{TRACER}` wrote nothing to {}: {error}", at.display()));
    (output, traced)
}

/// One invocation with the interposer loaded, and what it recorded.
#[cfg(target_os = "macos")]
fn observed(
    program: &Path,
    arguments: &[String],
    environment: &[(String, String)],
    scratch: &Path,
) -> (Output, String) {
    interposed(program, arguments, environment, scratch)
}

/// One invocation with the interposer loaded into its tree, on any platform,
/// and what the interposer recorded.
///
/// # Panics
///
/// Panics when the interposer cannot be built or was not loaded.
#[cfg(not(windows))]
pub fn interposed(
    program: &Path,
    arguments: &[String],
    environment: &[(String, String)],
    scratch: &Path,
) -> (Output, String) {
    let mut process = Command::new(program);
    process
        .args(arguments)
        .env_remove("PRINTOBSERVER_SERVER")
        .env_remove("PRINTOBSERVER_CREDENTIAL");
    for (name, value) in environment {
        process.env(name, value);
    }
    let run = interposer::interposed(process, scratch, &[]);
    (run.output, run.activity)
}

/// Every internet endpoint a recording names a connection to.
///
/// Reads either recording: a line `strace` wrote, and a `connected` line the
/// interposer wrote. Connections to anything that is not an internet endpoint —
/// a local socket to the name-service cache, say — are not endpoints this rule
/// is about and are not collected.
#[cfg(not(windows))]
pub fn connections_in(traced: &str) -> BTreeSet<SocketAddr> {
    let mut found = BTreeSet::new();
    for line in traced.lines() {
        if let Some(address) = interposed_endpoint_in(line)
            .or_else(|| endpoint_in(line, "sin_port=htons(", "inet_addr(\""))
            .or_else(|| endpoint_in(line, "sin6_port=htons(", "inet_pton(AF_INET6, \""))
        {
            found.insert(address);
        }
    }
    found
}

/// The endpoint one line the interposer wrote names, when it names one.
#[cfg(not(windows))]
fn interposed_endpoint_in(line: &str) -> Option<SocketAddr> {
    line.strip_prefix("connected ")?.trim().parse().ok()
}

/// The endpoint one traced line names, when it names one.
#[cfg(not(windows))]
fn endpoint_in(line: &str, port_marker: &str, address_marker: &str) -> Option<SocketAddr> {
    let port: u16 = after(line, port_marker, ')')?.parse().ok()?;
    let address = after(line, address_marker, '"')?;
    format!("{address}:{port}")
        .parse()
        .ok()
        .or_else(|| format!("[{address}]:{port}").parse().ok())
}

/// What one line carries between a marker and the next of one character.
#[cfg(not(windows))]
fn after(line: &str, marker: &str, until: char) -> Option<String> {
    let rest = line.split_once(marker)?.1;
    Some(rest.split(until).next()?.to_owned())
}

/// Run one invocation under an event-tracing session, and answer what it did.
///
/// # Panics
///
/// Panics when a session cannot be started, stopped or decoded — a host that
/// cannot make the observation this tier's whole claim rests on, which is
/// refused naming why rather than walked without it.
#[cfg(windows)]
pub fn traced(
    program: &Path,
    arguments: &[String],
    environment: &[(String, String)],
    scratch: &Path,
) -> Ran {
    let probe = tempfile::Builder::new()
        .prefix("trace-probe-")
        .tempfile_in(scratch)
        .unwrap_or_else(|error| panic!("{} could not be written: {error}", scratch.display()));
    probe.close().expect("trace scratch probe removal");
    let environment = with_its_own_config_home(environment, scratch);
    let (output, connected) = spike::timed("traced:etw-native-observation", || {
        windows_trace::run(program, arguments, &environment)
    });
    Ran {
        code: output.status.code(),
        out: String::from_utf8_lossy(&output.stdout).into_owned(),
        err: String::from_utf8_lossy(&output.stderr).into_owned(),
        connected,
        arguments: arguments.to_vec(),
    }
}

/// A process that cannot start is refused after its native trace session is
/// stopped, so one bad invocation cannot leak a session into later journeys.
#[cfg(windows)]
#[test]
fn an_invocation_that_cannot_start_is_refused() {
    let scratch = std::env::temp_dir().join(format!(
        "printobserver-traced-missing-{}",
        std::process::id()
    ));
    std::fs::create_dir_all(&scratch).expect("trace scratch could not be created");
    let missing = scratch.join("no-such-printobserver.exe");

    let panic = std::panic::catch_unwind(|| traced(&missing, &[], &[], &scratch))
        .expect_err("an invocation that does not exist unexpectedly ran");
    let message = panic
        .downcast_ref::<String>()
        .map(String::as_str)
        .or_else(|| panic.downcast_ref::<&str>().copied())
        .unwrap_or("panic carried no text");
    assert!(message.contains("could not be run"), "{message}");

    std::fs::remove_dir_all(&scratch).expect("trace scratch could not be removed");
}

/// A scratch path that cannot hold the native trace artifacts is refused
/// before a partial session can be started.
#[cfg(windows)]
#[test]
fn an_unwritable_trace_scratch_is_refused() {
    let scratch =
        std::env::temp_dir().join(format!("printobserver-traced-file-{}", std::process::id()));
    std::fs::write(&scratch, b"not a directory").expect("trace scratch file could not be made");

    let panic = std::panic::catch_unwind(|| traced(Path::new("unused"), &[], &[], &scratch))
        .expect_err("an unwritable trace scratch was unexpectedly accepted");
    let message = panic
        .downcast_ref::<String>()
        .map(String::as_str)
        .or_else(|| panic.downcast_ref::<&str>().copied())
        .unwrap_or("panic carried no text");
    assert!(message.contains("could not be written"), "{message}");

    std::fs::remove_file(&scratch).expect("trace scratch file could not be removed");
}

/// The observation itself, proven on whichever platform runs the tier.
#[cfg(all(test, not(windows)))]
mod observing {
    use std::collections::BTreeSet;
    use std::net::{SocketAddr, TcpListener};
    use std::path::{Path, PathBuf};
    use std::process::Command;

    use printobserver::failure::Exit;
    use tempfile::TempDir;

    use super::interposer::{activity, interposed};
    use super::{connections_in, traced};

    /// A print identifier the program accepts, naming no print anywhere.
    const SOME_PRINT: &str = "01900000-0000-7000-8000-000000000000";

    /// A listener that takes every connection and closes it at once.
    fn closing_listener() -> SocketAddr {
        let listener = TcpListener::bind("127.0.0.1:0").expect("a loopback port");
        let address = listener.local_addr().expect("the listener's own address");
        std::thread::spawn(move || {
            for stream in listener.incoming() {
                drop(stream);
            }
        });
        address
    }

    /// The program under test.
    fn program() -> PathBuf {
        PathBuf::from(env!("CARGO_BIN_EXE_printobserver"))
    }

    /// One command run with the interposer watching some paths.
    struct Watching {
        /// What it printed, on either stream.
        said: String,
        /// What it exited with.
        code: Option<i32>,
        /// Every watched access the interposer refused.
        touched: String,
        /// Every endpoint the interposer recorded a connection to.
        connected: BTreeSet<SocketAddr>,
    }

    /// Run one command with the interposer watching the paths given.
    fn watching(scratch: &Path, arguments: &[&str], watched: &[PathBuf]) -> Watching {
        let mut command = Command::new(program());
        command
            .args(arguments)
            .env_remove("PRINTOBSERVER_SERVER")
            .env_remove("PRINTOBSERVER_CREDENTIAL");
        let run = interposed(command, scratch, watched);
        let said = format!(
            "{}{}",
            String::from_utf8_lossy(&run.output.stdout),
            String::from_utf8_lossy(&run.output.stderr)
        );
        Watching {
            said,
            code: run.output.status.code(),
            touched: run.touched(),
            connected: connections_in(&run.activity),
        }
    }

    /// A trace `strace -f -qq -e trace=connect` wrote and a recording the
    /// interposer wrote, of the same connections, read as the same endpoints —
    /// the local socket in neither.
    #[test]
    fn either_recording_reads_as_the_same_endpoints() {
        let strace = "\
4039275 connect(3, {sa_family=AF_INET, sin_port=htons(9), sin_addr=inet_addr(\"127.0.0.1\")}, 16) = -1 ECONNREFUSED (Connection refused)
4039275 connect(4, {sa_family=AF_INET6, sin6_port=htons(9), sin6_flowinfo=htonl(0), inet_pton(AF_INET6, \"::1\", &sin6_addr), sin6_scope_id=0}, 28) = -1 ECONNREFUSED (Connection refused)
4039275 connect(3, {sa_family=AF_UNIX, sun_path=\"/nonexistent\"}, 15) = -1 ENOENT (No such file or directory)
";
        let recorded = "loaded 4039275\nconnected 127.0.0.1:9\nconnected [::1]:9\n";
        let interposer = activity(recorded, 4_039_275)
            .expect("a recording of the process it was loaded into reads");
        let expected: BTreeSet<SocketAddr> = ["127.0.0.1:9", "[::1]:9"]
            .iter()
            .map(|address| address.parse().expect("an endpoint"))
            .collect();
        assert_eq!(connections_in(strace), expected);
        assert_eq!(connections_in(&interposer), expected);
    }

    /// A recording says what the tree did only once the interposer said it was
    /// loaded into the tree's own process, and its loading lines are not
    /// activity.
    #[test]
    fn a_recording_is_read_only_for_the_process_it_was_loaded_into() {
        assert_eq!(
            activity("loaded 7\nconnected 127.0.0.1:9\nloaded 8\n", 7).as_deref(),
            Some("connected 127.0.0.1:9\n")
        );
        assert_eq!(activity("loaded 7\n", 7).as_deref(), Some(""));
        assert_eq!(activity("loaded 8\nconnected 127.0.0.1:9\n", 7), None);
        assert_eq!(activity("", 7), None);
    }

    /// A run the interposer is loaded into records the one endpoint it
    /// connects to, and this platform's own launcher reads the same one.
    #[test]
    fn the_interposer_records_the_endpoint_a_run_connects_to() {
        let scratch = TempDir::new().expect("a scratch directory");
        let address = closing_listener();
        let arguments = [
            "context".to_owned(),
            "--print-id".to_owned(),
            SOME_PRINT.to_owned(),
        ];
        let environment = [
            (
                "PRINTOBSERVER_SERVER".to_owned(),
                format!("http://{address}"),
            ),
            (
                "PRINTOBSERVER_CREDENTIAL".to_owned(),
                "a-credential".to_owned(),
            ),
        ];

        let (_, recorded) = super::interposed(&program(), &arguments, &environment, scratch.path());
        assert_eq!(
            connections_in(&recorded),
            BTreeSet::from([address]),
            "the interposer did not record the one connection the run made:\n{recorded}"
        );
        assert_eq!(
            traced(&program(), &arguments, &environment, scratch.path()).connected,
            BTreeSet::from([address]),
            "this platform's own launcher does not read the connection the interposer read"
        );
    }

    /// A watched path is refused as a file this user may not read, and the
    /// refusal is recorded naming it; a path nobody watches is read, and only
    /// the connection it configures is recorded.
    #[test]
    fn the_interposer_refuses_and_records_a_watched_path() {
        let scratch = TempDir::new().expect("a scratch directory");
        let address = closing_listener();
        let watched = scratch.path().join("watched");
        std::fs::create_dir_all(&watched).expect("a watched directory");
        let file = watched.join("client.toml");
        std::fs::write(
            &file,
            format!("[client]\nserver = \"http://{address}\"\ncredential = \"a-credential\"\n"),
        )
        .expect("a client configuration");
        let named = file.display().to_string();
        let arguments = [
            "context",
            "--print-id",
            SOME_PRINT,
            "--config",
            named.as_str(),
        ];

        let refused = watching(scratch.path(), &arguments, &[watched]);
        assert!(
            refused.touched.contains(&named) && refused.touched.contains("EACCES"),
            "the refused access was not recorded naming the path:\n{}",
            refused.touched
        );
        assert!(
            refused.connected.is_empty(),
            "a run refused its configuration still connected to {:?}",
            refused.connected
        );
        assert_eq!(
            refused.code,
            Some(i32::from(Exit::Unconfigured.status())),
            "a configuration refused as unreadable was not answered as unconfigured: {}",
            refused.said
        );
        assert!(
            refused.said.contains("Permission denied"),
            "the program was not refused its configuration the way the kernel refuses it: {}",
            refused.said
        );

        let elsewhere = scratch.path().join("elsewhere");
        let read = watching(scratch.path(), &arguments, &[elsewhere]);
        assert!(
            read.touched.trim().is_empty() && !read.said.contains("Permission denied"),
            "a run touching no watched path recorded a refused access:\n{}{}",
            read.touched,
            read.said
        );
        assert_eq!(
            read.connected,
            BTreeSet::from([address]),
            "a run reading a path nobody watches did not connect where that path configured"
        );
    }
}

/// Real socket traffic, including a child that outlives its parent, observed
/// through the same interface both command walks use.
#[cfg(test)]
mod regression {
    use std::collections::BTreeSet;
    use std::net::{SocketAddr, TcpListener, TcpStream};
    use std::process::Command;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicBool, Ordering};

    use super::traced;

    /// A subprocess fixture that connects itself or starts a connecting child.
    /// An ordinary test invocation has no fixture environment and does nothing.
    // llmlint: ignore[tests_assert_real_behavior] This libtest entry is the real subprocess socket fixture, selected by its parent through --exact. The owning regression asserts its exit status and every endpoint the actual OS tracer attributes to it and its descendants; an unselected fixture makes no test claim.
    #[test]
    fn connector() {
        if std::env::var("PRINTOBSERVER_SPIKE_WRITER").as_deref() == Ok("yes") {
            use std::io::Read as _;
            println!("writer ready");
            let mut release = [0];
            std::io::stdin()
                .read_exact(&mut release)
                .expect("writer barrier");
            assert_eq!(release, [b'Y']);
            for _ in 0..1024 {
                super::spike::note("traced:shared-sink", std::time::Instant::now());
            }
            return;
        }
        let Ok(address) = std::env::var("PRINTOBSERVER_TRACE_CONNECT") else {
            return;
        };
        if std::env::var("PRINTOBSERVER_TRACE_SCOPE").as_deref() == Ok("yes") {
            scoped_observer(&address);
            return;
        }
        if std::env::var("PRINTOBSERVER_TRACE_RECOVER").as_deref() == Ok("yes") {
            let scratch = tempfile::TempDir::new().expect("recovery scratch");
            let ran = traced(
                &std::env::current_exe().expect("test executable"),
                &[
                    "--exact".to_owned(),
                    "traced::regression::connector".to_owned(),
                    "--nocapture".to_owned(),
                ],
                &[
                    ("PRINTOBSERVER_TRACE_CONNECT".to_owned(), address.clone()),
                    ("PRINTOBSERVER_TRACE_RECOVER".to_owned(), "no".to_owned()),
                ],
                scratch.path(),
            );
            assert_eq!(ran.code, Some(0), "{}", ran.said());
            assert_eq!(
                ran.connected,
                address
                    .split(',')
                    .map(|address| address.parse().expect("endpoint"))
                    .collect()
            );
            return;
        }
        if std::env::var("PRINTOBSERVER_TRACE_LOUD").as_deref() == Ok("yes") {
            print!("{}", "observed stdout\n".repeat(8192));
            eprint!("{}", "observed stderr\n".repeat(8192));
        }
        if std::env::var_os("PRINTOBSERVER_TRACE_CHILD").is_some() {
            let inherited = std::env::var("PRINTOBSERVER_TRACE_INHERIT").as_deref() == Ok("yes");
            let child = Command::new(std::env::current_exe().expect("test executable"))
                .args(["--exact", "traced::regression::connector", "--nocapture"])
                .env_remove("PRINTOBSERVER_TRACE_CHILD")
                .env(
                    "PRINTOBSERVER_TRACE_DELAY",
                    std::env::var("PRINTOBSERVER_TRACE_DELAY").unwrap_or_else(|_| "100".to_owned()),
                )
                .stdout(if inherited {
                    std::process::Stdio::inherit()
                } else {
                    std::process::Stdio::null()
                })
                .stderr(if inherited {
                    std::process::Stdio::inherit()
                } else {
                    std::process::Stdio::null()
                })
                .spawn()
                .expect("connecting descendant");
            if let Some(path) = std::env::var_os("PRINTOBSERVER_TRACE_CHILD_PID") {
                std::fs::write(path, child.id().to_string()).expect("our child's captured PID");
            }
            // The root exits without waiting; the descendant's sockets and
            // optionally inherited pipes remain part of this invocation.
            drop(child);
            return;
        }
        if let Ok(delay) = std::env::var("PRINTOBSERVER_TRACE_DELAY") {
            std::thread::sleep(std::time::Duration::from_millis(
                delay.parse().expect("fixture delay"),
            ));
        }
        for address in address.split(',') {
            let address: SocketAddr = address.parse().expect("fixture endpoint");
            let _ = TcpStream::connect_timeout(&address, std::time::Duration::from_millis(200));
        }
    }

    /// Contains every character the timing sink must escape; it is also the
    /// actual thread on which the socket walk and native observation run.
    pub const OBSERVING_THREAD: &str = "trace \"\\\n\t\u{1}";

    /// Accepted and refused attempts belong to this invocation's lifetime and
    /// ancestry; adjacent invocations and simultaneous host traffic do not.
    #[test]
    fn each_invocation_observes_its_descendants_and_refused_attempts_only() {
        std::thread::Builder::new()
            .name(OBSERVING_THREAD.to_owned())
            .spawn(exercise)
            .expect("observing thread")
            .join()
            .unwrap_or_else(|error| std::panic::resume_unwind(error));
    }

    /// The real observer process holds its session while a scoped worker traces.
    fn scoped_observer(address: &str) {
        use std::io::{Read as _, Write as _};
        let scratch = tempfile::TempDir::new().expect("scoped observer scratch");
        let program = std::env::current_exe().expect("test executable");
        let arguments = [
            "--exact".to_owned(),
            "traced::regression::connector".to_owned(),
            "--nocapture".to_owned(),
        ];
        let environment = [
            ("PRINTOBSERVER_TRACE_CONNECT".to_owned(), address.to_owned()),
            ("PRINTOBSERVER_TRACE_SCOPE".to_owned(), "no".to_owned()),
        ];
        let expected: BTreeSet<SocketAddr> = address
            .split(',')
            .map(|address| address.parse().expect("scoped endpoint"))
            .collect();
        let first = traced(&program, &arguments, &environment, scratch.path());
        assert_eq!(first.code, Some(0), "{}", first.said());
        assert_eq!(first.connected, expected);
        #[cfg(windows)]
        let identity = super::windows_trace::controller_id();
        println!("process-session held");
        std::io::stdout().flush().expect("session readiness");
        let mut release = [0];
        std::io::stdin()
            .read_exact(&mut release)
            .expect("scoped invocation barrier");
        assert_eq!(release, [b'Y']);
        std::thread::scope(|scope| {
            let worker = scope.spawn(|| {
                let ran = traced(&program, &arguments, &environment, scratch.path());
                #[cfg(windows)]
                assert_eq!(super::windows_trace::controller_id(), identity);
                ran
            });
            let second = worker.join().expect("scoped observer thread");
            assert_eq!(second.code, Some(0), "{}", second.said());
            assert_eq!(second.connected, expected);
        });
    }

    /// Observe real accepted/refused sockets while another observer stays alive.
    fn another_live_process_can_trace(
        child: &mut std::process::Child,
        accepted: SocketAddr,
        refused: SocketAddr,
    ) {
        use std::process::Stdio;
        use std::time::{Duration, Instant};
        // The first process remains alive at its explicit barrier while a second
        // real process traces accepted and refused sockets. Its complete Ran
        // assertions must finish without waiting for the first process to exit.
        let mut competitor = Command::new(std::env::current_exe().expect("test binary"))
            .args(["--exact", "traced::regression::connector", "--nocapture"])
            .env(
                "PRINTOBSERVER_TRACE_CONNECT",
                format!("{accepted},{refused}"),
            )
            .env("PRINTOBSERVER_TRACE_RECOVER", "yes")
            .env_remove("PRINTOBSERVER_SPIKE_TIMINGS")
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .expect("real concurrent observer process");
        let fairness_deadline = Instant::now() + Duration::from_secs(20);
        loop {
            if competitor
                .try_wait()
                .expect("concurrent observer status")
                .is_some()
            {
                break;
            }
            if Instant::now() >= fairness_deadline {
                competitor
                    .kill()
                    .expect("stop our captured competing observer");
                competitor
                    .wait()
                    .expect("reap our captured competing observer");
                child.kill().expect("stop our captured barrier observer");
                child.wait().expect("reap our captured barrier observer");
                panic!("a live idle observer blocked another real process for 20 seconds");
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        let competing = competitor
            .wait_with_output()
            .expect("concurrent observer output");
        if !competing.status.success() {
            child.kill().expect("stop our captured barrier observer");
            child.wait().expect("reap our captured barrier observer");
        }
        assert!(
            competing.status.success(),
            "{}{}",
            String::from_utf8_lossy(&competing.stdout),
            String::from_utf8_lossy(&competing.stderr)
        );
    }

    /// A child reproduces the parent-held-session/scoped-worker shape. The
    /// watchdog begins only after its first real trace acquired the host logger.
    /// Killing our captured child on failure makes the old deadlock a bounded failure.
    fn a_process_shares_its_session_with_a_scoped_worker(
        accepted: SocketAddr,
        refused: SocketAddr,
    ) {
        use std::io::{BufRead as _, BufReader, Write as _};
        use std::process::Stdio;
        use std::time::{Duration, Instant};

        let mut child = Command::new(std::env::current_exe().expect("test binary"))
            .args(["--exact", "traced::regression::connector", "--nocapture"])
            .env("PRINTOBSERVER_TRACE_SCOPE", "yes")
            .env(
                "PRINTOBSERVER_TRACE_CONNECT",
                format!("{accepted},{refused}"),
            )
            .env_remove("PRINTOBSERVER_SPIKE_TIMINGS")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .spawn()
            .expect("real scoped observer process");
        let mut lines = BufReader::new(child.stdout.take().expect("observer stdout"));
        loop {
            let mut line = String::new();
            assert!(
                lines.read_line(&mut line).expect("observer readiness") > 0,
                "observer exited before holding its session"
            );
            if line.contains("process-session held") {
                break;
            }
        }
        another_live_process_can_trace(&mut child, accepted, refused);
        child
            .stdin
            .take()
            .expect("observer barrier")
            .write_all(b"Y")
            .expect("start scoped trace");
        let deadline = Instant::now() + Duration::from_secs(20);
        let status = loop {
            if let Some(status) = child.try_wait().expect("observer status") {
                break Some(status);
            }
            if Instant::now() >= deadline {
                child.kill().expect("stop our captured stalled observer");
                child.wait().expect("reap our captured stalled observer");
                break None;
            }
            std::thread::sleep(Duration::from_millis(20));
        };
        let status = status
            .expect("a parent-held observer session blocked its scoped worker for 20 seconds");
        assert!(status.success(), "scoped observer failed: {status}");
        drop(lines);
        // A different real process must acquire the logger after normal process exit.
        let recovered = Command::new(std::env::current_exe().expect("test binary"))
            .args(["--exact", "traced::regression::connector", "--nocapture"])
            .env(
                "PRINTOBSERVER_TRACE_CONNECT",
                format!("{accepted},{refused}"),
            )
            .env("PRINTOBSERVER_TRACE_RECOVER", "yes")
            .env_remove("PRINTOBSERVER_SPIKE_TIMINGS")
            .output()
            .expect("real process-exit recovery");
        assert!(
            recovered.status.success(),
            "{}{}",
            String::from_utf8_lossy(&recovered.stdout),
            String::from_utf8_lossy(&recovered.stderr)
        );
    }

    /// The real subprocess/socket workload, on the caller's named thread.
    fn exercise() {
        let scratch = tempfile::TempDir::new().expect("trace scratch");
        let listener = TcpListener::bind("127.0.0.1:0").expect("accepted endpoint");
        let accepted = listener.local_addr().expect("listener endpoint");
        let reservation = TcpListener::bind("127.0.0.1:0").expect("refused endpoint");
        let refused = reservation.local_addr().expect("reserved endpoint");
        drop(reservation);
        let listener6 = TcpListener::bind("[::1]:0").expect("accepted IPv6 endpoint");
        let accepted6 = listener6.local_addr().expect("IPv6 listener endpoint");
        let reservation6 = TcpListener::bind("[::1]:0").expect("refused IPv6 endpoint");
        let refused6 = reservation6.local_addr().expect("reserved IPv6 endpoint");
        drop(reservation6);
        let outside = TcpListener::bind("127.0.0.1:0").expect("unrelated endpoint");
        let unrelated = outside.local_addr().expect("outside endpoint");
        let done = Arc::new(AtomicBool::new(false));
        let stop = Arc::clone(&done);
        let traffic = std::thread::spawn(move || {
            while !stop.load(Ordering::Acquire) {
                let _ = TcpStream::connect(unrelated);
                let _ = outside.accept();
                std::thread::sleep(std::time::Duration::from_millis(10));
            }
        });
        let result = std::panic::catch_unwind(|| {
            #[cfg(windows)]
            let mut controller = None;
            for (endpoints, child) in [
                (format!("{accepted},{refused},{accepted6},{refused6}"), true),
                (refused.to_string(), false),
            ] {
                let mut environment =
                    vec![("PRINTOBSERVER_TRACE_CONNECT".to_owned(), endpoints.clone())];
                if child {
                    environment.push(("PRINTOBSERVER_TRACE_CHILD".to_owned(), "yes".to_owned()));
                }
                let ran = traced(
                    &std::env::current_exe().expect("test executable"),
                    &[
                        "--exact".to_owned(),
                        "traced::regression::connector".to_owned(),
                        "--nocapture".to_owned(),
                    ],
                    &environment,
                    scratch.path(),
                );
                assert_eq!(ran.code, Some(0), "{}", ran.said());
                let expected: BTreeSet<SocketAddr> = endpoints
                    .split(',')
                    .map(|address| address.parse().expect("endpoint"))
                    .collect();
                assert_eq!(ran.connected, expected, "{}", ran.said());
                #[cfg(windows)]
                {
                    let current = super::windows_trace::controller_id();
                    if let Some(previous) = controller {
                        assert_eq!(
                            current, previous,
                            "adjacent invocations did not share the native controller"
                        );
                    }
                    controller = Some(current);
                }
            }
            #[cfg(windows)]
            super::windows_trace::release();
            a_process_shares_its_session_with_a_scoped_worker(accepted, refused);
        });
        done.store(true, Ordering::Release);
        traffic.join().expect("unrelated traffic completed");
        if let Err(error) = result {
            std::panic::resume_unwind(error);
        }
    }
}

/// Focused endpoint/lifetime scenarios replacing the retired XML decoder's
/// fixtures with the actual observation interface and real processes/sockets.
#[cfg(test)]
mod native_boundaries {
    use std::collections::BTreeSet;
    use std::net::{SocketAddr, TcpListener};

    use super::{Ran, traced};

    /// A real invocation of the socket fixture, on this platform's tracer.
    fn sockets(endpoints: &[SocketAddr], descendant: bool) -> Ran {
        sockets_with_output(endpoints, descendant, false)
    }

    /// The descendant can retain its root's actual output handles until exit.
    fn sockets_with_output(endpoints: &[SocketAddr], descendant: bool, inherited: bool) -> Ran {
        let scratch = tempfile::TempDir::new().expect("trace scratch");
        let mut environment = vec![(
            "PRINTOBSERVER_TRACE_CONNECT".to_owned(),
            endpoints
                .iter()
                .map(ToString::to_string)
                .collect::<Vec<_>>()
                .join(","),
        )];
        environment.push(("PRINTOBSERVER_TRACE_LOUD".to_owned(), "yes".to_owned()));
        if descendant {
            environment.push(("PRINTOBSERVER_TRACE_CHILD".to_owned(), "yes".to_owned()));
        }
        if inherited {
            environment.push(("PRINTOBSERVER_TRACE_INHERIT".to_owned(), "yes".to_owned()));
        }
        let ran = traced(
            &std::env::current_exe().expect("test executable"),
            &[
                "--exact".to_owned(),
                "traced::regression::connector".to_owned(),
                "--nocapture".to_owned(),
            ],
            &environment,
            scratch.path(),
        );
        assert_eq!(ran.code, Some(0), "{}", ran.said());
        assert!(
            ran.out.contains(&"observed stdout\n".repeat(8192)),
            "stdout truncated"
        );
        let copies = if descendant && inherited { 2 } else { 1 };
        assert_eq!(
            ran.out
                .lines()
                .filter(|line| *line == "observed stdout")
                .count(),
            8192 * copies
        );
        assert_eq!(ran.err, "observed stderr\n".repeat(8192 * copies));
        ran
    }

    /// A TCP attempt rejected by the kernel is still an endpoint observation
    /// of the real shipped command, with its normal unreachable exit.
    #[test]
    fn a_refused_attempt_from_the_real_command_is_observed() {
        let reservation = TcpListener::bind("127.0.0.1:0").expect("reserved port");
        let endpoint = reservation.local_addr().expect("reserved endpoint");
        drop(reservation);
        let scratch = tempfile::TempDir::new().expect("trace scratch");
        let ran = traced(
            std::path::Path::new(env!("CARGO_BIN_EXE_printobserver")),
            &[
                "context".to_owned(),
                "--print-id".to_owned(),
                "01900000-0000-7000-8000-000000000000".to_owned(),
            ],
            &[
                (
                    "PRINTOBSERVER_SERVER".to_owned(),
                    format!("http://{endpoint}"),
                ),
                (
                    "PRINTOBSERVER_CREDENTIAL".to_owned(),
                    "a-credential".to_owned(),
                ),
            ],
            scratch.path(),
        );
        assert_eq!(
            ran.code,
            Some(i32::from(
                printobserver::failure::Exit::Unreachable.status()
            )),
            "{}",
            ran.said()
        );
        assert_eq!(ran.connected, BTreeSet::from([endpoint]));
        let unroutable = "192.0.2.1:9".parse().expect("documentation endpoint");
        let attempted = sockets(&[unroutable], false);
        assert_eq!(attempted.connected, BTreeSet::from([unroutable]));
    }

    /// IPv6 is read at the native boundary, for both accepted and refused
    /// attempts; this is no longer a string-format parser fixture.
    #[test]
    fn ipv6_accepted_and_refused_attempts_are_both_observed() {
        let listener = TcpListener::bind("[::1]:0").expect("IPv6 listener");
        let accepted = listener.local_addr().expect("accepted endpoint");
        let reservation = TcpListener::bind("[::1]:0").expect("IPv6 refused port");
        let refused = reservation.local_addr().expect("refused endpoint");
        drop(reservation);
        assert_eq!(
            sockets(&[accepted, refused], false).connected,
            BTreeSet::from([accepted, refused])
        );
    }

    /// An observed process tree that genuinely connects nowhere is distinct
    /// from unavailable or incomplete tracing.
    #[test]
    fn the_real_commands_version_has_complete_empty_connection_evidence() {
        let scratch = tempfile::TempDir::new().expect("trace scratch");
        let ran = traced(
            std::path::Path::new(env!("CARGO_BIN_EXE_printobserver")),
            &["--version".to_owned()],
            &[],
            scratch.path(),
        );
        assert_eq!(ran.code, Some(0), "{}", ran.said());
        assert!(ran.out.starts_with("printobserver "), "{}", ran.said());
        assert!(ran.connected.is_empty(), "{ran:?}");
    }

    /// Descendants connect after their root exits, with either closed or
    /// inherited output handles. Both outputs and endpoint evidence are complete.
    #[test]
    fn a_descendant_connecting_after_its_parent_exits_is_observed() {
        let reservation = TcpListener::bind("127.0.0.1:0").expect("refused port");
        let endpoint = reservation.local_addr().expect("refused endpoint");
        drop(reservation);
        for inherited in [false, true] {
            assert_eq!(
                sockets_with_output(&[endpoint], true, inherited).connected,
                BTreeSet::from([endpoint])
            );
        }
    }

    /// Adjacent invocations have distinct endpoint sets while reusing the
    /// native controller across independently observed process trees.
    #[test]
    fn adjacent_invocations_do_not_inherit_each_others_endpoints() {
        let first = TcpListener::bind("127.0.0.1:0").expect("first listener");
        let second = TcpListener::bind("127.0.0.1:0").expect("second listener");
        for endpoint in [
            first.local_addr().expect("first endpoint"),
            second.local_addr().expect("second endpoint"),
        ] {
            assert_eq!(
                sockets(&[endpoint], false).connected,
                BTreeSet::from([endpoint])
            );
        }
    }
}
