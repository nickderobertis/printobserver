//! End-to-end over the compiled `printobserver` command.
//!
//! This drives the real artifact the way a user does — spawning the built
//! binary as a subprocess and asserting on its exit status — rather than
//! calling `main` in-process. The command carries no behavior yet, so the one
//! journey here is that it runs and exits successfully; the journeys for each
//! subcommand land with the subcommand.

use std::process::Command;

#[test]
fn the_installed_command_runs_and_exits_zero() {
    let output = Command::new(env!("CARGO_BIN_EXE_printobserver"))
        .output()
        .expect("the built `printobserver` binary should be spawnable");

    assert!(
        output.status.success(),
        "`printobserver` exited {:?}\nstdout: {}\nstderr: {}",
        output.status.code(),
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    );
}
