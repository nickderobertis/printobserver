//! The closed surface: which commands exist, and what each one accepts.
//!
//! Nothing here is compared against a list this crate keeps. The command set is
//! compared against the contracts' own `PrintAction` and the six reads the
//! server declares beside that vocabulary; each command's accepted options are
//! compared against the fields that `PrintAction` variant declares, or against
//! the request schema the server declares for that route; and the global
//! options are compared against the four this program's own task fixes and
//! spells out below. So growing a declaration in this crate cannot satisfy any
//! of it.
//!
//! # The bound is both ways
//!
//! An **upper** bound alone would be satisfied by a command that accepted no
//! target temperature and always sent one fixed number: a closed command set, a
//! closed schema, every rejection check passed, and the wrong request made. So
//! every field the derivation names as required is also asserted to be
//! **accepted**, and the fixture for that is a command whose declared payload
//! field is absent from its parser while the rest of its schema is intact —
//! which is the shape a fixed internal value hides behind.
//!
//! # Every assertion here is driven over a violation as well as over the tree
//!
//! A check that has never refused anything is a check nobody has proven refuses
//! anything, so each rule is driven over a surface carrying exactly the defect
//! it is about.

use std::collections::{BTreeMap, BTreeSet};
use std::process::Command as Process;

use printobserver::surface::{
    Field, Form, GLOBAL_OPTIONS, SERVE_COMMAND, Supply, command_for, option_for, surface,
};
use printobserver_server::{BESIDE_THE_ACTIONS, Located, OPERATIONS, Parameter, ValueKind};
use printobserver_types::serde_json::Value;
use printobserver_types::{ActionKind, PrintAction};

/// The reads this program has beside the action vocabulary, as its own task
/// names them: status, context, the image read and materialization, history,
/// and the manifest's read and write.
///
/// Written out here rather than read from the server, and then held against
/// what the server declares — so a server that grew a seventh read would fail
/// this rather than quietly grow this program's surface.
const READS: [&str; 6] = [
    "status",
    "context",
    "image",
    "history",
    "manifest_get",
    "manifest_set",
];

/// The four options every command takes beside its own values, as this
/// program's own task fixes them. Nothing may be added without amending that
/// task.
const GLOBALS: [&str; 4] = ["--json", "--config", "--help", "--version"];

/// Every spelling of an address or a credential a surface must never accept.
const NEVER_ACCEPTED: [&str; 10] = [
    "server",
    "address",
    "url",
    "host",
    "credential",
    "token",
    "secret",
    "password",
    "api-key",
    "auth",
];

/// The contracts' own action vocabulary, as its schema declares it.
fn action_schema() -> Value {
    printobserver_types::schemars::schema_for!(PrintAction).to_value()
}

/// One variant of that vocabulary, found by the tag it is spelled with.
fn variant(tag: &str) -> Value {
    action_schema()
        .get("oneOf")
        .and_then(Value::as_array)
        .expect("the action vocabulary is a closed set of variants")
        .iter()
        .find(|variant| {
            variant
                .pointer("/properties/action/const")
                .and_then(Value::as_str)
                == Some(tag)
        })
        .unwrap_or_else(|| panic!("the contracts declare no action tagged `{tag}`"))
        .clone()
}

/// Every tag the contracts' own action vocabulary declares.
fn action_tags() -> BTreeSet<String> {
    action_schema()
        .get("oneOf")
        .and_then(Value::as_array)
        .expect("the action vocabulary is a closed set of variants")
        .iter()
        .map(|variant| {
            variant
                .pointer("/properties/action/const")
                .and_then(Value::as_str)
                .expect("every variant is tagged with the action it is")
                .to_owned()
        })
        .collect()
}

/// The fields one variant declares, and which of them it declares as required.
///
/// The tag itself is not one: a caller never supplies it, because the command
/// they typed is what names the action.
fn variant_fields(tag: &str) -> (BTreeSet<String>, BTreeSet<String>) {
    let declared = variant(tag);
    let all = declared
        .get("properties")
        .and_then(Value::as_object)
        .expect("a variant declares its fields")
        .keys()
        .filter(|name| name.as_str() != "action")
        .cloned()
        .collect();
    let required = declared
        .get("required")
        .and_then(Value::as_array)
        .map(|names| {
            names
                .iter()
                .filter_map(Value::as_str)
                .filter(|name| *name != "action")
                .map(str::to_owned)
                .collect()
        })
        .unwrap_or_default();
    (all, required)
}

/// The values one route takes outside its body: its path's own segments, and
/// whatever it declares after the question mark.
fn outside_the_body(name: &str) -> (BTreeSet<String>, BTreeSet<String>) {
    let operation = printobserver_server::operation(name).expect("the server serves this");
    let mut all = BTreeSet::new();
    let mut required = BTreeSet::new();
    for segment in operation.path.split('/') {
        if let Some(held) = segment
            .strip_prefix('{')
            .and_then(|rest| rest.strip_suffix('}'))
        {
            all.insert(held.to_owned());
            required.insert(held.to_owned());
        }
    }
    for (asked, needed, _) in operation.query {
        all.insert((*asked).to_owned());
        if *needed {
            required.insert((*asked).to_owned());
        }
    }
    (all, required)
}

/// Every value one client command may take, and every value it must take.
///
/// For a command carrying an action of the vocabulary, the body is what that
/// variant declares — read from the contracts rather than from anything this
/// crate or the server writes down. For a read it is the request schema the
/// server declares for that route. Both are joined with the values the route
/// takes outside its body, because a route with a hole in its path is not a
/// route.
fn derived(name: &str) -> (BTreeSet<String>, BTreeSet<String>) {
    let operation = printobserver_server::operation(name).expect("the server serves this");
    let (mut all, mut required) = outside_the_body(name);
    match operation.action_kind() {
        Some(kind) => {
            let tag = printobserver_types::serde_json::to_value(kind)
                .ok()
                .and_then(|value| value.as_str().map(str::to_owned))
                .expect("an action renders as the tag it is spelled by");
            let (fields, needed) = variant_fields(&tag);
            all.extend(fields);
            required.extend(needed);
        }
        None => {
            for parameter in operation.request() {
                if parameter.located != Located::Body {
                    continue;
                }
                all.insert(parameter.name.clone());
                if parameter.required {
                    required.insert(parameter.name);
                }
            }
        }
    }
    (all, required)
}

/// Every finding against one surface, over the closed-surface rule entire.
///
/// The globals are a parameter rather than a constant read here, so that a
/// surface carrying a fifth one can be driven through this and refused.
fn findings(surface: &[printobserver::surface::Command], globals: &[&str]) -> Vec<String> {
    let mut found = Vec::new();

    let mut wanted: BTreeSet<String> = action_tags().iter().map(|tag| command_for(tag)).collect();
    wanted.extend(READS.iter().map(|read| command_for(read)));
    wanted.insert(SERVE_COMMAND.to_owned());
    let present: BTreeSet<String> = surface.iter().map(|command| command.name.clone()).collect();
    found.extend(
        wanted
            .difference(&present)
            .map(|missing| format!("`{missing}` is an operation this program does not have")),
    );
    found.extend(
        present
            .difference(&wanted)
            .map(|extra| format!("`{extra}` is a command naming no operation this program has")),
    );

    for spelled in globals {
        if !GLOBALS.contains(spelled) {
            found.push(format!(
                "`{spelled}` is a global option outside the four this program's own task fixes"
            ));
        }
    }

    for command in surface {
        let taken: BTreeSet<String> = command.options().into_iter().collect();
        for option in &taken {
            if NEVER_ACCEPTED
                .iter()
                .any(|forbidden| option.contains(forbidden))
            {
                found.push(format!(
                    "`{}` accepts `{option}`, and where a server is and what authenticates \
                     to it are never an argument or an option",
                    command.name
                ));
            }
            if globals.contains(&option.as_str()) {
                found.push(format!(
                    "`{}` accepts `{option}` as one of its own values, and it is a global",
                    command.name
                ));
            }
        }
        if command.name == SERVE_COMMAND {
            found.extend(taken.iter().map(|option| {
                format!("`{SERVE_COMMAND}` accepts `{option}`, and it takes no value of its own")
            }));
            continue;
        }
        let Some(operation) = command.operation.as_ref() else {
            found.push(format!("`{}` names no operation", command.name));
            continue;
        };
        let (all, required) = derived(operation.name);
        let admitted: BTreeSet<String> = all
            .iter()
            .flat_map(|name| {
                let spelled = option_for(name);
                [format!("{spelled}-file"), spelled]
            })
            .collect();
        found.extend(taken.difference(&admitted).map(|extra| {
            format!(
                "`{}` accepts `{extra}`, which nothing outside this crate names",
                command.name
            )
        }));
        let carried: BTreeSet<String> = command
            .fields
            .iter()
            .map(|field| field.parameter.name.clone())
            .collect();
        found.extend(required.difference(&carried).map(|missing| {
            format!(
                "`{}` accepts no form of `{missing}`, which the declaration names as \
                 required — so this program supplies it rather than the caller",
                command.name
            )
        }));
    }
    found
}

/// The surface this program ships satisfies its own closed-surface rule.
#[test]
fn the_surface_this_program_ships_is_the_one_the_contracts_and_the_server_declare() {
    let found = findings(&surface(), &GLOBAL_OPTIONS);

    assert!(
        found.is_empty(),
        "the shipped surface is refused: {found:#?}"
    );
}

/// The server declares exactly the six reads this program's own task names.
#[test]
fn the_server_declares_exactly_the_six_reads_this_task_names() {
    assert_eq!(
        BESIDE_THE_ACTIONS.iter().copied().collect::<BTreeSet<_>>(),
        READS.iter().copied().collect::<BTreeSet<_>>(),
        "the reads the server serves beside the action vocabulary are not the reads this \
         program's own task names"
    );
    assert_eq!(
        OPERATIONS.len(),
        action_tags().len() + READS.len(),
        "the server serves an operation that is neither an action of the vocabulary nor one \
         of the reads this program's own task names"
    );
}

/// The global options are exactly the four the task fixes.
#[test]
fn the_global_options_are_the_four_the_task_fixes() {
    assert_eq!(GLOBAL_OPTIONS, GLOBALS);
}

/// One command missing from a surface is refused naming it.
#[test]
fn an_operation_with_no_command_is_refused() {
    let mut broken = surface();
    broken.retain(|command| command.name != "pause");

    let found = findings(&broken, &GLOBAL_OPTIONS);

    assert!(
        found.iter().any(|finding| finding.contains("`pause`")),
        "a surface that lost the command for an action of the vocabulary was accepted: \
         {found:#?}"
    );
}

/// One command naming no operation is refused naming it.
#[test]
fn a_command_naming_no_operation_is_refused() {
    let mut broken = surface();
    let mut invented = broken[1].clone();
    invented.name = "reboot".to_owned();
    broken.push(invented);

    let found = findings(&broken, &GLOBAL_OPTIONS);

    assert!(
        found.iter().any(|finding| finding.contains("`reboot`")),
        "a command naming no operation was accepted: {found:#?}"
    );
}

/// One field of one command, invented for a fixture.
fn invented_field(name: &str, kind: ValueKind, required: bool) -> Field {
    Field {
        parameter: Parameter {
            name: name.to_owned(),
            required,
            located: Located::Body,
            kind,
        },
        forms: vec![Form {
            option: option_for(name),
            supply: Supply::Value,
        }],
    }
}

/// One command of a surface, doctored by a fixture.
fn doctored(
    name: &str,
    change: impl Fn(&mut printobserver::surface::Command),
) -> Vec<printobserver::surface::Command> {
    let mut broken = surface();
    let found = broken
        .iter_mut()
        .find(|command| command.name == name)
        .unwrap_or_else(|| panic!("this program has a `{name}` command"));
    change(found);
    broken
}

/// An option nothing outside this crate names is refused.
///
/// The fixture is an unrelated option — neither an address nor a credential —
/// which a declaration beside the parser would have admitted. That is the whole
/// point: the comparison is against another crate's closed type, so growing
/// anything here cannot buy it back.
#[test]
fn an_option_nothing_outside_this_crate_names_is_refused() {
    let broken = doctored("pause", |command| {
        command
            .fields
            .push(invented_field("fast", ValueKind::Boolean, false));
    });

    let found = findings(&broken, &GLOBAL_OPTIONS);

    assert!(
        found
            .iter()
            .any(|finding| finding.contains("--fast") && finding.contains("nothing outside")),
        "a command accepting an option nothing declares was accepted: {found:#?}"
    );
}

/// A global option outside the fixed four is refused.
#[test]
fn a_global_option_outside_the_fixed_four_is_refused() {
    let found = findings(
        &surface(),
        &["--json", "--config", "--help", "--version", "--verbose"],
    );

    assert!(
        found.iter().any(|finding| finding.contains("--verbose")),
        "a fifth global option was accepted: {found:#?}"
    );
}

/// A command accepting a server address or a credential is refused.
#[test]
fn a_command_accepting_an_address_or_a_credential_is_refused() {
    for (spelled, kind) in [("server", ValueKind::Text), ("credential", ValueKind::Text)] {
        let broken = doctored("status", |command| {
            command.fields.push(invented_field(spelled, kind, false));
        });

        let found = findings(&broken, &GLOBAL_OPTIONS);

        assert!(
            found
                .iter()
                .any(|finding| finding.contains(&option_for(spelled))),
            "a command accepting `{spelled}` was accepted: {found:#?}"
        );
    }
}

/// A command that accepts no form of a required field is refused.
///
/// This is the lower bound, and the fixture is the shape a fixed internal value
/// hides behind: the rest of the command's schema is intact and the one
/// declared payload field is simply not there, so the caller cannot supply it
/// and the program supplies it for them.
#[test]
fn a_command_that_accepts_no_form_of_a_required_field_is_refused() {
    for (command, field) in [
        ("set-bed-target-c", "target_c"),
        ("start-print", "manifest"),
        ("pause", "reason"),
        ("acknowledge-failure", "disposition"),
    ] {
        let broken = doctored(command, |found| {
            found.fields.retain(|held| held.parameter.name != field);
        });

        let found = findings(&broken, &GLOBAL_OPTIONS);

        assert!(
            found
                .iter()
                .any(|finding| finding.contains(field) && finding.contains("required")),
            "`{command}` with no way to supply `{field}` was accepted: {found:#?}"
        );
    }
}

/// One invocation of the built program, and everything it said.
fn run(arguments: &[&str]) -> (Option<i32>, String) {
    let output = Process::new(env!("CARGO_BIN_EXE_printobserver"))
        .args(arguments)
        // No configuration anywhere, so nothing here reaches a server: what is
        // being read is what the parser makes of the arguments.
        .env_remove("PRINTOBSERVER_SERVER")
        .env_remove("PRINTOBSERVER_CREDENTIAL")
        .output()
        .expect("the built `printobserver` binary should be spawnable");
    (
        output.status.code(),
        String::from_utf8_lossy(&output.stdout).into_owned()
            + &String::from_utf8_lossy(&output.stderr),
    )
}

/// The parser accepts exactly the options the surface declares.
///
/// Both directions, driven over the real program: every declared option is one
/// the parser knows, and an option no command declares is refused naming it. So
/// the surface the tests above rule on is the surface the program actually has
/// rather than a description of it.
#[test]
fn the_parser_accepts_exactly_the_options_the_surface_declares() {
    let mut spellings: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for command in surface() {
        spellings.insert(command.name.clone(), command.options());
    }
    for (name, options) in spellings {
        for option in &options {
            let (_, said) = run(&[&name, option, "1"]);
            assert!(
                !said.contains(&format!("`{option}` is not an option")),
                "`{name}` declares `{option}` and its parser refuses it: {said}"
            );
        }
        let (code, said) = run(&[&name, "--fast", "1"]);
        assert_eq!(code, Some(2), "`{name} --fast` was not refused: {said}");
        assert!(
            said.contains("`--fast` is not an option"),
            "`{name} --fast` was refused without naming it: {said}"
        );
    }
}

/// Every command of the surface names an operation, except the one that runs
/// the server.
#[test]
fn only_the_command_that_runs_the_server_names_no_operation() {
    let without: Vec<String> = surface()
        .into_iter()
        .filter(|command| !command.is_client())
        .map(|command| command.name)
        .collect();

    assert_eq!(without, vec![SERVE_COMMAND.to_owned()]);
}

/// Every action of the vocabulary is one this program has a command for.
#[test]
fn every_action_of_the_vocabulary_has_a_command() {
    let named: BTreeSet<String> = surface()
        .into_iter()
        .filter_map(|command| command.operation)
        .filter_map(|operation| operation.action_kind())
        .map(|kind| format!("{kind:?}"))
        .collect();
    let declared: BTreeSet<String> = [
        ActionKind::Pause,
        ActionKind::Resume,
        ActionKind::Cancel,
        ActionKind::StartPrint,
        ActionKind::SetFeedrateFactor,
        ActionKind::SetFlowrateFactor,
        ActionKind::SetToolTargetC,
        ActionKind::SetBedTargetC,
        ActionKind::SetFanPercent,
        ActionKind::AcknowledgeFailure,
    ]
    .into_iter()
    .map(|kind| format!("{kind:?}"))
    .collect();

    assert_eq!(named, declared);
}
