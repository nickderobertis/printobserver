//! The store behaves under the concurrency the server actually has.
//!
//! The server is one process but not one thread, so a read taken while a writer
//! holds an open transaction is ordinary rather than exceptional. What this
//! journey turns on is ordering against a synchronization point it controls —
//! the writer's own commit, a later statement of this same test — rather than a
//! duration it guessed: the read either answers the pre-write state while that
//! transaction is open, or it comes back with the connection's own lock-timeout
//! refusal. A loaded host cannot make correct work fail here, and no bound
//! generous enough to pass slow work can let incorrect work through.

use crate::block_on::block_on;
use printobserver_store_api::StorePort;
use printobserver_store_sqlite::{DATABASE_FILE_NAME, SqliteStore, connect};
use printobserver_types::PrintId;
use rusqlite::params;
use tempfile::TempDir;

/// How many rows the uncommitted write writes, which is enough to spill.
const SPILLING_ROWS: usize = 5_000;

/// Write enough inside the open transaction that the page cache spills.
fn fill(writer: &rusqlite::Connection) {
    let mut statement = writer
        .prepare(
            "INSERT INTO prints (id, obico_print_id, file_name, state, opened_at, ended_at, \
             end_reason, narrowings) VALUES (?1, NULL, NULL, '\"printing\"', ?2, NULL, NULL, '[]')",
        )
        .expect("the statement prepares");
    for _ in 0..SPILLING_ROWS {
        statement
            .execute(params![
                PrintId::new().to_string(),
                "2026-03-01T12:00:00.000000000Z"
            ])
            .expect("the row is written");
    }
}

/// A read taken while a writer holds an open transaction answers.
#[test]
fn a_read_answers_the_pre_write_state_while_a_write_is_uncommitted() {
    let dir = TempDir::new().expect("a temporary state directory");
    let store = SqliteStore::open(dir.path()).expect("the store opens");
    let port: &dyn StorePort = &store;
    let print =
        block_on(port.open_print(None, Some("before.gcode".to_owned()))).expect("a print opens");

    let writer = connect(&dir.path().join(DATABASE_FILE_NAME)).expect("a connection opens");
    // One page of cache, so that a modest write spills to the database file.
    // Under a rollback journal that is what makes the writer take the lock a
    // reader waits on; under write-ahead logging it changes nothing, which is
    // the difference this journey is about.
    writer
        .execute_batch("PRAGMA cache_size = 1")
        .expect("the writer's cache is set");
    writer
        .execute_batch("BEGIN IMMEDIATE")
        .expect("a write transaction opens");
    writer
        .execute(
            "UPDATE prints SET file_name = ?2 WHERE id = ?1",
            params![print.id.to_string(), "after.gcode"],
        )
        .expect("the write applies");
    fill(&writer);
    assert!(
        !writer.is_autocommit(),
        "the writer's transaction is not open, so this journey proves nothing"
    );

    let read = block_on(port.print(print.id))
        .expect("the read answered rather than refusing on this connection's lock timeout");
    assert_eq!(
        read.expect("the print is there").file_name.as_deref(),
        Some("before.gcode"),
        "the read did not answer the state as of before the uncommitted write"
    );
    assert!(
        !writer.is_autocommit(),
        "the writer's transaction closed before the read answered"
    );

    writer.execute_batch("COMMIT").expect("the write commits");
    assert_eq!(
        block_on(port.print(print.id))
            .expect("the read answers")
            .expect("the print is there")
            .file_name
            .as_deref(),
        Some("after.gcode"),
        "the read after the commit did not see the write"
    );
}

/// A second store on the same directory reads what the first one wrote.
#[test]
fn a_second_store_on_the_same_directory_reads_what_the_first_wrote() {
    let dir = TempDir::new().expect("a temporary state directory");
    let first = SqliteStore::open(dir.path()).expect("the store opens");
    let second = SqliteStore::open(dir.path()).expect("a second store opens");
    let writer: &dyn StorePort = &first;
    let reader: &dyn StorePort = &second;

    let print =
        block_on(writer.open_print(Some(11), None)).expect("a print opens on the first store");
    assert_eq!(
        block_on(reader.print_by_obico_id(11)),
        Ok(Some(print)),
        "a second store on the same state directory did not read what the first wrote"
    );
}

/// The store opens its connections in the mode this ordering rests on.
#[test]
fn the_store_opens_its_connections_in_write_ahead_logging_mode() {
    let dir = TempDir::new().expect("a temporary state directory");
    let _store = SqliteStore::open(dir.path()).expect("the store opens");
    let connection = connect(&dir.path().join(DATABASE_FILE_NAME)).expect("a connection opens");
    assert_eq!(
        connection
            .query_row("PRAGMA journal_mode", [], |row| row.get::<_, String>(0))
            .expect("the pragma answers"),
        "wal",
        "the store is in a mode that serialises a reader behind a writer"
    );
}
