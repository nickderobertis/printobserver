//! The checked-in API description is what this server declares, and nothing else.
//!
//! # Why this server checks a description in beside the contracts' schemas
//!
//! The three clients are generated rather than written, and a generator needs
//! two things: the shapes, which `printobserver-types` already checks in, and
//! **which operations there are, what each takes and what each answers**, which
//! until now existed only as [`OPERATIONS`] inside this crate. A generator that
//! read that array would have to link this crate, and a client that linked this
//! crate would be a second copy of the server. So the array is written out,
//! under `schemas/printobserver-server/`, by the same generate-or-refuse
//! mechanism the contracts use: run with `PRINTOBSERVER_SCHEMAS=write` this
//! writes the description, and run without it refuses a tree whose checked-in
//! description no longer matches what this server serves.
//!
//! # Every answer shape is written out beside it
//!
//! One file per shape a route can answer, generated through
//! [`schema_of`](printobserver_types::contract::schema_of) — the contracts'
//! own generator settings — so that a `$defs` entry here is byte-for-byte the
//! type file `schemas/printobserver-types/` carries for it. That is what lets
//! the client generator resolve a reference to the contracts' own file rather
//! than to a second copy of it.

#[path = "support/schema_files.rs"]
mod schema_files;

use printobserver_server::{Answer, Effect, OPERATIONS, Operation, VERSION_PREFIX};
use printobserver_types::contract::schema_of;
use printobserver_types::serde_json::{Value, json};
use schema_files::reconcile;

/// The file the operation list is written under.
const OPERATIONS_FILE: &str = "operations.json";

/// The version this description's own shape is at.
///
/// It is the description's shape rather than the API's: a consumer that reads
/// this file needs to know whether it still knows how to read it.
const DESCRIPTION_VERSION: u64 = 1;

/// The name of the shape one answer of one operation carries.
///
/// Read off the schema's own title rather than restated, so a renamed answer
/// type cannot leave a description naming the old one.
fn answer_type(operation: &Operation, answer: Answer) -> String {
    operation.response_schema(answer)["title"]
        .as_str()
        .unwrap_or_else(|| panic!("`{}`'s {answer} answer is a titled shape", operation.name))
        .to_owned()
}

/// One operation, as a consumer outside this crate reads it.
fn described(operation: &Operation) -> Value {
    let parameters: Vec<Value> = operation
        .request_shapes()
        .into_iter()
        .map(|(parameter, shape)| {
            json!({
                "name": parameter.name,
                "required": parameter.required,
                "located": format!("{:?}", parameter.located).to_lowercase(),
                "kind": format!("{:?}", parameter.kind).to_lowercase(),
                "shape": shape,
            })
        })
        .collect();
    let answers: Vec<Value> = operation
        .answers_with()
        .iter()
        .map(|answer| json!({ "answer": answer.as_str(), "type": answer_type(operation, *answer) }))
        .collect();
    json!({
        "name": operation.name,
        "method": operation.method.as_str(),
        "path": operation.full_path(),
        "effect": match operation.effect {
            Effect::Read => "read".to_owned(),
            Effect::Write => "write".to_owned(),
            Effect::Mutating(kind) => format!(
                "mutating:{}",
                printobserver_types::serde_json::to_value(kind)
                    .ok()
                    .and_then(|value| value.as_str().map(str::to_owned))
                    .expect("an action renders as the tag it is spelled by")
            ),
        },
        "accepts": operation.accepts,
        "answers": operation.answers,
        "image_path_field": operation.image_path_field,
        "parameters": parameters,
        "responses": answers,
    })
}

/// The whole API description, as one document.
fn operations_document() -> Value {
    json!({
        "description_version": DESCRIPTION_VERSION,
        "version_prefix": VERSION_PREFIX,
        "media_type": printobserver_server::MEDIA_TYPE,
        "operations": OPERATIONS.iter().map(described).collect::<Vec<_>>(),
    })
}

/// Every answer shape any route can produce, by the file it is written under.
///
/// [`printobserver_server::ErrorAnswer`] is here although no operation declares
/// it: it is what this server answers a request it will not act on at all, and
/// a client that could not read one would report a transport failure where the
/// server had said in words what was wrong.
fn answer_schemas() -> Vec<(String, Value)> {
    vec![
        (
            "ActionAnswer.json".to_owned(),
            schema_of::<printobserver_server::ActionAnswer>(),
        ),
        (
            "ContextAnswer.json".to_owned(),
            schema_of::<printobserver_server::ContextAnswer>(),
        ),
        (
            "ErrorAnswer.json".to_owned(),
            schema_of::<printobserver_server::ErrorAnswer>(),
        ),
        (
            "HistoryAnswer.json".to_owned(),
            schema_of::<printobserver_server::HistoryAnswer>(),
        ),
        (
            "ImageAnswer.json".to_owned(),
            schema_of::<printobserver_server::ImageAnswer>(),
        ),
        (
            "ManifestAnswer.json".to_owned(),
            schema_of::<printobserver_server::ManifestAnswer>(),
        ),
        (
            "StatusAnswer.json".to_owned(),
            schema_of::<printobserver_server::StatusAnswer>(),
        ),
    ]
}

/// Everything this crate writes into its own schema directory.
fn generated() -> Vec<(String, Value)> {
    let mut entries = answer_schemas();
    entries.push((OPERATIONS_FILE.to_owned(), operations_document()));
    entries.sort_by(|left, right| left.0.cmp(&right.0));
    entries
}

/// The checked-in API description is what this server declares.
#[test]
fn the_checked_in_api_description_is_what_this_server_declares() {
    let findings = reconcile("printobserver-server", &generated());
    assert!(
        findings.is_empty(),
        "the checked-in API description has drifted:\n{}",
        findings.join("\n")
    );
}

/// The description names every operation this server serves, and no other.
#[test]
fn the_description_names_every_operation_and_no_other() {
    let document = operations_document();
    let described: Vec<String> = document["operations"]
        .as_array()
        .expect("the description carries an operation list")
        .iter()
        .map(|entry| {
            entry["name"]
                .as_str()
                .expect("a named operation")
                .to_owned()
        })
        .collect();

    let served: Vec<String> = OPERATIONS
        .iter()
        .map(|operation| operation.name.to_owned())
        .collect();
    assert_eq!(described, served);
}

/// Every value a request carries is described with a shape a consumer can type.
///
/// A generated client needs the contracts' own type where the value is one of
/// theirs, and a scalar shape where it is not. A description that said only
/// `structured` would leave a generator typing a manifest as anything.
#[test]
fn every_value_a_request_carries_is_described_with_a_shape() {
    let mut named_types = 0_usize;
    for operation in &OPERATIONS {
        for (parameter, shape) in operation.request_shapes() {
            let referenced = shape.get("$ref").and_then(Value::as_str);
            // A value that may be absent is declared as its own type or null,
            // which is a list of names rather than a name.
            let scalar = shape.get("type").is_some_and(Value::is_string)
                || shape
                    .get("type")
                    .and_then(Value::as_array)
                    .is_some_and(|names| names.iter().any(Value::is_string));
            assert!(
                referenced.is_some() || scalar,
                "`{}` describes `{}` as {shape}, which is neither one of the contracts' \
                 own types nor a scalar",
                operation.name,
                parameter.name
            );
            if let Some(reference) = referenced {
                named_types += 1;
                let name = reference
                    .strip_prefix("#/$defs/")
                    .expect("a reference names one of the contracts' own types");
                assert!(
                    schema_files::schema_dir("printobserver-types")
                        .join(format!("{name}.json"))
                        .is_file(),
                    "`{}` describes `{}` as `{name}`, which the contracts check in no \
                     schema for",
                    operation.name,
                    parameter.name
                );
            }
        }
    }
    assert!(
        named_types > 0,
        "no request value is described by one of the contracts' own types, so this \
         walk is over shapes nothing types"
    );
}

/// Every described operation carries the values its own request declares.
#[test]
fn every_described_operation_carries_the_values_its_request_declares() {
    for operation in &OPERATIONS {
        let entry = described(operation);
        let names: Vec<String> = entry["parameters"]
            .as_array()
            .expect("a described operation carries its parameters")
            .iter()
            .map(|parameter| {
                parameter["name"]
                    .as_str()
                    .expect("a named value")
                    .to_owned()
            })
            .collect();
        let declared: Vec<String> = operation
            .request()
            .into_iter()
            .map(|parameter| parameter.name)
            .collect();
        assert_eq!(names, declared, "`{}`", operation.name);
    }
}

/// A mutating operation is described as answering the policy's own rejection.
///
/// The rejection is an answer rather than a transport failure, and a client
/// generated from a description that did not say so would report one as the
/// other.
#[test]
fn every_mutating_operation_is_described_as_answering_a_rejection() {
    for operation in &OPERATIONS {
        let entry = described(operation);
        let answers: Vec<String> = entry["responses"]
            .as_array()
            .expect("a described operation carries its answers")
            .iter()
            .map(|answer| {
                answer["answer"]
                    .as_str()
                    .expect("a named answer")
                    .to_owned()
            })
            .collect();
        match operation.effect {
            Effect::Mutating(_) => assert_eq!(
                answers,
                vec!["success".to_owned(), "rejected".to_owned()],
                "`{}`",
                operation.name
            ),
            Effect::Read | Effect::Write => {
                assert_eq!(answers, vec!["success".to_owned()], "`{}`", operation.name);
            }
        }
    }
}
