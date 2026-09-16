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
//! **Windows** has no `strace`. It starts an event-tracing session with `logman`
//! over two providers of the operating system's own, runs the invocation, stops
//! the session and decodes it with `tracerpt`. `Microsoft-Windows-TCPIP` records
//! event 1002, *requested to connect*, for every TCP connect request in the
//! context of the process that made it, before a single packet is sent: so an
//! attempt to an address nothing listens on, or one nothing routes to, is
//! recorded exactly as a completed connection is. `Microsoft-Windows-Kernel-Process`
//! records every process start with its parent, which is how the tree is
//! followed. It cannot see a UDP `connect`, which that provider does not record:
//! this program speaks HTTP over TCP and resolves no names, so every endpoint it
//! could reach is a TCP one. Starting a session needs an administrator, which
//! the hosted runners are; without one this refuses naming why.
//!
//! On any other platform — macOS among them — the `strace` observation is what
//! runs, and a host without it is refused rather than passed.

use std::collections::BTreeSet;
use std::net::SocketAddr;
use std::path::Path;
#[cfg(not(windows))]
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};

/// The tracer this tier reads a process tree's connections out of.
#[cfg(not(windows))]
const TRACER: &str = "strace";

/// How one traced invocation's own output file is named apart from every other.
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

/// Run one invocation under the tracer, and answer what it did.
///
/// # Panics
///
/// Panics when the tracer is not on this host, which is a toolchain that
/// cannot make the observation this tier's whole claim rests on — rather than
/// a walk that quietly stops making it.
#[cfg(not(windows))]
pub fn traced(
    program: &Path,
    arguments: &[String],
    environment: &[(String, String)],
    scratch: &Path,
) -> Ran {
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
    Ran {
        code: output.status.code(),
        out: String::from_utf8_lossy(&output.stdout).into_owned(),
        err: String::from_utf8_lossy(&output.stderr).into_owned(),
        connected: connections_in(&traced),
        arguments: arguments.to_vec(),
    }
}

/// Every internet endpoint a trace records a connection to.
#[cfg(not(windows))]
///
/// Connections to anything that is not an internet endpoint — a local socket
/// to the name-service cache, say — are not endpoints this rule is about and
/// are not collected.
pub fn connections_in(traced: &str) -> BTreeSet<SocketAddr> {
    let mut found = BTreeSet::new();
    for line in traced.lines() {
        if let Some(address) = endpoint_in(line, "sin_port=htons(", "inet_addr(\"")
            .or_else(|| endpoint_in(line, "sin6_port=htons(", "inet_pton(AF_INET6, \""))
        {
            found.insert(address);
        }
    }
    found
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
    use std::process::{Command, Stdio};

    let invocation = format!(
        "{}-{}",
        std::process::id(),
        TRACED.fetch_add(1, Ordering::Relaxed)
    );
    let session = format!("printobserver-traced-{invocation}");
    let etl = scratch.join(format!("connections-{invocation}.etl"));
    let dump = scratch.join(format!("connections-{invocation}.xml"));
    let providers = scratch.join(format!("connections-{invocation}.providers"));
    std::fs::write(&providers, etw::PROVIDERS)
        .unwrap_or_else(|error| panic!("{} could not be written: {error}", providers.display()));

    etw::tool(
        "logman",
        &[
            "start".as_ref(),
            session.as_ref(),
            "-ets".as_ref(),
            "-o".as_ref(),
            etl.as_os_str(),
            "-pf".as_ref(),
            providers.as_os_str(),
            "-max".as_ref(),
            "1024".as_ref(),
            "-bs".as_ref(),
            "1024".as_ref(),
            "-nb".as_ref(),
            "64".as_ref(),
            "256".as_ref(),
        ],
    );
    let mut process = Command::new(program);
    process
        .args(arguments)
        .env_remove("PRINTOBSERVER_SERVER")
        .env_remove("PRINTOBSERVER_CREDENTIAL")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    for (name, value) in environment {
        process.env(name, value);
    }
    let started = process.spawn();
    let output = started.and_then(|child| {
        let root = child.id();
        child.wait_with_output().map(|output| (root, output))
    });
    // Stopped whatever the invocation did, so a program that could not be
    // started leaves no session behind to fill the host's limit of them.
    etw::tool(
        "logman",
        &["stop".as_ref(), session.as_ref(), "-ets".as_ref()],
    );
    let (root, output) =
        output.unwrap_or_else(|error| panic!("{} could not be run: {error}", program.display()));
    etw::tool(
        "tracerpt",
        &[
            etl.as_os_str(),
            "-of".as_ref(),
            "XML".as_ref(),
            "-o".as_ref(),
            dump.as_os_str(),
            "-y".as_ref(),
        ],
    );
    let decoded =
        etw::text_of(&std::fs::read(&dump).unwrap_or_else(|error| {
            panic!("`tracerpt` wrote nothing to {}: {error}", dump.display())
        }));
    for written in [&etl, &dump, &providers] {
        let _ = std::fs::remove_file(written);
    }
    Ran {
        code: output.status.code(),
        out: String::from_utf8_lossy(&output.stdout).into_owned(),
        err: String::from_utf8_lossy(&output.stderr).into_owned(),
        connected: etw::connections_of(&decoded, root),
        arguments: arguments.to_vec(),
    }
}

/// Reading a decoded event-tracing session: which processes are one tree, and
/// which endpoints that tree asked to connect to.
///
/// Only starting and stopping a session is Windows' own. Reading what
/// `tracerpt` wrote is not, so it is built and proven everywhere, over a dump a
/// Windows runner recorded.
mod etw {
    use std::collections::{BTreeMap, BTreeSet};
    #[cfg(windows)]
    use std::ffi::OsStr;
    use std::net::{IpAddr, SocketAddr};
    #[cfg(windows)]
    use std::process::Command;

    /// The providers a session records, as `logman` reads a provider file:
    /// the TCP/IP stack's connect path, and process starts.
    #[cfg(windows)]
    pub const PROVIDERS: &str = "\"Microsoft-Windows-TCPIP\" 0xFFFFFFFFFFFFFFFF 0xFF\r\n\
         \"Microsoft-Windows-Kernel-Process\" 0xFFFFFFFFFFFFFFFF 0xFF\r\n";

    /// The provider recording a TCP connect request, and its event for one.
    const TCPIP: &str = "Microsoft-Windows-TCPIP";
    const REQUESTED_TO_CONNECT: &str = "1002";

    /// The provider recording a process start, and its event for one.
    const KERNEL_PROCESS: &str = "Microsoft-Windows-Kernel-Process";
    const PROCESS_STARTED: &str = "1";

    /// Run one of the operating system's own tracing tools, and require it worked.
    #[cfg(windows)]
    pub fn tool(name: &str, arguments: &[&OsStr]) {
        let output = Command::new(name)
            .args(arguments)
            .output()
            .unwrap_or_else(|error| {
                panic!(
                    "`{name}` could not run, and this tier's whole claim about which endpoints \
                     an invocation reaches rests on it: {error}"
                )
            });
        assert!(
            output.status.success(),
            "`{name}` refused, and this tier's whole claim about which endpoints an \
             invocation reaches rests on it (starting a trace session needs an \
             administrator): {}{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
    }

    /// A dump `tracerpt` wrote, as text, whichever encoding it chose.
    pub fn text_of(bytes: &[u8]) -> String {
        match bytes {
            [0xFF, 0xFE, rest @ ..] => {
                let units: Vec<u16> = rest
                    .chunks_exact(2)
                    .map(|pair| u16::from_le_bytes([pair[0], pair[1]]))
                    .collect();
                String::from_utf16_lossy(&units)
            }
            [0xEF, 0xBB, 0xBF, rest @ ..] => String::from_utf8_lossy(rest).into_owned(),
            other => String::from_utf8_lossy(other).into_owned(),
        }
    }

    /// Every endpoint the tree rooted at one process requested a connection to.
    pub fn connections_of(dump: &str, root: u32) -> BTreeSet<SocketAddr> {
        let events: Vec<&str> = dump.split("<Event ").skip(1).collect();
        let parents: BTreeMap<u32, u32> = events
            .iter()
            .filter(|event| is(event, KERNEL_PROCESS, PROCESS_STARTED))
            .filter_map(|event| {
                Some((
                    number(&data(event, "ProcessID")?)?,
                    number(&data(event, "ParentProcessID")?)?,
                ))
            })
            .collect();
        let mut tree = BTreeSet::from([root]);
        loop {
            let grown: BTreeSet<u32> = parents
                .iter()
                .filter(|(_, parent)| tree.contains(parent))
                .map(|(child, _)| *child)
                .collect();
            if grown.is_subset(&tree) {
                break;
            }
            tree.extend(grown);
        }
        events
            .iter()
            .filter(|event| is(event, TCPIP, REQUESTED_TO_CONNECT))
            .filter(|event| {
                between(event, "<Execution ProcessID=\"", "\"")
                    .and_then(number)
                    .is_some_and(|pid| tree.contains(&pid))
            })
            .filter_map(|event| endpoint(&data(event, "RemoteAddress")?))
            .collect()
    }

    /// Whether one event is the one event of one provider.
    fn is(event: &str, provider: &str, id: &str) -> bool {
        between(event, "<Provider Name=\"", "\"") == Some(provider)
            && between(event, "<EventID>", "</EventID>").map(str::trim) == Some(id)
    }

    /// One named datum of one event.
    fn data(event: &str, name: &str) -> Option<String> {
        between(event, &format!("<Data Name=\"{name}\">"), "</Data>")
            .map(|value| value.trim().to_owned())
    }

    /// What one event carries between a marker and the next of another.
    fn between<'a>(event: &'a str, open: &str, close: &str) -> Option<&'a str> {
        let rest = event.split_once(open)?.1;
        Some(rest.split_once(close)?.0)
    }

    /// A process number, as `tracerpt` renders one: decimal, or hexadecimal.
    fn number(rendered: &str) -> Option<u32> {
        let rendered = rendered.trim();
        rendered.strip_prefix("0x").map_or_else(
            || rendered.parse().ok(),
            |hex| u32::from_str_radix(hex, 16).ok(),
        )
    }

    /// An address and port, as `tracerpt` renders a socket address.
    fn endpoint(rendered: &str) -> Option<SocketAddr> {
        rendered.parse().ok().or_else(|| {
            let (address, port) = rendered.rsplit_once(':')?;
            let address: IpAddr = address.trim_matches(['[', ']']).parse().ok()?;
            Some(SocketAddr::new(address, port.parse().ok()?))
        })
    }

    /// What a Windows runner's `tracerpt` wrote for one traced invocation.
    const RECORDED: &str = include_str!("tracerpt-connects.xml");

    /// The tree's own requests are every one its root and the process it
    /// started made, to any address — refused, accepted or unroutable — and
    /// none a process outside it made.
    #[test]
    fn a_recorded_session_answers_every_connect_its_tree_requested_and_no_other() {
        let addresses = |found: BTreeSet<SocketAddr>| -> Vec<String> {
            found.iter().map(ToString::to_string).collect()
        };

        assert_eq!(
            addresses(connections_of(RECORDED, 8508)),
            [
                "127.0.0.1:53947",
                "127.0.0.1:53948",
                "127.0.0.1:53949",
                "192.0.2.1:9"
            ]
        );
        assert_eq!(
            addresses(connections_of(RECORDED, 7280)),
            ["127.0.0.1:53949"]
        );
        assert_eq!(addresses(connections_of(RECORDED, 1396)), ["127.0.0.1:1"]);
        assert!(connections_of(RECORDED, 4).is_empty());
    }

    /// A dump written as UTF-16, as `tracerpt` may write one, reads the same.
    #[test]
    fn a_dump_written_as_utf16_reads_as_the_same_session() {
        let mut bytes = vec![0xFF, 0xFE];
        bytes.extend(RECORDED.encode_utf16().flat_map(u16::to_le_bytes));

        assert_eq!(
            connections_of(&text_of(&bytes), 8508),
            connections_of(RECORDED, 8508)
        );
    }

    /// A UTF-8 marker from `tracerpt` is not part of the XML it prefixes.
    #[test]
    fn a_dump_written_as_utf8_with_a_marker_reads_as_the_same_session() {
        let mut bytes = vec![0xEF, 0xBB, 0xBF];
        bytes.extend(RECORDED.as_bytes());

        assert_eq!(text_of(&bytes), RECORDED);
    }

    /// A missing native tracer is a refusal, never an empty observation.
    #[cfg(windows)]
    #[test]
    fn an_unavailable_trace_tool_is_refused() {
        let panic = std::panic::catch_unwind(|| tool("printobserver-no-such-trace-tool", &[]))
            .expect_err("a tool that does not exist unexpectedly ran");
        let message = panic
            .downcast_ref::<String>()
            .map(String::as_str)
            .or_else(|| panic.downcast_ref::<&str>().copied())
            .unwrap_or("panic carried no text");

        assert!(message.contains("could not run"), "{message}");
        assert!(message.contains("whole claim"), "{message}");
    }

    /// An address `tracerpt` renders without brackets is still an endpoint.
    #[test]
    fn an_ipv6_endpoint_is_read_with_or_without_brackets() {
        let expected = Some(SocketAddr::from((std::net::Ipv6Addr::LOCALHOST, 8420)));

        assert_eq!(endpoint("[::1]:8420"), expected);
        assert_eq!(endpoint("::1:8420"), expected);
        assert_eq!(number("0x1C70"), Some(7280));
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
