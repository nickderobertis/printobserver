//! End-to-end over the compiled `printobserver` command.
//!
//! This drives the real artifact the way a user does — spawning the built
//! binary as a subprocess and asserting on what it says and what it exits with
//! — rather than calling `main` in-process. What the `server` subcommand does
//! once it starts is `tests/service.rs`, which starts it exactly as the
//! installed unit's own command names it.

use std::process::Command;

/// One invocation of the built program, and everything it said.
fn run(arguments: &[&str]) -> (Option<i32>, String) {
    let output = Command::new(env!("CARGO_BIN_EXE_printobserver"))
        .args(arguments)
        .output()
        .expect("the built `printobserver` binary should be spawnable");
    (
        output.status.code(),
        String::from_utf8_lossy(&output.stdout).into_owned()
            + &String::from_utf8_lossy(&output.stderr),
    )
}

#[test]
fn the_installed_command_runs_and_exits_zero() {
    let (code, said) = run(&[]);

    assert_eq!(code, Some(0), "`printobserver` exited {code:?}: {said}");
    assert!(
        said.contains("server --config"),
        "the command surface does not name the subcommand that runs the supervisor: {said}"
    );
}

/// An invocation this program does not answer to is refused, naming it.
#[test]
fn an_invocation_this_program_does_not_answer_to_is_refused() {
    for (arguments, named) in [
        (vec!["fly"], "fly"),
        (vec!["server"], "configuration"),
        (vec!["server", "--fast"], "--fast"),
        (vec!["server", "--config"], "--config"),
        (
            vec!["server", "--config", "one.toml", "--config", "another.toml"],
            "twice",
        ),
        (vec!["context"], "needs the server it reads from"),
        (vec!["context", "--server"], "--server"),
        (
            vec!["context", "--print", "x"],
            "needs the server it reads from",
        ),
        (vec!["context", "--quickly"], "--quickly"),
        (
            vec![
                "context",
                "--server",
                "ftp://x:1",
                "--print",
                "01a08000-0000-7000-8000-000000000001",
            ],
            "http://",
        ),
        (
            vec![
                "context",
                "--server",
                "http://not-an-address",
                "--print",
                "01a08000-0000-7000-8000-000000000001",
            ],
            "not an address and a port",
        ),
        (
            vec![
                "context",
                "--server",
                "http://127.0.0.1:1",
                "--print",
                "a print\r\nGET /elsewhere",
            ],
            "is not a print this system minted",
        ),
        (
            vec!["context", "--print", "a", "--print", "b", "--server", "s"],
            "twice",
        ),
    ] {
        let (code, said) = run(&arguments);
        assert_eq!(code, Some(1), "`{arguments:?}` was accepted: {said}");
        assert!(
            said.contains(named),
            "`{arguments:?}` was refused without naming `{named}`: {said}"
        );
    }
}

/// A context read of a supervisor that is not there says so, naming where it
/// looked.
#[test]
fn a_context_read_of_a_supervisor_that_is_not_there_says_where_it_looked() {
    // A port is bound to learn one that is free and then released, so what the
    // read meets is a refused connection rather than a served refusal.
    let address = {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("a loopback port");
        listener.local_addr().expect("the bound address")
    };

    let (code, said) = run(&[
        "context",
        "--server",
        &format!("http://{address}"),
        "--print",
        "01a08000-0000-7000-8000-000000000001",
    ]);

    assert_eq!(
        code,
        Some(1),
        "a read of a supervisor that is not there succeeded"
    );
    assert!(
        said.contains(&address.to_string()),
        "the refusal does not name where it looked: {said}"
    );
}

/// A server this program does not speak to is refused where it is named.
#[test]
fn a_server_this_program_does_not_speak_to_is_refused() {
    let (code, said) = run(&[
        "context",
        "--server",
        "https://elsewhere.example",
        "--print",
        "01a08000-0000-7000-8000-000000000001",
    ]);

    assert_eq!(code, Some(1));
    assert!(
        said.contains("https://elsewhere.example"),
        "the refusal does not name the address: {said}"
    );
}

/// An answer that is not one this program can read is refused rather than
/// printed.
///
/// A truncated answer would hand the supervising agent a document that parses
/// as less than the supervisor said, which is the one thing a context read must
/// not do quietly.
#[test]
fn an_answer_this_program_cannot_read_is_refused_rather_than_printed() {
    for (described, answer) in [
        ("an answer with no head at all", "not http at all"),
        (
            "an answer declaring no length",
            "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\n\r\n{}",
        ),
        (
            "an answer declaring more than it sent",
            "HTTP/1.1 200 OK\r\nContent-Length: 400\r\n\r\n{}",
        ),
        (
            "an answer with no status",
            "GARBLED\r\nContent-Length: 2\r\n\r\n{}",
        ),
    ] {
        let address = answering_with(answer);

        let (code, said) = run(&[
            "context",
            "--server",
            &format!("http://{address}"),
            "--print",
            "01a08000-0000-7000-8000-000000000001",
        ]);

        assert_eq!(code, Some(1), "{described} was taken: {said}");
        assert!(
            said.contains("could not read that context"),
            "{described} was refused without saying so: {said}"
        );
    }
}

/// A host answering one fixed thing to every request, for as long as this
/// process runs.
fn answering_with(answer: &'static str) -> std::net::SocketAddr {
    use std::io::{Read as _, Write as _};

    let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("a loopback port");
    let address = listener.local_addr().expect("the bound address");
    std::thread::spawn(move || {
        for stream in listener.incoming().flatten() {
            let mut stream = stream;
            let mut buffer = [0_u8; 1024];
            let _ = stream.read(&mut buffer);
            let _ = stream.write_all(answer.as_bytes());
        }
    });
    address
}

/// A configuration that is not there refuses the start, naming the path.
#[test]
fn a_configuration_that_is_not_there_refuses_the_start() {
    let root = tempfile::TempDir::new().expect("a journey's own root");
    let missing = root.path().join("nowhere.toml");

    let (code, said) = run(&["server", "--config", &missing.display().to_string()]);

    assert_eq!(code, Some(1), "the server started with no configuration");
    assert!(
        said.contains("nowhere.toml"),
        "the refusal does not name the configuration: {said}"
    );
}
