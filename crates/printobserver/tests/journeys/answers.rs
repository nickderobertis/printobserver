//! What one invocation answered, whichever rendering it was asked for in.
//!
//! Both renderings carry the same fields by construction, so a journey that
//! wants a value out of an answer reads it the same way whether the invocation
//! asked for the document or for labelled lines. That is what lets the walk
//! assert the same things about every point of the cross-product rather than
//! about the machine-readable half of it.

use std::collections::BTreeMap;

use printobserver::render::{LABEL_SEPARATOR, fields};
use printobserver_types::serde_json::{self, Value};

use crate::traced::Ran;

/// Every field one answer carries, by the path it sits at.
///
/// # Panics
///
/// Panics when a machine-readable answer is not a document, which the format
/// journey is what rules on.
pub fn answered(ran: &Ran, machine_readable: bool) -> BTreeMap<String, String> {
    if machine_readable {
        let document: Value = serde_json::from_str(&ran.out).unwrap_or_else(|error| {
            panic!(
                "{:?} answered something that is not a document ({error}): {}",
                ran.arguments, ran.out
            )
        });
        return fields(&document).into_iter().collect();
    }
    labelled(&ran.out)
}

/// Every field a labelled answer carries, read back off its own lines.
///
/// A line carrying no label is a continuation of the value above it, which is
/// what a value with a line ending in it looks like.
pub fn labelled(printed: &str) -> BTreeMap<String, String> {
    let mut found: BTreeMap<String, String> = BTreeMap::new();
    let mut last: Option<String> = None;
    for line in printed.lines() {
        match line.split_once(LABEL_SEPARATOR) {
            Some((at, value)) => {
                found.insert(at.to_owned(), value.to_owned());
                last = Some(at.to_owned());
            }
            None => {
                if let Some(at) = last.as_ref()
                    && let Some(held) = found.get_mut(at)
                {
                    held.push('\n');
                    held.push_str(line);
                }
            }
        }
    }
    found
}

/// One value an answer carries.
///
/// # Panics
///
/// Panics when the answer carries no such field, naming what it did carry.
pub fn at(answered: &BTreeMap<String, String>, path: &str) -> String {
    answered
        .get(path)
        .unwrap_or_else(|| panic!("the answer carries no `{path}`: {answered:#?}"))
        .clone()
}
