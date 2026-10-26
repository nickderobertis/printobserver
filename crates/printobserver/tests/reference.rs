//! The declared command surface, written out for a reader that is not Rust.
//!
//! `skills/printobserver/reference/surface.json` is the one artifact the
//! documentation checks read this program's surface off. It is **generated** from
//! [`surface`](printobserver::surface::surface) and [`Exit::ALL`] rather than
//! written: a command the server grows, an option a contract variant grows and
//! an exit this program grows all reach those checks the moment the generation
//! target writes them, and none of them can be added to the artifact by hand
//! without this suite refusing the tree.
//!
//! It exists because the checks over the reference documents are Python and the
//! surface is Rust. Parsing this crate's sources from Python would be a second,
//! drifting reading of a declaration this crate already folds over; a generated
//! artifact under one drift gate is one reading.
//!
//! Run with `PRINTOBSERVER_DOCS=write` — which is what `just docs-generate`
//! does — this writes the artifact. Run without it, it refuses a tree whose
//! artifact is not what the surface declares.

use std::path::PathBuf;

use printobserver::failure::Exit;
use printobserver::surface::{Command, GLOBAL_OPTIONS, SERVE_COMMAND, Supply, surface};
use printobserver_server::Located;
use printobserver_types::serde_json::{Value, json};

/// Where the generated surface manifest is checked in.
const MANIFEST: &str = "skills/printobserver/reference/surface.json";

/// The variable that makes this suite write the artifact rather than check it.
const WRITING: &str = "PRINTOBSERVER_DOCS";

/// The repository root, from this crate's own directory.
fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
}

/// Where one value of a request travels, as the artifact spells it.
const fn located(located: Located) -> &'static str {
    match located {
        Located::Path => "path",
        Located::Query => "query",
        Located::Body => "body",
    }
}

/// One command of the surface, as the artifact carries it.
fn command_entry(command: &Command) -> Value {
    let options: Vec<Value> = command
        .fields
        .iter()
        .flat_map(|field| {
            field.forms.iter().map(move |form| {
                let mut entry = json!({
                    "option": form.option,
                    "field": field.parameter.name,
                    "required": field.required() && form.supply == Supply::Value,
                    "supply": match form.supply {
                        Supply::Value => "value",
                        Supply::File => "file",
                    },
                    "located": located(field.parameter.located),
                });
                // Carried only by an option that may be given more than once,
                // so an entry that may not reads as it always did.
                if field.parameter.repeated {
                    entry["repeated"] = Value::Bool(true);
                }
                entry
            })
        })
        .chain(command.flags.iter().map(|flag| {
            json!({
                "option": flag,
                "field": flag.trim_start_matches('-'),
                "required": false,
                "supply": "flag",
                "located": "local",
            })
        }))
        .collect();
    match &command.operation {
        None => json!({
            "command": command.name,
            "operation": Value::Null,
            "options": options,
        }),
        Some(operation) => json!({
            "command": command.name,
            "operation": operation.name,
            "method": operation.method.as_str(),
            "path": operation.full_path(),
            "mutating": operation.is_mutating(),
            "answers": operation
                .answers_with()
                .iter()
                .map(|answer| Value::String(answer.as_str().to_owned()))
                .collect::<Vec<_>>(),
            "image_path_field": operation.image_path_field,
            "options": options,
        }),
    }
}

/// The whole artifact, as this program's surface declares it.
fn manifest() -> Value {
    json!({
        "generated_by": "just docs-generate",
        "serve_command": SERVE_COMMAND,
        "global_options": GLOBAL_OPTIONS,
        "exits": Exit::ALL
            .iter()
            .map(|exit| json!({"name": exit.as_str(), "status": exit.status()}))
            .collect::<Vec<_>>(),
        "commands": surface().iter().map(command_entry).collect::<Vec<_>>(),
    })
}

/// The artifact's text, as it is written and as it is compared.
fn text_of(document: &Value) -> String {
    let mut text = printobserver_types::serde_json::to_string_pretty(document)
        .expect("a generated manifest serializes");
    text.push('\n');
    text
}

/// The checked-in surface manifest is what this program's surface declares.
#[test]
fn the_checked_in_surface_manifest_is_what_the_program_declares() {
    let path = repo_root().join(MANIFEST);
    let generated = text_of(&manifest());
    if std::env::var(WRITING).is_ok_and(|mode| mode == "write") {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).expect("the reference directory is writable");
        }
        std::fs::write(&path, &generated).expect("the surface manifest is writable");
        return;
    }
    let found = std::fs::read_to_string(&path).unwrap_or_else(|error| {
        panic!(
            "{MANIFEST} could not be read ({error}). Run `just docs-generate` to write it: \
             the documentation checks read this program's surface off that artifact."
        )
    });
    assert_eq!(
        found, generated,
        "{MANIFEST} is not what this program's surface declares. Run `just docs-generate`."
    );
}

/// The artifact names every command the surface has, and nothing else.
///
/// The comparison above is over bytes, which a stale artifact and a stale
/// generator would agree on together. This reads the command set out of the
/// artifact on disk and holds it to `surface()` itself, so a generator that
/// stopped emitting a command is refused rather than agreed with.
#[test]
fn the_artifact_names_every_command_the_surface_has() {
    let path = repo_root().join(MANIFEST);
    let text = std::fs::read_to_string(&path).expect("the generated surface manifest is readable");
    let found: Value =
        printobserver_types::serde_json::from_str(&text).expect("the artifact is JSON");
    let named: Vec<String> = found["commands"]
        .as_array()
        .expect("the artifact carries a command list")
        .iter()
        .map(|entry| {
            entry["command"]
                .as_str()
                .expect("each entry names its command")
                .to_owned()
        })
        .collect();
    let declared: Vec<String> = surface()
        .iter()
        .map(|command| command.name.clone())
        .collect();
    assert_eq!(
        named, declared,
        "{MANIFEST} does not name the surface's own commands"
    );
}

/// The command surface's document, which restates two facts of the server's.
const COMMAND_SURFACE: &str = "skills/printobserver/reference/command-surface.md";

/// The API document, which restates one of them too.
const API_AND_CLIENTS: &str = "skills/printobserver/reference/api-and-clients.md";

/// How many of a spool's own events its read carries, as the documents write
/// the number. A server that changed the window is a document that has to say
/// so, and a window with no word here is a finding naming the one to add.
fn spoken(count: u32) -> &'static str {
    match count {
        10 => "ten",
        20 => "twenty",
        50 => "fifty",
        other => panic!("the documents have no word for a window of {other}; add it here"),
    }
}

/// The two facts the reference documents restate rather than generate — the
/// printer history's kinds, and how many events a spool's read carries — are
/// the server's own: the kinds its timeline takes, and the window its spool
/// read answers.
#[test]
fn the_documents_restate_the_timeline_kinds_and_the_spool_window_as_the_server_has_them() {
    let surface = std::fs::read_to_string(repo_root().join(COMMAND_SURFACE))
        .unwrap_or_else(|error| panic!("{COMMAND_SURFACE} reads: {error}"));
    let (listed, _) = surface
        .split_once("The kinds are ")
        .and_then(|(_, rest)| rest.split_once("; any other is refused"))
        .unwrap_or_else(|| panic!("{COMMAND_SURFACE} lists no printer history kinds"));
    let mut documented: Vec<&str> = listed.split('`').skip(1).step_by(2).collect();
    documented.sort_unstable();
    let mut served: Vec<&str> = printobserver_core::TIMELINE_KINDS.to_vec();
    served.sort_unstable();
    assert_eq!(
        documented, served,
        "{COMMAND_SURFACE} lists other printer history kinds than the timeline takes"
    );

    let window = format!(
        "newest {}",
        spoken(printobserver_server::SPOOL_RECENT_EVENTS)
    );
    for document in [COMMAND_SURFACE, API_AND_CLIENTS] {
        let text = std::fs::read_to_string(repo_root().join(document))
            .unwrap_or_else(|error| panic!("{document} reads: {error}"));
        assert!(
            text.contains(&window),
            "{document} does not say a spool's read carries the {window} of its events"
        );
    }
}

/// The bounds the reference documents restate for the events read and the
/// watch are the ones the server and this program enforce.
#[test]
fn the_documents_restate_the_events_and_watch_bounds_as_the_code_has_them() {
    use printobserver::watch::DEFAULT_RECONNECT_S;
    use printobserver_server::{DEFAULT_EVENTS_LIMIT, MAX_EVENTS_LIMIT, MAX_EVENTS_WAIT_S};
    let read = |document: &str| {
        std::fs::read_to_string(repo_root().join(document))
            .unwrap_or_else(|error| panic!("{document} reads: {error}"))
    };
    let (surface, api) = (read(COMMAND_SURFACE), read(API_AND_CLIENTS));
    for (document, text, said) in [
        (
            COMMAND_SURFACE,
            &surface,
            format!("0 to {MAX_EVENTS_WAIT_S}"),
        ),
        (
            COMMAND_SURFACE,
            &surface,
            format!("1 to {MAX_EVENTS_LIMIT} (optional; {DEFAULT_EVENTS_LIMIT} by default)"),
        ),
        (
            COMMAND_SURFACE,
            &surface,
            format!("{DEFAULT_RECONNECT_S} by default)."),
        ),
        (
            COMMAND_SURFACE,
            &surface,
            "0, the default, prints none).".to_owned(),
        ),
        (
            COMMAND_SURFACE,
            &surface,
            "a number of seconds below zero — the only bound either takes — is refused".to_owned(),
        ),
        (
            API_AND_CLIENTS,
            &api,
            format!("`wait_s` is 0 to {MAX_EVENTS_WAIT_S}, default 0"),
        ),
        (
            API_AND_CLIENTS,
            &api,
            format!("`limit` is 1 to {MAX_EVENTS_LIMIT}, default {DEFAULT_EVENTS_LIMIT}"),
        ),
    ] {
        assert!(text.contains(&said), "{document} does not say `{said}`");
    }
}

/// The installers that carry the spool check's defaults, commented out.
const INSTALLERS: [&str; 2] = ["scripts/install-service.sh", "scripts/install-service.ps1"];

/// The intervention policy, which restates the spool check's defaults.
const INTERVENTION_POLICY: &str = "skills/printobserver/reference/intervention-policy.md";

/// The worked examples, which restate the printer history's window.
const COMMON_OPERATIONS: &str = "skills/printobserver/reference/common-operations.md";

/// One document of the tree.
fn document(path: &str) -> String {
    std::fs::read_to_string(repo_root().join(path))
        .unwrap_or_else(|error| panic!("{path} reads: {error}"))
}

/// A whole number as the documents write it: thousands separated by commas.
fn written(count: u32) -> String {
    let digits = count.to_string();
    let mut out = String::new();
    for (index, digit) in digits.chars().enumerate() {
        if index > 0 && (digits.len() - index).is_multiple_of(3) {
            out.push(',');
        }
        out.push(digit);
    }
    out
}

/// The defaults the installers' commented `[spools]` table and the
/// intervention policy restate are the supervision core's own, and the printer
/// history's window the documents restate is the store's.
#[test]
fn the_documents_restate_the_spool_check_and_history_window_defaults_as_the_code_has_them() {
    let defaults = printobserver_core::SpoolPolicy::default();
    for installer in INSTALLERS {
        let text = document(installer);
        for line in [
            "# preflight = \"refuse\"".to_owned(),
            format!("# buffer_percent = {:.1}", defaults.buffer_percent),
            format!("# buffer_min_g = {:.1}", defaults.buffer_min_g),
            format!("# analysis_wait_seconds = {}", defaults.analysis_wait_s),
        ] {
            assert!(
                text.lines().any(|held| held == line),
                "{installer} does not carry `{line}`, the core's default"
            );
        }
    }
    assert_eq!(
        defaults.preflight,
        printobserver_core::PreflightMode::Refuse
    );
    let policy = document(INTERVENTION_POLICY)
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");
    for said in [
        format!(
            "{} and {} by default",
            defaults.buffer_percent, defaults.buffer_min_g
        ),
        format!("{} by default", defaults.analysis_wait_s),
        "`refuse`, the default".to_owned(),
    ] {
        assert!(
            policy.contains(&said),
            "{INTERVENTION_POLICY} does not say `{said}`, the core's default"
        );
    }

    let window = written(printobserver_core::DEFAULT_HISTORY_WINDOW);
    let most = written(printobserver_core::MAX_HISTORY_LIMIT);
    for path in [COMMAND_SURFACE, API_AND_CLIENTS, COMMON_OPERATIONS] {
        let text = document(path)
            .split_whitespace()
            .collect::<Vec<_>>()
            .join(" ");
        assert!(
            text.contains(&format!("{window} when left out")) && text.contains(&most),
            "{path} does not state the printer history's window of {window} and its most of \
             {most} as the store has them"
        );
    }
}
