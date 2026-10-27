//! The spool ledger across restarts, over the real store on disk.
//!
//! Every journey here composes the real server over the real `SqliteStore`
//! under a temporary state directory, and every restart stops that server and
//! opens a **fresh** supervisor and a fresh store over the same file — so
//! nothing a journey proves can have been carried across in memory. The
//! machine tells the story on the far side of the printer port: the file it
//! holds and the analysis of it, how far into it the job has got, and the
//! tally of how often it was printed.
//!
//! What is asserted is read back through the surface a caller has — `spools`,
//! `spool` and `printer_history` — and is the same two things every time:
//! each spool holds exactly its initial figure less every charge against it
//! plus every correction, and no event of the printer's history was written
//! twice.
//!
//! A process dying between a write and the step after it is
//! [`CommitThenFail`]: the write commits and its caller is told it did not.

use std::collections::BTreeMap;

use printobserver_core::{density_for, grams_for_length};
use printobserver_printer_api::{PrinterState, ToolFilament};
use printobserver_store_sqlite::SqliteStore;
use printobserver_types::Timestamp;
use printobserver_types::serde_json::{Value, json};

use crate::committing_store::{CommitThenFail, DETAIL};
use crate::spooling::{close, loaded, plain, registered};
use crate::world::{SECRET, World};

/// The file every journey here prints.
pub(crate) const FILE: &str = "benchy.gcode";

/// Its size, which the machine's job reports too.
pub(crate) const SIZE_BYTES: i64 = 4_096;

/// What its analysis says tool 0 feeds over the whole of it, in millimetres.
pub(crate) const WHOLE_MM: f64 = 8_000.0;

/// What a spool holds when it is registered, in grams.
pub(crate) const FULL_G: f64 = 1_000.0;

/// The grams a length of PLA at the default diameter weighs.
pub(crate) fn grams(length_mm: f64) -> f64 {
    grams_for_length(
        length_mm,
        printobserver_core::DEFAULT_DIAMETER_MM,
        density_for("PLA").expect("PLA has a known density"),
    )
}

/// A world whose machine holds the file, analysed, part-way through printing it.
pub(crate) async fn printing_world() -> World {
    let world = World::open().await;
    analysed(&world);
    world
}

/// The machine holds the file, freshly uploaded, with its analysis.
pub(crate) fn analysed(world: &World) {
    world.printer.holds_file(
        FILE,
        SIZE_BYTES,
        Some(vec![ToolFilament {
            tool: 0,
            length_mm: WHOLE_MM,
            volume_cm3: None,
        }]),
    );
}

/// Read the prints, which is what has the supervisor look at the machine's job:
/// adopting it, noting where it has got to, or closing what it shows is over.
pub(crate) async fn look_at_the_job(world: &World) -> Value {
    let (status, listing) = world.get(&plain(world, "prints")).await;
    assert_eq!(status, reqwest::StatusCode::OK, "{listing}");
    listing
}

/// The machine finishes the job it is running: it reports itself idle with the
/// file read to its end at completion 1.0, and the file's tally counts one more
/// completed print.
pub(crate) fn finishes(world: &World) {
    world.printer.reports_finished();
    world.printer.counts_print(FILE, true, Timestamp::now());
    world.printer.in_state(PrinterState::Operational);
}

/// Every event of the printer's history, newest first, read a page at a time.
pub(crate) async fn the_whole_history(world: &World) -> Vec<Value> {
    let history = plain(world, "printer_history");
    let mut events = Vec::new();
    let mut before: Option<String> = None;
    loop {
        let url = before.as_ref().map_or_else(
            || format!("{history}?limit=1000"),
            |before| format!("{history}?limit=1000&before={before}"),
        );
        let (status, page) = world.get(&url).await;
        assert_eq!(status, reqwest::StatusCode::OK, "{page}");
        events.extend(page["events"].as_array().cloned().unwrap_or_default());
        match page.get("next").and_then(Value::as_str) {
            Some(next) => before = Some(next.to_owned()),
            None => return events,
        }
    }
}

/// The events of one kind, oldest first.
pub(crate) fn of_kind<'a>(events: &'a [Value], kind: &str) -> Vec<&'a Value> {
    events
        .iter()
        .rev()
        .filter(|event| event["kind"] == json!(kind))
        .collect()
}

/// Every spool, retired ones included, by its identifier.
async fn spools_by_id(world: &World) -> BTreeMap<String, Value> {
    let (status, listing) = world
        .get(&format!("{}?include_retired=true", plain(world, "spools")))
        .await;
    assert_eq!(status, reqwest::StatusCode::OK, "{listing}");
    listing["spools"]
        .as_array()
        .expect("the listing carries spools")
        .iter()
        .map(|spool| {
            (
                spool["id"].as_str().unwrap_or_default().to_owned(),
                spool.clone(),
            )
        })
        .collect()
}

/// One spool, as its own read answers it.
pub(crate) async fn spool_of(world: &World, spool_id: &str) -> Value {
    let (status, read) = world.get(&world.spool_url("spool", spool_id)).await;
    assert_eq!(status, reqwest::StatusCode::OK, "{read}");
    read
}

/// The grams every `filament_consumed` in the history debited one spool.
pub(crate) fn debited(events: &[Value], spool_id: &str) -> Vec<f64> {
    of_kind(events, "filament_consumed")
        .into_iter()
        .filter(|event| event["payload"]["charged"]["spool_id"] == json!(spool_id))
        .map(|event| {
            event["payload"]["charged"]["grams"]
                .as_f64()
                .unwrap_or_default()
        })
        .collect()
}

/// Hold every spool and the history to the two things every journey asserts:
/// no event was written twice, and each spool holds its initial figure less
/// every charge against it plus every correction.
async fn the_ledger_balances(world: &World) -> Vec<Value> {
    let events = the_whole_history(world).await;
    let mut seen = std::collections::BTreeSet::new();
    for event in &events {
        assert!(
            seen.insert(event["id"].as_str().unwrap_or_default().to_owned()),
            "the history carries one event twice: {event}"
        );
    }
    let charge_keys: Vec<String> = of_kind(&events, "filament_consumed")
        .iter()
        .map(|event| event["payload"]["charge_key"].to_string())
        .collect();
    let distinct: std::collections::BTreeSet<&String> = charge_keys.iter().collect();
    assert_eq!(
        distinct.len(),
        charge_keys.len(),
        "one charge was debited twice: {charge_keys:#?}"
    );
    for (spool_id, spool) in spools_by_id(world).await {
        let charged: f64 = debited(&events, &spool_id).iter().sum();
        let corrected: f64 = of_kind(&events, "spool_adjusted")
            .into_iter()
            .filter(|event| event["payload"]["spool_id"] == json!(spool_id))
            .map(|event| {
                event["payload"]["remaining_after_g"]
                    .as_f64()
                    .unwrap_or_default()
                    - event["payload"]["remaining_before_g"]
                        .as_f64()
                        .unwrap_or_default()
            })
            .sum();
        let initial = spool["initial_g"].as_f64().unwrap_or_default();
        assert!(
            close(&spool["remaining_g"], initial - charged + corrected),
            "spool {spool_id} holds {} and its history says {initial} - {charged} + \
             {corrected}: {spool}",
            spool["remaining_g"]
        );
        // The spool's own read answers the same figure the listing does.
        assert_eq!(spool_of(world, &spool_id).await["spool"], spool);
    }
    events
}

/// (a) A spool swapped mid-print, the server restarted, and the print then
/// finishing: the outgoing spool is charged its segment and the incoming one
/// only the remainder.
#[tokio::test(flavor = "multi_thread")]
async fn a_swap_before_a_restart_charges_each_spool_its_own_segment() {
    let world = printing_world().await;
    let outgoing = registered(&world, "PLA white", FULL_G).await;
    let incoming = registered(&world, "PLA grey", FULL_G).await;
    loaded(&world, &outgoing, 0).await;
    world.printer.at_byte(1_024);
    look_at_the_job(&world).await;

    // Half-way through the file, the operator swaps the spool on tool 0.
    world.printer.at_byte(SIZE_BYTES / 2);
    let swapped = loaded(&world, &incoming, 0).await;
    assert_eq!(
        swapped["spool"]["lifecycle"],
        json!({ "state": "loaded", "tool": 0 }),
        "{swapped}"
    );

    let world = world.restart().await;
    world.printer.at_byte(3 * SIZE_BYTES / 4);
    look_at_the_job(&world).await;
    finishes(&world);
    look_at_the_job(&world).await;

    let events = the_ledger_balances(&world).await;
    let half = grams(WHOLE_MM / 2.0);
    let outgoing_charges = debited(&events, &outgoing);
    let incoming_charges = debited(&events, &incoming);
    assert_eq!(outgoing_charges.len(), 1, "{outgoing_charges:?}");
    assert!(
        close(&json!(outgoing_charges[0]), half),
        "{outgoing_charges:?}"
    );
    assert_eq!(incoming_charges.len(), 1, "{incoming_charges:?}");
    assert!(
        close(&json!(incoming_charges[0]), half),
        "{incoming_charges:?}"
    );
    assert_eq!(of_kind(&events, "print_started").len(), 1);
    assert_eq!(of_kind(&events, "print_ended").len(), 1);

    // Starting again over the same file changes nothing.
    let world = world.restart().await;
    assert_eq!(the_ledger_balances(&world).await, events);
    world.server.stop().await;
}

/// (b) A close-out whose debit and settlement committed and whose caller was
/// told they did not: the next start ends that print, and debits nothing again.
#[tokio::test(flavor = "multi_thread")]
async fn a_close_out_that_committed_and_died_is_finished_once_on_the_next_start() {
    let world = printing_world().await;
    let spool = registered(&world, "PLA white", FULL_G).await;
    loaded(&world, &spool, 0).await;
    world.printer.at_byte(1_024);
    let listing = look_at_the_job(&world).await;
    let print_id = listing["active"]
        .as_str()
        .expect("the job was adopted")
        .to_owned();

    let state_dir = world.state_dir();
    let world = world
        .restart_over(|_| {
            CommitThenFail::once(
                SqliteStore::open(&state_dir).expect("the store opens"),
                "settle_print",
            )
        })
        .await;
    look_at_the_job(&world).await;
    world.printer.at_byte(SIZE_BYTES / 2);
    look_at_the_job(&world).await;
    // The print is cancelled at the machine; the close-out's write commits and
    // is reported failed, so the print record is never ended.
    world.printer.in_state(PrinterState::Operational);
    let (status, failed) = world.get(&plain(&world, "prints")).await;
    assert_eq!(
        status,
        reqwest::StatusCode::INTERNAL_SERVER_ERROR,
        "{failed}"
    );
    assert!(
        failed["error"]
            .as_str()
            .unwrap_or_default()
            .contains(DETAIL),
        "{failed}"
    );

    let world = world.restart().await;
    let listing = look_at_the_job(&world).await;
    let ended = listing["prints"]
        .as_array()
        .and_then(|prints| prints.iter().find(|print| print["id"] == json!(print_id)))
        .cloned()
        .unwrap_or_default();
    assert!(
        !ended["ended_at"].is_null(),
        "the next start did not end the print: {ended}"
    );
    let events = the_ledger_balances(&world).await;
    let charges = debited(&events, &spool);
    assert_eq!(charges.len(), 1, "{charges:?}");
    assert!(
        close(&json!(charges[0]), grams(WHOLE_MM / 2.0)),
        "{charges:?}"
    );
    assert_eq!(of_kind(&events, "print_ended").len(), 1);

    let world = world.restart().await;
    assert_eq!(the_ledger_balances(&world).await, events);
    world.server.stop().await;
}

/// (c) A print the machine counted while nothing watched, whose debit
/// committed on a start that was then told it had not: that start is refused,
/// and the next charges the print once.
#[tokio::test(flavor = "multi_thread")]
async fn an_unseen_print_whose_debit_committed_and_died_is_charged_once() {
    let world = World::open().await;
    analysed(&world);
    world.printer.in_state(PrinterState::Operational);
    let spool = registered(&world, "PLA white", FULL_G).await;
    loaded(&world, &spool, 0).await;
    // The first start over this file's tally takes it as the baseline.
    let world = world.restart().await;

    // Printed while the server was down.
    world.printer.counts_print(FILE, true, Timestamp::now());
    let state_dir = world.state_dir();
    let (world, refused) = world
        .restart_refused_over(|_| {
            CommitThenFail::once(
                SqliteStore::open(&state_dir).expect("the store opens"),
                "account_tallies",
            )
        })
        .await;
    assert!(refused.contains(DETAIL), "{refused}");
    assert!(
        world
            .server
            .reconciliation()
            .unobserved
            .accounted()
            .is_empty(),
        "the start after the one that died accounted the print again: {:?}",
        world.server.reconciliation()
    );

    let events = the_ledger_balances(&world).await;
    assert_eq!(of_kind(&events, "unaccounted_print").len(), 1);
    let charges = debited(&events, &spool);
    assert_eq!(charges.len(), 1, "{charges:?}");
    assert!(close(&json!(charges[0]), grams(WHOLE_MM)), "{charges:?}");
    world.server.stop().await;
}

/// (d) The startup accounting, run on three consecutive starts over tallies
/// that did not move, writes nothing after the first.
#[tokio::test(flavor = "multi_thread")]
async fn the_startup_accounting_writes_nothing_over_tallies_that_did_not_move() {
    let world = World::open().await;
    analysed(&world);
    world.printer.in_state(PrinterState::Operational);
    let spool = registered(&world, "PLA white", FULL_G).await;
    loaded(&world, &spool, 0).await;
    let world = world.restart().await;
    world.printer.counts_print(FILE, true, Timestamp::now());

    let world = world.restart().await;
    assert!(
        !world
            .server
            .reconciliation()
            .unobserved
            .accounted()
            .is_empty(),
        "the start after a print nobody saw accounted nothing"
    );
    let first = the_ledger_balances(&world).await;
    let mut world = world;
    for start in 2..=3 {
        world = world.restart().await;
        assert!(
            world
                .server
                .reconciliation()
                .unobserved
                .accounted()
                .is_empty(),
            "start {start} over unmoved tallies wrote {:?}",
            world.server.reconciliation().unobserved.accounted()
        );
        assert_eq!(the_ledger_balances(&world).await, first, "start {start}");
    }
    world.server.stop().await;
}

/// A start that cannot read the printer skips the accounting, says why in its
/// own record, and starts; the next start that reaches the printer accounts
/// what that one could not, once.
#[tokio::test(flavor = "multi_thread")]
async fn a_start_that_cannot_read_the_printer_skips_the_accounting_and_starts() {
    let world = World::open().await;
    analysed(&world);
    world.printer.in_state(PrinterState::Operational);
    let spool = registered(&world, "PLA white", FULL_G).await;
    loaded(&world, &spool, 0).await;
    let world = world.restart().await;
    assert_eq!(world.server.reconciliation().unobserved.skipped(), None);

    world.printer.counts_print(FILE, true, Timestamp::now());
    world.printer.unreadable(true);
    let world = world.restart().await;
    let skipped = world
        .server
        .reconciliation()
        .unobserved
        .skipped()
        .map(str::to_owned)
        .expect("a start that could not read the printer says it skipped the accounting");
    assert!(
        skipped.contains("could not be read") && skipped.contains("switched off"),
        "{skipped}"
    );
    assert!(
        world
            .server
            .reconciliation()
            .unobserved
            .accounted()
            .is_empty()
    );
    assert!(debited(&the_ledger_balances(&world).await, &spool).is_empty());

    world.printer.unreadable(false);
    let world = world.restart().await;
    assert_eq!(world.server.reconciliation().unobserved.skipped(), None);
    assert!(
        !world
            .server
            .reconciliation()
            .unobserved
            .accounted()
            .is_empty()
    );
    let charges = debited(&the_ledger_balances(&world).await, &spool);
    assert_eq!(charges.len(), 1, "{charges:?}");
    world.server.stop().await;
}

/// (e) One file printed as two jobs — the first watched across a restart, the
/// second while the server was down — is charged as two prints, each once.
#[tokio::test(flavor = "multi_thread")]
async fn one_file_printed_twice_watched_and_unwatched_is_charged_twice_once_each() {
    let world = printing_world().await;
    let spool = registered(&world, "PLA white", FULL_G).await;
    loaded(&world, &spool, 0).await;
    world.printer.at_byte(1_024);
    look_at_the_job(&world).await;

    let world = world.restart().await;
    world.printer.at_byte(SIZE_BYTES / 2);
    look_at_the_job(&world).await;
    finishes(&world);
    look_at_the_job(&world).await;

    // The second job, while nothing watched.
    world.printer.counts_print(FILE, true, Timestamp::now());
    let world = world.restart().await;
    let world = world.restart().await;

    let events = the_ledger_balances(&world).await;
    let charges = debited(&events, &spool);
    assert_eq!(charges.len(), 2, "{charges:?}");
    for charge in &charges {
        assert!(close(&json!(charge), grams(WHOLE_MM)), "{charges:?}");
    }
    assert_eq!(of_kind(&events, "print_ended").len(), 1);
    assert_eq!(of_kind(&events, "unaccounted_print").len(), 1);
    world.server.stop().await;
}

/// (f) A file printed unseen, its tally then reset by a re-upload, and printed
/// unseen again on the ordinal the first print had: both are debited once, the
/// reset is written once, and no start after it writes either again.
#[tokio::test(flavor = "multi_thread")]
async fn a_reset_tally_reused_ordinal_is_a_new_print_charged_once() {
    let world = World::open().await;
    analysed(&world);
    world.printer.in_state(PrinterState::Operational);
    let spool = registered(&world, "PLA white", FULL_G).await;
    loaded(&world, &spool, 0).await;
    let world = world.restart().await;

    world.printer.counts_print(FILE, true, Timestamp::now());
    let world = world.restart().await;
    // Deleted and uploaded again: its tally starts over.
    analysed(&world);
    let world = world.restart().await;
    world.printer.counts_print(FILE, true, Timestamp::now());
    let mut world = world.restart().await;

    let events = the_ledger_balances(&world).await;
    for _ in 0..2 {
        world = world.restart().await;
        assert_eq!(the_ledger_balances(&world).await, events);
    }
    assert_eq!(of_kind(&events, "file_tally_rebaselined").len(), 1);
    assert_eq!(of_kind(&events, "unaccounted_print").len(), 2);
    let charges = debited(&events, &spool);
    assert_eq!(charges.len(), 2, "{charges:?}");
    for charge in &charges {
        assert!(close(&json!(charge), grams(WHOLE_MM)), "{charges:?}");
    }
    world.server.stop().await;
}

/// The `Obico` notification of a filament change about the print the machine
/// is running, started when the job did.
pub(crate) fn filament_change(started_at: i64) -> Value {
    json!({
        "event": { "type": "FilamentChange", "is_warning": false, "print_paused": false },
        "printer": { "id": 17, "name": "Prusa MK4" },
        "print": { "id": 4211, "filename": FILE, "started_at": started_at, "ended_at": "" },
        "img_url": "http://127.0.0.1:1/frame.jpg",
    })
}

/// (g) A filament change signalled mid-print, the print finishing with nobody
/// saying which spool is now on the tool, and the server restarted: the
/// remainder is held across the restart, then taken once by the next load —
/// charged to the incoming spool, never to the outgoing one.
#[tokio::test(flavor = "multi_thread")]
async fn a_held_remainder_survives_a_restart_and_is_taken_once_by_the_next_load() {
    let world = printing_world().await;
    let outgoing = registered(&world, "PLA white", FULL_G).await;
    let incoming = registered(&world, "PLA grey", FULL_G).await;
    loaded(&world, &outgoing, 0).await;
    world.printer.at_byte(SIZE_BYTES / 4);

    let mut completions = world.server.completions();
    let job_started = printobserver_core::unix_seconds(Timestamp::now()) - 900;
    let status = reqwest::Client::new()
        .post(format!("{}?token={SECRET}", world.server.ingress_url()))
        .header(reqwest::header::CONTENT_TYPE, "application/json")
        .body(filament_change(job_started).to_string())
        .send()
        .await
        .expect("the ingress answers")
        .status();
    assert_eq!(status, reqwest::StatusCode::ACCEPTED);
    completions.changed().await.expect("the handling finishes");

    world.printer.at_byte(SIZE_BYTES / 2);
    look_at_the_job(&world).await;
    finishes(&world);
    look_at_the_job(&world).await;

    let world = world.restart().await;
    let (_, listing) = world.get(&plain(&world, "spools")).await;
    let pending = listing["pending"].as_array().cloned().unwrap_or_default();
    assert_eq!(pending.len(), 1, "the remainder is not held: {listing}");
    let held_mm = pending[0]["length_mm"].as_f64().unwrap_or_default();
    assert!(close(&json!(held_mm), 3.0 * WHOLE_MM / 4.0), "{listing}");
    let events = the_ledger_balances(&world).await;
    assert_eq!(of_kind(&events, "charge_pended").len(), 1);
    assert_eq!(of_kind(&events, "filament_change_signalled").len(), 1);

    let took = loaded(&world, &incoming, 0).await;
    let spool_loaded = took["events"]
        .as_array()
        .and_then(|written| {
            written
                .iter()
                .find(|event| event["kind"] == json!("spool_loaded"))
        })
        .cloned()
        .unwrap_or_default();
    assert_eq!(
        spool_loaded["payload"]["took_pending"]
            .as_array()
            .map(Vec::len),
        Some(1),
        "the load did not name the remainder it took: {took}"
    );
    let (_, listing) = world.get(&plain(&world, "spools")).await;
    assert_eq!(listing["pending"], json!([]), "{listing}");

    let world = world.restart().await;
    let events = the_ledger_balances(&world).await;
    let outgoing_charges = debited(&events, &outgoing);
    assert_eq!(outgoing_charges.len(), 1, "{outgoing_charges:?}");
    assert!(
        close(&json!(outgoing_charges[0]), grams(WHOLE_MM / 4.0)),
        "the outgoing spool was charged past the signal: {outgoing_charges:?}"
    );
    let incoming_charges = debited(&events, &incoming);
    assert_eq!(incoming_charges.len(), 1, "{incoming_charges:?}");
    assert!(
        close(&json!(incoming_charges[0]), grams(3.0 * WHOLE_MM / 4.0)),
        "{incoming_charges:?}"
    );
    world.server.stop().await;
}

/// Ask the operator's close of one print, for one outcome.
async fn closes(world: &World, print_id: &str, outcome: &str) -> (reqwest::StatusCode, Value) {
    let path = printobserver_server::operation("close_print")
        .expect("the operation is declared")
        .full_path();
    let print = print_id.parse().expect("a print identifier");
    world
        .post(
            &world.operation_url(&path, print),
            &json!({
                "outcome": outcome,
                "reason": "the printer finished while the supervisor was not looking",
                "actor": "operator",
            }),
        )
        .await
}

/// A print the machine is running, adopted by a listing, over a tally the
/// supervisor has already taken as its baseline; answers the print.
async fn an_adopted_print(world: World) -> (World, String, String) {
    analysed(&world);
    world.printer.in_state(PrinterState::Operational);
    let spool = registered(&world, "PLA white", FULL_G).await;
    loaded(&world, &spool, 0).await;
    let world = world.restart().await;
    world.printer.in_state(PrinterState::Printing);
    world.printer.at_byte(1_024);
    let listing = look_at_the_job(&world).await;
    let print_id = listing["active"]
        .as_str()
        .expect("the job was adopted")
        .to_owned();
    (world, print_id, spool)
}

/// (e) A print the operator closed as completed is the print the machine's
/// tally counts: the next start accounts no unseen print for it, and no close
/// or start debits its spool.
#[tokio::test(flavor = "multi_thread")]
async fn a_print_the_operator_closed_is_not_charged_again_as_unseen() {
    let (world, print_id, spool) = an_adopted_print(World::open().await).await;
    // The job finishes with nothing reading the machine, so the record stays
    // open until the operator closes it.
    finishes(&world);
    let (status, closed) = closes(&world, &print_id, "completed").await;
    assert_eq!(status, reqwest::StatusCode::OK, "{closed}");

    let world = world.restart().await;
    assert!(
        world
            .server
            .reconciliation()
            .unobserved
            .accounted()
            .is_empty(),
        "the start after the close accounted the closed print as unseen: {:?}",
        world.server.reconciliation()
    );
    let events = the_ledger_balances(&world).await;
    assert!(
        of_kind(&events, "unaccounted_print").is_empty(),
        "{events:?}"
    );
    assert!(debited(&events, &spool).is_empty(), "{events:?}");
    let ended = of_kind(&events, "print_ended");
    assert_eq!(ended.len(), 1, "{ended:?}");
    assert_eq!(ended[0]["payload"]["outcome"], json!("completed"));
    world.server.stop().await;
}

/// (f) A close whose settlement committed and whose caller was told it did
/// not leaves the print settled and open; the next close only ends it, with no
/// second `print_ended` and no debit.
#[tokio::test(flavor = "multi_thread")]
async fn a_close_whose_settlement_committed_and_died_is_finished_by_the_next_close() {
    let (world, print_id, spool) = an_adopted_print(World::open().await).await;
    let state_dir = world.state_dir();
    let world = world
        .restart_over(|_| {
            CommitThenFail::once(
                SqliteStore::open(&state_dir).expect("the store opens"),
                "settle_print",
            )
        })
        .await;
    world.printer.in_state(PrinterState::Printing);
    look_at_the_job(&world).await;
    world.printer.in_state(PrinterState::Operational);

    let (status, failed) = closes(&world, &print_id, "interrupted").await;
    assert_eq!(
        status,
        reqwest::StatusCode::INTERNAL_SERVER_ERROR,
        "{failed}"
    );
    assert!(
        failed["error"]
            .as_str()
            .unwrap_or_default()
            .contains(DETAIL),
        "{failed}"
    );
    let (status, finished) = closes(&world, &print_id, "interrupted").await;
    assert_eq!(status, reqwest::StatusCode::OK, "{finished}");

    let listing = look_at_the_job(&world).await;
    let ended = listing["prints"]
        .as_array()
        .and_then(|prints| prints.iter().find(|print| print["id"] == json!(print_id)))
        .cloned()
        .unwrap_or_default();
    assert!(
        !ended["ended_at"].is_null(),
        "the print was not ended: {ended}"
    );
    let events = the_ledger_balances(&world).await;
    let ends = of_kind(&events, "print_ended");
    assert_eq!(ends.len(), 1, "{ends:?}");
    assert_eq!(ends[0]["payload"]["outcome"], json!("interrupted"));
    assert!(debited(&events, &spool).is_empty(), "{events:?}");

    let world = world.restart().await;
    assert_eq!(the_ledger_balances(&world).await, events);
    world.server.stop().await;
}

/// (g) A close whose session write committed and whose caller was told it did
/// not leaves the record open; the next close ends it, with one `print_ended`,
/// one `supervision_session_closed` and the session closed.
#[tokio::test(flavor = "multi_thread")]
async fn a_close_whose_session_write_committed_and_died_is_finished_by_the_next_close() {
    Box::pin(a_close_whose_write_committed_and_died_is_finished_by_the_next_close("put_session"))
        .await;
}

/// (h) A close whose `supervision_session_closed` committed after the session
/// was stored closed, and whose caller was told it did not, leaves the record
/// open; the next close ends it and writes that closure no second time.
#[tokio::test(flavor = "multi_thread")]
async fn a_close_whose_session_closure_committed_and_died_is_finished_by_the_next_close() {
    Box::pin(
        a_close_whose_write_committed_and_died_is_finished_by_the_next_close(
            "append_session_closed",
        ),
    )
    .await;
}

/// A close of a print with a session, whose one named write commits and is
/// reported failed, and the close that finishes it: one `print_ended`, one
/// `supervision_session_closed`, the session closed and the record ended.
async fn a_close_whose_write_committed_and_died_is_finished_by_the_next_close(write: &'static str) {
    use printobserver_core::store::SessionStore as _;

    let (world, print_id, _) = an_adopted_print(World::open().await).await;
    let state_dir = world.state_dir();
    let print: printobserver_types::PrintId = print_id.parse().expect("a print identifier");
    let opened = Timestamp::now();
    let session = printobserver_types::serde_json::from_value(json!({
        "print_id": print,
        "session_name": format!("print-{print}"),
        "harness_identity": "claude-code",
        "created_at": opened,
        "last_turn_at": opened,
    }))
    .expect("a session reads from its own fields");
    SqliteStore::open(&state_dir)
        .expect("the store opens")
        .put_session(session)
        .await
        .expect("the print's session is stored");
    let world = world
        .restart_over(|_| {
            CommitThenFail::once(
                SqliteStore::open(&state_dir).expect("the store opens"),
                write,
            )
        })
        .await;
    world.printer.in_state(PrinterState::Printing);
    look_at_the_job(&world).await;
    world.printer.in_state(PrinterState::Operational);

    let (status, failed) = closes(&world, &print_id, "interrupted").await;
    assert_eq!(
        status,
        reqwest::StatusCode::INTERNAL_SERVER_ERROR,
        "{failed}"
    );
    assert!(
        failed["error"]
            .as_str()
            .unwrap_or_default()
            .contains(DETAIL),
        "{failed}"
    );
    let (status, finished) = closes(&world, &print_id, "interrupted").await;
    assert_eq!(status, reqwest::StatusCode::OK, "{finished}");

    let (status, read) = world
        .get(
            &world.operation_url(
                &printobserver_server::operation("status")
                    .expect("the operation is declared")
                    .full_path(),
                print,
            ),
        )
        .await;
    assert_eq!(status, reqwest::StatusCode::OK, "{read}");
    assert!(read["print"]["ended_at"].is_string(), "{read}");
    assert!(read["session"]["closed_at"].is_string(), "{read}");
    let events = the_ledger_balances(&world).await;
    assert_eq!(of_kind(&events, "print_ended").len(), 1, "{events:?}");
    let (status, history) = world
        .get(&format!(
            "{}?limit=100",
            world.operation_url(
                &printobserver_server::operation("history")
                    .expect("the operation is declared")
                    .full_path(),
                print,
            )
        ))
        .await;
    assert_eq!(status, reqwest::StatusCode::OK, "{history}");
    let closed = of_kind(
        history["events"].as_array().map_or(&[][..], Vec::as_slice),
        "supervision_session_closed",
    );
    assert_eq!(closed.len(), 1, "{history}");
    world.server.stop().await;
}
