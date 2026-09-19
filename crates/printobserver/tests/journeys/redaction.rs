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
use std::io::{BufRead as _, BufReader, Read as _};
use std::path::Path;
use std::process::{Command, Stdio};

use printobserver::failure::Exit;
use printobserver_server::{API_CREDENTIAL_FILE, CLIENT_CONFIG_FILE};

use crate::machine::Reports;
use crate::walk;
use crate::world::{CREDENTIAL, OTHER_CREDENTIAL, World};

use super::{failures, running};

/// How long a fragment has to be to be looked for.
const FRAGMENT: usize = 4;

/// How long a credential the server generates is, as it is written: the
/// unpadded URL-safe base64 of the bytes it draws.
const GENERATED_LENGTH: usize = (printobserver_server::GENERATED_CREDENTIAL_BYTES * 4).div_ceil(3);

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
            assert!(
                !said.1.contains(OTHER_CREDENTIAL),
                "`{}` printed the control credential it was configured with",
                said.0
            );
        }
    }
    a_refused_credential_is_named_and_neither_credential_is_printed(world);
    the_server_command_under_a_credential_it_generated(world);
}

/// A credential the supervisor refuses is reported by where it is read from.
///
/// The supervisor serves under the walk's own credential, so every command
/// configured with the control credential is refused — and what each prints is
/// required to say where the credential is read from while printing neither
/// the credential it presented nor the one in force.
fn a_refused_credential_is_named_and_neither_credential_is_printed(world: &World) {
    for one in walk::walk(world) {
        world.wants(one.reports);
        let ran = running::configured(world, &failures::succeeding(&one), OTHER_CREDENTIAL);
        let said = ran.said();
        assert_eq!(
            ran.code,
            Some(i32::from(Exit::Unconfigured.status())),
            "`{}` under a refused credential did not exit as `unconfigured`: {said}",
            one.command.name
        );
        assert!(
            said.contains("refused the credential this program presented")
                && said.contains("`credential` in the `[client]` table")
                && said.contains("PRINTOBSERVER_CREDENTIAL"),
            "`{}` under a refused credential did not say where the credential is read from: \
             {said}",
            one.command.name
        );
        for credential in [CREDENTIAL, OTHER_CREDENTIAL] {
            assert!(
                !said.contains(credential),
                "`{}` under a refused credential printed a credential: {said}",
                one.command.name
            );
        }
    }
}

/// The command that runs the server, under a credential it generated itself.
///
/// Of the shipped length and alphabet, so the search is for exactly what an
/// installed service holds. Its startup line, a refusal it makes after the
/// credential is settled, a client command reading the configuration it wrote,
/// and the configuration it was started under all go without it — while each
/// still says what it is required to — and the one file that carries it is the
/// client configuration, which is private.
fn the_server_command_under_a_credential_it_generated(world: &World) {
    let (configuration, state) = world.generating_server_config("generating", "127.0.0.1:0");
    let mut serving = Command::new(running::program())
        .arg("server")
        .arg("--config")
        .arg(&configuration)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("the command that runs the server runs");
    let mut printed = String::new();
    let mut lines = BufReader::new(serving.stderr.take().expect("the server's own output"));
    while !printed.contains("is serving on") {
        let mut line = String::new();
        if lines.read_line(&mut line).expect("the output reads") == 0 {
            break;
        }
        printed.push_str(&line);
    }
    assert!(
        printed.contains("is serving on"),
        "the generating supervisor did not start: {printed}"
    );

    let credential = std::fs::read_to_string(state.join(API_CREDENTIAL_FILE))
        .expect("the server wrote the credential it generated");
    assert_eq!(
        credential.len(),
        GENERATED_LENGTH,
        "the generated credential is not of the shipped length"
    );
    assert!(
        credential
            .chars()
            .all(|character| character.is_ascii_alphanumeric() || "-_".contains(character)),
        "the generated credential is not of the shipped alphabet"
    );

    let client_config = state.join(CLIENT_CONFIG_FILE);
    let read = running::with(
        world,
        &[
            "status".to_owned(),
            "--print-id".to_owned(),
            world.print_id.clone(),
            "--config".to_owned(),
            client_config.display().to_string(),
        ],
        &[],
    );
    assert!(
        read.said().contains(&world.print_id),
        "a read through the configuration the server wrote said nothing about the print it \
         asked for: {}",
        read.said()
    );
    assert!(
        !read.said().contains(&credential),
        "a read through the configuration the server wrote printed the credential"
    );

    let _ = serving.kill();
    let _ = lines.read_to_string(&mut printed);
    let finished = serving.wait_with_output().expect("the server exits");
    printed.push_str(&String::from_utf8_lossy(&finished.stdout));
    assert!(
        !printed.contains(&credential),
        "the server printed the credential it generated"
    );

    // A refusal made once the credential is settled: the address to listen on
    // is already taken, and the credential was read before the listener was.
    let taken = std::net::TcpListener::bind("127.0.0.1:0").expect("a loopback port");
    let occupied = taken.local_addr().expect("the bound address").to_string();
    let (refusing, _) = world.generating_server_config("generating", &occupied);
    let refused = Command::new(running::program())
        .arg("server")
        .arg("--config")
        .arg(&refusing)
        .output()
        .expect("the command that runs the server runs");
    let said = String::from_utf8_lossy(&refused.stdout).into_owned()
        + &String::from_utf8_lossy(&refused.stderr);
    assert!(
        said.contains("will not start") && said.contains(&occupied),
        "the refusal did not say why the server will not start: {said}"
    );
    assert!(
        !said.contains(&credential),
        "the server's refusal printed the credential it generated"
    );

    for file in [configuration.as_path(), refusing.as_path()] {
        assert!(
            !std::fs::read_to_string(file)
                .expect("the configuration reads")
                .contains(&credential),
            "{} carries the credential the server generated",
            file.display()
        );
    }
    assert_private(&client_config, &credential);
    world.wants(Reports::Printing);
}

/// The client configuration is the one file carrying the credential, and it is
/// the service's own user's alone.
///
/// Its mode is asserted on Unix alone: Windows has none, and a file there
/// carries the access its directory grants.
fn assert_private(client_config: &Path, credential: &str) {
    assert!(
        std::fs::read_to_string(client_config)
            .expect("the client configuration reads")
            .contains(credential),
        "the client configuration does not carry the credential in force"
    );
    #[cfg(unix)]
    assert_eq!(
        std::os::unix::fs::PermissionsExt::mode(
            &std::fs::metadata(client_config)
                .expect("the client configuration is there")
                .permissions()
        ) & 0o777,
        0o600,
        "the client configuration carrying the credential is readable by others"
    );
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
///
/// Stopped once it has said it is serving rather than after a fixed pause: a
/// loaded runner has taken longer than any pause to bring a coverage-
/// instrumented program to its first line, and a supervisor killed before it
/// printed anything says nothing about what it prints.
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
    let mut printed = String::new();
    let mut lines = BufReader::new(child.stderr.take().expect("the server's own output"));
    while !printed.contains("is serving on") {
        let mut line = String::new();
        if lines.read_line(&mut line).expect("the output reads") == 0 {
            break;
        }
        printed.push_str(&line);
    }
    assert!(
        printed.contains("is serving on"),
        "the second supervisor did not start, so this says nothing about what it prints: \
         {printed}"
    );
    let _ = child.kill();
    let said = child.wait_with_output().expect("the server exits");
    lines
        .read_to_string(&mut printed)
        .expect("the rest of the output reads");
    printed.push_str(&String::from_utf8_lossy(&said.stdout));
    world.wants(Reports::Printing);
    printed
}
