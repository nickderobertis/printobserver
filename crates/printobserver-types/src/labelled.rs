//! The labelled-lines rendering of a document: every leaf as its dotted path,
//! [`LABEL_SEPARATOR`] and its value.
//!
//! It is the rendering the `printobserver` program prints an answer in when no
//! machine-readable output is asked for, and so the rendering a supervision
//! turn reads its context in. It is declared here, beside the document type it
//! walks, so that the program printing it and a measurement of what a turn
//! reads count the same lines.

use crate::serde_json::Value;

/// How one leaf's path is spelled.
pub const PATH_SEPARATOR: char = '.';

/// What separates a leaf's path from its value in a labelled line.
pub const LABEL_SEPARATOR: &str = ": ";

/// Every leaf of one document, by the path it sits at, in path order.
///
/// A branch with nothing under it is a leaf of its own, so an answer carrying
/// an empty list of events says so rather than saying nothing. A list holding
/// numbers alone, at any depth — a rectangle, or a list of them — is one value
/// rather than a branch, and is written as its JSON on one line: its numbers
/// mean something only together.
#[must_use]
pub fn fields(document: &Value) -> Vec<(String, String)> {
    let mut found = Vec::new();
    walk(String::new(), document, &mut found);
    found
}

/// One leaf as the labelled rendering writes it, its newline included.
#[must_use]
pub fn line((at, value): &(String, String)) -> String {
    format!("{at}{LABEL_SEPARATOR}{value}\n")
}

/// Enumerate one value's leaves beneath one path.
fn walk(at: String, value: &Value, found: &mut Vec<(String, String)>) {
    let under = |name: &str| {
        if at.is_empty() {
            name.to_owned()
        } else {
            format!("{at}{PATH_SEPARATOR}{name}")
        }
    };
    match value {
        Value::Array(entries) if !entries.is_empty() && numbers_alone(value) => {
            found.push((at, value.to_string()));
        }
        Value::Object(entries) if !entries.is_empty() => {
            for (name, held) in entries {
                walk(under(name), held, found);
            }
        }
        Value::Array(entries) if !entries.is_empty() => {
            for (index, held) in entries.iter().enumerate() {
                walk(under(&index.to_string()), held, found);
            }
        }
        Value::Object(_) => found.push((at, "{}".to_owned())),
        Value::Array(_) => found.push((at, "[]".to_owned())),
        Value::String(text) => found.push((at, text.clone())),
        other => found.push((at, other.to_string())),
    }
}

/// Whether a value is a number, or a non-empty list holding numbers alone at
/// every depth.
fn numbers_alone(value: &Value) -> bool {
    match value {
        Value::Number(_) => true,
        Value::Array(entries) => !entries.is_empty() && entries.iter().all(numbers_alone),
        _ => false,
    }
}
