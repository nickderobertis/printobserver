//! A configuration that cannot be valid, and the run request a valid one makes.
//!
//! Every field a caller could get wrong carries a type whose only constructor
//! refuses the wrong value, so the journeys here drive those constructors and
//! then drive a real turn: what the port hands `OneHarness` is what the
//! constrained values said, rather than strings this port re-checked on the way
//! past.

use std::sync::Arc;

use std::fs;

use oneharness_core::domain::mode::PermissionMode;
use printobserver_oneharness::{
    AgentCommand, AssessmentSchema, ConfigError, EnvAssignment, HarnessIdentity, ModelName,
    TurnTimeout,
};
use printobserver_supervisor_api::SupervisorPort;
use printobserver_types::{EventBody, PrintId};

use crate::support::{
    Fixture, HARNESS, OTHER_HARNESS, Watch, always, assessment, assignment, block_on, config,
    event, generated_assessment_schema, port, schema, schema_read_lock, turn, unreadable,
};

/// An event to hang a turn off.
fn payload() -> EventBody {
    unreadable("the body was not JSON")
}

/// A harness identity is something `OneHarness` could select.
#[test]
fn an_empty_harness_identity_is_not_an_identity() {
    for empty in ["", "   ", "\n\t"] {
        assert_eq!(
            HarnessIdentity::new(empty),
            Err(ConfigError::HarnessIdentityEmpty),
            "`{empty:?}` was accepted as a harness identity"
        );
    }
    let named = HarnessIdentity::new("  claude-code  ").expect("a named identity");
    assert_eq!(named.as_str(), "claude-code");
    assert_eq!(named.to_string(), "claude-code");
}

/// A turn timeout bounds a turn, so zero seconds is not one.
#[test]
fn a_turn_timeout_of_zero_seconds_is_not_a_bound() {
    assert_eq!(TurnTimeout::new(0), Err(ConfigError::TurnTimeoutZero));
    assert_eq!(
        TurnTimeout::new(45).expect("a bound").seconds(),
        45,
        "a bound did not answer the seconds it was made from"
    );
    assert_eq!(TurnTimeout::DEFAULT.to_string(), "900s");
}

/// An environment assignment is a name and a value, or it is nothing.
#[test]
fn an_assignment_with_no_key_equals_value_shape_is_refused() {
    for malformed in ["MOCK_STDOUT", "", "=value", "1MOCK=value", "a b=value"] {
        let refused = EnvAssignment::new(malformed)
            .expect_err(&format!("`{malformed}` was accepted as an assignment"));
        assert!(
            matches!(refused, ConfigError::EnvAssignmentMalformed { .. }),
            "`{malformed}` was refused as something else: {refused:?}"
        );
        assert!(
            refused.to_string().contains(malformed),
            "the refusal does not name what was offered: {refused}"
        );
    }

    // A value carrying its own `=` is ordinary — the responder is scripted with
    // JSON — and all of it stays in the value.
    let json = EnvAssignment::new(r#"MOCK_STDOUT={"a":"b=c"}"#).expect("an assignment");
    assert_eq!(json.name(), "MOCK_STDOUT");
    assert_eq!(json.value(), r#"{"a":"b=c"}"#);
    assert_eq!(json.to_string(), r#"MOCK_STDOUT={"a":"b=c"}"#);
}

/// A model nobody pinned is the absence of a name rather than an empty one.
#[test]
fn a_model_named_as_nothing_is_not_a_pin() {
    for empty in ["", "  "] {
        assert_eq!(
            ModelName::new(empty),
            Err(ConfigError::ModelNameEmpty),
            "`{empty:?}` was accepted as a model"
        );
    }
    assert_eq!(
        ModelName::new(" claude-opus-5 ")
            .expect("a pinned model")
            .as_str(),
        "claude-opus-5"
    );
}

/// An agent command is a prefix a permission rule can name, and nothing that
/// could close the rule early, widen it or spill onto a second line.
#[test]
fn an_agent_command_no_rule_could_name_is_refused() {
    for unnamable in [
        "",
        "   ",
        "printobserver\ncontext",
        "printobserver context)",
        "printobserver (context",
        "printobserver *",
        "printobserver: context",
    ] {
        let refused = AgentCommand::new(unnamable)
            .expect_err(&format!("`{unnamable:?}` was accepted as an agent command"));
        assert!(
            matches!(refused, ConfigError::AgentCommandInvalid { .. }),
            "`{unnamable:?}` was refused as something else: {refused:?}"
        );
        assert!(
            refused.to_string().contains("cannot be allowed as an agent command"),
            "the refusal does not say what was refused: {refused}"
        );
    }
    let named = AgentCommand::new("  printobserver context  ").expect("a command prefix");
    assert_eq!(named.as_str(), "printobserver context");
    assert_eq!(named.to_string(), "printobserver context");
}

/// The commands a configuration allows.
fn allowed(commands: &[&str]) -> Vec<AgentCommand> {
    commands
        .iter()
        .map(|command| AgentCommand::new(command).expect("a command prefix"))
        .collect()
}

/// A Claude Code turn with agent commands has a shell, and the shell runs
/// those commands and no other: the argument vector `OneHarness` built for the
/// harness says so, not only the request this port handed it.
#[test]
fn a_turn_with_agent_commands_has_a_shell_narrowed_to_them() {
    let schemas = schema_read_lock();
    let fixture = Fixture::new("configuration-commands");
    let watch = Arc::new(Watch::default());
    let mut configured = config(
        &schemas,
        &fixture,
        HARNESS,
        &generated_assessment_schema(),
        always("SID-COMMANDS", &assessment("the print is fine", "high")),
    );
    configured.agent_commands = allowed(&["printobserver context", "printobserver pause"]);
    let supervisor = port(configured, &watch);

    let print_id = PrintId::new();
    block_on(supervisor.run_turn(turn(print_id, event(print_id, payload()), None)))
        .expect("the turn runs");

    let request = watch
        .requests()
        .into_iter()
        .next()
        .expect("the port built a run request");
    assert_eq!(request.mode, Some(PermissionMode::Default));
    assert_eq!(
        request.passthrough,
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
    let report = watch.reports().into_iter().next().expect("the run reported");
    let command = &report.results.first().expect("one result").command;
    for expected in [
        ["--permission-mode", "dontAsk"].as_slice(),
        ["--tools", "Read", "Grep", "Glob", "Bash"].as_slice(),
    ] {
        assert!(
            command.windows(expected.len()).any(|window| window == expected),
            "the harness was not started with {expected:?}: {command:?}"
        );
    }
    assert!(
        !command.iter().any(|argument| argument == "bypassPermissions"),
        "a turn with a shell was started with every permission granted: {command:?}"
    );
}

/// With no agent commands, or on a harness the rules are not written for, a
/// turn keeps `OneHarness`'s read-only mode and no shell.
#[test]
fn a_turn_without_rules_for_its_harness_stays_read_only() {
    let schemas = schema_read_lock();
    for (harness, commands) in [
        (HARNESS, Vec::new()),
        (OTHER_HARNESS, allowed(&["printobserver context"])),
    ] {
        let fixture = Fixture::new(&format!("configuration-read-only-{harness}"));
        let watch = Arc::new(Watch::default());
        let mut configured = config(
            &schemas,
            &fixture,
            harness,
            &generated_assessment_schema(),
            always("SID-READ-ONLY", &assessment("the print is fine", "high")),
        );
        configured.agent_commands = commands;
        let supervisor = port(configured, &watch);

        let print_id = PrintId::new();
        block_on(supervisor.run_turn(turn(print_id, event(print_id, payload()), None)))
            .expect("the turn runs");

        let request = watch
            .requests()
            .into_iter()
            .next()
            .expect("the port built a run request");
        assert_eq!(request.mode, Some(PermissionMode::ReadOnly), "{harness}");
        assert!(
            request.passthrough.is_empty(),
            "{harness} was handed arguments of its own: {:?}",
            request.passthrough
        );
    }
}

/// A file that constrains no answer is refused where it is named, rather than
/// hours later as every answer being turned away.
#[test]
fn a_schema_that_constrains_no_answer_is_refused_where_it_is_named() {
    let fixture = Fixture::new("configuration-schema");

    let absent = fixture.path("no-such-schema.json");
    let not_json = fixture.path("not-json.json");
    fs::write(&not_json, "this is not a schema").expect("a scratch file");
    let not_a_document = fixture.path("an-array.json");
    fs::write(&not_a_document, r#"["summary", "confidence"]"#).expect("a scratch file");
    // An object, and still no schema: nothing could be judged against it.
    let not_a_schema = fixture.path("not-a-schema.json");
    fs::write(&not_a_schema, r#"{"type": 17}"#).expect("a scratch file");
    // A schema, and one that admits every answer there is.
    let declares_nothing = fixture.path("empty.json");
    fs::write(&declares_nothing, "{}").expect("a scratch file");

    // Each of them is refused naming the file. What is said about the absent
    // one, the unparsable one and the one that compiles to nothing is the
    // operating system's own text, serde's and the schema compiler's; only the
    // array is refused in this crate's own words.
    for path in [
        &absent,
        &not_json,
        &not_a_document,
        &not_a_schema,
        &declares_nothing,
    ] {
        let refused = AssessmentSchema::at(path)
            .expect_err(&format!("{} was accepted as a schema", path.display()));
        let said = refused.to_string();
        assert!(
            said.contains(&path.display().to_string()),
            "the refusal does not name the file: {said}"
        );
        let (_, why) = said
            .split_once("does not constrain an answer: ")
            .unwrap_or_else(|| panic!("the refusal does not say why: {said}"));
        assert!(!why.is_empty(), "the refusal says why with nothing: {said}");
    }
    assert!(
        AssessmentSchema::at(&not_a_document)
            .expect_err("an array was accepted as a schema")
            .to_string()
            .contains("not a schema document"),
        "a JSON array was refused as something other than what it is"
    );
    assert!(
        AssessmentSchema::at(&declares_nothing)
            .expect_err("a schema declaring nothing was accepted")
            .to_string()
            .contains("declares nothing"),
        "a schema admitting every answer was refused as something else"
    );

    // The generated artifact is a schema document, and reading it says so. It
    // is held still while it is read: the assessment-schema journey rewrites
    // that file beside this test, and a read that landed between its truncate
    // and its write would find no document at all.
    let schemas = schema_read_lock();
    let named = schema(&schemas, &generated_assessment_schema());
    assert_eq!(named.path(), generated_assessment_schema());
}

/// What the constrained values say is what reaches `OneHarness`.
#[test]
fn the_run_request_carries_what_the_constrained_configuration_says() {
    // Every answer this journey drives is judged by the checked-in assessment
    // schema, so it is held still while the journey reads it.
    let schemas = schema_read_lock();
    let fixture = Fixture::new("configuration-request");
    let watch = Arc::new(Watch::default());
    let mut configured = config(
        &schemas,
        &fixture,
        HARNESS,
        &generated_assessment_schema(),
        always("SID-CONFIG", &assessment("the print is fine", "high")),
    );
    configured.turn_timeout = TurnTimeout::new(97).expect("a bound");
    configured.model = Some(ModelName::new("a-pinned-model").expect("a pinned model"));
    configured.harness_env.push(assignment("MOCK_EXTRA=beside"));
    let supervisor = port(configured, &watch);

    let print_id = PrintId::new();
    block_on(supervisor.run_turn(turn(print_id, event(print_id, payload()), None)))
        .expect("the turn runs");

    let request = watch
        .requests()
        .into_iter()
        .next()
        .expect("the port built a run request");
    assert_eq!(request.harness, vec![HARNESS.to_owned()]);
    assert_eq!(request.timeout, Some(97));
    assert_eq!(request.model, vec!["a-pinned-model".to_owned()]);
    assert!(
        request.env.iter().any(|line| line == "MOCK_EXTRA=beside"),
        "the assignment did not reach the run request as KEY=VALUE: {:?}",
        request.env
    );
}
