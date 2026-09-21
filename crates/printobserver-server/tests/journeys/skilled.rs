//! The installed skill the configuration names is what the agent reads, and
//! the directory it is installed in is where the agent stands.
//!
//! This program carries no skill: an operator installs the Agent Skill with
//! `gh skill install`, which lays down one directory — `SKILL.md` and the
//! `reference/` documents it links to — and points `supervisor.skill_path` at
//! it. So this installs a copy of the committed skill directory alone, loads
//! the configuration and composes the agent exactly as `Server::start` does,
//! and runs one turn on it with only the paid provider process replaced by the
//! responder this crate ships. What is asserted is read off that process: the
//! directory it ran in, and the system prompt it was handed.

use std::path::{Path, PathBuf};

use printobserver_oneharness::{EnvAssignment, OneharnessSupervisor};
use printobserver_server::{ServerConfig, agent_config};
use printobserver_supervisor_api::{SupervisorPort as _, TurnRequest};
use printobserver_types::serde_json::{Value, json};
use printobserver_types::{
    EventBody, EventId, EventKind, EventRecord, EventSource, PrintId, Timestamp,
};
use tempfile::TempDir;

use crate::world::{committed_skill, document, set, write};

/// Copy one directory tree into another, as regular files.
fn copy_tree(from: &Path, to: &Path) {
    std::fs::create_dir_all(to).expect("the copy's directory is writable");
    for entry in std::fs::read_dir(from).expect("the skill directory is readable") {
        let path = entry.expect("a readable directory entry").path();
        let target = to.join(path.file_name().expect("a file name"));
        if path.is_dir() {
            copy_tree(&path, &target);
        } else {
            std::fs::copy(&path, &target).expect("a skill file is copyable");
        }
    }
}

/// The skill's prose, split off its frontmatter by hand rather than by the
/// function the port uses: everything after the second line that is `---`.
fn prose_of(skill: &str) -> String {
    let rest = skill
        .strip_prefix("---\n")
        .expect("the skill opens with a frontmatter block");
    let closing = rest.find("\n---\n").expect("the frontmatter closes");
    rest[closing + "\n---\n".len()..].to_owned()
}

/// One turn about an event nothing could read, which is enough for the harness
/// to be started.
fn a_turn() -> TurnRequest {
    let print_id = PrintId::new();
    TurnRequest {
        print_id,
        event: EventRecord {
            id: EventId::new(),
            print_id: Some(print_id),
            source: EventSource::new("obico"),
            received_at: Timestamp::now(),
            image: None,
            body: EventBody {
                kind: EventKind::new("malformed_external_event").expect("a kind name"),
                payload: json!({ "detail": "the body was not JSON" }),
            },
            raw: None,
        },
        image_path: None,
        context_command: format!("printobserver context --print-id {print_id}"),
    }
}

/// The same directory, however either side spells it.
fn resolved(path: &Path) -> PathBuf {
    path.canonicalize()
        .unwrap_or_else(|error| panic!("{} resolves: {error}", path.display()))
}

/// A skill installed on its own is the turn's system prompt, and the turn runs
/// in the directory it was installed in.
#[tokio::test(flavor = "multi_thread")]
async fn the_configured_skill_is_the_system_prompt_and_the_turn_runs_beside_it() {
    let root = TempDir::new().expect("a journey's own root");
    // The shape `gh skill install --dir <root>/skills` lays down: the skill's
    // directory, holding nothing but the skill's own files.
    let installed = root.path().join("skills").join("printobserver");
    copy_tree(
        committed_skill()
            .parent()
            .expect("the skill sits in a directory"),
        &installed,
    );
    let skill = installed.join("SKILL.md");

    let mut configuration = document(root.path(), "http://127.0.0.1:9");
    set(
        &mut configuration,
        "supervisor.skill_path",
        toml::Value::String(skill.display().to_string()),
    );
    let loaded = ServerConfig::load(write(root.path(), &configuration))
        .expect("a configuration naming an installed skill is accepted");
    let mut composed =
        agent_config(&loaded).expect("the agent is composed over the installed skill");

    // Only the paid provider process is replaced, and it writes down what it
    // was started with.
    let seen = root.path().join("seen.json");
    composed.harness_bin = Some(PathBuf::from(env!(
        "CARGO_BIN_EXE_printobserver-server-responder"
    )));
    composed.harness_env.extend([
        EnvAssignment::new(&format!(
            "MOCK_STDOUT={}",
            json!({
                "result": json!({
                    "summary": "nothing to read",
                    "confidence": "high",
                    "should_continue": true,
                    "did": "read the event",
                    "why": "the event carried nothing readable",
                    "escalating": false,
                })
                .to_string(),
                "session_id": "SID-SKILLED",
                "thread_id": "SID-SKILLED",
            })
        ))
        .expect("an assignment"),
        EnvAssignment::new(&format!("PRINTOBSERVER_RESPONDER_SEEN={}", seen.display()))
            .expect("an assignment"),
    ]);
    let agent = OneharnessSupervisor::open(composed).expect("the agent opens");
    agent.run_turn(a_turn()).await.expect("the turn runs");

    let recorded: Value = printobserver_types::serde_json::from_str(
        &std::fs::read_to_string(&seen).expect("the harness wrote down what it was started with"),
    )
    .expect("what the harness wrote down is a document");
    let ran_in = PathBuf::from(recorded["cwd"].as_str().unwrap_or_else(|| {
        panic!(
            "the harness recorded no working directory: {}",
            recorded["cwd_error"]
        )
    }));
    assert_eq!(
        resolved(&ran_in),
        resolved(&installed),
        "the harness ran somewhere other than the directory the skill is installed in, \
         so the skill's own links do not resolve from where the agent stands"
    );
    let installed_text = std::fs::read_to_string(&skill).expect("the installed skill reads");
    assert_eq!(
        recorded["system"].as_str(),
        Some(prose_of(&installed_text).as_str()),
        "the harness was not handed the installed skill's prose as its system prompt ({})",
        recorded["system_error"]
    );
    assert!(
        prose_of(&installed_text).starts_with("# Supervising a 3D print\n"),
        "the installed skill's prose does not open on its title"
    );
}
