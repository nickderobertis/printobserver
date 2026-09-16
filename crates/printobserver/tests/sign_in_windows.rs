//! Windows `printobserver sign-in`, driven as the real binary.

#![cfg(windows)]

use std::process::Command;

use tempfile::TempDir;

#[test]
fn signing_in_exits_with_the_windows_harness_status() {
    let root = TempDir::new().expect("a journey root");
    let bin = root.path().join("bin");
    let state = root.path().join("state");
    std::fs::create_dir_all(&bin).expect("a stand-in directory");
    std::fs::create_dir_all(&state).expect("a state directory");
    std::fs::write(bin.join("claude.cmd"), "@exit /b 23\r\n").expect("a harness stand-in");
    let config = root.path().join("config.toml");
    let state_toml = state.display().to_string().replace('\\', "\\\\");
    std::fs::write(
        &config,
        format!("state_dir = \"{state_toml}\"\n\n[supervisor]\nharness = \"claude\"\n"),
    )
    .expect("a configuration");

    let path = std::env::join_paths(std::iter::once(bin).chain(std::env::split_paths(
        &std::env::var_os("PATH").unwrap_or_default(),
    )))
    .expect("a Windows PATH");
    let output = Command::new(env!("CARGO_BIN_EXE_printobserver"))
        .args(["sign-in", "--config"])
        .arg(config)
        .env("PATH", path)
        .output()
        .expect("the built program runs");

    assert_eq!(
        output.status.code(),
        Some(23),
        "the command did not preserve the harness exit: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}
