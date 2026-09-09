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

use std::collections::BTreeSet;
use std::net::SocketAddr;
use std::path::Path;
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};

/// The tracer this tier reads a process tree's connections out of.
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
