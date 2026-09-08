//! Turning the domain's values into stored ones and back.
//!
//! Every timestamp is stored in exactly one representation: the RFC 3339
//! spelling of its UTC instant, at a fixed nine fractional digits. The fixed
//! width is what makes the text sort as the instant does, which is what the
//! history read's ordering and the audit read's cursor rest on — the variable
//! precision a human-facing spelling uses would sort `12:00:00.5Z` before
//! `12:00:00Z`.

use std::path::Path;
use std::str::FromStr;

use chrono::SecondsFormat;
use printobserver_store_api::StoreError;
use printobserver_types::Timestamp;
use printobserver_types::serde::Serialize;
use printobserver_types::serde::de::DeserializeOwned;
use printobserver_types::serde_json::{self, Value};
use rusqlite::Row;
use rusqlite::types::Type;

/// The failure a driver reported, as this port's own vocabulary.
pub(crate) fn database_error(error: &rusqlite::Error) -> StoreError {
    StoreError::Database {
        detail: error.to_string(),
    }
}

/// A file the store could not read or write, as this port's own vocabulary.
pub(crate) fn io_error(path: &Path, error: &std::io::Error) -> StoreError {
    StoreError::Io {
        detail: format!("{}: {error}", path.display()),
    }
}

/// A constraint refused the write.
pub(crate) fn refused(constraint: &str) -> StoreError {
    StoreError::ConstraintRefused {
        constraint: constraint.to_owned(),
    }
}

/// There is no such record.
pub(crate) fn not_found(what: &str) -> StoreError {
    StoreError::NotFound {
        what: what.to_owned(),
    }
}

/// Whether the driver refused a write because a constraint refused it.
pub(crate) fn is_constraint_violation(error: &rusqlite::Error) -> bool {
    matches!(
        error,
        rusqlite::Error::SqliteFailure(failure, _)
            if failure.code == rusqlite::ErrorCode::ConstraintViolation
    )
}

/// One instant, in the one representation this store keeps instants in.
pub(crate) fn instant_text(instant: Timestamp) -> String {
    instant.as_utc().to_rfc3339_opts(SecondsFormat::Nanos, true)
}

/// One value, as the JSON text a column holds it as.
///
/// This crate's own types serialize by construction; a refusal is reported
/// rather than unwrapped so that no path here can panic on a value.
pub(crate) fn json_text<T: Serialize>(value: &T) -> Result<String, StoreError> {
    serde_json::to_string(value).map_err(|error| StoreError::Database {
        detail: format!("a value could not be written as JSON: {error}"),
    })
}

/// The bare spelling of a value whose wire form is one string.
pub(crate) fn tag_text<T: Serialize>(value: &T) -> Result<String, StoreError> {
    match serde_json::to_value(value) {
        Ok(Value::String(text)) => Ok(text),
        Ok(other) => Err(StoreError::Database {
            detail: format!("expected a value spelled as one string, found {other}"),
        }),
        Err(error) => Err(StoreError::Database {
            detail: format!("a value could not be written as JSON: {error}"),
        }),
    }
}

/// A column that could not be read as what the schema says it is.
fn conversion(
    column: usize,
    error: impl core::error::Error + Send + Sync + 'static,
) -> rusqlite::Error {
    rusqlite::Error::FromSqlConversionFailure(column, Type::Text, Box::new(error))
}

/// One column, read as a value with a strict spelling of its own.
pub(crate) fn parsed<T>(row: &Row<'_>, column: usize) -> rusqlite::Result<T>
where
    T: FromStr,
    T::Err: core::error::Error + Send + Sync + 'static,
{
    let text: String = row.get(column)?;
    text.parse().map_err(|error| conversion(column, error))
}

/// One nullable column, read as a value with a strict spelling of its own.
pub(crate) fn parsed_option<T>(row: &Row<'_>, column: usize) -> rusqlite::Result<Option<T>>
where
    T: FromStr,
    T::Err: core::error::Error + Send + Sync + 'static,
{
    let Some(text) = row.get::<_, Option<String>>(column)? else {
        return Ok(None);
    };
    text.parse()
        .map(Some)
        .map_err(|error| conversion(column, error))
}

/// One column, read as the JSON value it holds.
pub(crate) fn from_json<T: DeserializeOwned>(row: &Row<'_>, column: usize) -> rusqlite::Result<T> {
    let text: String = row.get(column)?;
    serde_json::from_str(&text).map_err(|error| conversion(column, error))
}

/// One nullable column, read as the JSON value it holds.
pub(crate) fn from_json_option<T: DeserializeOwned>(
    row: &Row<'_>,
    column: usize,
) -> rusqlite::Result<Option<T>> {
    let Some(text) = row.get::<_, Option<String>>(column)? else {
        return Ok(None);
    };
    serde_json::from_str(&text)
        .map(Some)
        .map_err(|error| conversion(column, error))
}

/// One column, read as a value whose wire form is one string.
pub(crate) fn from_tag<T: DeserializeOwned>(row: &Row<'_>, column: usize) -> rusqlite::Result<T> {
    let text: String = row.get(column)?;
    serde_json::from_value(Value::String(text)).map_err(|error| conversion(column, error))
}
