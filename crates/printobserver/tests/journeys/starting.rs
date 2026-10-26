//! A supervisor refusing the configuration it was started with.
//!
//! Each start here is the real `printobserver server`, over the configuration
//! this tier writes for every supervisor, as one journey edits it: a value
//! that cannot be served is refused before anything is served.

use printobserver_types::serde_json::Value;

/// Start `printobserver server` over the configuration this tier writes, as
/// one journey edits it, and answer what it said refusing to start.
///
/// # Panics
///
/// Panics when it started serving instead — said by its announcing where it
/// serves — or had not exited within the deadline, stopping it either way.
#[must_use]
pub fn refused_start(edit: impl FnOnce(&mut Value)) -> String {
    let root = tempfile::TempDir::new().expect("this start's own root");
    let state = root.path().join("state");
    std::fs::create_dir_all(&state).expect("a state directory");
    let machine = crate::world::Printer::StoodIn(crate::machine::Machine::start());
    let mut document =
        crate::world::server_value(&state, &machine, &crate::world::committed_skill());
    edit(&mut document);
    let configuration = root.path().join("server.toml");
    std::fs::write(&configuration, crate::world::toml_of(&document))
        .expect("the configuration is writable");
    let mut server = std::process::Command::new(env!("CARGO_BIN_EXE_printobserver"))
        .arg("server")
        .arg("--config")
        .arg(&configuration)
        .env("PATH", root.path())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .expect("the server command starts");
    let deadline = std::time::Instant::now() + REFUSAL_DEADLINE;
    while server.try_wait().expect("the server's status").is_none() {
        if std::time::Instant::now() > deadline {
            let _ = server.kill();
            let ran = server.wait_with_output().expect("what it said");
            panic!(
                "the server did not refuse its configuration, and was stopped: {}{}",
                String::from_utf8_lossy(&ran.stdout),
                String::from_utf8_lossy(&ran.stderr)
            );
        }
        std::thread::sleep(core::time::Duration::from_millis(200));
    }
    let ran = server.wait_with_output().expect("what it said");
    let said = format!(
        "{}{}",
        String::from_utf8_lossy(&ran.stdout),
        String::from_utf8_lossy(&ran.stderr)
    );
    assert!(
        !ran.status.success(),
        "the server exited successfully over a configuration it should refuse: {said}"
    );
    said
}

/// How long a server refusing its configuration is given to exit.
const REFUSAL_DEADLINE: core::time::Duration = core::time::Duration::from_secs(30);
