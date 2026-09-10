//! The declared command surface, written out for a reader that is not Rust.
//!
//! `docs/reference/surface.json` is the one artifact the documentation checks
//! read this program's surface off. It is **generated** from
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
const MANIFEST: &str = "docs/reference/surface.json";

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
                json!({
                    "option": form.option,
                    "field": field.parameter.name,
                    "required": field.required() && form.supply == Supply::Value,
                    "supply": match form.supply {
                        Supply::Value => "value",
                        Supply::File => "file",
                    },
                    "located": located(field.parameter.located),
                })
            })
        })
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
    let Ok(text) = std::fs::read_to_string(&path) else {
        return;
    };
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
    let declared: Vec<String> = surface().iter().map(|command| command.name.clone()).collect();
    assert_eq!(named, declared, "{MANIFEST} does not name the surface's own commands");
}
