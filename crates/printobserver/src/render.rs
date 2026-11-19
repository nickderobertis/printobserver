//! The two renderings of one answer, carrying the same fields.
//!
//! # One document, two ways of reading it
//!
//! The answer this program prints is the document the supervisor sent. Asked
//! for machine-readable output it prints that document; asked for nothing it
//! prints the same values as labelled lines. Neither rendering computes a
//! value, drops one or adds one: [`fields`] enumerates the leaves of the
//! document once and both renderings are made from that enumeration, so the
//! two cannot carry different fields.
//!
//! # Nothing here can put an image's bytes anywhere
//!
//! Both renderings carry values the answer carried and nothing else, and no
//! answer this server declares has a byte-sequence field or a string field
//! carrying image content. The one exception is stated where it is made: an
//! image path that names no file on this host is replaced by a message saying
//! so, which is this program's own text and is the only text of its own that
//! either rendering carries.

use printobserver_types::serde_json::Value;

/// Which rendering of an answer a caller asked for.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Rendering {
    /// Labelled lines of text.
    Labelled,
    /// The document itself.
    Machine,
}

impl Rendering {
    /// The rendering the machine-readable flag asks for, or the other one.
    #[must_use]
    pub const fn asked_for(machine_readable: bool) -> Self {
        if machine_readable {
            Self::Machine
        } else {
            Self::Labelled
        }
    }
}

pub use printobserver_types::labelled::{LABEL_SEPARATOR, PATH_SEPARATOR, fields};

/// One answer, in the rendering a caller asked for.
///
/// # Panics
///
/// Panics when a document that was parsed from JSON cannot be rendered back to
/// it, which no value reaching here can be.
#[must_use]
pub fn render(document: &Value, rendering: Rendering) -> String {
    match rendering {
        Rendering::Machine => {
            let mut rendered = printobserver_types::serde_json::to_string_pretty(document)
                .unwrap_or_else(|error| panic!("a parsed document renders back: {error}"));
            rendered.push('\n');
            rendered
        }
        Rendering::Labelled => fields(document)
            .iter()
            .map(printobserver_types::labelled::line)
            .collect(),
    }
}
