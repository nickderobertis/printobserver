//! The default timeline read stays an index walk at five years of history.
//!
//! The upper end of realistic use is about a million events over fifteen
//! thousand prints in five years, most of them the periodic observation every
//! active print takes and about a tenth of them of a timeline kind. That
//! history is generated here, inside one transaction on the store's own
//! database file, in those proportions — rather than committed as a file —
//! and the default read is held to two things: it answers exactly the newest
//! hundred timeline events, and the database answers it through an index with
//! no full scan of `events`.

use crate::block_on::block_on;
use crate::fixture::Store;
use printobserver_core::kinds::TIMELINE_KINDS;
use printobserver_core::store::{DEFAULT_HISTORY_WINDOW, TimelineQuery};
use printobserver_store_sqlite::{DATABASE_FILE_NAME, SqliteStore, connect};
use printobserver_types::EventId;
use tempfile::TempDir;

/// How many events the history holds.
const EVENTS: u64 = 1_000_000;

/// How many prints those events are spread over.
const PRINTS: u64 = 15_000;

/// The seconds between two events: five years over a million events.
const SPACING_S: u64 = 157;

/// The instant the history starts at, 2024-01-01T00:00:00Z.
const START_S: u64 = 1_704_067_200;

/// Of every hundred events, how many are of a timeline kind.
const TIMELINE_PER_HUNDRED: u64 = 10;

/// Of every hundred events, how many are a detector's alert; every other one is
/// a periodic observation.
const ALERTS_PER_HUNDRED: u64 = 5;

/// The identifier of the `n`th event, ascending with `n` as the store's own do.
fn event_id(n: u64) -> String {
    format!("0191f0a0-0000-7000-8000-{n:012x}")
}

/// Whether the `n`th event is of a timeline kind.
const fn is_timeline(n: u64) -> bool {
    n % 100 < TIMELINE_PER_HUNDRED
}

/// The statement that writes the whole history in the proportions above.
///
/// The prints first, since every event naming one references it; then the
/// events, each a periodic observation, a detector alert or — one in ten — one
/// of the sixteen timeline kinds in turn, a quarter of those with no print as a
/// spool command has none.
fn history_statement() -> String {
    let timeline_kind = TIMELINE_KINDS
        .iter()
        .enumerate()
        .map(|(index, kind)| format!("WHEN {index} THEN '{kind}'"))
        .collect::<Vec<_>>()
        .join(" ");
    let kinds = TIMELINE_KINDS.len();
    format!(
        "WITH RECURSIVE p(i) AS (SELECT 0 UNION ALL SELECT i + 1 FROM p WHERE i < {last_print})
         INSERT INTO prints (id, provider_print_id, file_name, state, opened_at, ended_at,
                             end_reason, narrowings)
         SELECT printf('0191f0a0-0000-7000-9000-%012x', i), NULL, 'part.gcode',
                '\"operational\"', '2024-01-01T00:00:00.000000000Z', NULL, NULL, '[]'
         FROM p;
         WITH RECURSIVE n(i) AS (SELECT 1 UNION ALL SELECT i + 1 FROM n WHERE i < {EVENTS}),
         k(i, kind) AS (
             SELECT i, CASE
                 WHEN i % 100 < {TIMELINE_PER_HUNDRED}
                     THEN CASE (i / 100) % {kinds} {timeline_kind} END
                 WHEN i % 100 < {alerts} THEN 'obico_failure_alert'
                 ELSE 'periodic_observation' END
             FROM n)
         INSERT INTO events (id, print_id, source, received_at, kind, payload, raw)
         SELECT printf('0191f0a0-0000-7000-8000-%012x', i),
                CASE WHEN i % 100 < {TIMELINE_PER_HUNDRED} AND (i / 100) % 4 = 0 THEN NULL
                     ELSE printf('0191f0a0-0000-7000-9000-%012x', (i - 1) * {PRINTS} / {EVENTS})
                END,
                'system',
                strftime('%Y-%m-%dT%H:%M:%S', {START_S} + i * {SPACING_S}, 'unixepoch')
                    || '.000000000Z',
                kind,
                json_object('kind', kind, 'payload', json_object('n', i)),
                NULL
         FROM k;",
        last_print = PRINTS - 1,
        alerts = TIMELINE_PER_HUNDRED + ALERTS_PER_HUNDRED,
    )
}

/// The default timeline read answers the newest hundred timeline events of a
/// five-year history, through an index and with no full scan of `events`.
#[test]
fn the_default_timeline_read_walks_an_index_over_five_years_of_history() {
    let dir = TempDir::new().expect("a temporary state directory");
    let store = SqliteStore::open(dir.path()).expect("the store opens");
    let mut connection = connect(&dir.path().join(DATABASE_FILE_NAME)).expect("a connection opens");
    let transaction = connection.transaction().expect("a transaction opens");
    transaction
        .execute_batch(&history_statement())
        .expect("the history is written");
    transaction.commit().expect("the history commits");
    let (events, timeline): (u64, u64) = connection
        .query_row(
            &format!(
                "SELECT COUNT(*), SUM(kind IN ({})) FROM events",
                TIMELINE_KINDS
                    .iter()
                    .map(|kind| format!("'{kind}'"))
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .expect("the history counts");
    assert_eq!(
        (events, timeline),
        (EVENTS, EVENTS / 100 * TIMELINE_PER_HUNDRED)
    );

    let query = TimelineQuery {
        kinds: Vec::new(),
        since: None,
        until: None,
        spool_id: None,
        before: None,
        limit: None,
    };
    let port: &dyn Store = &store;
    let answered: Vec<EventId> = block_on(port.timeline(query.clone()))
        .expect("the timeline reads")
        .iter()
        .map(|event| event.id)
        .collect();
    let newest: Vec<EventId> = (1..=EVENTS)
        .rev()
        .filter(|n| is_timeline(*n))
        .take(DEFAULT_HISTORY_WINDOW as usize)
        .map(|n| event_id(n).parse().expect("a fixed identifier"))
        .collect();
    assert_eq!(
        answered, newest,
        "the default read did not answer the newest hundred timeline events"
    );

    let plan = store.timeline_plan(&query).expect("the plan reads");
    assert!(
        plan.iter()
            .any(|step| step.contains("INDEX events_by_kind_and_instant")),
        "the timeline read names no index over events' kinds: {plan:#?}"
    );
    assert!(
        !plan.iter().any(|step| {
            let words: Vec<&str> = step.split_whitespace().collect();
            words.first() == Some(&"SCAN") && matches!(words.get(1), Some(&("events" | "e")))
        }),
        "the timeline read scans the whole of events: {plan:#?}"
    );
}
