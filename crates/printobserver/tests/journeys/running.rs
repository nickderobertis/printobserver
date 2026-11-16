//! Driving the built program, and reading back through it.
//!
//! Everything every journey here does goes through this: the compiled binary as
//! a subprocess, with nothing of this program called in process. Reading an
//! effect back goes through it too — a journey confirms what a command did by
//! running another command, which is what "through this same surface" means.
//!
//! # Which runs are traced
//!
//! An invocation of the walk ([`run`]) runs under the tracer, because every
//! point of the cross-product is held to having connected to no endpoint but
//! the configured one. Every other run here ([`with`] and everything built on
//! it) runs the same binary untraced: no assertion reads what such a run
//! connected to, so tracing it would be work whose answer nothing reads — and on
//! Windows each trace leases the host's one kernel logger, so every journey
//! running beside it would wait on that lease too. An untraced run answers no
//! connections at all, so holding one to the endpoint rule fails rather than
//! passes.

use std::collections::BTreeSet;
use std::net::{Ipv4Addr, SocketAddr};
use std::path::{Path, PathBuf};
use std::process::Command;

use printobserver_types::serde_json::{self, Value};

use crate::traced::{Ran, traced, with_its_own_config_home};
use crate::walk::Invocation;
use crate::world::World;

/// The program every journey drives.
pub fn program() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_printobserver"))
}

/// Where the tracer writes what it saw.
pub fn traces(world: &World) -> PathBuf {
    let at = world.root.path().join("traces");
    std::fs::create_dir_all(&at).expect("a directory for what the tracer saw");
    at
}

/// Run one invocation of the walk.
pub fn run(world: &World, invocation: &Invocation) -> Ran {
    traced(
        &program(),
        &invocation.arguments,
        &invocation.environment,
        &traces(world),
    )
}

/// Run one command under the environment given, untraced.
pub fn with(world: &World, arguments: &[String], environment: &[(String, String)]) -> Ran {
    untraced(&program(), arguments, environment, &traces(world))
}

/// One invocation exactly as [`traced`] makes it — the same binary, arguments
/// and environment, with its configuration home a directory of its own unless
/// the journey named one — with no tracer around it, answering no connections.
///
/// # Panics
///
/// Panics when the program cannot be started.
fn untraced(
    program: &Path,
    arguments: &[String],
    environment: &[(String, String)],
    scratch: &Path,
) -> Ran {
    let mut process = Command::new(program);
    process
        .args(arguments)
        .env_remove("PRINTOBSERVER_SERVER")
        .env_remove("PRINTOBSERVER_CREDENTIAL");
    for (name, value) in with_its_own_config_home(environment, scratch) {
        process.env(name, value);
    }
    let output = process
        .output()
        .unwrap_or_else(|error| panic!("{} could not run: {error}", program.display()));
    Ran {
        code: output.status.code(),
        out: String::from_utf8_lossy(&output.stdout).into_owned(),
        err: String::from_utf8_lossy(&output.stderr).into_owned(),
        connected: BTreeSet::new(),
        arguments: arguments.to_vec(),
    }
}

/// Run one command configured by the file this world wrote.
pub fn command(world: &World, arguments: &[&str]) -> Ran {
    let mut given: Vec<String> = arguments.iter().map(|word| (*word).to_owned()).collect();
    given.push("--config".to_owned());
    given.push(world.client_config().display().to_string());
    with(world, &given, &[])
}

/// Run one command configured by the environment, under one credential.
pub fn configured(world: &World, arguments: &[String], credential: &str) -> Ran {
    with(world, arguments, &world.environment(credential))
}

/// Read one answer back through this same surface, as a document.
///
/// # Panics
///
/// Panics when the read did not answer, which is a world a journey cannot
/// confirm anything against.
pub fn read(world: &World, arguments: &[&str]) -> Value {
    let mut asked: Vec<&str> = arguments.to_vec();
    asked.push("--json");
    let ran = command(world, &asked);
    assert_eq!(
        ran.code,
        Some(0),
        "reading {arguments:?} back failed: {}",
        ran.said()
    );
    serde_json::from_str(&ran.out)
        .unwrap_or_else(|error| panic!("{arguments:?} answered {error}: {}", ran.out))
}

/// An address no supervisor can be answering at: port zero on IPv4 loopback.
///
/// Nothing can listen on port zero — a listener asking for it is handed some
/// other port — so nothing is held and nothing can come to take it. Connecting
/// there fails on every host, refused on Linux and refused before any packet is
/// sent on Darwin and Windows, and the program reports every connect failure
/// as unreachable.
pub fn nowhere() -> SocketAddr {
    SocketAddr::from((Ipv4Addr::LOCALHOST, 0))
}

/// One command run against a supervisor that is not there.
pub fn against_nothing(world: &World, arguments: &[String], credential: &str) -> Ran {
    with(
        world,
        arguments,
        &[
            (
                "PRINTOBSERVER_SERVER".to_owned(),
                format!("http://{}", nowhere()),
            ),
            ("PRINTOBSERVER_CREDENTIAL".to_owned(), credential.to_owned()),
        ],
    )
}

/// One command run against the supervisor, configured by a file naming it and
/// no credential.
pub fn without_a_credential(world: &World, arguments: &[String]) -> Ran {
    let mut given = arguments.to_vec();
    given.push("--config".to_owned());
    given.push(world.credentialless_config().display().to_string());
    with(world, &given, &[])
}

/// One command run with nothing naming a supervisor.
///
/// The credential is still configured, because what the redaction walk searches
/// every run for is a credential this program was given.
pub fn unconfigured(world: &World, arguments: &[String], credential: &str) -> Ran {
    let mut given = arguments.to_vec();
    given.push("--config".to_owned());
    given.push(world.serverless_config().display().to_string());
    with(
        world,
        &given,
        &[("PRINTOBSERVER_CREDENTIAL".to_owned(), credential.to_owned())],
    )
}
