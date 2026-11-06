//! The real-print replay's pass-through in front of the harness, as a process.
//!
//! It is installed beside a `launch.json` naming what to start and whom to tell,
//! so each journey copies it into a directory of its own. What it starts is the
//! published responder, and what it tells is a real socket here.

use std::io::{Read as _, Write as _};
use std::net::TcpListener;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::sync::mpsc;

use printobserver_types::serde_json::json;
use tempfile::TempDir;

/// A control endpoint answering `/started` with where to keep the output, and
/// handing each path it was told over the channel, in order.
fn control(record: &Path) -> (String, mpsc::Receiver<String>) {
    let listener = TcpListener::bind("127.0.0.1:0").expect("a loopback port");
    let address = listener.local_addr().expect("its address").to_string();
    let record = record.display().to_string();
    let (told, heard) = mpsc::channel();
    std::thread::spawn(move || {
        for connection in listener.incoming() {
            let mut connection = connection.expect("a connection");
            let mut request = [0_u8; 1024];
            let read = connection.read(&mut request).expect("a request");
            let text = String::from_utf8_lossy(&request[..read]).into_owned();
            let path = text.split_whitespace().nth(1).unwrap_or("").to_owned();
            let body = if path == "/started" {
                record.clone()
            } else {
                String::new()
            };
            connection
                .write_all(
                    format!(
                        "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                        body.len()
                    )
                    .as_bytes(),
                )
                .expect("an answer");
            told.send(path).expect("a reader");
        }
    });
    (address, heard)
}

/// The launcher, copied where it reads its launch file from, and that file.
fn installed(directory: &Path, control: &str, program: &Path) -> PathBuf {
    let launcher = directory.join(format!("claude{}", std::env::consts::EXE_SUFFIX));
    std::fs::copy(
        env!("CARGO_BIN_EXE_printobserver-server-turn-launcher"),
        &launcher,
    )
    .expect("the launcher copies");
    std::fs::write(
        directory.join("launch.json"),
        json!({ "control": control, "program": program }).to_string(),
    )
    .expect("the launch file writes");
    launcher
}

/// Run the launcher with one argument, the published responder scripted by `env`.
fn launched(launcher: &Path, env: &[(&str, &str)]) -> Output {
    let mut command = Command::new(launcher);
    command.arg("--a-harness-argument").stdin(Stdio::null());
    for (name, value) in env {
        command.env(name, value);
    }
    command.output().expect("the launcher starts")
}

fn responder() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_printobserver-server-responder"))
}

/// What the harness prints reaches the caller and the record unchanged, its exit
/// is the launcher's, and the exit is reported before the launcher itself exits.
#[test]
fn the_harness_runs_exactly_as_it_was_asked_and_its_output_is_kept() {
    let scratch = TempDir::new().expect("a directory");
    let record = scratch.path().join("record");
    let (address, heard) = control(&record);
    let launcher = installed(scratch.path(), &address, &responder());
    let argv = scratch.path().join("argv");
    let answer = "{\"result\":\"an answer\"}";
    let output = launched(
        &launcher,
        &[
            ("MOCK_STDOUT", answer),
            ("MOCK_EXIT", "3"),
            ("MOCK_ARGV_FILE", argv.to_str().expect("a text path")),
            ("MOCK_PRESERVE_STDOUT", "1"),
        ],
    );
    let said = String::from_utf8_lossy(&output.stderr);
    assert_eq!(output.status.code(), Some(3), "{said}");
    assert_eq!(String::from_utf8_lossy(&output.stdout), answer);
    assert_eq!(std::fs::read_to_string(&record).expect("kept"), answer);
    let handed = std::fs::read_to_string(&argv).expect("the responder wrote its arguments");
    assert!(handed.contains("--a-harness-argument"), "{handed}");
    assert_eq!(heard.recv().expect("a report"), "/started");
    assert_eq!(heard.recv().expect("a report"), "/exited");
}

/// With no launch file nothing is started, and the failure says why.
#[test]
fn a_launcher_with_no_launch_file_says_so() {
    let scratch = TempDir::new().expect("a directory");
    let launcher = scratch
        .path()
        .join(format!("claude{}", std::env::consts::EXE_SUFFIX));
    std::fs::copy(
        env!("CARGO_BIN_EXE_printobserver-server-turn-launcher"),
        &launcher,
    )
    .expect("the launcher copies");
    let output = launched(&launcher, &[]);
    assert_eq!(output.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&output.stderr).contains("launch.json is unreadable"));
}

/// A launch file that names no program, or a target that cannot start, fails with
/// the reason rather than passing for a harness that answered nothing.
#[test]
fn a_target_that_cannot_start_is_a_failure_naming_it() {
    let scratch = TempDir::new().expect("a directory");
    let record = scratch.path().join("record");
    let (address, _heard) = control(&record);
    let missing = scratch.path().join("no-such-program");
    let launcher = installed(scratch.path(), &address, &missing);
    let output = launched(&launcher, &[]);
    assert_eq!(output.status.code(), Some(1));
    let said = String::from_utf8_lossy(&output.stderr);
    assert!(said.contains("could not be started"), "{said}");

    std::fs::write(
        scratch.path().join("launch.json"),
        json!({ "control": address, "program": "x", "target": 3 }).to_string(),
    )
    .expect("the launch file writes");
    let output = launched(&launcher, &[]);
    assert_eq!(output.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&output.stderr).contains("is no launch file"));
}

/// A control endpoint answering anything but success is refused by name.
#[test]
fn a_refused_report_is_a_failure() {
    let scratch = TempDir::new().expect("a directory");
    let listener = TcpListener::bind("127.0.0.1:0").expect("a loopback port");
    let address = listener.local_addr().expect("its address").to_string();
    std::thread::spawn(move || {
        let (mut connection, _) = listener.accept().expect("a connection");
        let mut request = [0_u8; 1024];
        let _ = connection.read(&mut request);
        let _ = connection.write_all(b"HTTP/1.1 500 No\r\nContent-Length: 0\r\n\r\n");
    });
    let launcher = installed(scratch.path(), &address, &responder());
    let output = launched(&launcher, &[]);
    assert_eq!(output.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&output.stderr).contains("refused /started"));
}

/// A control address nothing answers at stops the run before it starts anything.
#[test]
fn a_control_address_nothing_answers_at_starts_nothing() {
    let scratch = TempDir::new().expect("a directory");
    let closed = TcpListener::bind("127.0.0.1:0")
        .expect("a loopback port")
        .local_addr()
        .expect("its address")
        .to_string();
    let launcher = installed(scratch.path(), &closed, &responder());
    let output = launched(&launcher, &[]);
    assert_eq!(output.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&output.stderr).contains("did not answer"));
}
