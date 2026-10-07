//! What a turn's harness is permitted to do.
//!
//! A turn reads its print's context, and acts, through this program's own
//! commands, so a Claude Code turn configured with agent commands is given a
//! shell narrowed to them and nothing else. Every other turn keeps
//! `OneHarness`'s read-only mode. The journeys here drive a real turn through
//! the real `OneHarness` and read both the run request this port built and the
//! argument vector `OneHarness` started the harness with.
//!
//! # Why the proof stops at the argument vector
//!
//! What runs an allowed command and refuses any other is Claude Code itself,
//! reading those arguments; this repository's boundary with it is the argument
//! vector, which is what is asserted here. Watching a command allowed and
//! another refused would mean a real model deciding to run each, which is a
//! paid, non-deterministic run no gate can make — the deterministic responder
//! that replaces the provider runs no tools at all. That half was proven on the
//! real printer, where the deployed build's turns ran `printobserver context`
//! and acted under exactly these arguments.

use std::sync::Arc;

use oneharness_core::domain::mode::PermissionMode;
use printobserver_oneharness::{AgentCommand, ConfigError};
use printobserver_supervisor_api::SupervisorPort;
use printobserver_types::{EventBody, PrintId};

use crate::support::{
    Fixture, HARNESS, OTHER_HARNESS, Watch, always, assessment, block_on, config, event,
    generated_assessment_schema, port, schema_read_lock, turn, unreadable,
};

/// An event to hang a turn off.
fn payload() -> EventBody {
    unreadable("the body was not JSON")
}

/// The commands a configuration allows.
fn allowed(commands: &[&str]) -> Vec<AgentCommand> {
    commands
        .iter()
        .map(|command| AgentCommand::new(command).expect("a command prefix"))
        .collect()
}

/// What one turn on `harness`, allowed `commands`, was built with: the run
/// request this port handed `OneHarness`, and the argument vector `OneHarness`
/// started the harness with.
fn one_turn(
    harness: &str,
    commands: Vec<AgentCommand>,
) -> (PermissionMode, Vec<String>, Vec<String>) {
    let schemas = schema_read_lock();
    let fixture = Fixture::new(&format!("permissions-{harness}-{}", commands.len()));
    let watch = Arc::new(Watch::default());
    let mut configured = config(
        &schemas,
        &fixture,
        harness,
        &generated_assessment_schema(),
        always("SID-PERMISSIONS", &assessment("the print is fine", "high")),
    );
    configured.agent_commands = commands;
    let supervisor = port(configured, &watch);

    let print_id = PrintId::new();
    block_on(supervisor.run_turn(
        turn(print_id, event(print_id, payload()), None),
        crate::support::access(),
    ))
    .expect("the turn runs");

    let request = watch
        .requests()
        .into_iter()
        .next()
        .expect("the port built a run request");
    let report = watch
        .reports()
        .into_iter()
        .next()
        .expect("the run reported");
    let started = report.results.first().expect("one result").command.clone();
    (
        request.mode.expect("the request names a mode"),
        request.passthrough,
        started,
    )
}

/// Whether `whole` carries `part` as consecutive arguments.
fn carries(whole: &[String], part: &[&str]) -> bool {
    whole.windows(part.len()).any(|window| window == part)
}

/// An agent command is a prefix a permission rule can name, and nothing that
/// could close the rule early, widen it or spill onto a second line.
#[test]
fn an_agent_command_no_rule_could_name_is_refused() {
    for unnamable in [
        "",
        "   ",
        "printobserver\ncontext",
        "printobserver\rcontext",
        "printobserver context)",
        "printobserver (context",
        "printobserver *",
        "printobserver: context",
    ] {
        let refused = AgentCommand::new(unnamable)
            .expect_err(&format!("`{unnamable:?}` was accepted as an agent command"));
        assert!(
            matches!(refused, ConfigError::AgentCommandInvalid { ref text, .. } if text == unnamable),
            "`{unnamable:?}` was refused as something else: {refused:?}"
        );
        assert!(
            refused
                .to_string()
                .contains("cannot be allowed as an agent command"),
            "the refusal does not say what was refused: {refused}"
        );
    }
    let named = AgentCommand::new("  printobserver context  ").expect("a command prefix");
    assert_eq!(named.as_str(), "printobserver context");
    assert_eq!(named.to_string(), "printobserver context");
}

/// A Claude Code turn with agent commands has a shell, and the shell runs those
/// commands and no other: the argument vector `OneHarness` started the harness
/// with says so, not only the request this port handed it.
#[test]
fn a_claude_code_turn_with_agent_commands_has_a_shell_narrowed_to_them() {
    let (mode, passthrough, started) = one_turn(
        HARNESS,
        allowed(&["printobserver context", "printobserver pause"]),
    );

    assert_eq!(mode, PermissionMode::Default);
    assert_eq!(
        passthrough,
        [
            "--tools",
            "Read",
            "Grep",
            "Glob",
            "Bash",
            "--allowedTools",
            "Read",
            "Grep",
            "Glob",
            "Bash(printobserver context:*)",
            "Bash(printobserver pause:*)",
        ]
    );
    for expected in [
        ["--permission-mode", "dontAsk"].as_slice(),
        ["--tools", "Read", "Grep", "Glob", "Bash"].as_slice(),
        [
            "--allowedTools",
            "Read",
            "Grep",
            "Glob",
            "Bash(printobserver context:*)",
            "Bash(printobserver pause:*)",
        ]
        .as_slice(),
    ] {
        assert!(
            carries(&started, expected),
            "the harness was not started with {expected:?}: {started:?}"
        );
    }
    assert!(
        !started
            .iter()
            .any(|argument| argument == "bypassPermissions"),
        "a turn with a shell was started with every permission granted: {started:?}"
    );
}

/// With no agent commands, or on a harness the rules are not written for, a
/// turn keeps `OneHarness`'s read-only mode and is handed no shell.
#[test]
fn a_turn_without_rules_for_its_harness_stays_read_only() {
    for (harness, commands) in [
        (HARNESS, Vec::new()),
        (OTHER_HARNESS, allowed(&["printobserver context"])),
    ] {
        let (mode, passthrough, started) = one_turn(harness, commands);

        assert_eq!(mode, PermissionMode::ReadOnly, "{harness}");
        assert!(
            passthrough.is_empty(),
            "{harness} was handed arguments of its own: {passthrough:?}"
        );
        assert!(
            !started.iter().any(|argument| argument.starts_with("Bash")),
            "{harness} was started with a shell: {started:?}"
        );
    }
}
