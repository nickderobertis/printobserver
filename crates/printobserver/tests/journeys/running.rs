//! Driving the built program, and reading back through it.
//!
//! Everything every journey here does goes through this: the compiled binary as
//! a subprocess, under the tracer, with nothing of this program called in
//! process. Reading an effect back goes through it too — a journey confirms
//! what a command did by running another command, which is what "through this
//! same surface" means.

use std::path::PathBuf;

use printobserver_types::serde_json::{self, Value};

use crate::traced::{Ran, traced};
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

/// Run one command under the environment given.
pub fn with(world: &World, arguments: &[String], environment: &[(String, String)]) -> Ran {
    traced(&program(), arguments, environment, &traces(world))
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

/// One command run against a supervisor that is not there.
pub fn against_nothing(world: &World, arguments: &[String], credential: &str) -> Ran {
    // A port is bound to learn one that is free and then released, so what the
    // command meets is a refused connection rather than a served refusal.
    let nowhere = {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("a loopback port");
        listener.local_addr().expect("the bound address")
    };
    with(
        world,
        arguments,
        &[
            (
                "PRINTOBSERVER_SERVER".to_owned(),
                format!("http://{nowhere}"),
            ),
            ("PRINTOBSERVER_CREDENTIAL".to_owned(), credential.to_owned()),
        ],
    )
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
