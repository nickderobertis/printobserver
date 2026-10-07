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
use std::path::Path;
use std::process::{Command, Stdio};

use printobserver::failure::Exit;

use crate::announced;
use crate::machine::Reports;
use crate::walk;
use crate::world::{CREDENTIAL, OTHER_CREDENTIAL, World};

use super::{failures, running};

/// How long a fragment has to be to be looked for.
const FRAGMENT: usize = 4;

/// How long a credential `credential issue` draws is, as it is written: the
/// unpadded URL-safe base64 of the bytes it draws.
const GENERATED_LENGTH: usize = (printobserver_server::GENERATED_CREDENTIAL_BYTES * 4).div_ceil(3);

/// The variables a configuration home is read from.
const CONFIG_HOME_VARIABLES: [&str; 3] = ["XDG_CONFIG_HOME", "HOME", "APPDATA"];

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
    the_issued_credential_is_printed_by_nothing(world);
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
        let one = walk::about_now(&one, world);
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

/// One run of this program with its configuration home at `home`, its
/// standard input `input`, and `PRINTOBSERVER_SERVER` naming `server`.
fn at_home(home: &Path, arguments: &[&str], server: &str, input: &[u8]) -> std::process::Output {
    use std::io::Write as _;

    let mut command = Command::new(running::program());
    command
        .args(arguments)
        .env_remove("PRINTOBSERVER_CREDENTIAL")
        .env("PRINTOBSERVER_SERVER", server)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    for name in CONFIG_HOME_VARIABLES {
        command.env(name, home);
    }
    let mut child = command.spawn().expect("the program runs");
    child
        .stdin
        .take()
        .expect("standard input is piped")
        .write_all(input)
        .expect("standard input is written");
    child.wait_with_output().expect("the program exits")
}

/// What a run printed, on either stream.
fn printed_by(output: &std::process::Output) -> String {
    String::from_utf8_lossy(&output.stdout).into_owned() + &String::from_utf8_lossy(&output.stderr)
}

/// The operator's credential, issued by `credential issue` and put in force by
/// the verifier it printed.
///
/// Of the shipped length and alphabet, so the search is for exactly what an
/// operator holds. Issuing it, computing its verifier again from standard
/// input, the server's startup line, a refusal the server makes after its
/// verifier is settled, a client command reading the operator's own
/// configuration, and every file the server was configured by or wrote all go
/// without it — while each still says what it is required to — and the one
/// file that carries it is the operator's own client configuration, which is
/// private.
fn the_issued_credential_is_printed_by_nothing(world: &World) {
    let home = world.root.path().join("issuing-home");
    std::fs::create_dir_all(&home).expect("the operator's own home");
    let listen = {
        let free = std::net::TcpListener::bind("127.0.0.1:0").expect("a loopback port");
        free.local_addr().expect("the bound address").to_string()
    };
    let server = format!("http://{listen}");

    let (line, credential, client_config) = issued_at(&home, &server);

    let (configuration, state) = world.unverified_server_config("issued", &listen);
    let held = std::fs::read_to_string(&configuration).expect("the configuration reads");
    std::fs::write(&configuration, format!("{line}\n{held}"))
        .expect("the configuration is writable");
    let mut serving = Command::new(running::program())
        .arg("server")
        .arg("--config")
        .arg(&configuration)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("the command that runs the server runs");
    let (_, stream) = announced::serving(&mut serving, "the issued supervisor did not start");

    let read = at_home(
        &home,
        &["status", "--print-id", &world.print_id()],
        &server,
        b"",
    );
    let said = printed_by(&read);
    assert!(
        said.contains("there is no print") || said.contains(&world.print_id()),
        "a read through the operator's own configuration was not served: {said}"
    );
    assert_ne!(
        read.status.code(),
        Some(i32::from(Exit::Unconfigured.status())),
        "a read through the operator's own configuration was not admitted: {said}"
    );
    assert!(
        !said.contains(&credential),
        "a read through the operator's own configuration printed the credential"
    );

    let _ = serving.kill();
    let finished = serving.wait_with_output().expect("the server exits");
    let mut printed = stream.collected();
    printed.push_str(&String::from_utf8_lossy(&finished.stdout));
    assert!(
        !printed.contains(&credential),
        "the server printed the operator's credential"
    );

    let refusing = a_refusal_quotes_nothing(world, &line, &credential);

    for file in [configuration.as_path(), refusing.as_path()] {
        assert!(
            !std::fs::read_to_string(file)
                .expect("the configuration reads")
                .contains(&credential),
            "{} carries the operator's credential",
            file.display()
        );
    }
    for written in files_under(&state) {
        assert!(
            !std::fs::read(&written)
                .unwrap_or_default()
                .windows(credential.len())
                .any(|window| window == credential.as_bytes()),
            "{} carries the operator's credential",
            written.display()
        );
    }
    assert_private(&client_config, &credential);
    world.wants(Reports::Printing);
}

/// Issue a credential into `home`, naming `server`, and hold `credential
/// issue` and `credential verifier` to printing its verifier and never it:
/// the verifier line, the credential as the operator reads it back, and the
/// file it was written to.
fn issued_at(home: &Path, server: &str) -> (String, String, std::path::PathBuf) {
    let issued = at_home(home, &["credential", "issue"], server, b"");
    assert!(issued.status.success(), "{}", printed_by(&issued));
    let line = String::from_utf8_lossy(&issued.stdout)
        .lines()
        .find(|line| line.starts_with("api.credential_verifier = "))
        .expect("`credential issue` printed the verifier line")
        .to_owned();
    let client_config = issued_configuration(home);
    let written: toml::Value = toml::from_str(
        &std::fs::read_to_string(&client_config).expect("the issued configuration reads"),
    )
    .expect("the issued configuration is a document");
    let credential = written["client"]["credential"]
        .as_str()
        .expect("the issued configuration carries the credential")
        .to_owned();
    assert_eq!(written["client"]["server"].as_str(), Some(server));
    assert_eq!(
        credential.len(),
        GENERATED_LENGTH,
        "the issued credential is not of the shipped length"
    );
    assert!(
        credential
            .chars()
            .all(|character| character.is_ascii_alphanumeric() || "-_".contains(character)),
        "the issued credential is not of the shipped alphabet"
    );
    assert!(
        !printed_by(&issued).contains(&credential),
        "`credential issue` printed the credential it issued"
    );
    let again = at_home(
        home,
        &["credential", "verifier"],
        server,
        format!("{credential}\n").as_bytes(),
    );
    assert!(again.status.success(), "{}", printed_by(&again));
    assert!(
        String::from_utf8_lossy(&again.stdout)
            .lines()
            .any(|said| said == line),
        "`credential verifier` printed another verifier than `credential issue`: {}",
        printed_by(&again)
    );
    assert!(
        !printed_by(&again).contains(&credential),
        "`credential verifier` echoed the credential it was handed"
    );
    (line, credential, client_config)
}

/// A refusal the server makes once its verifier is settled quotes nothing of
/// the credential: answers the configuration it refused to start under.
fn a_refusal_quotes_nothing(world: &World, line: &str, credential: &str) -> std::path::PathBuf {
    // A refusal made once the verifier is settled: the address to listen on is
    // already taken, and the verifier was read before the listener was.
    let taken = std::net::TcpListener::bind("127.0.0.1:0").expect("a loopback port");
    let occupied = taken.local_addr().expect("the bound address").to_string();
    let (refusing, _) = world.unverified_server_config("issued-refusing", &occupied);
    let held = std::fs::read_to_string(&refusing).expect("the configuration reads");
    std::fs::write(&refusing, format!("{line}\n{held}")).expect("the configuration is writable");
    let refused = Command::new(running::program())
        .arg("server")
        .arg("--config")
        .arg(&refusing)
        .output()
        .expect("the command that runs the server runs");
    let said = printed_by(&refused);
    assert!(
        said.contains("will not start") && said.contains(&occupied),
        "the refusal did not say why the server will not start: {said}"
    );
    assert!(
        !said.contains(credential),
        "the server's refusal printed the operator's credential"
    );
    refusing
}

/// Where `credential issue` writes the operator's configuration under one
/// configuration home, on this platform.
fn issued_configuration(home: &Path) -> std::path::PathBuf {
    let base = if cfg!(target_os = "macos") {
        home.join("Library").join("Application Support")
    } else {
        home.to_path_buf()
    };
    base.join("printobserver").join("client.toml")
}

/// Every file under a directory.
fn files_under(root: &Path) -> Vec<std::path::PathBuf> {
    let mut found = Vec::new();
    for entry in std::fs::read_dir(root).into_iter().flatten().flatten() {
        let path = entry.path();
        if path.is_dir() {
            found.extend(files_under(&path));
        } else {
            found.push(path);
        }
    }
    found
}

/// The operator's own client configuration is the one file carrying the
/// credential, and it is the operator's alone.
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
        let one = walk::about_now(&one, world);
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
            world.wants(failures::rejected_from(&one));
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
    said.push((
        "the supervisor the walk ran against".to_owned(),
        world.said_so_far(),
    ));
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
    let (_, stream) = announced::serving(
        &mut child,
        "the second supervisor did not start, so this says nothing about what it prints",
    );
    let _ = child.kill();
    let said = child.wait_with_output().expect("the server exits");
    let mut printed = stream.collected();
    printed.push_str(&String::from_utf8_lossy(&said.stdout));
    world.wants(Reports::Printing);
    printed
}
