//! Which configuration a command reads when nothing names one, with and
//! without a per-user install, driven as the real binary.
//!
//! With the per-user configuration present, sign-in and client commands read
//! it with no --config. Without it, both fall back to the isolated default.
//!
//! What a command read is observed from outside it: `sign-in`'s refusal names
//! the file it read, and a client command connects to the address that file
//! names, which a listener of this test's own accepts.

use std::net::TcpListener;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::mpsc;
use std::time::Duration;

use printobserver::failure::Exit;
use tempfile::TempDir;

/// Every variable any platform reads a configuration home or a per-user
/// install from: each is set by this test or taken away.
const HOME_VARIABLES: [&str; 4] = ["XDG_CONFIG_HOME", "HOME", "APPDATA", "LOCALAPPDATA"];

/// A harness no table names, so `sign-in` refuses it naming the file it read
/// before anything runs.
const UNKNOWN_HARNESS: &str = "a-harness-nothing-signs-in";

/// One run of the program with `local` as `LOCALAPPDATA` and every
/// configuration home pointed at an empty directory of the test's own.
fn run(root: &Path, local: &Path, arguments: &[&str]) -> Output {
    let homes = root.join("homes");
    std::fs::create_dir_all(&homes).expect("a configuration home of the test's own");
    let mut command = Command::new(env!("CARGO_BIN_EXE_printobserver"));
    command
        .args(arguments)
        .env_remove("PRINTOBSERVER_SERVER")
        .env_remove("PRINTOBSERVER_CREDENTIAL");
    for name in HOME_VARIABLES {
        command.env_remove(name);
    }
    for name in ["XDG_CONFIG_HOME", "HOME", "APPDATA"] {
        command.env(name, &homes);
    }
    command.env("LOCALAPPDATA", local);
    command.output().expect("the program runs")
}

/// Everything a run printed, on either stream.
fn said(output: &Output) -> String {
    String::from_utf8_lossy(&output.stdout).into_owned() + &String::from_utf8_lossy(&output.stderr)
}

/// Where a per-user install under `local` keeps its configuration.
fn per_user_config(local: &Path) -> PathBuf {
    let base = if cfg!(windows) {
        local.to_path_buf()
    } else if cfg!(target_os = "macos") {
        local
            .parent()
            .expect("root")
            .join("homes/Library/Application Support")
    } else {
        local.parent().expect("root").join("homes")
    };
    base.join("printobserver").join("config.toml")
}

/// A per-user configuration under `local`: its state directory, the harness
/// `sign-in` reads, and the address the server listens on.
fn write_per_user(local: &Path, listen: &str) -> PathBuf {
    let path = per_user_config(local);
    std::fs::create_dir_all(path.parent().expect("a parent")).expect("a configuration directory");
    let state = local.join("printobserver").join("state");
    std::fs::write(
        &path,
        format!(
            "state_dir = '{}'\nlisten = \"{listen}\"\n\n[supervisor]\nharness = \"{UNKNOWN_HARNESS}\"\n",
            state.display()
        ),
    )
    .expect("the per-user configuration is written");
    path
}

/// A listener that reports whether anything connected within a bound.
fn listening() -> (String, mpsc::Receiver<()>) {
    let listener = TcpListener::bind("127.0.0.1:0").expect("a port of the test's own");
    let address = listener.local_addr().expect("a bound address").to_string();
    let (sender, received) = mpsc::channel();
    std::thread::spawn(move || {
        if listener.accept().is_ok() {
            let _ = sender.send(());
        }
    });
    (address, received)
}

/// Sign-in reads the per-user configuration with no --config.
#[test]
fn sign_in_reads_the_per_user_configuration_where_the_platform_has_one() {
    let root = TempDir::new().expect("a test's own root");
    let local = root.path().join("Local");
    let written = write_per_user(&local, "127.0.0.1:9");

    let output = run(root.path(), &local, &["sign-in"]);

    assert_eq!(
        output.status.code(),
        Some(i32::from(Exit::Unconfigured.status())),
        "{}",
        said(&output)
    );
    assert!(
        said(&output).contains(&format!(
            "`supervisor.harness` in {} is `{UNKNOWN_HARNESS}`",
            written.display()
        )),
        "sign-in did not read the per-user configuration: {}",
        said(&output)
    );
}

/// With no per-user configuration, `sign-in` reads the service's, as before.
#[test]
fn sign_in_falls_back_to_the_services_configuration_without_a_per_user_one() {
    let root = TempDir::new().expect("a test's own root");
    let local = root.path().join("Local");
    std::fs::create_dir_all(&local).expect("an empty local application data");

    let output = run(root.path(), &local, &["sign-in"]);

    assert_eq!(
        output.status.code(),
        Some(i32::from(Exit::Unconfigured.status())),
        "{}",
        said(&output)
    );
    assert!(
        said(&output).contains(printobserver::DEFAULT_CONFIG_PATH),
        "sign-in did not name the service's configuration: {}",
        said(&output)
    );
}

/// A client command with nothing else configured reaches the
/// server the per-user configuration listens on.
#[test]
fn a_client_command_reaches_the_server_the_per_user_configuration_names() {
    let root = TempDir::new().expect("a test's own root");
    let local = root.path().join("Local");
    let (address, connected) = listening();
    write_per_user(&local, &address);

    let output = run(root.path(), &local, &["prints"]);

    assert!(
        connected.recv_timeout(Duration::from_secs(60)).is_ok(),
        "the client did not connect to {address}, which the per-user configuration names: {}",
        said(&output)
    );
}

/// Without a per-user file, the client falls back to the service default.
#[test]
fn a_client_command_without_a_per_user_configuration_falls_back_as_before() {
    let root = TempDir::new().expect("a test's own root");
    let local = root.path().join("Local");
    let (address, connected) = listening();
    std::fs::create_dir_all(&local).expect("an empty local application data");

    let output = run(root.path(), &local, &["prints"]);

    assert!(
        connected.recv_timeout(Duration::from_millis(500)).is_err(),
        "the client reached {address} with no per-user install to name it: {}",
        said(&output)
    );
    if !Path::new(printobserver::DEFAULT_CONFIG_PATH).exists() {
        assert_eq!(
            output.status.code(),
            Some(i32::from(Exit::Unconfigured.status())),
            "{}",
            said(&output)
        );
    }
}

/// `credential issue` names the configuration its line goes into: the
/// per-user one where the platform has one and it is there, the service's
/// otherwise.
#[test]
fn credential_issue_names_the_per_user_configuration_where_the_platform_has_one() {
    let root = TempDir::new().expect("a test's own root");
    let local = root.path().join("Local");
    let written = write_per_user(&local, "127.0.0.1:9");

    let output = run(root.path(), &local, &["credential", "issue"]);

    assert!(output.status.success(), "{}", said(&output));
    let named = written.display().to_string();
    assert!(
        said(&output).contains(&format!("the server's configuration, {named},")),
        "credential issue did not name {named}: {}",
        said(&output)
    );
}
