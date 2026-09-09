//! Every field the configuration declares, and the refusal it produces.
//!
//! The walk here is over [`ConfigField::ALL`] rather than over a sample: for
//! each field in turn it starts **the real server** — `Server::start`, the
//! composition root the installed unit's own command reaches — against a
//! configuration in which that one field is unacceptable, and asserts the
//! refusal names that field.
//!
//! The unacceptable value is one of the field's own kind. A required field is
//! left out; a range is inverted so it admits nothing; the address the machine
//! is at is one nothing answers at; a path is one that cannot be created or
//! read; a vocabulary value is one outside the vocabulary; the answer bound is
//! one at the producer's own timeout. A field whose unacceptable value were
//! merely "not a number" would prove the parser rather than the validation.
//!
//! Two things keep the walk from falling behind the type. It matches on
//! [`ConfigField`], so a variant added and not handled will not compile; and
//! [`the_declared_fields_are_exactly_what_the_configuration_type_carries`]
//! reads the configuration type's own schema and refuses a field the type
//! declares and the walk does not — and refuses a declaration carrying fewer
//! than the fields this program is required to take.

use std::collections::BTreeSet;
use std::path::PathBuf;

use printobserver_server::{ConfigField, ConfigFile, Server};
use printobserver_types::serde_json::Value;
use tempfile::TempDir;

use crate::probes::{base_url, silent_host, unreachable_address};
use crate::world::{document, remove, set, write};

/// The fields this program is required to take, whatever else it declares.
///
/// Named here rather than read off the type, because a walk over a type this
/// crate writes could otherwise be satisfied by declaring almost nothing.
const REQUIRED_FIELDS: [&str; 9] = [
    "state_dir",
    "octoprint.url",
    "octoprint.api_key",
    "safety",
    "supervisor.harness",
    "supervisor.skill_path",
    "listen",
    "ingress.answer_bound_ms",
    "ingress.shared_secret",
];

/// A path that is not a file this program can read.
fn unreadable(root: &std::path::Path) -> PathBuf {
    root.join("nothing-is-here.md")
}

/// One configuration in which exactly `field` is unacceptable.
///
/// Answers the document, and the path of anything it had to put on disk first.
async fn unacceptable(field: ConfigField, root: &std::path::Path, reachable: &str) -> toml::Value {
    let mut document = document(root, reachable);
    match field {
        // A path that cannot be created: this one names a directory under a
        // file, which no `mkdir -p` can make.
        ConfigField::StateDir => {
            let occupied = root.join("occupied");
            std::fs::write(&occupied, b"not a directory").expect("a file is writable");
            set(
                &mut document,
                "state_dir",
                toml::Value::String(occupied.join("state").display().to_string()),
            );
        }
        ConfigField::Listen => set(
            &mut document,
            "listen",
            toml::Value::String("this is not an address".to_owned()),
        ),
        // An address nothing answers at, which only the machine can rule on.
        ConfigField::OctoprintUrl => set(
            &mut document,
            "octoprint.url",
            toml::Value::String(format!("http://{}", unreachable_address().await)),
        ),
        ConfigField::OctoprintApiKey => remove(&mut document, "octoprint.api_key"),
        ConfigField::OctoprintFan => set(
            &mut document,
            "octoprint.fan",
            toml::Value::String("maybe".to_owned()),
        ),
        // A range whose minimum is above its maximum admits no value at all.
        ConfigField::SafetyEnvelope => {
            let mut inverted = toml::Table::new();
            inverted.insert("min".to_owned(), toml::Value::Float(1.5));
            inverted.insert("max".to_owned(), toml::Value::Float(0.5));
            set(
                &mut document,
                "safety.allowed.feedrate",
                toml::Value::Table(inverted),
            );
        }
        ConfigField::Harness => remove(&mut document, "supervisor.harness"),
        ConfigField::Model => set(
            &mut document,
            "supervisor.model",
            toml::Value::String(String::new()),
        ),
        ConfigField::SkillPath => set(
            &mut document,
            "supervisor.skill_path",
            toml::Value::String(unreadable(root).display().to_string()),
        ),
        ConfigField::PromptTemplatePath => set(
            &mut document,
            "supervisor.prompt_template_path",
            toml::Value::String(unreadable(root).display().to_string()),
        ),
        ConfigField::IngressAnswerBoundMs => set(
            &mut document,
            "ingress.answer_bound_ms",
            toml::Value::Integer(
                i64::try_from(printobserver_server::OBICO_POSTING_TIMEOUT_MS)
                    .expect("the recorded timeout is a number"),
            ),
        ),
        ConfigField::IngressSharedSecret => remove(&mut document, "ingress.shared_secret"),
    }
    document
}

/// Every field the configuration declares is refused by its own name.
#[tokio::test(flavor = "multi_thread")]
async fn every_declared_field_is_refused_naming_itself() {
    // One address that answers, so that the walk is about the field it is
    // about: a server refused for an unreachable machine would pass the
    // OctoPrint-address journey for the wrong reason on every other one.
    let reachable = silent_host().await;
    let base = base_url(&reachable);

    for field in ConfigField::ALL {
        let root = TempDir::new().expect("a journey's own root");
        let document = unacceptable(field, root.path(), &base).await;
        let path = write(root.path(), &document);

        let refusal = Server::start(&path)
            .await
            .err()
            .unwrap_or_else(|| panic!("`{field}` was made unacceptable and the server started"));

        assert_eq!(
            refusal.field(),
            Some(field),
            "the refusal for `{field}` named {:?} instead: {refusal}",
            refusal.field()
        );
        assert!(
            refusal.to_string().contains(field.key()),
            "the refusal for `{field}` does not name it: {refusal}"
        );
    }
}

/// The configuration this repository's own journeys ship is accepted, and its
/// server serves.
#[tokio::test(flavor = "multi_thread")]
async fn the_base_configuration_starts_a_server() {
    let reachable = silent_host().await;
    let root = TempDir::new().expect("a journey's own root");
    let path = write(root.path(), &document(root.path(), &base_url(&reachable)));

    let running = Server::start(&path)
        .await
        .expect("the base configuration starts the composition root");

    assert!(
        running.address().port() != 0,
        "the server did not take a port"
    );
    running.stop().await;
}

/// A file that is not this program's configuration is refused naming the file.
#[tokio::test(flavor = "multi_thread")]
async fn a_document_that_is_not_this_configuration_is_refused() {
    let root = TempDir::new().expect("a journey's own root");
    let path = root.path().join("config.toml");
    std::fs::write(&path, b"this is not = = a document\n").expect("a file is writable");

    let Err(refusal) = Server::start(&path).await else {
        panic!("a document that is not this configuration was accepted");
    };

    assert!(
        refusal.to_string().contains("config.toml"),
        "the refusal does not name the file: {refusal}"
    );
}

/// A configuration file that is not there is refused naming the path.
#[tokio::test(flavor = "multi_thread")]
async fn a_configuration_that_is_not_there_is_refused() {
    let root = TempDir::new().expect("a journey's own root");
    let path = root.path().join("nowhere.toml");

    let Err(refusal) = Server::start(&path).await else {
        panic!("a configuration that is not there was accepted");
    };

    assert!(
        refusal.to_string().contains("nowhere.toml"),
        "the refusal does not name the path: {refusal}"
    );
}

/// The declared field set is exactly what the configuration type carries.
///
/// The type's own generated schema is what this reads, so a field added to the
/// document and not to [`ConfigField::ALL`] fails here rather than being
/// silently unwalked. A section of this program's own is a `…Section` and is
/// recursed into; anything else is one value the operator supplies whole, and
/// the safety envelope is one of those — it is the contracts' own type and is
/// ruled on as a whole.
#[test]
fn the_declared_fields_are_exactly_what_the_configuration_type_carries() {
    let declared: BTreeSet<String> = ConfigField::ALL
        .iter()
        .map(|field| field.key().to_owned())
        .collect();

    assert_eq!(
        carried(&schema()),
        declared,
        "the configuration type and the declared field set have come apart"
    );

    for required in REQUIRED_FIELDS {
        assert!(
            declared.contains(required),
            "the configuration declares no `{required}`, which this program is required to take"
        );
    }
}

/// The reading refuses a type carrying a field the walk does not cover.
///
/// Driven over the committed schema with one property added, so that what
/// refuses the fixture is what reads the real pair.
#[test]
fn the_reading_refuses_a_field_the_walk_does_not_cover() {
    let mut altered = schema();
    altered["properties"]["surprise"] =
        printobserver_types::serde_json::json!({ "type": "string" });

    let carried = carried(&altered);

    assert!(
        carried.contains("surprise"),
        "the reading did not find the field added to the type: {carried:?}"
    );
    assert_ne!(
        carried,
        ConfigField::ALL
            .iter()
            .map(|field| field.key().to_owned())
            .collect::<BTreeSet<String>>(),
        "a type carrying a field the walk does not cover was accepted"
    );
}

/// The configuration type's own generated schema.
fn schema() -> Value {
    printobserver_types::contract::schema_of::<ConfigFile>()
}

/// Every dotted key one configuration schema carries.
fn carried(schema: &Value) -> BTreeSet<String> {
    let mut found = BTreeSet::new();
    collect(schema, schema, "", &mut found);
    found
}

/// Walk one schema's properties, recursing into this program's own sections.
fn collect(root: &Value, here: &Value, prefix: &str, found: &mut BTreeSet<String>) {
    let Some(properties) = here.get("properties").and_then(Value::as_object) else {
        return;
    };
    for (name, property) in properties {
        let dotted = if prefix.is_empty() {
            name.clone()
        } else {
            format!("{prefix}.{name}")
        };
        match section_reference(property) {
            Some(reference) => {
                let target = &root["$defs"][reference];
                collect(root, target, &dotted, found);
            }
            None => {
                found.insert(dotted);
            }
        }
    }
}

/// The section this property refers to, when it refers to one of this
/// program's own.
fn section_reference(property: &Value) -> Option<&str> {
    let reference = property.get("$ref")?.as_str()?;
    let name = reference.strip_prefix("#/$defs/")?;
    name.ends_with("Section").then_some(name)
}
