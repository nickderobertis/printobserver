//! The schema enforces every relationship the contracts declare.
//!
//! The set of relationships is derived from the contracts' own record types
//! rather than read off this crate's schema, so a reference whose foreign key
//! was never declared is caught rather than passed over — a check that read the
//! schema would agree with whatever the schema happened to say.

#[path = "support/block_on.rs"]
mod block_on;
#[path = "support/contracts.rs"]
mod contracts;
#[path = "support/fixture.rs"]
mod fixture;
#[path = "support/surface.rs"]
mod surface;

use block_on::block_on;
use contracts::{Reference, record_kinds, references};
use fixture::{draft, instant, manifest, request, session};
use printobserver_store_api::{StoreError, StorePort};
use printobserver_store_sqlite::{DATABASE_FILE_NAME, MIGRATIONS, SqliteStore, connect};
use printobserver_types::{
    ActionId, Adjustable, ExecutionOutcome, PolicyDecision, PrintId, PrinterState, RawBytes,
};
use rusqlite::Connection;
use rusqlite::types::Value;
use tempfile::TempDir;

/// A store carrying one row in every table, and the directory it lives in.
fn seeded() -> TempDir {
    let dir = TempDir::new().expect("a temporary state directory");
    let store = SqliteStore::open(dir.path()).expect("the store opens");
    let port: &dyn StorePort = &store;

    let print = block_on(port.open_print(Some(7), Some("bracket.gcode".to_owned())))
        .expect("a print opens");
    let event = block_on(port.append_event(draft(
        Some(print.id),
        "obico_failure_alert",
        instant("2026-03-01T12:00:00Z"),
    )))
    .expect("an event is appended");
    block_on(port.put_image(
        print.id,
        event.id,
        None,
        "image/jpeg".to_owned(),
        RawBytes::new(b"a snapshot".to_vec()),
    ))
    .expect("an image is stored");
    let action = block_on(port.record_action(
        request(instant("2026-03-01T12:01:00Z")),
        PolicyDecision::Accepted,
    ))
    .expect("an action is recorded");
    block_on(port.record_execution(action.id, ExecutionOutcome::Succeeded))
        .expect("an execution is recorded");
    block_on(port.open_intervention(
        action.id,
        Adjustable::Feedrate,
        Some(1.0),
        0.8,
        instant("2026-03-01T12:02:00Z"),
        instant("2026-03-01T12:32:00Z"),
    ))
    .expect("an intervention opens");
    block_on(port.put_manifest(print.id, manifest())).expect("a manifest is stored");
    block_on(port.put_session(session(print.id, instant("2026-03-01T11:59:00Z"))))
        .expect("a session is stored");
    dir
}

/// The connection this crate opens onto a seeded database.
fn opened(dir: &TempDir) -> Connection {
    connect(&dir.path().join(DATABASE_FILE_NAME)).expect("a connection opens")
}

/// Whether this connection enforces the foreign keys the schema declares.
fn enforced(connection: &Connection) -> bool {
    connection
        .query_row("PRAGMA foreign_keys", [], |row| row.get::<_, i64>(0))
        .expect("the pragma answers")
        == 1
}

/// The foreign keys one table declares, as `(column, table, column)`.
fn declared_keys(connection: &Connection, table: &str) -> Vec<(String, String, String)> {
    let mut statement = connection
        .prepare(&format!("PRAGMA foreign_key_list({table})"))
        .expect("the pragma prepares");
    let rows = statement
        .query_map([], |row| {
            Ok((
                row.get::<_, String>("from")?,
                row.get::<_, String>("table")?,
                row.get::<_, Option<String>>("to")?.unwrap_or_default(),
            ))
        })
        .expect("the pragma answers");
    rows.map(|row| row.expect("a pragma row")).collect()
}

/// Every derived reference this database does not declare and enforce.
fn findings(connection: &Connection, wanted: &[Reference]) -> Vec<String> {
    let mut found = Vec::new();
    if !enforced(connection) {
        found.push("this connection does not enforce the foreign keys it declares".to_owned());
    }
    for reference in wanted {
        let declared = declared_keys(connection, &reference.table);
        let matched = declared.iter().any(|(column, table, to)| {
            *column == reference.column && *table == reference.references && to == "id"
        });
        if !matched {
            found.push(format!(
                "{}.{} is no declared foreign key to {}(id): {} declares {declared:?}",
                reference.table, reference.column, reference.references, reference.table
            ));
        }
    }
    found
}

/// One column of a table: its name, and whether it is part of the primary key.
fn columns(connection: &Connection, table: &str) -> Vec<(String, bool)> {
    let mut statement = connection
        .prepare(&format!("PRAGMA table_info({table})"))
        .expect("the pragma prepares");
    let rows = statement
        .query_map([], |row| {
            Ok((row.get::<_, String>("name")?, row.get::<_, i64>("pk")? > 0))
        })
        .expect("the pragma answers");
    rows.map(|row| row.expect("a pragma row")).collect()
}

/// Attempt a write naming a value no row has, and answer what refused it.
fn attempt_orphan(connection: &Connection, reference: &Reference) -> rusqlite::Error {
    let table = &reference.table;
    let columns = columns(connection, table);
    let names: Vec<String> = columns.iter().map(|(name, _)| name.clone()).collect();
    let existing: Vec<Value> = connection
        .query_row(&format!("SELECT * FROM {table} LIMIT 1"), [], |row| {
            (0..names.len()).map(|index| row.get(index)).collect()
        })
        .unwrap_or_else(|error| panic!("the seed wrote no row into {table}: {error}"));

    let values: Vec<Value> = columns
        .iter()
        .zip(existing)
        .map(|((name, is_key), value)| {
            if *name == reference.column || *is_key {
                Value::Text(PrintId::new().to_string())
            } else {
                value
            }
        })
        .collect();
    let placeholders: Vec<String> = (1..=names.len()).map(|index| format!("?{index}")).collect();
    let statement = format!(
        "INSERT INTO {table} ({}) VALUES ({})",
        names.join(", "),
        placeholders.join(", ")
    );

    let transaction = connection
        .unchecked_transaction()
        .expect("a transaction opens");
    let error = transaction
        .execute(&statement, rusqlite::params_from_iter(values.iter()))
        .expect_err(&format!(
            "a row naming no {} was accepted into {table}",
            reference.references
        ));
    drop(transaction);
    error
}

/// Whether a refusal is the foreign key's own.
fn is_foreign_key_refusal(error: &rusqlite::Error) -> bool {
    matches!(
        error,
        rusqlite::Error::SqliteFailure(failure, _)
            if failure.extended_code == rusqlite::ffi::SQLITE_CONSTRAINT_FOREIGNKEY
    )
}

/// Every reference the contracts declare is declared and enforced here.
#[test]
fn every_reference_the_contracts_declare_is_a_declared_and_enforced_foreign_key() {
    let wanted = references();
    assert!(
        wanted.len() >= 6,
        "the derivation found only {} references, which is not this contract: {wanted:?}",
        wanted.len()
    );
    assert!(
        record_kinds().len() >= 7,
        "the derivation found only {} record kinds",
        record_kinds().len()
    );

    let dir = seeded();
    let connection = opened(&dir);
    assert_eq!(
        findings(&connection, &wanted),
        Vec::<String>::new(),
        "the database this crate created does not declare and enforce every \
         reference the contracts declare"
    );

    for reference in &wanted {
        let error = attempt_orphan(&connection, reference);
        assert!(
            is_foreign_key_refusal(&error),
            "a write naming no {} in {}.{} was refused by something other than \
             the foreign key: {error}",
            reference.references,
            reference.table,
            reference.column
        );
    }
}

/// The check refuses a schema whose declared reference is absent.
#[test]
fn the_check_refuses_a_schema_with_a_reference_removed() {
    let weakened = MIGRATIONS[0]
        .sql
        .replacen("TEXT REFERENCES prints(id)", "TEXT", 1);
    assert_ne!(weakened, MIGRATIONS[0].sql, "the fixture removed nothing");
    let connection = Connection::open_in_memory().expect("an in-memory database opens");
    connection
        .pragma_update(None, "foreign_keys", "ON")
        .expect("the pragma sets");
    connection
        .execute_batch(&weakened)
        .expect("the weakened schema applies");

    let found = findings(&connection, &references());
    assert!(
        found
            .iter()
            .any(|finding| finding.contains("events.print_id")),
        "the check did not refuse a schema whose events.print_id reference is gone: {found:?}"
    );
}

/// The check refuses a connection that does not enforce what it declares.
///
/// Foreign keys are enforced per connection, so a schema declaring every one of
/// them can still be reached over a connection that refuses nothing. The
/// fixture is such a connection, which is what `connect` turning them on
/// explicitly — rather than resting on how the driver happens to be built —
/// answers.
#[test]
fn the_check_refuses_a_connection_that_does_not_enforce_them() {
    let dir = seeded();
    let unenforcing =
        Connection::open(dir.path().join(DATABASE_FILE_NAME)).expect("a connection opens");
    unenforcing
        .pragma_update(None, "foreign_keys", "OFF")
        .expect("the pragma sets");
    let found = findings(&unenforcing, &references());
    assert!(
        found
            .iter()
            .any(|finding| finding.contains("does not enforce")),
        "the check did not refuse a connection with foreign keys off: {found:?}"
    );
}

/// An action row cannot exist without the decision taken on it.
///
/// Read off the schema the live database declares rather than off this crate's
/// source, because there is no port method that creates an action without a
/// decision for a test to attempt.
#[test]
fn the_action_rows_decision_column_admits_no_absent_value() {
    let dir = seeded();
    let connection = opened(&dir);
    let mut statement = connection
        .prepare("PRAGMA table_info(actions)")
        .expect("the pragma prepares");
    let notnull: Vec<(String, i64)> = statement
        .query_map([], |row| {
            Ok((row.get::<_, String>("name")?, row.get("notnull")?))
        })
        .expect("the pragma answers")
        .map(|row| row.expect("a pragma row"))
        .collect();
    assert_eq!(
        notnull
            .iter()
            .find(|(name, _)| name == "decision")
            .map(|(_, notnull)| *notnull),
        Some(1),
        "the live database's actions table admits an action with no decision: {notnull:?}"
    );
}

/// An execution outcome against no action is refused for the constraint.
#[test]
fn an_execution_outcome_against_no_action_is_refused_for_the_constraint() {
    let dir = TempDir::new().expect("a temporary state directory");
    let store = SqliteStore::open(dir.path()).expect("the store opens");
    let port: &dyn StorePort = &store;
    let absent = ActionId::new();

    assert_eq!(
        block_on(port.record_execution(absent, ExecutionOutcome::Succeeded)),
        Err(StoreError::ConstraintRefused {
            constraint: "executions.action_id".to_owned()
        }),
        "an outcome against an action nothing minted was not refused for the constraint"
    );

    let absent_print = PrintId::new();
    assert_eq!(
        block_on(port.end_print(
            absent_print,
            PrinterState::Operational,
            instant("2026-03-01T12:00:00Z"),
            "nothing".to_owned()
        )),
        Err(StoreError::NotFound {
            what: format!("print {absent_print}")
        }),
        "the two answers are told apart by the variant each carries, and this is \
         the other one"
    );
}
