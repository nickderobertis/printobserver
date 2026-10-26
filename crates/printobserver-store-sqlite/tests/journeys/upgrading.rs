//! A database a published release wrote, opened by this build.
//!
//! `tests/fixtures/v0.4.3/printobserver.sqlite3` was written through
//! printobserver 0.4.3's own store, whose spools carry a loaded tool beside a
//! retirement and whose charges, pending charges and filament events carry the
//! shapes that release wrote; `PROVENANCE.md` beside it says how. Opened here,
//! it is migrated forward with what could not be carried as it was written
//! down, and every record and event it held reads back in the shapes this
//! build answers.

use std::path::{Path, PathBuf};

use crate::block_on::block_on;
use crate::fixture::Store;
use printobserver_core::kinds::{
    ChargePendedPayload, FilamentChangeSignalledPayload, FilamentConsumedPayload,
    PositionFreshness, PreflightCheckedPayload, PrintStartedPayload, SpoolCommandRejectedPayload,
    SpoolRegisteredPayload,
};
use printobserver_core::store::{HistoryQuery, StoreError, TimelineQuery};
use printobserver_core::{
    ChargeKey, PolicyDecision, PreflightObjection, PreflightToolResult, RejectionReason, SpoolId,
    SpoolLifecycle, SpoolRefusalCause,
};
use printobserver_store_sqlite::{
    CURRENT_SCHEMA_VERSION, DATABASE_FILE_NAME, SqliteStore, connect,
};
use printobserver_types::{EventPayload, EventRecord, PrintId};
use tempfile::TempDir;

/// The print the fixture holds.
pub const PRINT: &str = "01a176c4-548a-7bb1-80e5-a5a058ca2ad2";
/// The spool 0.4.3 retired while it was still loaded on tool 0.
pub const RETIRED_ON_TOOL: &str = "01a176c4-5487-73b0-a80f-753b6f3fdc64";
/// The spool loaded on tool 1.
pub const LOADED: &str = "01a176c4-5488-7532-8297-73e157be17bc";
/// The spool nothing names but its registration, ever.
pub const UNKNOWN: &str = "01950000-0000-7000-8000-000000000001";

/// The fixture, as it is checked in.
#[must_use]
pub fn fixture() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join("v0.4.3")
        .join("printobserver.sqlite3")
}

/// A state directory holding a copy of the fixture, which a test may migrate.
fn copied() -> TempDir {
    let dir = TempDir::new().expect("a temporary state directory");
    std::fs::copy(fixture(), dir.path().join(DATABASE_FILE_NAME)).expect("the fixture copies");
    dir
}

/// The version a database file records.
fn version_of(path: &Path) -> i64 {
    connect(path)
        .expect("the database opens")
        .query_row("PRAGMA user_version", [], |row| row.get(0))
        .expect("the version reads")
}

/// The one payload of one kind among events, read as that kind.
fn every<P: EventPayload>(events: &[EventRecord]) -> Vec<P> {
    events
        .iter()
        .filter_map(EventRecord::payload_as::<P>)
        .map(|read| read.expect("an old event reads in this build's shape"))
        .collect()
}

/// Opened by this build, the 0.4.3 database is migrated to this build's
/// version; the spool 0.4.3 left retired and loaded is retired and on no tool,
/// and the migration's report names it — once, by the open that migrated it,
/// and in the database's own record from then on.
#[test]
fn a_database_0_4_3_wrote_is_migrated_with_the_retired_loaded_spool_reported() {
    let dir = copied();
    let path = dir.path().join(DATABASE_FILE_NAME);
    assert_eq!(version_of(&path), 5, "the fixture is not the 0.4.3 schema");

    let store = SqliteStore::open(dir.path()).expect("the 0.4.3 database opens");
    assert_eq!(version_of(&path), i64::from(CURRENT_SCHEMA_VERSION));
    let migrated = store.migrated().to_vec();
    assert_eq!(migrated.len(), 1, "{migrated:#?}");
    assert_eq!(migrated[0].version, 6);
    assert_eq!(migrated[0].subject, format!("spool {RETIRED_ON_TOOL}"));
    assert!(
        migrated[0].detail.contains("PLA retired-on-tool")
            && migrated[0].detail.contains("while still loaded on tool 0")
            && migrated[0].detail.contains("loaded on no tool"),
        "{}",
        migrated[0]
    );

    let port: &dyn Store = &store;
    let retired = block_on(port.spool(RETIRED_ON_TOOL.parse().expect("an id")))
        .expect("the spool reads")
        .expect("the spool is there");
    assert!(
        matches!(retired.lifecycle, SpoolLifecycle::Retired { .. }),
        "{retired:?}"
    );
    let loaded: Vec<String> = block_on(port.loaded_spools())
        .expect("the loaded spools read")
        .iter()
        .map(|spool| spool.id.to_string())
        .collect();
    assert_eq!(loaded, [LOADED]);
    drop(store);

    let reopened = SqliteStore::open(dir.path()).expect("the database opens again");
    assert!(
        reopened.migrated().is_empty(),
        "a second open reported a migration it did not make"
    );
    assert_eq!(
        reopened.migration_report().expect("the report reads"),
        migrated,
        "the database's own record of the migration is not what the migrating open said"
    );
}

/// The charge and the pending charge the 0.4.3 database held read back in this
/// build's shapes: the charge's calculation whole, read from its own event,
/// and the pending charge keyed by its print segment alone.
#[test]
fn every_charge_0_4_3_wrote_reads_back_whole() {
    let dir = copied();
    let store = SqliteStore::open(dir.path()).expect("the 0.4.3 database opens");
    let port: &dyn Store = &store;
    let print: PrintId = PRINT.parse().expect("an id");
    let charges = block_on(port.charges(print)).expect("the charges read");
    assert_eq!(charges.len(), 1, "{charges:#?}");
    let calculation = charges[0]
        .charged
        .as_ref()
        .expect("the charge names its spool");
    assert_eq!(calculation.spool_id.to_string(), LOADED);
    assert!(
        (calculation.diameter_mm - 1.75).abs() < 1e-9,
        "{calculation:?}"
    );
    assert!(
        (calculation.remaining_before_g - calculation.grams - calculation.remaining_after_g).abs()
            < 1e-9,
        "{calculation:?}"
    );
    assert_eq!(charges[0].identity.print_id(), Some(print));
    let pending = block_on(port.pending_charges(Some(1))).expect("the pending charges read");
    assert_eq!(pending.len(), 1, "{pending:#?}");
    assert_eq!(pending[0].print_id(), print);
    assert_eq!(pending[0].charge_key.segment, 1);
}

/// Every event the 0.4.3 database held reads back in this build's shapes:
/// each filament event's position and calculation in one field, each preflight
/// tool one tagged result, and a refusal 0.4.3 wrote as a sentence with its
/// cause.
#[test]
fn every_event_0_4_3_wrote_reads_back_in_this_builds_shapes() {
    let dir = copied();
    let store = SqliteStore::open(dir.path()).expect("the 0.4.3 database opens");
    let port: &dyn Store = &store;
    let print: PrintId = PRINT.parse().expect("an id");
    let history = block_on(port.history(HistoryQuery {
        print_id: print,
        kinds: Vec::new(),
        since: None,
        until: None,
        limit: None,
    }))
    .expect("the print's history reads");
    let timeline = block_on(port.timeline(TimelineQuery {
        kinds: Vec::new(),
        since: None,
        until: None,
        spool_id: None,
        before: None,
        limit: Some(1_000),
    }))
    .expect("the timeline reads");
    let held: i64 = connect(&dir.path().join(DATABASE_FILE_NAME))
        .expect("the database opens")
        .query_row("SELECT COUNT(*) FROM events", [], |row| row.get(0))
        .expect("the events count");
    assert_eq!(
        i64::try_from(timeline.len()).expect("a count"),
        held,
        "the timeline does not answer every event 0.4.3 wrote"
    );

    let signals = every::<FilamentChangeSignalledPayload>(&history);
    assert_eq!(signals.len(), 2, "{history:#?}");
    let fresh = signals
        .iter()
        .find_map(|signal| signal.position.as_ref())
        .expect("one signal had a position");
    assert_eq!(fresh.freshness, PositionFreshness::ReadAtSignal);
    assert_eq!(fresh.position.bytes, 40_960);
    assert!(signals.iter().any(|signal| signal.position.is_none()));

    let consumed = every::<FilamentConsumedPayload>(&timeline);
    assert_eq!(consumed.len(), 2, "{timeline:#?}");
    for consumption in &consumed {
        match &consumption.charge_key {
            ChargeKey::Print { .. } => {
                let whole = consumption.charged.as_ref().expect("charged whole");
                assert_eq!(whole.spool_id.to_string(), LOADED);
            }
            ChargeKey::Unobserved { .. } => assert_eq!(consumption.charged, None),
        }
    }
    let pended = every::<ChargePendedPayload>(&history);
    assert_eq!(pended.len(), 1);
    assert_eq!(pended[0].pending.charge_key.tool, 1);

    let started = every::<PrintStartedPayload>(&history);
    let overridden = &started[0]
        .preflight
        .as_ref()
        .expect("the start carried its check")
        .tools[0]
        .result;
    let PreflightToolResult::Overridden {
        objection: PreflightObjection::Insufficient(short),
    } = overridden
    else {
        panic!("the overridden tool did not keep what it objected to: {overridden:?}");
    };
    assert_eq!(short.spool.name, "PETG loaded");
    assert!((short.need.required_g - 120.0).abs() < 1e-9);
    let checked = every::<PreflightCheckedPayload>(&timeline);
    let results: Vec<&PreflightToolResult> = checked[0]
        .preflight
        .tools
        .iter()
        .map(|tool| &tool.result)
        .collect();
    assert!(matches!(results[0], PreflightToolResult::Insufficient(_)));
    assert_eq!(results[1], &PreflightToolResult::Unchecked);

    let rejected = every::<SpoolCommandRejectedPayload>(&timeline);
    let unknown: SpoolId = UNKNOWN.parse().expect("an id");
    assert_eq!(
        rejected[0].decision,
        PolicyDecision::Rejected(RejectionReason::SpoolRefused {
            cause: SpoolRefusalCause::UnknownSpool { spool_id: unknown },
            message: format!("there is no spool {unknown}; list the spools to find the one meant"),
        })
    );
    let registered = every::<SpoolRegisteredPayload>(&timeline);
    assert_eq!(registered.len(), 3);
    assert!(
        registered
            .iter()
            .any(|spool| spool.spool.lifecycle == SpoolLifecycle::Loaded { tool: 0 })
    );
}

/// From the version this build migrates to, the database itself refuses a
/// charge whose print or tool is not the one its key names, one naming a spool
/// with half a calculation, and a spool both retired and loaded — whoever
/// writes it.
#[test]
fn the_database_refuses_an_incoherent_charge_or_spool_on_write() {
    let dir = copied();
    drop(SqliteStore::open(dir.path()).expect("the 0.4.3 database opens"));
    let connection = connect(&dir.path().join(DATABASE_FILE_NAME)).expect("the database opens");
    for statement in [
        "UPDATE charges SET tool = 7 WHERE charge_key LIKE 'print:%'",
        "UPDATE charges SET print_id = NULL WHERE charge_key LIKE 'print:%'",
        "UPDATE charges SET diameter_mm = NULL WHERE spool_id IS NOT NULL",
        "UPDATE charges SET tool = 7 WHERE charge_key LIKE 'unobserved:%'",
        "UPDATE spools SET loaded_on_tool = 2 WHERE retired_at IS NOT NULL",
    ] {
        let refused = connection
            .execute(statement, [])
            .err()
            .unwrap_or_else(|| panic!("the database wrote `{statement}`"));
        let said = refused.to_string();
        assert!(
            said.contains("charges.coherent") || said.contains("spools.lifecycle"),
            "`{statement}` was refused for another reason: {said}"
        );
    }
    let store = SqliteStore::open(dir.path()).expect("the database opens");
    let port: &dyn Store = &store;
    assert!(
        block_on(port.charges(PRINT.parse().expect("an id"))).is_ok(),
        "a refused write left a row this build cannot read"
    );
    assert!(matches!(
        block_on(port.spool(SpoolId::new())),
        Ok(None) | Err(StoreError::NotFound { .. })
    ));
}

/// A 0.4.3 database whose one charge row was changed, as a hand or a build
/// before this one's refusals could leave it, before this build opens it.
fn copied_and_changed(statement: &str) -> TempDir {
    let dir = copied();
    connect(&dir.path().join(DATABASE_FILE_NAME))
        .expect("the database opens")
        .execute_batch(statement)
        .unwrap_or_else(|error| panic!("`{statement}` was refused: {error}"));
    dir
}

/// A charge 0.4.3's database holds that names a spool with only part of a
/// calculation — its event no longer says the diameter — and one whose stored
/// tool is not the one its key names are each carried forward as they are,
/// written into the migration's report, and reported by every read of them.
#[test]
fn an_incoherent_charge_0_4_3_held_is_reported_by_the_migration_and_every_read() {
    for (statement, said) in [
        (
            "UPDATE events SET payload = json_remove(payload, '$.payload.diameter_mm') \
             WHERE kind = 'filament_consumed' AND print_id IS NOT NULL",
            "without the whole calculation",
        ),
        (
            "UPDATE charges SET tool = 7 WHERE charge_key LIKE 'print:%'",
            "which its key does not name",
        ),
    ] {
        let dir = copied_and_changed(statement);
        let store = SqliteStore::open(dir.path()).expect("the 0.4.3 database opens");
        let reported: Vec<_> = store
            .migrated()
            .iter()
            .filter(|note| note.subject.starts_with("charge print:"))
            .cloned()
            .collect();
        assert_eq!(
            reported.len(),
            1,
            "after `{statement}`: {:#?}",
            store.migrated()
        );
        assert_eq!(reported[0].version, 6);
        assert!(reported[0].detail.contains(said), "{}", reported[0]);
        let port: &dyn Store = &store;
        let read = block_on(port.charges(PRINT.parse().expect("an id")));
        assert!(
            matches!(&read, Err(StoreError::Database { detail }) if detail.contains(said)),
            "after `{statement}` the charge read answered {read:?}"
        );
        drop(store);
        let reopened = SqliteStore::open(dir.path()).expect("the database opens again");
        assert!(
            reopened
                .migration_report()
                .expect("the report reads")
                .contains(&reported[0]),
            "the database's own report lost the charge"
        );
    }
}

/// The unobserved charge's own event, which a hand-written charge row names.
fn unobserved_event(connection: &rusqlite::Connection) -> String {
    connection
        .query_row(
            "SELECT event_id FROM charges WHERE charge_key LIKE 'unobserved:%'",
            [],
            |row| row.get(0),
        )
        .expect("the 0.4.3 database holds an unobserved charge")
}

/// Write one uncharged charge row under `key` and `tool`, as any writer could.
fn insert_charge(
    connection: &rusqlite::Connection,
    key: &str,
    tool: i64,
) -> rusqlite::Result<usize> {
    let event = unobserved_event(connection);
    connection.execute(
        "INSERT INTO charges (charge_key, print_id, tool, spool_id, length_mm, grams, event_id, \
         charged_at) VALUES (?1, NULL, ?2, NULL, 10.0, NULL, ?3, '2026-03-01T12:00:00.000000000Z')",
        rusqlite::params![key, tool, event],
    )
}

/// An unobserved key's tool is held exactly on write: a row whose stored tool
/// is the key's generation, its print number or a number its file name
/// carries — each a field the key spells beside its tool — is refused on
/// insert and on update, while a row whose tool is the key's own is written,
/// an origin carrying a colon included.
#[test]
fn an_unobserved_charge_is_refused_any_tool_but_the_one_its_key_names() {
    let dir = copied();
    drop(SqliteStore::open(dir.path()).expect("the 0.4.3 database opens"));
    let connection = connect(&dir.path().join(DATABASE_FILE_NAME)).expect("the database opens");
    let refused = |answer: rusqlite::Result<usize>, what: &str| {
        let said = answer
            .err()
            .unwrap_or_else(|| panic!("the database wrote {what}"))
            .to_string();
        assert!(said.contains("charges.coherent"), "{what}: {said}");
    };

    // 0.4.3's own row: `unobserved:local:0:3:0:benchy.gcode`, tool 0.
    let update = |tool: i64| {
        connection.execute(
            "UPDATE charges SET tool = ?1 WHERE charge_key LIKE 'unobserved:%'",
            [tool],
        )
    };
    refused(update(3), "the print number as the tool, on update");
    refused(update(7), "a tool the key does not spell, on update");
    assert_eq!(update(0), Ok(1), "the key's own tool was refused on update");

    let key = "unobserved:local:5:3:0:v:7:part.gcode";
    for (tool, what) in [
        (5, "the generation as the tool, on insert"),
        (3, "the print number as the tool, on insert"),
        (7, "a number of the file name as the tool, on insert"),
    ] {
        refused(insert_charge(&connection, key, tool), what);
    }
    assert_eq!(insert_charge(&connection, key, 0), Ok(1));
    let colon = "unobserved:sd:card:5:3:2:benchy.gcode";
    refused(
        insert_charge(&connection, colon, 3),
        "a tool after an origin with a colon",
    );
    assert_eq!(insert_charge(&connection, colon, 2), Ok(1));
    for (tool, what) in [
        (5, "the generation as the tool, on update"),
        (7, "a number of the file name as the tool, on update"),
    ] {
        refused(
            connection.execute(
                "UPDATE charges SET tool = ?1 WHERE charge_key = ?2",
                rusqlite::params![tool, key],
            ),
            what,
        );
    }

    let store = SqliteStore::open(dir.path()).expect("the database opens");
    let port: &dyn Store = &store;
    assert!(
        block_on(port.charges(PRINT.parse().expect("an id"))).is_ok(),
        "a refused write left a row this build cannot read"
    );
}
