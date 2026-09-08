//! The persisted schema, its versions, and how a database is opened at one.
//!
//! Every relationship the contracts declare between records is a declared and
//! enforced foreign key here rather than a rule the code above the database
//! keeps: a reference the schema does not declare is one a later writer can
//! break with no diagnostic at all, and this database outlives every process
//! that writes it.
//!
//! Two constraints are worth naming because neither is a foreign key:
//! `actions.decision` is `NOT NULL`, so an action row cannot exist without the
//! policy decision taken on it; and an execution outcome is a row of the
//! `executions` table whose primary key references `actions`, so an outcome
//! cannot exist without the action it is the outcome of. Both are refusals the
//! database makes rather than checks the store performs.

use std::path::Path;
use std::time::Duration;

use printobserver_store_api::StoreError;
use rusqlite::Connection;

use crate::values::{database_error, io_error};

/// The database's file name under the configured state directory.
pub const DATABASE_FILE_NAME: &str = "printobserver.sqlite3";

/// How long a connection waits for a lock before refusing.
///
/// The store's own declared bound, so that a test proving a reader is not
/// serialised behind a writer has a finite refusal to distinguish an answer
/// from, rather than a latency it invented.
pub const LOCK_TIMEOUT: Duration = Duration::from_secs(5);

/// One forward-only step of the schema.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Migration {
    /// The version this step brings a database to.
    pub version: u32,
    /// The statements that bring it there.
    pub sql: &'static str,
}

/// Every table the contracts' records are held in.
///
/// `executions` is the one table here that is not a record the contracts
/// declare: it is how "an execution outcome cannot exist without its action"
/// is expressed as a constraint rather than as a rule the code keeps.
const V1_TABLES: &str = "\
CREATE TABLE prints (
    id             TEXT    NOT NULL PRIMARY KEY,
    obico_print_id INTEGER,
    file_name      TEXT,
    state          TEXT    NOT NULL,
    opened_at      TEXT    NOT NULL,
    ended_at       TEXT,
    end_reason     TEXT,
    narrowings     TEXT    NOT NULL
) STRICT;

CREATE TABLE events (
    id          TEXT NOT NULL PRIMARY KEY,
    print_id    TEXT REFERENCES prints(id),
    source      TEXT NOT NULL,
    received_at TEXT NOT NULL,
    kind        TEXT NOT NULL,
    payload     TEXT NOT NULL,
    raw         BLOB
) STRICT;

CREATE TABLE images (
    id            TEXT    NOT NULL PRIMARY KEY,
    print_id      TEXT    NOT NULL REFERENCES prints(id),
    event_id      TEXT    NOT NULL REFERENCES events(id),
    source_url    TEXT,
    fetched_at    TEXT    NOT NULL,
    content_type  TEXT    NOT NULL,
    byte_len      INTEGER NOT NULL,
    sha256        TEXT    NOT NULL,
    relative_path TEXT    NOT NULL
) STRICT;

CREATE TABLE actions (
    id           TEXT NOT NULL PRIMARY KEY,
    print_id     TEXT NOT NULL REFERENCES prints(id),
    action       TEXT NOT NULL,
    actor        TEXT NOT NULL,
    requested_at TEXT NOT NULL,
    decision     TEXT NOT NULL
) STRICT;

CREATE TABLE executions (
    action_id   TEXT NOT NULL PRIMARY KEY REFERENCES actions(id),
    executed_at TEXT NOT NULL,
    outcome     TEXT NOT NULL
) STRICT;

CREATE TABLE interventions (
    id            TEXT NOT NULL PRIMARY KEY,
    print_id      TEXT NOT NULL REFERENCES prints(id),
    action_id     TEXT NOT NULL REFERENCES actions(id),
    adjustable    TEXT NOT NULL,
    prior_value   REAL,
    applied_value REAL NOT NULL,
    applied_at    TEXT NOT NULL,
    expires_at    TEXT NOT NULL,
    restored_at   TEXT,
    outcome       TEXT NOT NULL
) STRICT;

CREATE TABLE manifests (
    print_id TEXT NOT NULL PRIMARY KEY REFERENCES prints(id),
    manifest TEXT NOT NULL
) STRICT;

CREATE TABLE sessions (
    print_id         TEXT NOT NULL PRIMARY KEY REFERENCES prints(id),
    session_name     TEXT NOT NULL,
    harness_identity TEXT NOT NULL,
    created_at       TEXT NOT NULL,
    last_turn_at     TEXT NOT NULL,
    closed_at        TEXT,
    close_reason     TEXT
) STRICT;
";

/// The indexes the reads this port declares are answered through.
///
/// A step of its own rather than part of the tables, because it is derived from
/// the *reads* rather than from the records: the history read's ordering and
/// its two filters, the expiry sweep, and the lookup by Obico's own identifier.
const V2_READ_INDEXES: &str = "\
CREATE INDEX events_by_print_and_instant ON events(print_id, received_at, id);
CREATE INDEX events_by_print_kind_and_instant ON events(print_id, kind, received_at, id);
CREATE INDEX images_by_event ON images(event_id);
CREATE INDEX actions_by_print ON actions(print_id, requested_at);
CREATE INDEX interventions_by_expiry ON interventions(outcome, expires_at);
CREATE INDEX interventions_by_print ON interventions(print_id, outcome);
CREATE INDEX prints_by_obico_id ON prints(obico_print_id);
";

/// Every migration, in the order they are applied.
///
/// Forward-only: a step is never rewritten once it has run anywhere, because a
/// database in the field was created by the text as it stood.
pub const MIGRATIONS: [Migration; 2] = [
    Migration {
        version: 1,
        sql: V1_TABLES,
    },
    Migration {
        version: 2,
        sql: V2_READ_INDEXES,
    },
];

/// The version a database this build creates is at.
pub const CURRENT_SCHEMA_VERSION: u32 = MIGRATIONS[MIGRATIONS.len() - 1].version;

/// Open one connection onto a database, in the mode this store needs.
///
/// Write-ahead logging is what makes a read taken while a writer holds an open
/// transaction answer the pre-write state rather than wait for the commit, and
/// foreign keys are enforced per connection rather than per database, so a
/// connection that did not ask for them would not be refused by them.
///
/// # Errors
///
/// Returns [`StoreError::Database`] when the database cannot be opened or the
/// connection cannot be put in that mode.
pub fn connect(path: &Path) -> Result<Connection, StoreError> {
    let connection = Connection::open(path).map_err(|error| database_error(&error))?;
    connection
        .busy_timeout(LOCK_TIMEOUT)
        .map_err(|error| database_error(&error))?;
    connection
        .query_row("PRAGMA journal_mode = WAL", [], |row| {
            row.get::<_, String>(0)
        })
        .map_err(|error| database_error(&error))?;
    connection
        .pragma_update(None, "foreign_keys", "ON")
        .map_err(|error| database_error(&error))?;
    connection
        .pragma_update(None, "synchronous", "NORMAL")
        .map_err(|error| database_error(&error))?;
    Ok(connection)
}

/// The version a database is at.
fn version_of(connection: &Connection) -> Result<u32, StoreError> {
    connection
        .query_row("PRAGMA user_version", [], |row| row.get::<_, i64>(0))
        .map_err(|error| database_error(&error))
        .and_then(|version| {
            u32::try_from(version).map_err(|_| StoreError::Database {
                detail: format!("the database records the impossible schema version {version}"),
            })
        })
}

/// Bring a database to [`CURRENT_SCHEMA_VERSION`], refusing a later one.
///
/// # Errors
///
/// Returns [`StoreError::Database`] naming both versions when the database is
/// from a later version than this build understands, and when a step fails.
pub fn migrate(connection: &mut Connection) -> Result<(), StoreError> {
    let found = version_of(connection)?;
    if found > CURRENT_SCHEMA_VERSION {
        return Err(StoreError::Database {
            detail: format!(
                "the database is at schema version {found}, and this build understands \
                 schema version {CURRENT_SCHEMA_VERSION}: migrations are forward-only, \
                 so it is refused rather than opened"
            ),
        });
    }
    for step in MIGRATIONS.iter().filter(|step| step.version > found) {
        let transaction = connection
            .transaction()
            .map_err(|error| database_error(&error))?;
        transaction
            .execute_batch(step.sql)
            .map_err(|error| StoreError::Database {
                detail: format!("migration to version {}: {error}", step.version),
            })?;
        transaction
            .pragma_update(None, "user_version", i64::from(step.version))
            .map_err(|error| database_error(&error))?;
        transaction
            .commit()
            .map_err(|error| database_error(&error))?;
    }
    Ok(())
}

/// Open the state directory's database, creating and migrating it.
///
/// # Errors
///
/// Returns [`StoreError::Io`] when the state directory cannot be created, and
/// [`StoreError::Database`] when the database cannot be opened or migrated.
pub fn open_database(state_dir: &Path) -> Result<Connection, StoreError> {
    std::fs::create_dir_all(state_dir).map_err(|error| io_error(state_dir, &error))?;
    let mut connection = connect(&state_dir.join(DATABASE_FILE_NAME))?;
    migrate(&mut connection)?;
    Ok(connection)
}
