//! A supervisor of this build started over the store a published release
//! wrote, and read through this program.
//!
//! The store is `crates/printobserver-store-sqlite/tests/fixtures/v0.4.3`'s,
//! written through printobserver 0.4.3's own store and carrying what that
//! release could hold and this one cannot: a spool retired while still loaded,
//! and charges, pending charges and filament events in 0.4.3's shapes. The
//! first thing to open it is the supervisor this program talks to.

use std::collections::BTreeMap;
use std::path::PathBuf;

use printobserver_types::serde_json::{Value, json};

use crate::running;
use crate::world::World;

/// The print the fixture holds.
const PRINT: &str = "01a176c4-548a-7bb1-80e5-a5a058ca2ad2";

/// The spool 0.4.3 retired while it was still loaded on tool 0.
const RETIRED_ON_TOOL: &str = "01a176c4-5487-73b0-a80f-753b6f3fdc64";

/// The store 0.4.3 wrote.
fn fixture() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("printobserver-store-sqlite")
        .join("tests")
        .join("fixtures")
        .join("v0.4.3")
        .join("printobserver.sqlite3")
}

/// Every event the fixture holds, by identifier, with its kind and its print.
///
/// Read off a copy, because a connection of the store's own puts the file it
/// opens in write-ahead mode, and the checked-in fixture is never written.
fn written_by_0_4_3() -> BTreeMap<String, (String, Option<String>)> {
    let copy = tempfile::TempDir::new().expect("a directory for a copy");
    let path = copy
        .path()
        .join(printobserver_store_sqlite::DATABASE_FILE_NAME);
    std::fs::copy(fixture(), &path).expect("the fixture copies");
    let connection = printobserver_store_sqlite::connect(&path).expect("the fixture opens");
    let mut statement = connection
        .prepare("SELECT id, kind, print_id FROM events")
        .expect("the events select");
    statement
        .query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                (row.get::<_, String>(1)?, row.get::<_, Option<String>>(2)?),
            ))
        })
        .expect("the events read")
        .collect::<Result<_, _>>()
        .expect("every event reads")
}

/// Every event the printer's history answers, by identifier.
fn printer_history(world: &World) -> BTreeMap<String, Value> {
    running::read(world, &["printer-history", "--limit", "1000"])["events"]
        .as_array()
        .cloned()
        .unwrap_or_default()
        .into_iter()
        .map(|event| (event["id"].as_str().unwrap_or_default().to_owned(), event))
        .collect()
}

/// Started over the store 0.4.3 wrote, the supervisor migrates it and says
/// what it could not carry forward as it was — the spool retired while loaded,
/// now retired and on no tool — and every event 0.4.3 wrote is still read,
/// through `history` and `printer-history`, in this build's shapes.
pub fn a_store_0_4_3_wrote_is_migrated_and_every_event_still_reads(world: &str) {
    let written = written_by_0_4_3();
    let world = World::upgraded_from(world, &fixture());

    let said = world.said_so_far();
    assert!(
        said.contains("migrating the store to schema version 6")
            && said.contains(RETIRED_ON_TOOL)
            && said.contains("while still loaded on tool 0"),
        "the supervisor did not say what its migration changed: {said}"
    );
    let retired = running::read(&world, &["spool", "--spool-id", RETIRED_ON_TOOL]);
    assert_eq!(
        retired["spool"]["lifecycle"]["state"],
        json!("retired"),
        "{retired}"
    );
    assert!(
        retired["spool"]["lifecycle"].get("tool").is_none(),
        "a migrated retired spool still names a tool: {retired}"
    );

    let answered = printer_history(&world);
    for (id, (kind, _)) in &written {
        let event = answered
            .get(id)
            .unwrap_or_else(|| panic!("`printer-history` lost the {kind} 0.4.3 wrote as {id}"));
        assert_eq!(event["kind"], json!(kind), "{event}");
    }
    let of_print = running::read(&world, &["history", "--print-id", PRINT, "--limit", "1000"]);
    let read: BTreeMap<String, Value> = of_print["events"]
        .as_array()
        .cloned()
        .unwrap_or_default()
        .into_iter()
        .map(|event| (event["id"].as_str().unwrap_or_default().to_owned(), event))
        .collect();
    for (id, (kind, print)) in &written {
        if print.as_deref() == Some(PRINT) {
            assert!(
                read.contains_key(id),
                "`history` lost the {kind} 0.4.3 wrote as {id}: {of_print}"
            );
        }
    }

    let of_kind = |kind: &str| -> Vec<&Value> {
        answered
            .values()
            .filter(|event| event["kind"] == json!(kind))
            .collect()
    };
    for signal in of_kind("filament_change_signalled") {
        let payload = &signal["payload"];
        assert!(
            payload.get("position_read_at_signal").is_none()
                && payload
                    .get("position")
                    .is_none_or(|position| position.get("freshness").is_some()),
            "a filament change reads in 0.4.3's shape: {signal}"
        );
    }
    for consumed in of_kind("filament_consumed") {
        let payload = &consumed["payload"];
        assert!(
            payload.get("spool_id").is_none() && payload.get("grams").is_none(),
            "a consumption reads in 0.4.3's shape: {consumed}"
        );
    }
    let rejected = of_kind("spool_command_rejected");
    assert_eq!(
        rejected[0]["payload"]["decision"]["rejected"]["spool_refused"]["cause"]["cause"],
        json!("unknown_spool"),
        "{}",
        rejected[0]
    );
    for registered in of_kind("spool_registered") {
        assert!(
            registered["payload"]["spool"].get("lifecycle").is_some(),
            "a registration reads in 0.4.3's shape: {registered}"
        );
    }
}
