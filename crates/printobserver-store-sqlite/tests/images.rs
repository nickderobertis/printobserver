//! Image bytes are addressed by their content, written atomically, and honest.
//!
//! The digests here are this test's own — the SHA-256 of the two byte
//! sequences, taken outside this crate — rather than whatever the store
//! answered, so a store that assigned every image one constant address fails
//! the first assertion of the addressing journeys rather than passing them all.

#[path = "support/block_on.rs"]
mod block_on;
#[path = "support/child.rs"]
mod child;
#[path = "support/fixture.rs"]
mod fixture;

use std::path::{Path, PathBuf};

use block_on::block_on;
use fixture::{draft, instant};
use printobserver_store_api::{ImageLookup, StorePort};
use printobserver_store_sqlite::{DATABASE_FILE_NAME, SqliteStore, connect};
use printobserver_types::{EventRecord, PrintRecord, RawBytes};
use tempfile::TempDir;

/// One byte sequence, and the SHA-256 of it taken outside this crate.
const FIRST: &[u8] = b"the first snapshot";
/// The SHA-256 of [`FIRST`].
const FIRST_DIGEST: &str = "66265553660911bda95316322356fb2658852d009cfb72c62f02ba0232ea1e16";
/// A different byte sequence.
const SECOND: &[u8] = b"the second snapshot";
/// The SHA-256 of [`SECOND`].
const SECOND_DIGEST: &str = "750c7605caf541c72896aae3f169f70c07bedfa222c6925dbbf8d2d036afeeb6";
/// What the interrupted child exits with, so its interruption is not a crash.
const INTERRUPTED: i32 = 70;

/// A print and an event to hang an image on.
fn subject(store: &SqliteStore) -> (PrintRecord, EventRecord) {
    let port: &dyn StorePort = store;
    let print = block_on(port.open_print(None, None)).expect("a print opens");
    let event = block_on(port.append_event(draft(
        Some(print.id),
        "obico_failure_alert",
        instant("2026-03-01T12:00:00Z"),
    )))
    .expect("an event is appended");
    (print, event)
}

/// Every file beneath one directory.
fn files_under(dir: &Path) -> Vec<PathBuf> {
    let mut found = Vec::new();
    let Ok(entries) = std::fs::read_dir(dir) else {
        return found;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            found.extend(files_under(&path));
        } else {
            found.push(path);
        }
    }
    found.sort();
    found
}

/// How many rows the images table holds.
fn image_rows(state_dir: &Path) -> i64 {
    let connection = connect(&state_dir.join(DATABASE_FILE_NAME)).expect("a connection opens");
    connection
        .query_row("SELECT COUNT(*) FROM images", [], |row| row.get(0))
        .expect("the count answers")
}

/// The same bytes arriving twice are stored once, with two rows naming them.
#[test]
fn the_same_bytes_twice_are_stored_once_under_two_rows() {
    let dir = TempDir::new().expect("a temporary state directory");
    let store = SqliteStore::open(dir.path()).expect("the store opens");
    let port: &dyn StorePort = &store;
    let (print, event) = subject(&store);

    let first = block_on(port.put_image(
        print.id,
        event.id,
        None,
        "image/jpeg".to_owned(),
        RawBytes::new(FIRST.to_vec()),
    ))
    .expect("an image is stored");
    let again = block_on(port.put_image(
        print.id,
        event.id,
        None,
        "image/jpeg".to_owned(),
        RawBytes::new(FIRST.to_vec()),
    ))
    .expect("the same image is stored again");

    assert_ne!(first.id, again.id, "the two rows are two rows");
    assert_eq!(first.sha256, again.sha256);
    assert_eq!(first.byte_len, again.byte_len);
    assert_eq!(first.content_type, again.content_type);
    assert_eq!(first.relative_path, again.relative_path);

    let files = files_under(&dir.path().join("images"));
    assert_eq!(
        files.len(),
        1,
        "the same bytes twice left {} files: {files:?}",
        files.len()
    );
    assert_eq!(image_rows(dir.path()), 2, "two rows name the one file");
}

/// Two different images take two different addresses, and neither aliases.
#[test]
fn distinct_content_takes_a_distinct_address() {
    let dir = TempDir::new().expect("a temporary state directory");
    let store = SqliteStore::open(dir.path()).expect("the store opens");
    let port: &dyn StorePort = &store;
    let (print, event) = subject(&store);

    let stored: Vec<_> = [(FIRST, FIRST_DIGEST), (SECOND, SECOND_DIGEST)]
        .into_iter()
        .map(|(bytes, digest)| {
            let record = block_on(port.put_image(
                print.id,
                event.id,
                None,
                "image/jpeg".to_owned(),
                RawBytes::new(bytes.to_vec()),
            ))
            .expect("an image is stored");
            assert_eq!(
                record.sha256, digest,
                "the row does not name this content's own digest"
            );
            (record, bytes)
        })
        .collect();

    assert_ne!(
        stored[0].0.sha256, stored[1].0.sha256,
        "two different images were given one digest"
    );
    assert_ne!(
        stored[0].0.relative_path, stored[1].0.relative_path,
        "two different images were given one path"
    );
    let files = files_under(&dir.path().join("images"));
    assert_eq!(files.len(), 2, "two images left {files:?}");

    for (record, bytes) in &stored {
        let path = match block_on(port.image(record.id)).expect("the image reads") {
            ImageLookup::Found { path, .. } => path,
            ImageLookup::FileMissing { record } => {
                panic!("the file of {} is not where its row says", record.id)
            }
        };
        assert_eq!(
            std::fs::read(&path).expect("the file reads"),
            *bytes,
            "the file at {} is not the bytes that were written to it",
            path.display()
        );
    }
}

/// The same content takes the same address through a store opened afresh.
#[test]
fn the_same_content_takes_the_same_address_by_a_different_route() {
    let dir = TempDir::new().expect("a temporary state directory");
    let first = {
        let store = SqliteStore::open(dir.path()).expect("the store opens");
        let port: &dyn StorePort = &store;
        let (print, event) = subject(&store);
        block_on(port.put_image(
            print.id,
            event.id,
            None,
            "image/jpeg".to_owned(),
            RawBytes::new(FIRST.to_vec()),
        ))
        .expect("an image is stored")
    };

    let store = SqliteStore::open(dir.path()).expect("the store opens again");
    let port: &dyn StorePort = &store;
    let (print, event) = subject(&store);
    let again = block_on(port.put_image(
        print.id,
        event.id,
        None,
        "image/png".to_owned(),
        RawBytes::new(FIRST.to_vec()),
    ))
    .expect("the same image is stored again");

    assert_eq!(again.sha256, FIRST_DIGEST);
    assert_eq!(
        (again.sha256.as_str(), again.relative_path.as_str()),
        (first.sha256.as_str(), first.relative_path.as_str()),
        "the address is a function of the call rather than of the bytes"
    );
    assert_eq!(
        files_under(&dir.path().join("images")).len(),
        1,
        "a store opened afresh wrote the same bytes a second time"
    );
}

/// A row whose file has gone is a missing image, not a failed read.
#[test]
fn a_row_whose_file_is_gone_is_answered_as_missing() {
    let dir = TempDir::new().expect("a temporary state directory");
    let store = SqliteStore::open(dir.path()).expect("the store opens");
    let port: &dyn StorePort = &store;
    let (print, event) = subject(&store);
    let record = block_on(port.put_image(
        print.id,
        event.id,
        None,
        "image/jpeg".to_owned(),
        RawBytes::new(FIRST.to_vec()),
    ))
    .expect("an image is stored");

    let path = dir
        .path()
        .join("images")
        .join(&FIRST_DIGEST[..2])
        .join(FIRST_DIGEST);
    assert!(
        path.is_file(),
        "the image is not at the address its content gives it"
    );
    std::fs::remove_file(&path).expect("the image file is removable");

    assert_eq!(
        block_on(port.image(record.id)),
        Ok(ImageLookup::FileMissing { record }),
        "a row whose file is gone was not answered as a missing image"
    );
}

/// A write interrupted before the bytes are in place leaves no row and no file.
#[test]
fn an_interrupted_write_leaves_no_row_and_no_file() {
    let dir = TempDir::new().expect("a temporary state directory");
    let output = child::run("the_child_interrupts_an_image_write", dir.path());
    assert_eq!(
        output.status.code(),
        Some(INTERRUPTED),
        "the child did not end where the write was interrupted: {}",
        child::reported(&output)
    );

    let addressed = dir
        .path()
        .join("images")
        .join(&FIRST_DIGEST[..2])
        .join(FIRST_DIGEST);
    assert!(
        !addressed.exists(),
        "an interrupted write left a file at the addressed path"
    );
    assert_eq!(
        image_rows(dir.path()),
        0,
        "an interrupted write left a row pointing at bytes that were never put in place"
    );
}

/// Interrupt one image write between its bytes and their being put in place.
///
/// Run in a process of its own by the journey above; ignored so that nothing
/// else runs it.
#[test]
#[ignore = "run in a process of its own by an_interrupted_write_leaves_no_row_and_no_file"]
fn the_child_interrupts_an_image_write() {
    let dir = child::state_dir();
    let store = SqliteStore::open(&dir).expect("the store opens");
    let port: &dyn StorePort = &store;
    let (print, event) = subject(&store);
    let interruption = store.hold_points().image_write();
    interruption.arm();

    std::thread::scope(|scope| {
        scope.spawn(|| {
            interruption.await_holding(&[FIRST_DIGEST]);
            std::process::exit(INTERRUPTED);
        });
        let _ = block_on(port.put_image(
            print.id,
            event.id,
            None,
            "image/jpeg".to_owned(),
            RawBytes::new(FIRST.to_vec()),
        ));
    });
    panic!("the write was never held where it is interrupted");
}
