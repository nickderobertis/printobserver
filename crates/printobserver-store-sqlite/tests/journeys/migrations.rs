//! Migrations are versioned, applied on open, and forward-only.
//!
//! A database created under each prior version is opened and read back through
//! the port, so a step is proven over rows that were written before it ran
//! rather than over an empty database. The rows are seeded with the column
//! order that version declares — a step that changed a table's columns would
//! need its own seeding here, which is the point: the version a database was
//! written at is a fact about the database, not about this build.

use std::path::Path;

use crate::block_on::block_on;
use crate::fixture::manifest;
use printobserver_store_api::{HistoryQuery, ImageLookup, StoreError, StorePort};
use printobserver_store_sqlite::{
    CURRENT_SCHEMA_VERSION, DATABASE_FILE_NAME, MIGRATIONS, SqliteStore,
};
use printobserver_types::serde_json;
use printobserver_types::{
    ActionId, Actor, Adjustable, EventId, EventPayload, ExecutionOutcome, ImageId,
    InterventionOutcome, ObicoFailureAlertPayload, PolicyDecision, PrintAction, PrintId, Timestamp,
};
use rusqlite::Connection;
use tempfile::TempDir;

/// The identifiers the seeded rows carry, fixed so a read-back names them.
const PRINT: &str = "0191f0a0-0000-7000-8000-0000000000a1";
/// The seeded event.
const EVENT: &str = "0191f0a0-0000-7000-8000-0000000000a2";
/// The seeded image.
const IMAGE: &str = "0191f0a0-0000-7000-8000-0000000000a3";
/// The seeded action.
const ACTION: &str = "0191f0a0-0000-7000-8000-0000000000a4";
/// The seeded intervention.
const INTERVENTION: &str = "0191f0a0-0000-7000-8000-0000000000a5";

/// The instant every seeded record carries, in the store's own spelling.
const SEEDED_AT: &str = "2026-03-01T12:00:00.000000000Z";

/// The payload the seeded event carries.
fn seeded_payload() -> EventPayload {
    EventPayload::ObicoFailureAlert(ObicoFailureAlertPayload {
        is_warning: false,
        print_paused: true,
        obico_print_id: Some(7),
        file_name: Some("bracket.gcode".to_owned()),
    })
}

/// One value, as the JSON text a column holds it as.
fn json(value: &impl printobserver_types::serde::Serialize) -> String {
    serde_json::to_string(value).expect("a canonical value serializes")
}

/// One text value a seeded column holds.
fn text(value: &str) -> rusqlite::types::Value {
    rusqlite::types::Value::Text(value.to_owned())
}

/// A database at one prior version, with the rows that version can hold.
fn database_at(path: &Path, version: u32) -> Connection {
    let connection = Connection::open(path).expect("a database opens");
    connection
        .pragma_update(None, "foreign_keys", "ON")
        .expect("the pragma sets");
    for step in MIGRATIONS.iter().filter(|step| step.version <= version) {
        connection
            .execute_batch(step.sql)
            .unwrap_or_else(|error| panic!("migration {} applies: {error}", step.version));
    }
    connection
        .pragma_update(None, "user_version", i64::from(version))
        .expect("the version is recorded");
    if version >= 1 {
        seed(&connection);
    }
    connection
}

/// One row in every table the first version declares.
fn seed(connection: &Connection) {
    let statements: Vec<(&str, Vec<rusqlite::types::Value>)> = vec![
        (
            "INSERT INTO prints (id, obico_print_id, file_name, state, opened_at, ended_at, \
             end_reason, narrowings) VALUES (?1, ?2, ?3, ?4, ?5, NULL, NULL, ?6)",
            vec![
                text(PRINT),
                7_i64.into(),
                text("bracket.gcode"),
                text(&json(&printobserver_types::PrinterState::Printing)),
                text(SEEDED_AT),
                text("[]"),
            ],
        ),
        (
            "INSERT INTO events (id, print_id, source, received_at, kind, payload, raw) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, NULL)",
            vec![
                text(EVENT),
                text(PRINT),
                text("obico"),
                text(SEEDED_AT),
                text("obico_failure_alert"),
                text(&json(&seeded_payload())),
            ],
        ),
        (
            "INSERT INTO images (id, print_id, event_id, source_url, fetched_at, content_type, \
             byte_len, sha256, relative_path) VALUES (?1, ?2, ?3, NULL, ?4, ?5, ?6, ?7, ?8)",
            vec![
                text(IMAGE),
                text(PRINT),
                text(EVENT),
                text(SEEDED_AT),
                text("image/jpeg"),
                4_i64.into(),
                text("abcd"),
                text("images/ab/abcd"),
            ],
        ),
        (
            "INSERT INTO actions (id, print_id, action, actor, requested_at, decision) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            vec![
                text(ACTION),
                text(PRINT),
                text(&json(&PrintAction::Pause {
                    reason: "the first layer lifted".to_owned(),
                    actor: Actor::Operator,
                })),
                text(&json(&Actor::Operator)),
                text(SEEDED_AT),
                text(&json(&PolicyDecision::Accepted)),
            ],
        ),
        (
            "INSERT INTO interventions (id, print_id, action_id, adjustable, prior_value, \
             applied_value, applied_at, expires_at, restored_at, outcome) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, NULL, ?9)",
            vec![
                text(INTERVENTION),
                text(PRINT),
                text(ACTION),
                text(&Adjustable::Feedrate.to_string()),
                1.0_f64.into(),
                0.8_f64.into(),
                text(SEEDED_AT),
                text("2026-03-01T12:30:00.000000000Z"),
                text(&json(&InterventionOutcome::StillActive)),
            ],
        ),
        (
            "INSERT INTO manifests (print_id, manifest) VALUES (?1, ?2)",
            vec![text(PRINT), text(&json(&manifest()))],
        ),
        (
            "INSERT INTO sessions (print_id, session_name, harness_identity, created_at, \
             last_turn_at, closed_at, close_reason) VALUES (?1, ?2, ?3, ?4, ?5, NULL, NULL)",
            vec![
                text(PRINT),
                text("watch-7"),
                text("oneharness"),
                text(SEEDED_AT),
                text(SEEDED_AT),
            ],
        ),
    ];
    for (statement, values) in statements {
        connection
            .execute(statement, rusqlite::params_from_iter(values.iter()))
            .unwrap_or_else(|error| panic!("the seed row applies: {statement}: {error}"));
    }
}

/// One identifier, as this system spells them.
fn identifier<T: std::str::FromStr>(text: &str) -> T
where
    T::Err: core::fmt::Debug,
{
    text.parse().expect("a fixed version 7 identifier")
}

/// Read every seeded record back through the port.
fn read_every_seeded_record_back(store: &SqliteStore) {
    let port: &dyn StorePort = store;
    let print_id: PrintId = identifier(PRINT);
    let at: Timestamp = identifier(SEEDED_AT);

    let print = block_on(port.print(print_id))
        .expect("the print reads")
        .expect("the seeded print survived the migration");
    assert_eq!(print.id, print_id);
    assert_eq!(print.opened_at, at);
    assert_eq!(print.obico_print_id, Some(7));

    let history = block_on(port.history(HistoryQuery {
        print_id,
        kinds: Vec::new(),
        since: None,
        until: None,
        limit: None,
    }))
    .expect("the history reads");
    assert_eq!(history.len(), 1, "the seeded event survived the migration");
    assert_eq!(history[0].id, identifier::<EventId>(EVENT));
    assert_eq!(history[0].received_at, at);
    assert_eq!(history[0].payload, seeded_payload());

    let image = match block_on(port.image(identifier::<ImageId>(IMAGE))).expect("the image reads") {
        ImageLookup::Found { record, .. } | ImageLookup::FileMissing { record } => record,
    };
    assert_eq!(image.id, identifier::<ImageId>(IMAGE));
    assert_eq!(image.fetched_at, at);
    assert_eq!(image.sha256, "abcd");

    // The port answers an action only by recording what the printer made of it,
    // and the record it answers is read out of the row the seed wrote.
    let action = block_on(
        port.record_execution(identifier::<ActionId>(ACTION), ExecutionOutcome::Succeeded),
    )
    .expect("the action reads");
    assert_eq!(action.id, identifier::<ActionId>(ACTION));
    assert_eq!(action.print_id, print_id);
    assert_eq!(action.request.requested_at, at);
    assert_eq!(action.decision, PolicyDecision::Accepted);

    let active = block_on(port.active_interventions(print_id)).expect("the interventions read");
    assert_eq!(active.len(), 1, "the seeded intervention survived");
    assert_eq!(active[0].applied_at, at);
    assert_eq!(active[0].adjustable, Adjustable::Feedrate);

    assert_eq!(
        block_on(port.manifest(print_id)).expect("the manifest reads"),
        Some(manifest())
    );
    let session = block_on(port.session(print_id))
        .expect("the session reads")
        .expect("the seeded session survived");
    assert_eq!(session.created_at, at);
    assert_eq!(session.session_name, "watch-7");
}

/// A database created at each prior version is migrated and read back.
#[test]
fn a_database_at_each_prior_version_is_migrated_and_read_back() {
    for version in 0..CURRENT_SCHEMA_VERSION {
        let dir = TempDir::new().expect("a temporary state directory");
        let path = dir.path().join(DATABASE_FILE_NAME);
        drop(database_at(&path, version));

        let store = SqliteStore::open(dir.path())
            .unwrap_or_else(|error| panic!("a database at version {version} opens: {error}"));
        let connection = printobserver_store_sqlite::connect(&path).expect("a connection opens");
        assert_eq!(
            connection
                .query_row("PRAGMA user_version", [], |row| row.get::<_, i64>(0))
                .expect("the version reads"),
            i64::from(CURRENT_SCHEMA_VERSION),
            "opening a database at version {version} did not bring it to the current one"
        );
        if version >= 1 {
            read_every_seeded_record_back(&store);
        }
    }
}

/// A database from a later version is refused, naming both versions.
#[test]
fn a_database_from_a_later_version_is_refused_naming_both_versions() {
    let dir = TempDir::new().expect("a temporary state directory");
    let path = dir.path().join(DATABASE_FILE_NAME);
    let later = CURRENT_SCHEMA_VERSION + 1;
    let connection = database_at(&path, CURRENT_SCHEMA_VERSION);
    connection
        .pragma_update(None, "user_version", i64::from(later))
        .expect("the version is recorded");
    drop(connection);

    let refusal = SqliteStore::open(dir.path()).expect_err("a later version is refused");
    let StoreError::Database { detail } = &refusal else {
        panic!("a later version was refused as {refusal:?}");
    };
    assert!(
        detail.contains(&later.to_string()) && detail.contains(&CURRENT_SCHEMA_VERSION.to_string()),
        "the refusal names one version rather than both: {detail}"
    );
}

/// The migrations are versioned from one, ascending, and forward-only.
#[test]
fn the_migrations_are_versioned_ascending_and_forward_only() {
    assert_eq!(
        MIGRATIONS[0].version, 1,
        "the first migration is version one"
    );
    for pair in MIGRATIONS.windows(2) {
        assert!(
            pair[1].version > pair[0].version,
            "migration {} does not follow {}",
            pair[1].version,
            pair[0].version
        );
    }
    assert_eq!(
        CURRENT_SCHEMA_VERSION,
        MIGRATIONS[MIGRATIONS.len() - 1].version,
        "the current version is not the last migration's"
    );
    for step in MIGRATIONS {
        assert!(
            !step.sql.trim().is_empty(),
            "migration {} runs nothing",
            step.version
        );
    }
}
