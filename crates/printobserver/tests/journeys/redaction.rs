//! No run prints the credential, nor four consecutive characters of it.
//!
//! # Why the fragments are the subject
//!
//! A search for the whole string catches only a whole rendering. A program
//! printing a prefix, a suffix or a redacted middle satisfies it while exposing
//! credential material, which is the gap this journey exists to close. So what
//! is searched for is every contiguous four-character substring of the
//! configured credential: a whole rendering, a debug-formatted wrapper, a
//! message quoting the configuration back and any partial rendering leaving
//! four consecutive characters intact are each caught. A rendering leaving
//! three or fewer is outside what this reaches, and is not claimed.
//!
//! # The control is what makes the search mean anything
//!
//! The same walk is run a second time under a different credential and searched
//! for the **first** one's fragments. A fragment this program would have
//! printed whatever it was configured with cannot then be read as a leak, and
//! the assertion cannot pass by the credential being unsearchable.
//!
//! # Where the structural argument reaches and where it does not
//!
//! On a path where a command has a response to answer from, every value its
//! output carries is one that response carried and no response shape this
//! system declares carries a credential. That says nothing about the paths
//! where the output is produced **locally** — a refused argument, a refused
//! configuration file, an unreachable server, and the command that runs the
//! server — which are exactly the paths a program renders its own configuration
//! on. All of them are driven here, and the fragment search is what rules.

use std::collections::BTreeSet;
use std::process::{Command, Stdio};

use crate::machine::Reports;
use crate::walk;
use crate::world::{CREDENTIAL, OTHER_CREDENTIAL, World};

use super::{failures, running};

/// How long a fragment has to be to be looked for.
const FRAGMENT: usize = 4;

/// Every path this task defines a behaviour for, under both credentials.
pub fn no_run_of_the_walk_prints_the_credential(world: &World) {
    let looked_for = fragments(CREDENTIAL);
    assert!(
        looked_for.len() > 20,
        "the credential this walk searches for is too short to search for"
    );
    for credential in [CREDENTIAL, OTHER_CREDENTIAL] {
        for said in everything_every_path_says(world, credential) {
            let found: Vec<&String> = looked_for
                .iter()
                .filter(|fragment| said.1.contains(fragment.as_str()))
                .collect();
            assert!(
                found.is_empty(),
                "`{}` printed {found:?} of the credential it was configured with",
                said.0
            );
        }
    }
}

/// Every contiguous fragment of one credential, at the length searched for.
fn fragments(credential: &str) -> BTreeSet<String> {
    let characters: Vec<char> = credential.chars().collect();
    characters
        .windows(FRAGMENT)
        .map(|window| window.iter().collect())
        .collect()
}

/// Everything every path says, under one credential.
fn everything_every_path_says(world: &World, credential: &str) -> Vec<(String, String)> {
    let mut said = Vec::new();
    for one in walk::walk(world) {
        world.wants(one.reports);
        let arguments = failures::succeeding(&one);
        let name = one.command.name.clone();
        said.push((
            name.clone(),
            running::configured(world, &arguments, credential).said(),
        ));
        said.push((
            format!("{name} against nothing"),
            running::against_nothing(world, &arguments, credential).said(),
        ));
        said.push((
            format!("{name} unconfigured"),
            running::unconfigured(world, &arguments, credential).said(),
        ));
        said.push((
            format!("{name} with a refused argument"),
            refused_argument(world, &arguments, credential),
        ));
        said.push((
            format!("{name} against a refused configuration"),
            refused_configuration(world, &arguments, credential),
        ));
        if one.operation().action_kind().is_some() {
            let rejected = failures::rejected(world, &one);
            said.push((
                format!("{name} rejected"),
                running::configured(world, &rejected, credential).said(),
            ));
        }
        if one.operation().image_path_field.is_some() {
            said.push((
                format!("{name} with the image elsewhere"),
                image_elsewhere(world, &arguments, credential),
            ));
        }
    }
    said.extend(the_server_command(world, credential));
    said
}

/// One command run with an argument its parser refuses.
fn refused_argument(world: &World, arguments: &[String], credential: &str) -> String {
    let mut given = arguments.to_vec();
    given.push("--fast".to_owned());
    given.push("yes".to_owned());
    running::configured(world, &given, credential).said()
}

/// One command run against a configuration file the program refuses.
fn refused_configuration(world: &World, arguments: &[String], credential: &str) -> String {
    let mut given = arguments.to_vec();
    given.push("--config".to_owned());
    given.push(world.refused_config().display().to_string());
    running::with(
        world,
        &given,
        &[("PRINTOBSERVER_CREDENTIAL".to_owned(), credential.to_owned())],
    )
    .said()
}

/// One command whose answer carries a path that names no file here.
fn image_elsewhere(world: &World, arguments: &[String], credential: &str) -> String {
    world.freshen_the_image();
    world.proxy.losing_the_file(Some(world.image_path()));
    let ran = running::configured(world, arguments, credential);
    world.proxy.losing_the_file(None);
    world.restore_the_image();
    ran.said()
}

/// What the command that runs the server says, starting and refusing.
fn the_server_command(world: &World, credential: &str) -> Vec<(String, String)> {
    let refused = Command::new(running::program())
        .arg("server")
        .arg("--config")
        .arg(world.refused_config())
        .env("PRINTOBSERVER_CREDENTIAL", credential)
        .output()
        .expect("the command that runs the server runs");
    vec![
        (
            "server on its success path".to_owned(),
            a_second_server(world, credential),
        ),
        (
            "server against a configuration it refuses".to_owned(),
            String::from_utf8_lossy(&refused.stdout).into_owned()
                + &String::from_utf8_lossy(&refused.stderr),
        ),
    ]
}

/// A second supervisor, started and stopped, and everything it said.
fn a_second_server(world: &World, credential: &str) -> String {
    let configuration = world.second_server_config();
    let mut child = Command::new(running::program())
        .arg("server")
        .arg("--config")
        .arg(&configuration)
        .env("PRINTOBSERVER_CREDENTIAL", credential)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("the command that runs the server runs");
    std::thread::sleep(core::time::Duration::from_millis(800));
    let _ = child.kill();
    let said = child.wait_with_output().expect("the server exits");
    let printed =
        String::from_utf8_lossy(&said.stdout).into_owned() + &String::from_utf8_lossy(&said.stderr);
    assert!(
        printed.contains("is serving on"),
        "the second supervisor did not start, so this says nothing about what it prints: \
         {printed}"
    );
    world.wants(Reports::Printing);
    printed
}
