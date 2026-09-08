//! Running one of this test binary's own journeys in a process of its own.
//!
//! Two properties here are about a process rather than about a call: that a
//! record survives the process that wrote it, and that a write interrupted part
//! way leaves nothing behind. Neither is provable by reopening a handle, so the
//! journey spawns this same test binary, asks it for one ignored test by name,
//! and reads what the process that ran it left on the filesystem after it ended.

use std::path::Path;
use std::process::{Command, Output};

/// The environment variable a child reads its state directory from.
pub const STATE_DIR: &str = "PRINTOBSERVER_STORE_CHILD_STATE_DIR";

/// Run one ignored test of this binary in a process of its own, and wait.
///
/// # Panics
///
/// Panics when this binary has no path or the child cannot be spawned.
pub fn run(test_name: &str, state_dir: &Path) -> Output {
    let binary = std::env::current_exe().expect("this test binary has a path");
    Command::new(binary)
        .args([test_name, "--exact", "--ignored", "--nocapture"])
        .env(STATE_DIR, state_dir)
        .output()
        .expect("the child process runs")
}

/// The state directory a child was given.
///
/// # Panics
///
/// Panics when the variable is absent, which means the child was run directly.
pub fn state_dir() -> std::path::PathBuf {
    std::env::var(STATE_DIR)
        .unwrap_or_else(|_| panic!("{STATE_DIR} names the state directory a child writes into"))
        .into()
}

/// What a child's output said, for a journey reporting why it failed.
pub fn reported(output: &Output) -> String {
    format!(
        "exit {:?}\nstdout:\n{}\nstderr:\n{}",
        output.status.code(),
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    )
}
