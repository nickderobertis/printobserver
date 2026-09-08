//! The one piece of caller-supplied text that crosses to a printer's file API.
//!
//! [`FileName`] is a newtype rather than a record: it wraps one string,
//! declares no named fields of its own, and serializes as that string. It is a
//! type rather than a bare string because an agent can ask for a print, so this
//! is the one string in this vocabulary that reaches the printer's own file
//! API, and a name that is not a name is a path-traversal question rather than
//! a typing one.
//!
//! # What it refuses, and what it accepts
//!
//! The rule is one total condition over strings: a value is refused if and only
//! if it carries one of the six forbidden properties [`FileNameRefusal`]
//! declares, and every other value is accepted. The accepted set is not an
//! allowlist of characters, and the NUL byte — already on the forbidden list —
//! is the only character this type refuses for being the character it is. Four
//! policies follow from that and are stated rather than left to follow: a
//! control character other than NUL is **accepted**; a leading or trailing
//! space is **accepted**; a `.` at the start of a name that is not exactly `.`
//! or `..` is **accepted**, the parent-directory property being a whole segment
//! rather than a prefix; a colon that is not a single letter's drive prefix is
//! **accepted**; and a character outside ASCII is **accepted**, this type
//! declaring no script, normalization or encoding policy beyond the string
//! being one.
//!
//! # The separator set is a claim about the world
//!
//! [`SEPARATORS`] is exactly `/` and `\`, and that is a claim about the file
//! APIs this name crosses to rather than a fact about this crate: `OctoPrint`'s
//! file API is served from POSIX hosts, whose separator is `/`, and its own
//! path handling additionally treats `\` as a separator on Windows hosts. A
//! reader who believes another character reaches a separator on some host this
//! system runs against should argue with this set, here, rather than discover
//! its absence as a gap in a corpus.

use core::fmt;
use core::str::FromStr;
use std::borrow::Cow;

use schemars::{JsonSchema, Schema, SchemaGenerator, json_schema};
use serde::{Deserialize, Deserializer, Serialize, Serializer, de::Error as _};

/// The characters this type treats as path separators.
pub const SEPARATORS: [char; 2] = ['/', '\\'];

/// The property that makes a string no file name.
///
/// These six are the whole of what [`FileName`] refuses; a value carrying none
/// of them is accepted.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum FileNameRefusal {
    /// The value is empty.
    Empty,
    /// The value contains a NUL byte.
    Nul,
    /// The value begins with a separator.
    LeadingSeparator,
    /// The value contains a separator.
    Separator,
    /// The value carries a `.` or `..` segment, or is one.
    DotSegment,
    /// The value carries a drive prefix: one letter followed by a colon.
    DrivePrefix,
}

impl FileNameRefusal {
    /// Every forbidden property this type declares, in the order it tests them.
    ///
    /// The whole vocabulary, so that a test ranging over the refused categories
    /// can read them off this type rather than off a list of its own.
    pub const ALL: [Self; 6] = [
        Self::Empty,
        Self::Nul,
        Self::LeadingSeparator,
        Self::Separator,
        Self::DotSegment,
        Self::DrivePrefix,
    ];

    /// One line saying what the property is.
    #[must_use]
    pub const fn detail(self) -> &'static str {
        match self {
            Self::Empty => "it is empty",
            Self::Nul => "it contains a NUL byte",
            Self::LeadingSeparator => "it begins with a path separator",
            Self::Separator => "it contains a path separator",
            Self::DotSegment => "it carries a `.` or `..` segment",
            Self::DrivePrefix => "it carries a drive prefix",
        }
    }
}

impl fmt::Display for FileNameRefusal {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.detail())
    }
}

/// Why a string is no file name.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileNameError {
    /// The property that refused it.
    refusal: FileNameRefusal,
    /// The value that was refused.
    value: String,
}

impl FileNameError {
    /// The property that refused the value.
    #[must_use]
    pub const fn refusal(&self) -> FileNameRefusal {
        self.refusal
    }

    /// The value that was refused.
    #[must_use]
    pub fn value(&self) -> &str {
        &self.value
    }
}

impl fmt::Display for FileNameError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "{:?} is no file name: {}",
            self.value, self.refusal
        )
    }
}

impl core::error::Error for FileNameError {}

/// Whether the value carries a drive prefix: one letter, then a colon.
fn has_drive_prefix(value: &str) -> bool {
    let mut characters = value.chars();
    let Some(first) = characters.next() else {
        return false;
    };
    first.is_ascii_alphabetic() && characters.next() == Some(':')
}

/// The property that refuses this value, if one does.
fn refusal_for(value: &str) -> Option<FileNameRefusal> {
    if value.is_empty() {
        return Some(FileNameRefusal::Empty);
    }
    if value.contains('\0') {
        return Some(FileNameRefusal::Nul);
    }
    if value.starts_with(SEPARATORS) {
        return Some(FileNameRefusal::LeadingSeparator);
    }
    if value.contains(SEPARATORS) {
        return Some(FileNameRefusal::Separator);
    }
    if value
        .split(SEPARATORS)
        .any(|segment| segment == "." || segment == "..")
    {
        return Some(FileNameRefusal::DotSegment);
    }
    if has_drive_prefix(value) {
        return Some(FileNameRefusal::DrivePrefix);
    }
    None
}

/// A file name a printer's own file API can be asked for.
///
/// See the [module documentation](self) for the whole of what this type refuses
/// and what it accepts.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct FileName(String);

impl FileName {
    /// Read a file name, refusing a value carrying a forbidden property.
    ///
    /// # Errors
    ///
    /// Returns the property that refused the value.
    pub fn new(value: impl Into<String>) -> Result<Self, FileNameError> {
        let value = value.into();
        match refusal_for(&value) {
            Some(refusal) => Err(FileNameError { refusal, value }),
            None => Ok(Self(value)),
        }
    }

    /// The name, carrying exactly the characters it was given.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for FileName {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl FromStr for FileName {
    type Err = FileNameError;

    fn from_str(text: &str) -> Result<Self, Self::Err> {
        Self::new(text)
    }
}

impl Serialize for FileName {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.0)
    }
}

impl<'de> Deserialize<'de> for FileName {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let text = String::deserialize(deserializer)?;
        Self::new(text).map_err(D::Error::custom)
    }
}

impl JsonSchema for FileName {
    fn schema_name() -> Cow<'static, str> {
        Cow::Borrowed("FileName")
    }

    fn json_schema(_generator: &mut SchemaGenerator) -> Schema {
        json_schema!({
            "type": "string",
            "title": "FileName",
            "description": "A file name a printer's own file API can be asked for: no path \
                            separator, no NUL byte, no `.` or `..` segment, no drive prefix, \
                            and not empty.",
            "minLength": 1,
            "pattern": "^[^/\\\\\u{0}]+$",
            "not": { "pattern": "^([.]{1,2}$|[A-Za-z]:)" }
        })
    }
}
