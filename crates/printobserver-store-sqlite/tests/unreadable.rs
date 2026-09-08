//! What the store does when the state it is handed is not what it expects.
//!
//! A row this build cannot read is a real possibility for a record of record —
//! it outlives the build that wrote it — and the answer has to be a refusal
//! that says so rather than a panic in the middle of a supervision loop.

#[path = "support/block_on.rs"]
mod block_on;
#[path = "support/fixture.rs"]
mod fixture;

use block_on::block_on;
use fixture::{draft, instant, request};
use printobserver_store_api::{HistoryQuery, StoreError, StorePort};
use printobserver_store_sqlite::{DATABASE_FILE_NAME, MemoryStore, SqliteStore, connect};
use printobserver_types::{ActionId, Adjustable, ExecutionOutcome, PolicyDecision, PrintId};
use rusqlite::Connection;
use tempfile::TempDir;

/// One read of a store, for the walk over the columns a build cannot read.
type Read = fn(&SqliteStore, PrintId, ActionId) -> Result<(), StoreError>;

/// A store carrying one of everything, and the directory it lives in.
fn seeded() -> (TempDir, PrintId, ActionId) {
    let dir = TempDir::new().expect("a temporary state directory");
    let store = SqliteStore::open(dir.path()).expect("the store opens");
    let port: &dyn StorePort = &store;
    let print = block_on(port.open_print(None, None)).expect("a print opens");
    block_on(port.append_event(draft(
        Some(print.id),
        "obico_failure_alert",
        instant("2026-03-01T12:00:00Z"),
    )))
    .expect("an event is appended");
    let action = block_on(port.record_action(
        request(instant("2026-03-01T12:01:00Z")),
        PolicyDecision::Accepted,
    ))
    .expect("an action is recorded");
    block_on(port.open_intervention(
        action.id,
        Adjustable::Feedrate,
        None,
        0.8,
        instant("2026-03-01T12:02:00Z"),
        instant("2026-03-01T12:32:00Z"),
    ))
    .expect("an intervention opens");
    (dir, print.id, action.id)
}

/// Overwrite one column of one table with something this build cannot read.
fn corrupt(dir: &TempDir, statement: &str) {
    let connection = connect(&dir.path().join(DATABASE_FILE_NAME)).expect("a connection opens");
    connection
        .execute(statement, [])
        .unwrap_or_else(|error| panic!("the column is overwritten: {error}"));
}

/// The whole history of one print, as far as a read gets.
fn history(store: &SqliteStore, print_id: PrintId, _: ActionId) -> Result<(), StoreError> {
    let port: &dyn StorePort = store;
    block_on(port.history(HistoryQuery {
        print_id,
        kinds: Vec::new(),
        since: None,
        until: None,
        limit: None,
    }))
    .map(|_| ())
}

/// A column this build cannot read is reported rather than panicked on.
#[test]
fn a_row_this_build_cannot_read_is_reported() {
    let cases: [(&str, Read); 6] = [
        (
            "UPDATE prints SET opened_at = 'not an instant'",
            |store, print_id, _| {
                let port: &dyn StorePort = store;
                block_on(port.print(print_id)).map(|_| ())
            },
        ),
        (
            "UPDATE prints SET ended_at = 'not an instant'",
            |store, print_id, _| {
                let port: &dyn StorePort = store;
                block_on(port.print(print_id)).map(|_| ())
            },
        ),
        ("UPDATE events SET payload = 'not json'", history),
        ("UPDATE events SET source = 'nowhere'", history),
        (
            "UPDATE interventions SET adjustable = 'nothing adjustable'",
            |store, print_id, _| {
                let port: &dyn StorePort = store;
                block_on(port.active_interventions(print_id)).map(|_| ())
            },
        ),
        (
            "UPDATE actions SET decision = 'not a decision'",
            |store, _, action_id| {
                let port: &dyn StorePort = store;
                block_on(port.record_execution(action_id, ExecutionOutcome::Succeeded)).map(|_| ())
            },
        ),
    ];

    for (statement, read) in cases {
        let (dir, print_id, action_id) = seeded();
        corrupt(&dir, statement);
        let store = SqliteStore::open(dir.path()).expect("the store opens");
        let answer = read(&store, print_id, action_id);
        assert!(
            matches!(answer, Err(StoreError::Database { .. })),
            "after `{statement}` the read answered {answer:?} rather than saying \
             the database could not be read"
        );
    }
}

/// Recording an execution twice is refused rather than replacing the first.
#[test]
fn recording_an_execution_twice_is_refused() {
    let dir = TempDir::new().expect("a temporary state directory");
    for port in [
        Box::new(SqliteStore::open(dir.path()).expect("the store opens")) as Box<dyn StorePort>,
        Box::new(MemoryStore::new(dir.path()).expect("the store opens")) as Box<dyn StorePort>,
    ] {
        block_on(port.open_print(None, None)).expect("a print opens");
        let action = block_on(port.record_action(
            request(instant("2026-03-01T12:00:00Z")),
            PolicyDecision::Accepted,
        ))
        .expect("an action is recorded");
        block_on(port.record_execution(action.id, ExecutionOutcome::Succeeded))
            .expect("an execution is recorded");
        assert_eq!(
            block_on(port.record_execution(
                action.id,
                ExecutionOutcome::Failed {
                    reason: "the second word on it".to_owned()
                }
            )),
            Err(StoreError::ConstraintRefused {
                constraint: "executions.action_id".to_owned()
            }),
            "an action was given a second outcome"
        );
    }
}

/// A state directory that cannot be created is reported as what it is.
#[test]
fn a_state_directory_that_cannot_be_created_is_reported() {
    let dir = TempDir::new().expect("a temporary state directory");
    let occupied = dir.path().join("a-file");
    std::fs::write(&occupied, b"not a directory").expect("the file is written");

    let refusal = SqliteStore::open(occupied.join("state"))
        .expect_err("a state directory under a file cannot be created");
    assert!(
        matches!(refusal, StoreError::Io { .. }),
        "an uncreatable state directory was reported as {refusal:?}"
    );
    assert!(
        matches!(
            MemoryStore::new(occupied.join("state")),
            Err(StoreError::Io { .. })
        ),
        "the in-memory store answered something else for the same directory"
    );
}

/// A migration that cannot apply is reported, naming the step that failed.
#[test]
fn a_migration_that_cannot_apply_is_reported_naming_its_version() {
    let dir = TempDir::new().expect("a temporary state directory");
    let connection =
        Connection::open(dir.path().join(DATABASE_FILE_NAME)).expect("a database opens");
    connection
        .execute_batch("CREATE TABLE prints (something_else TEXT)")
        .expect("the conflicting table is created");
    drop(connection);

    let refusal =
        SqliteStore::open(dir.path()).expect_err("a migration onto a conflicting table fails");
    let StoreError::Database { detail } = &refusal else {
        panic!("a failed migration was reported as {refusal:?}");
    };
    assert!(
        detail.contains("migration to version 1"),
        "the refusal does not name the step that failed: {detail}"
    );
}

/// Each store answers the state directory it was opened on.
#[test]
fn each_store_answers_the_state_directory_it_was_opened_on() {
    let dir = TempDir::new().expect("a temporary state directory");
    let sqlite = SqliteStore::open(dir.path()).expect("the store opens");
    let memory = MemoryStore::new(dir.path()).expect("the store opens");
    assert_eq!(sqlite.state_dir(), dir.path());
    assert_eq!(memory.state_dir(), dir.path());
    let _ = (sqlite.hold_points(), memory.hold_points());
}

/// An unknown print reads back as no print rather than as a failure.
#[test]
fn an_unknown_print_reads_back_as_no_print() {
    let dir = TempDir::new().expect("a temporary state directory");
    let store = SqliteStore::open(dir.path()).expect("the store opens");
    let port: &dyn StorePort = &store;
    assert_eq!(block_on(port.print(PrintId::new())), Ok(None));
    assert_eq!(
        block_on(port.manifest(PrintId::new())),
        Ok(None),
        "a print with no manifest answered something other than none"
    );
    assert_eq!(
        block_on(port.session(PrintId::new())),
        Ok(None),
        "a print with no session answered something other than none"
    );
}
