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
    ] {
        let (code, said) = run(&arguments);
        assert_eq!(code, Some(1), "`{arguments:?}` was accepted: {said}");
        assert!(
            said.contains(named),
            "`{arguments:?}` was refused without naming `{named}`: {said}"
        );
    }
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
