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
//! # Two launchers, one reading
//!
//! On Linux the tree is traced with `strace -f`. macOS has no `strace`, so
//! there the tree is observed by the [`interposer`] loaded into it instead.
//! Only the launcher differs: [`connections_in`] reads either recording into
//! the same set of endpoints, and every assertion over a [`Ran`] is shared.

#[path = "interposer.rs"]
pub mod interposer;

use std::collections::BTreeSet;
use std::net::SocketAddr;
use std::path::Path;
use std::process::{Command, Output};
#[cfg(not(target_os = "macos"))]
use std::sync::atomic::{AtomicU64, Ordering};

/// The tracer this tier reads a process tree's connections out of.
#[cfg(not(target_os = "macos"))]
const TRACER: &str = "strace";

/// How one traced invocation's own output file is named apart from every other.
#[cfg(not(target_os = "macos"))]
static TRACED: AtomicU64 = AtomicU64::new(0);

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

/// Run one invocation under this platform's observation, and answer what it did.
///
/// # Panics
///
/// Panics when the observation cannot be made on this host — `strace` is not
/// there, or the interposer cannot be built or loaded — which is a toolchain
/// that cannot make the observation this tier's whole claim rests on, rather
/// than a walk that quietly stops making it.
pub fn traced(
    program: &Path,
    arguments: &[String],
    environment: &[(String, String)],
    scratch: &Path,
) -> Ran {
    let (output, recorded) = observed(program, arguments, environment, scratch);
    Ran {
        code: output.status.code(),
        out: String::from_utf8_lossy(&output.stdout).into_owned(),
        err: String::from_utf8_lossy(&output.stderr).into_owned(),
        connected: connections_in(&recorded),
        arguments: arguments.to_vec(),
    }
}

/// One invocation under `strace -f`, and the trace it wrote.
#[cfg(not(target_os = "macos"))]
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
fn interposed_endpoint_in(line: &str) -> Option<SocketAddr> {
    line.strip_prefix("connected ")?.trim().parse().ok()
}

/// The endpoint one traced line names, when it names one.
fn endpoint_in(line: &str, port_marker: &str, address_marker: &str) -> Option<SocketAddr> {
    let port: u16 = after(line, port_marker, ')')?.parse().ok()?;
    let address = after(line, address_marker, '"')?;
    format!("{address}:{port}")
        .parse()
        .ok()
        .or_else(|| format!("[{address}]:{port}").parse().ok())
}

/// What one line carries between a marker and the next of one character.
fn after(line: &str, marker: &str, until: char) -> Option<String> {
    let rest = line.split_once(marker)?.1;
    Some(rest.split(until).next()?.to_owned())
}

/// The observation itself, proven on whichever platform runs the tier.
#[cfg(test)]
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
