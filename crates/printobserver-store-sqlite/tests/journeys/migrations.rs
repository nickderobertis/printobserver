//! Migrations are versioned, applied on open, and forward-only.
//!
//! A database created under each prior version is opened and read back through
//! the port, so a step is proven over rows that were written before it ran
//! rather than over an empty database. The rows are seeded with the columns
//! that version declares — the print's external correlation under the name
//! the first version created it as, until the step that renamed it — which is
//! the point: the version a database was written at is a fact about the
//! database, not about this build, and the read-back below asks for the
//! print's correlation under the name the record carries today.

use std::path::Path;

use crate::block_on::block_on;
use crate::fixture::manifest;
use printobserver_core::store::{HistoryQuery, ImageLookup, StoreError};

use crate::fixture::Store;
use printobserver_core::{
    ActionId, Actor, ExecutionOutcome, InterventionOutcome, PolicyDecision, PrintAction,
};
use printobserver_printer_api::Adjustable;
use printobserver_store_sqlite::{
    CURRENT_SCHEMA_VERSION, DATABASE_FILE_NAME, MIGRATIONS, SqliteStore,
};
use printobserver_types::serde_json::{self, json};
use printobserver_types::{EventBody, EventId, EventKind, ImageId, PrintId, Timestamp};
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
/// The seeded event under a kind no crate of this workspace declares.
const UNDECLARED_EVENT: &str = "0191f0a0-0000-7000-8000-0000000000a6";

/// The instant every seeded record carries, in the store's own spelling.
const SEEDED_AT: &str = "2026-03-01T12:00:00.000000000Z";

/// A later instant, so the two seeded events order.
const SEEDED_LATER: &str = "2026-03-01T12:00:01.000000000Z";

/// The version that renamed the print's external correlation column.
const PROVIDER_PRINT_ID_VERSION: u32 = 3;

/// The `kind` column of the seeded event, as 0.2.0 wrote it.
const SEEDED_KIND: &str = "obico_failure_alert";

/// The `payload` column of the seeded event, **as the literal text 0.2.0
/// wrote it**: the closed vocabulary's tagged form, `kind` beside `payload`.
///
/// A literal rather than a value serialized from a type, because what this
/// journey proves is that a row written before the log was opened reads back
/// under the open store — and a text serialized by this build would prove only
/// that this build reads what this build writes.
const SEEDED_PAYLOAD: &str = r#"{"kind":"obico_failure_alert","payload":{"is_warning":false,"print_paused":true,"obico_print_id":7,"file_name":"bracket.gcode"}}"#;

/// The kind of the seeded event no crate declares.
const UNDECLARED_KIND: &str = "kind_from_a_newer_server";

/// The `payload` column of the event no crate declares, in the same pair.
const UNDECLARED_PAYLOAD: &str = r#"{"kind":"kind_from_a_newer_server","payload":{"anything":[1,2,3],"nested":{"deeply":true}}}"#;

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
        seed(&connection, version);
    }
    connection
}

/// The two event rows the first version can hold: one under a kind a crate of
/// this workspace declares, written as 0.2.0 wrote it, and one under a kind
/// none declares.
fn seeded_events() -> Vec<(&'static str, Vec<rusqlite::types::Value>)> {
    vec![
        (
            "INSERT INTO events (id, print_id, source, received_at, kind, payload, raw) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, NULL)",
            vec![
                text(EVENT),
                text(PRINT),
                text("obico"),
                text(SEEDED_AT),
                text(SEEDED_KIND),
                text(SEEDED_PAYLOAD),
            ],
        ),
        (
            "INSERT INTO events (id, print_id, source, received_at, kind, payload, raw) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, NULL)",
            vec![
                text(UNDECLARED_EVENT),
                text(PRINT),
                text("a_newer_server"),
                text(SEEDED_LATER),
                text(UNDECLARED_KIND),
                text(UNDECLARED_PAYLOAD),
            ],
        ),
    ]
}

/// One row in every table the first version declares, under the columns the
/// given version names them by.
fn seed(connection: &Connection, version: u32) {
    let prints_insert = if version >= PROVIDER_PRINT_ID_VERSION {
        "INSERT INTO prints (id, provider_print_id, file_name, state, opened_at, ended_at, \
         end_reason, narrowings) VALUES (?1, ?2, ?3, ?4, ?5, NULL, NULL, ?6)"
    } else {
        "INSERT INTO prints (id, obico_print_id, file_name, state, opened_at, ended_at, \
         end_reason, narrowings) VALUES (?1, ?2, ?3, ?4, ?5, NULL, NULL, ?6)"
    };
    let mut statements: Vec<(&str, Vec<rusqlite::types::Value>)> = vec![
        (
            prints_insert,
            vec![
                text(PRINT),
                7_i64.into(),
                text("bracket.gcode"),
                text(&json(&printobserver_printer_api::PrinterState::Printing)),
                text(SEEDED_AT),
                text("[]"),
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
    // The events reference the print, so they follow it and precede the image
    // that references one of them.
    let image_index = statements
        .iter()
        .position(|(statement, _)| statement.starts_with("INSERT INTO images"))
        .expect("an image row is seeded");
    statements.splice(image_index..image_index, seeded_events());
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

/// Read the seeded print back, by its own identifier and by the provider's.
fn read_the_seeded_print_back(port: &dyn Store) {
    let print_id: PrintId = identifier(PRINT);
    let at: Timestamp = identifier(SEEDED_AT);
    let print = block_on(port.print(print_id))
        .expect("the print reads")
        .expect("the seeded print survived the migration");
    assert_eq!(print.id, print_id);
    assert_eq!(print.opened_at, at);
    assert_eq!(
        print.provider_print_id,
        Some(7),
        "the correlation the first version wrote as obico_print_id did not read back \
         under the record's own name"
    );
    assert_eq!(
        (print.job_started_at, print.job_print_time_s),
        (None, None),
        "a print written before job sightings were recorded read back carrying one"
    );
    assert_eq!(
        block_on(port.print_by_provider_id(7))
            .expect("the lookup reads")
            .map(|found| found.id),
        Some(print_id),
        "the renamed column is not what the lookup by the provider's identifier reads"
    );
    assert_eq!(
        block_on(port.prints()),
        Ok(vec![print]),
        "a database written before prints were listed does not list the print it holds"
    );
}

/// Read every seeded record back through the port.
fn read_every_seeded_record_back(store: &SqliteStore) {
    let port: &dyn Store = store;
    let print_id: PrintId = identifier(PRINT);
    let at: Timestamp = identifier(SEEDED_AT);
    read_the_seeded_print_back(port);

    let history = block_on(port.history(HistoryQuery {
        print_id,
        kinds: Vec::new(),
        since: None,
        until: None,
        limit: None,
    }))
    .expect("the history reads");
    assert_eq!(history.len(), 2, "the seeded events survived the migration");
    // Newest first: the event under the undeclared kind was seeded a second later.
    let undeclared = &history[0];
    assert_eq!(undeclared.id, identifier::<EventId>(UNDECLARED_EVENT));
    assert_eq!(undeclared.source.as_str(), "a_newer_server");
    assert_eq!(
        undeclared.kind(),
        &EventKind::new(UNDECLARED_KIND).expect("a kind name")
    );
    assert_eq!(
        undeclared.body,
        EventBody {
            kind: EventKind::new(UNDECLARED_KIND).expect("a kind name"),
            payload: json!({ "anything": [1, 2, 3], "nested": { "deeply": true } }),
        },
        "a kind no crate declares did not read back as it was written"
    );
    let seeded = &history[1];
    assert_eq!(seeded.id, identifier::<EventId>(EVENT));
    assert_eq!(seeded.received_at, at);
    assert_eq!(
        seeded.kind(),
        &EventKind::new(SEEDED_KIND).expect("a kind name")
    );
    assert_eq!(
        seeded.body,
        EventBody {
            kind: EventKind::new(SEEDED_KIND).expect("a kind name"),
            payload: json!({
                "is_warning": false,
                "print_paused": true,
                "obico_print_id": 7,
                "file_name": "bracket.gcode",
            }),
        },
        "the row 0.2.0 wrote did not read back as the same kind and payload"
    );
    let by_kind = block_on(port.history(HistoryQuery {
        print_id,
        kinds: vec![EventKind::new(UNDECLARED_KIND).expect("a kind name")],
        since: None,
        until: None,
        limit: None,
    }))
    .expect("the filtered history reads");
    assert_eq!(
        by_kind.iter().map(|record| record.id).collect::<Vec<_>>(),
        vec![identifier::<EventId>(UNDECLARED_EVENT)],
        "the kind filter does not reach a kind no crate declares"
    );

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

/// The version before the filament ledger.
const BEFORE_THE_LEDGER: u32 = 4;

/// A database at version 4 gains the ledger, empty, and keeps every record.
#[test]
fn a_database_at_version_4_gains_an_empty_ledger_and_keeps_every_record() {
    let dir = TempDir::new().expect("a temporary state directory");
    let path = dir.path().join(DATABASE_FILE_NAME);
    drop(database_at(&path, BEFORE_THE_LEDGER));

    let store = SqliteStore::open(dir.path()).expect("a version 4 database opens");
    read_every_seeded_record_back(&store);
    let port: &dyn Store = &store;
    let print_id: PrintId = identifier(PRINT);
    assert_eq!(block_on(port.spools(true)), Ok(Vec::new()));
    assert_eq!(block_on(port.charges(print_id)), Ok(Vec::new()));
    assert_eq!(block_on(port.pending_charges(None)), Ok(Vec::new()));
    assert_eq!(block_on(port.print_settlement(print_id)), Ok(None));
    assert_eq!(block_on(port.file_position(print_id)), Ok(None));
    assert_eq!(block_on(port.file_tallies()), Ok(Vec::new()));
    assert_eq!(
        block_on(port.timeline(printobserver_core::store::TimelineQuery {
            kinds: Vec::new(),
            since: None,
            until: None,
            spool_id: None,
            before: None,
            limit: None,
        })),
        Ok(Vec::new()),
        "the events written before the ledger are of no timeline kind"
    );
}

/// The digest of each step written before the ledger, as it was first written.
///
/// A step runs once per database and is never run again, so a step edited after
/// it shipped is a schema the databases in the field never got. These pin the
/// text every one of them was created by.
const SHIPPED_STEPS: [(u32, &str); 4] = [
    (
        1,
        "3a97ca0441e9aeec21c2220ed1bb70e30ccd0787ac0792c25519f3b711f81cb6",
    ),
    (
        2,
        "3c1c382a343805f46613d4dfe8c0a2a5b91a8196496cd00987cd06e90fd7444d",
    ),
    (
        3,
        "ade92c2f7b5506b8805358dcefe9371bfd156d83cce18ea2e4839b901c4f4e32",
    ),
    (
        4,
        "445808843155665fcc47cf56ac654b3655d46ab1d3f0039a6f763e00de025af7",
    ),
];

/// The steps that shipped before the ledger are the text they shipped as.
#[test]
fn the_steps_that_shipped_before_the_ledger_are_unchanged() {
    for (version, digest) in SHIPPED_STEPS {
        let step = MIGRATIONS
            .iter()
            .find(|step| step.version == version)
            .unwrap_or_else(|| panic!("no step brings a database to version {version}"));
        assert_eq!(
            printobserver_store_sqlite::digest_of(step.sql.as_bytes()),
            digest,
            "migration {version} was edited after it shipped"
        );
    }
}

/// One column, as `PRAGMA table_info` reads it: its name, its declared type,
/// whether it is `NOT NULL`, and its place in the primary key.
type Column = (String, String, bool, i64);

/// One column the ledger's step is stated to create.
fn column(name: &str, ty: &str, not_null: bool, key: i64) -> Column {
    (name.to_owned(), ty.to_owned(), not_null, key)
}

/// Every column one table holds.
fn table_columns(connection: &Connection, table: &str) -> Vec<Column> {
    let mut statement = connection
        .prepare(&format!("PRAGMA table_info({table})"))
        .expect("the pragma prepares");
    statement
        .query_map([], |row| {
            Ok((
                row.get("name")?,
                row.get("type")?,
                row.get::<_, i64>("notnull")? == 1,
                row.get("pk")?,
            ))
        })
        .expect("the pragma answers")
        .map(|row| row.expect("a pragma row"))
        .collect()
}

/// Every foreign key one table declares, as `column -> table(column)`.
fn table_keys(connection: &Connection, table: &str) -> Vec<String> {
    let mut statement = connection
        .prepare(&format!("PRAGMA foreign_key_list({table})"))
        .expect("the pragma prepares");
    let mut keys: Vec<String> = statement
        .query_map([], |row| {
            Ok(format!(
                "{} -> {}({})",
                row.get::<_, String>("from")?,
                row.get::<_, String>("table")?,
                row.get::<_, String>("to")?
            ))
        })
        .expect("the pragma answers")
        .map(|row| row.expect("a pragma row"))
        .collect();
    keys.sort();
    keys
}

/// Every named index on one table: its name, whether it is unique, whether it
/// is partial, and its columns in order.
fn table_indexes(connection: &Connection, table: &str) -> Vec<(String, bool, bool, Vec<String>)> {
    let mut statement = connection
        .prepare(&format!("PRAGMA index_list({table})"))
        .expect("the pragma prepares");
    let listed: Vec<(String, bool, bool)> = statement
        .query_map([], |row| {
            Ok((
                row.get::<_, String>("name")?,
                row.get::<_, i64>("unique")? == 1,
                row.get::<_, i64>("partial")? == 1,
            ))
        })
        .expect("the pragma answers")
        .map(|row| row.expect("a pragma row"))
        .filter(|(name, _, _)| !name.starts_with("sqlite_autoindex"))
        .collect();
    let mut indexes: Vec<(String, bool, bool, Vec<String>)> = listed
        .into_iter()
        .map(|(name, unique, partial)| {
            let mut info = connection
                .prepare(&format!("PRAGMA index_info({name})"))
                .expect("the pragma prepares");
            let columns = info
                .query_map([], |row| row.get::<_, String>("name"))
                .expect("the pragma answers")
                .map(|row| row.expect("a pragma row"))
                .collect();
            (name, unique, partial, columns)
        })
        .collect();
    indexes.sort();
    indexes
}

/// Every table and named index a database holds.
fn objects(connection: &Connection) -> Vec<String> {
    let mut statement = connection
        .prepare(
            "SELECT type || ' ' || name FROM sqlite_master \
             WHERE name NOT LIKE 'sqlite_%' ORDER BY type, name",
        )
        .expect("the catalogue prepares");
    statement
        .query_map([], |row| row.get(0))
        .expect("the catalogue answers")
        .map(|row| row.expect("a catalogue row"))
        .collect()
}

/// Every table the ledger's step is stated to create, with its columns and its
/// foreign keys.
fn stated_ledger_tables() -> [(&'static str, Vec<Column>, Vec<&'static str>); 6] {
    [
        (
            "spools",
            vec![
                column("id", "TEXT", true, 1),
                column("name", "TEXT", true, 0),
                column("material", "TEXT", true, 0),
                column("color", "TEXT", false, 0),
                column("diameter_mm", "REAL", true, 0),
                column("density_g_cm3", "REAL", true, 0),
                column("initial_g", "REAL", true, 0),
                column("remaining_g", "REAL", true, 0),
                column("tare_g", "REAL", false, 0),
                column("loaded_on_tool", "INTEGER", false, 0),
                column("trust", "TEXT", true, 0),
                column("registered_at", "TEXT", true, 0),
                column("updated_at", "TEXT", true, 0),
                column("retired_at", "TEXT", false, 0),
            ],
            vec![],
        ),
        (
            "print_positions",
            vec![
                column("print_id", "TEXT", true, 1),
                column("bytes", "INTEGER", true, 0),
                column("size_bytes", "INTEGER", false, 0),
                column("observed_at", "TEXT", true, 0),
            ],
            vec!["print_id -> prints(id)"],
        ),
        (
            "file_tallies",
            vec![
                column("file_name", "TEXT", true, 1),
                column("origin", "TEXT", true, 2),
                column("generation", "INTEGER", true, 0),
                column("successes", "INTEGER", true, 0),
                column("failures", "INTEGER", true, 0),
                column("observed_at", "TEXT", true, 0),
            ],
            vec![],
        ),
        (
            "charges",
            vec![
                column("charge_key", "TEXT", true, 1),
                column("print_id", "TEXT", false, 0),
                column("tool", "INTEGER", true, 0),
                column("spool_id", "TEXT", false, 0),
                column("length_mm", "REAL", true, 0),
                column("grams", "REAL", false, 0),
                column("event_id", "TEXT", true, 0),
                column("charged_at", "TEXT", true, 0),
            ],
            vec![
                "event_id -> events(id)",
                "print_id -> prints(id)",
                "spool_id -> spools(id)",
            ],
        ),
        (
            "pending_charges",
            vec![
                column("charge_key", "TEXT", true, 1),
                column("print_id", "TEXT", true, 0),
                column("tool", "INTEGER", true, 0),
                column("pending", "TEXT", true, 0),
                column("event_id", "TEXT", true, 0),
                column("pended_at", "TEXT", true, 0),
            ],
            vec!["event_id -> events(id)", "print_id -> prints(id)"],
        ),
        (
            "print_settlements",
            vec![
                column("print_id", "TEXT", true, 1),
                column("outcome", "TEXT", true, 0),
                column("event_id", "TEXT", true, 0),
                column("settled_at", "TEXT", true, 0),
            ],
            vec!["event_id -> events(id)", "print_id -> prints(id)"],
        ),
    ]
}

/// The ledger's step creates exactly the stated indexes.
fn assert_the_ledger_indexes(after: &Connection) {
    let index = |name: &str, unique: bool, partial: bool, columns: &[&str]| {
        (
            name.to_owned(),
            unique,
            partial,
            columns.iter().map(|column| (*column).to_owned()).collect(),
        )
    };
    assert_eq!(
        table_indexes(after, "spools"),
        vec![index(
            "spools_one_per_tool",
            true,
            true,
            &["loaded_on_tool"]
        )]
    );
    assert_eq!(
        table_indexes(after, "charges"),
        vec![index(
            "charges_by_print",
            false,
            false,
            &["print_id", "tool"]
        )]
    );
    assert_eq!(
        table_indexes(after, "pending_charges"),
        vec![index(
            "pending_charges_by_tool",
            false,
            false,
            &["tool", "pended_at"]
        )]
    );
    assert!(
        table_indexes(after, "events").contains(&index(
            "events_by_kind_and_instant",
            false,
            false,
            &["kind", "received_at", "id"]
        )),
        "the timeline's index is not on events"
    );
    let partial: String = after
        .query_row(
            "SELECT sql FROM sqlite_master WHERE name = 'spools_one_per_tool'",
            [],
            |row| row.get(0),
        )
        .expect("the index is catalogued");
    assert!(
        partial
            .split_whitespace()
            .collect::<Vec<_>>()
            .join(" ")
            .ends_with("WHERE loaded_on_tool IS NOT NULL AND retired_at IS NULL"),
        "one spool per tool is not held of unretired loaded spools alone: {partial}"
    );
}

/// The ledger's step creates exactly the tables, columns, keys and indexes the
/// ledger is stated to have, and nothing beside them.
#[test]
fn the_ledger_step_creates_exactly_the_stated_schema() {
    let dir = TempDir::new().expect("a temporary state directory");
    let before = Connection::open(dir.path().join("before.sqlite3")).expect("a database opens");
    for step in MIGRATIONS
        .iter()
        .filter(|step| step.version <= BEFORE_THE_LEDGER)
    {
        before.execute_batch(step.sql).expect("the step applies");
    }
    drop(SqliteStore::open(dir.path()).expect("the store opens"));
    let after = Connection::open(dir.path().join(DATABASE_FILE_NAME)).expect("a database opens");

    let added: Vec<String> = objects(&after)
        .into_iter()
        .filter(|name| !objects(&before).contains(name))
        .collect();
    assert_eq!(
        added,
        [
            "index charges_by_print",
            "index events_by_kind_and_instant",
            "index pending_charges_by_tool",
            "index spools_one_per_tool",
            "table charges",
            "table file_tallies",
            "table pending_charges",
            "table print_positions",
            "table print_settlements",
            "table spools",
        ],
        "the ledger's step created another set of objects"
    );

    for (table, columns, keys) in stated_ledger_tables() {
        assert_eq!(table_columns(&after, table), columns, "{table}'s columns");
        assert_eq!(table_keys(&after, table), keys, "{table}'s foreign keys");
        let strict: String = after
            .query_row(
                "SELECT sql FROM sqlite_master WHERE type = 'table' AND name = ?1",
                [table],
                |row| row.get(0),
            )
            .expect("the table is catalogued");
        assert!(
            strict.trim_end().ends_with("STRICT"),
            "{table} is not STRICT"
        );
    }

    assert_the_ledger_indexes(&after);
}
