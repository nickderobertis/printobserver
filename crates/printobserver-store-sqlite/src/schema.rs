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

use printobserver_core::store::StoreError;
use rusqlite::Connection;
use rusqlite::functions::FunctionFlags;

use crate::ledger::key_from_canonical;
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
/// its two filters, the expiry sweep, and the lookup by the provider's own
/// identifier — spelled here as the first version named the column.
const V2_READ_INDEXES: &str = "\
CREATE INDEX events_by_print_and_instant ON events(print_id, received_at, id);
CREATE INDEX events_by_print_kind_and_instant ON events(print_id, kind, received_at, id);
CREATE INDEX images_by_event ON images(event_id);
CREATE INDEX actions_by_print ON actions(print_id, requested_at);
CREATE INDEX interventions_by_expiry ON interventions(outcome, expires_at);
CREATE INDEX interventions_by_print ON interventions(print_id, outcome);
CREATE INDEX prints_by_obico_id ON prints(obico_print_id);
";

/// The print's external correlation under a provider-neutral name.
///
/// The column the first version created as `obico_print_id` holds the
/// provider's own identifier for the print, whichever provider reported it,
/// and the record it is read into names it so. Renaming a column renames
/// nothing in an index of its own accord, so the index the second version
/// created over it is re-created under its new name here.
const V3_PROVIDER_PRINT_ID: &str = "\
ALTER TABLE prints RENAME COLUMN obico_print_id TO provider_print_id;
DROP INDEX prints_by_obico_id;
CREATE INDEX prints_by_provider_id ON prints(provider_print_id);
";

/// What reads of the printer saw of a print's job: when it began, and the
/// longest it was seen printing.
///
/// Nullable columns with no backfill: a print recorded before they existed has
/// no sighting anybody recorded, and one guessed now would be a sighting nobody
/// made. Such a print is matched to a running job by its file name alone, as it
/// was when it was written.
const V4_JOB_SIGHTING: &str = "\
ALTER TABLE prints ADD COLUMN job_started_at TEXT;
ALTER TABLE prints ADD COLUMN job_print_time_s INTEGER;
";

/// The filament ledger, and the index the printer-wide timeline is read through.
///
/// `spools.trust` holds the spool's trust as JSON; `charge_key`, in both
/// `charges` and `pending_charges`, holds a charge key's canonical spelling, so
/// that a key is recorded once whichever table it is in; `pending_charges.pending`
/// holds the pending charge as JSON; and `print_settlements.outcome` holds the
/// outcome's `snake_case` name. One unretired spool per tool is the partial
/// unique index rather than a rule the code keeps. The events table is not
/// touched: an event with no print was already one it could hold.
const V5_FILAMENT_LEDGER: &str = "\
CREATE TABLE spools (
    id             TEXT    NOT NULL PRIMARY KEY,
    name           TEXT    NOT NULL,
    material       TEXT    NOT NULL,
    color          TEXT,
    diameter_mm    REAL    NOT NULL,
    density_g_cm3  REAL    NOT NULL,
    initial_g      REAL    NOT NULL,
    remaining_g    REAL    NOT NULL,
    tare_g         REAL,
    loaded_on_tool INTEGER,
    trust          TEXT    NOT NULL,
    registered_at  TEXT    NOT NULL,
    updated_at     TEXT    NOT NULL,
    retired_at     TEXT
) STRICT;
CREATE UNIQUE INDEX spools_one_per_tool ON spools(loaded_on_tool)
    WHERE loaded_on_tool IS NOT NULL AND retired_at IS NULL;
CREATE TABLE print_positions (
    print_id    TEXT    NOT NULL PRIMARY KEY REFERENCES prints(id),
    bytes       INTEGER NOT NULL,
    size_bytes  INTEGER,
    observed_at TEXT    NOT NULL
) STRICT;
CREATE TABLE file_tallies (
    file_name   TEXT    NOT NULL,
    origin      TEXT    NOT NULL,
    generation  INTEGER NOT NULL,
    successes   INTEGER NOT NULL,
    failures    INTEGER NOT NULL,
    observed_at TEXT    NOT NULL,
    PRIMARY KEY (file_name, origin)
) STRICT;
CREATE TABLE charges (
    charge_key TEXT    NOT NULL PRIMARY KEY,
    print_id   TEXT    REFERENCES prints(id),
    tool       INTEGER NOT NULL,
    spool_id   TEXT    REFERENCES spools(id),
    length_mm  REAL    NOT NULL,
    grams      REAL,
    event_id   TEXT    NOT NULL REFERENCES events(id),
    charged_at TEXT    NOT NULL
) STRICT;
CREATE INDEX charges_by_print ON charges(print_id, tool);
CREATE TABLE pending_charges (
    charge_key TEXT    NOT NULL PRIMARY KEY,
    print_id   TEXT    NOT NULL REFERENCES prints(id),
    tool       INTEGER NOT NULL,
    pending    TEXT    NOT NULL,
    event_id   TEXT    NOT NULL REFERENCES events(id),
    pended_at  TEXT    NOT NULL
) STRICT;
CREATE INDEX pending_charges_by_tool ON pending_charges(tool, pended_at);
CREATE TABLE print_settlements (
    print_id   TEXT NOT NULL PRIMARY KEY REFERENCES prints(id),
    outcome    TEXT NOT NULL,
    event_id   TEXT NOT NULL REFERENCES events(id),
    settled_at TEXT NOT NULL
) STRICT;
CREATE INDEX events_by_kind_and_instant ON events(kind, received_at, id);
";

/// The filament ledger made coherent: a retired spool on no tool, a charge's
/// calculation whole or absent, and a charge's print and tool the ones its key
/// names.
///
/// A row an earlier version wrote that this step cannot carry forward as it
/// was is carried forward as the shape now requires and written down in
/// `migration_reports`, so the change is a record rather than a silence: a
/// spool retired while still loaded is unloaded, and a charge whose stored
/// print or tool disagrees with its key, or that names a spool with half a
/// calculation, is reported — and reported again by every read of it.
///
/// A charge's diameter, density and the spool's figures either side of it were
/// held only in its `filament_consumed` event, so they are read back from
/// there. The triggers are the refusals the database makes from here on. A
/// key's tool and watched print are read off it by
/// `printobserver_charge_key_tool` and `printobserver_charge_key_print`, which
/// [`connect`] registers on every connection: the store's own reading of a
/// canonical key, the one every read of a row holds it to, so the database
/// holds a row's projection to its key exactly — an unobserved key's origin
/// and file name may each carry a colon, which no reading in SQL could tell
/// from the separators. A connection that did not register them cannot write
/// a charge at all, rather than write one unchecked.
const V6_COHERENT_LEDGER: &str = "\
CREATE TABLE migration_reports (
    version INTEGER NOT NULL,
    subject TEXT    NOT NULL,
    detail  TEXT    NOT NULL
) STRICT;

INSERT INTO migration_reports (version, subject, detail)
    SELECT 6, 'spool ' || id,
        'spool ' || name || ' (' || id || ') was retired at ' || retired_at || ' while still loaded on tool '
        || loaded_on_tool || '; it is now retired and loaded on no tool'
    FROM spools WHERE retired_at IS NOT NULL AND loaded_on_tool IS NOT NULL;
UPDATE spools SET loaded_on_tool = NULL WHERE retired_at IS NOT NULL;

ALTER TABLE charges ADD COLUMN diameter_mm REAL;
ALTER TABLE charges ADD COLUMN density_g_cm3 REAL;
ALTER TABLE charges ADD COLUMN remaining_before_g REAL;
ALTER TABLE charges ADD COLUMN remaining_after_g REAL;
UPDATE charges SET
    diameter_mm = (SELECT json_extract(e.payload, '$.payload.diameter_mm')
        FROM events e WHERE e.id = charges.event_id),
    density_g_cm3 = (SELECT json_extract(e.payload, '$.payload.density_g_cm3')
        FROM events e WHERE e.id = charges.event_id),
    remaining_before_g = (SELECT json_extract(e.payload, '$.payload.remaining_before_g')
        FROM events e WHERE e.id = charges.event_id),
    remaining_after_g = (SELECT json_extract(e.payload, '$.payload.remaining_after_g')
        FROM events e WHERE e.id = charges.event_id)
    WHERE spool_id IS NOT NULL;
UPDATE charges SET grams = NULL WHERE spool_id IS NULL;
INSERT INTO migration_reports (version, subject, detail)
    SELECT 6, 'charge ' || charge_key,
        'charge ' || charge_key || ' names spool ' || spool_id
        || ' without the whole calculation of what it took; every read of it reports so'
    FROM charges WHERE spool_id IS NOT NULL AND (grams IS NULL OR diameter_mm IS NULL
        OR density_g_cm3 IS NULL OR remaining_before_g IS NULL OR remaining_after_g IS NULL);
INSERT INTO migration_reports (version, subject, detail)
    SELECT 6, 'charge ' || charge_key,
        'charge ' || charge_key || ' stores print ' || ifnull(print_id, 'none') || ' and tool '
        || tool || ', which its key does not name; every read of it reports so'
    FROM charges WHERE printobserver_charge_key_tool(charge_key) IS NOT tool
        OR (charge_key LIKE 'print:%'
            AND printobserver_charge_key_print(charge_key) IS NOT print_id);
UPDATE pending_charges SET pending = json_remove(
    json_set(pending, '$.charge_key', json(json_remove(json_extract(pending, '$.charge_key'), '$.key'))),
    '$.print_id', '$.tool')
    WHERE json_extract(pending, '$.charge_key.key') = 'print'
        AND json_extract(pending, '$.charge_key.print_id') = print_id
        AND json_extract(pending, '$.charge_key.tool') = tool;

CREATE TRIGGER spools_retired_on_no_tool_insert BEFORE INSERT ON spools
    WHEN NEW.retired_at IS NOT NULL AND NEW.loaded_on_tool IS NOT NULL
    BEGIN SELECT RAISE(ABORT, 'spools.lifecycle'); END;
CREATE TRIGGER spools_retired_on_no_tool_update BEFORE UPDATE ON spools
    WHEN NEW.retired_at IS NOT NULL AND NEW.loaded_on_tool IS NOT NULL
    BEGIN SELECT RAISE(ABORT, 'spools.lifecycle'); END;
CREATE TRIGGER charges_coherent_insert BEFORE INSERT ON charges
    WHEN printobserver_charge_key_tool(NEW.charge_key) IS NOT NEW.tool
        OR (NEW.charge_key LIKE 'print:%'
            AND printobserver_charge_key_print(NEW.charge_key) IS NOT NEW.print_id)
        OR ((NEW.spool_id IS NULL) <> (NEW.grams IS NULL)
            OR (NEW.spool_id IS NULL) <> (NEW.diameter_mm IS NULL)
            OR (NEW.spool_id IS NULL) <> (NEW.density_g_cm3 IS NULL)
            OR (NEW.spool_id IS NULL) <> (NEW.remaining_before_g IS NULL)
            OR (NEW.spool_id IS NULL) <> (NEW.remaining_after_g IS NULL))
    BEGIN SELECT RAISE(ABORT, 'charges.coherent'); END;
CREATE TRIGGER charges_coherent_update BEFORE UPDATE ON charges
    WHEN printobserver_charge_key_tool(NEW.charge_key) IS NOT NEW.tool
        OR (NEW.charge_key LIKE 'print:%'
            AND printobserver_charge_key_print(NEW.charge_key) IS NOT NEW.print_id)
        OR ((NEW.spool_id IS NULL) <> (NEW.grams IS NULL)
            OR (NEW.spool_id IS NULL) <> (NEW.diameter_mm IS NULL)
            OR (NEW.spool_id IS NULL) <> (NEW.density_g_cm3 IS NULL)
            OR (NEW.spool_id IS NULL) <> (NEW.remaining_before_g IS NULL)
            OR (NEW.spool_id IS NULL) <> (NEW.remaining_after_g IS NULL))
    BEGIN SELECT RAISE(ABORT, 'charges.coherent'); END;
";

/// Every migration, in the order they are applied.
///
/// Forward-only: a step is never rewritten once it has run anywhere, because a
/// database in the field was created by the text as it stood — which is why
/// the first two steps still spell the column the first version created it
/// as, and the third renames it.
pub const MIGRATIONS: [Migration; 6] = [
    Migration {
        version: 1,
        sql: V1_TABLES,
    },
    Migration {
        version: 2,
        sql: V2_READ_INDEXES,
    },
    Migration {
        version: 3,
        sql: V3_PROVIDER_PRINT_ID,
    },
    Migration {
        version: 4,
        sql: V4_JOB_SIGHTING,
    },
    Migration {
        version: 5,
        sql: V5_FILAMENT_LEDGER,
    },
    Migration {
        version: 6,
        sql: V6_COHERENT_LEDGER,
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
    register_key_readings(&connection)?;
    Ok(connection)
}

/// Register the two readings of a charge key the schema's triggers call: the
/// tool a canonical key names, and the print a watched one names — each
/// `NULL` for a text that is no key's canonical spelling, and the print `NULL`
/// for an unobserved key.
fn register_key_readings(connection: &Connection) -> Result<(), StoreError> {
    let flags = FunctionFlags::SQLITE_UTF8 | FunctionFlags::SQLITE_DETERMINISTIC;
    connection
        .create_scalar_function("printobserver_charge_key_tool", 1, flags, |call| {
            let text: Option<String> = call.get(0)?;
            Ok(text
                .as_deref()
                .and_then(key_from_canonical)
                .map(|key| key.tool()))
        })
        .map_err(|error| database_error(&error))?;
    connection
        .create_scalar_function("printobserver_charge_key_print", 1, flags, |call| {
            let text: Option<String> = call.get(0)?;
            Ok(text
                .as_deref()
                .and_then(key_from_canonical)
                .and_then(|key| key.watched_print())
                .map(|print_id| print_id.to_string()))
        })
        .map_err(|error| database_error(&error))
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

/// One row a migration could not carry forward as it was, and what it did
/// instead.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MigrationNote {
    /// The version whose step wrote it.
    pub version: u32,
    /// The record it is about, such as `spool <id>`.
    pub subject: String,
    /// What the step found and did, in a sentence.
    pub detail: String,
}

impl core::fmt::Display for MigrationNote {
    fn fmt(&self, formatter: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(
            formatter,
            "migrating the store to schema version {}: {}",
            self.version, self.detail
        )
    }
}

/// Every note a migration wrote, from steps after `after`, in the order
/// written.
fn notes_after(connection: &Connection, after: u32) -> Result<Vec<MigrationNote>, StoreError> {
    let exists: bool = connection
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type = 'table' \
             AND name = 'migration_reports')",
            [],
            |row| row.get(0),
        )
        .map_err(|error| database_error(&error))?;
    if !exists {
        return Ok(Vec::new());
    }
    let mut statement = connection
        .prepare(
            "SELECT version, subject, detail FROM migration_reports WHERE version > ?1 \
             ORDER BY rowid",
        )
        .map_err(|error| database_error(&error))?;
    let rows = statement
        .query_map([i64::from(after)], |row| {
            Ok(MigrationNote {
                version: u32::try_from(row.get::<_, i64>(0)?).unwrap_or(u32::MAX),
                subject: row.get(1)?,
                detail: row.get(2)?,
            })
        })
        .map_err(|error| database_error(&error))?;
    rows.collect::<rusqlite::Result<Vec<_>>>()
        .map_err(|error| database_error(&error))
}

/// Every note any migration of this database wrote, in the order written.
///
/// # Errors
///
/// Returns [`StoreError::Database`] when the report cannot be read.
pub fn migration_report(connection: &Connection) -> Result<Vec<MigrationNote>, StoreError> {
    notes_after(connection, 0)
}

/// Bring a database to [`CURRENT_SCHEMA_VERSION`], refusing a later one, and
/// answer the notes the steps this call applied wrote.
///
/// # Errors
///
/// Returns [`StoreError::Database`] naming both versions when the database is
/// from a later version than this build understands, and when a step fails.
pub fn migrate(connection: &mut Connection) -> Result<Vec<MigrationNote>, StoreError> {
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
    notes_after(connection, found)
}

/// Open the state directory's database, creating and migrating it, with the
/// notes the steps this open applied wrote.
///
/// # Errors
///
/// Returns [`StoreError::Io`] when the state directory cannot be created, and
/// [`StoreError::Database`] when the database cannot be opened or migrated.
pub fn open_database(state_dir: &Path) -> Result<(Connection, Vec<MigrationNote>), StoreError> {
    std::fs::create_dir_all(state_dir).map_err(|error| io_error(state_dir, &error))?;
    let mut connection = connect(&state_dir.join(DATABASE_FILE_NAME))?;
    let notes = migrate(&mut connection)?;
    Ok((connection, notes))
}
