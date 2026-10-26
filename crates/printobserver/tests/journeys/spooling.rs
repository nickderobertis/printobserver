//! The spool ledger, kept through this program against a real supervisor.
//!
//! Every step is the compiled program as a subprocess, and every effect is
//! read back through the same program: an operator registering, loading,
//! swapping, unloading, correcting and retiring spools and reading the
//! printer's history of all of it; the listing's one line per spool; and the
//! notes a start the filament check refused prints beside its answer.

use printobserver::failure::Exit;
use printobserver_types::serde_json::{Value, json};

use crate::machine::Reports;
use crate::traced::Ran;
use crate::walk;
use crate::world::World;

use super::running;

/// The reason every step of these journeys gives.
const REASON: &str = "the operator is keeping the shelf's spools";

/// Run one change as the operator, and answer what it printed.
fn changed(world: &World, asked: &[&str]) -> Ran {
    let mut given = asked.to_vec();
    given.extend_from_slice(&["--actor", "operator", "--reason", REASON]);
    running::command(world, &given)
}

/// Run one change that has to succeed, and answer its document.
fn done(world: &World, asked: &[&str]) -> Value {
    let mut given = asked.to_vec();
    given.push("--json");
    let ran = changed(world, &given);
    assert_eq!(
        ran.code,
        Some(0),
        "{asked:?} did not succeed: {}",
        ran.said()
    );
    printobserver_types::serde_json::from_str(&ran.out)
        .unwrap_or_else(|error| panic!("{asked:?} answered {error}: {}", ran.out))
}

/// Register one spool of PLA, and answer its identifier.
fn registered(world: &World, name: &str, also: &[&str]) -> String {
    let mut asked = vec![
        "register-spool",
        "--name",
        name,
        "--material",
        "PLA",
        "--initial-g",
        "1000",
    ];
    asked.extend_from_slice(also);
    done(world, &asked)["spool"]["id"]
        .as_str()
        .expect("a registration answers its spool")
        .to_owned()
}

/// Every event of the printer's history, newest first, read a page of `limit`
/// at a time by the cursor each page answers.
fn paged(world: &World, limit: &str) -> Vec<Value> {
    let mut events = Vec::new();
    let mut before: Option<String> = None;
    loop {
        let mut asked = vec!["printer-history", "--limit", limit];
        if let Some(cursor) = before.as_deref() {
            asked.extend_from_slice(&["--before", cursor]);
        }
        let page = running::read(world, &asked);
        events.extend(page["events"].as_array().cloned().unwrap_or_default());
        match page.get("next").and_then(Value::as_str) {
            Some(next) => before = Some(next.to_owned()),
            None => return events,
        }
        assert!(events.len() < 1_000, "the cursor never reached the end");
    }
}

/// An operator keeps two spools through this program — registering, loading,
/// swapping, unloading, correcting by a scale reading and retiring — and reads
/// all of it back: one spool with its own events, an unknown one refused, the
/// listing with and without the retired one, and the printer's history paged
/// by its cursor and narrowed by kind and by spool, carrying every change with
/// who asked and why. A change with an empty reason reaches no server.
pub fn an_operator_keeps_the_ledger_through_this_program(world: &World) {
    let white = registered(world, "PLA white", &["--tare-g", "250"]);
    let grey = registered(world, "PLA grey", &[]);
    done(world, &["load-spool", "--spool-id", &white, "--tool", "0"]);
    let swapped = done(world, &["load-spool", "--spool-id", &grey, "--tool", "0"]);
    let kinds: Vec<&str> = swapped["events"]
        .as_array()
        .expect("a load answers its events")
        .iter()
        .filter_map(|event| event["kind"].as_str())
        .collect();
    assert!(
        kinds.contains(&"spool_unloaded") && kinds.contains(&"spool_loaded"),
        "a load onto an occupied tool did not swap the spool there: {swapped}"
    );
    done(world, &["unload-spool", "--spool-id", &grey]);
    let weighed = done(
        world,
        &["adjust-spool", "--spool-id", &white, "--gross-g", "900"],
    );
    assert_eq!(weighed["spool"]["remaining_g"], json!(650.0), "{weighed}");
    done(world, &["retire-spool", "--spool-id", &grey]);

    let read = running::read(world, &["spool", "--spool-id", &white]);
    assert_eq!(read["spool"]["remaining_g"], json!(650.0), "{read}");
    let recent: Vec<&str> = read["recent"]
        .as_array()
        .expect("a spool's read carries its events")
        .iter()
        .filter_map(|event| event["kind"].as_str())
        .collect();
    assert_eq!(
        recent,
        // The grey spool's load names the white one it replaced, so the white
        // spool's own events carry it too.
        vec![
            "spool_adjusted",
            "spool_unloaded",
            "spool_loaded",
            "spool_loaded",
            "spool_registered"
        ],
        "{read}"
    );
    let unknown = running::command(
        world,
        &[
            "spool",
            "--spool-id",
            &printobserver_types::EventId::new().to_string(),
        ],
    );
    assert_eq!(unknown.code, Some(i32::from(Exit::Refused.status())));
    assert!(
        unknown.err.contains("404") && unknown.err.contains("there is no spool"),
        "an unknown spool was not refused as not found: {}",
        unknown.said()
    );

    let listed = |also: &[&str]| -> Vec<String> {
        let mut asked = vec!["spools"];
        asked.extend_from_slice(also);
        running::read(world, &asked)["spools"]
            .as_array()
            .expect("the listing carries spools")
            .iter()
            .filter_map(|spool| spool["id"].as_str().map(str::to_owned))
            .collect()
    };
    assert!(listed(&[]).contains(&white) && !listed(&[]).contains(&grey));
    assert!(listed(&["--include-retired", "true"]).contains(&grey));

    the_history_carries_every_change(world, &white, &grey);

    world.proxy.forget();
    let blank = running::command(
        world,
        &[
            "register-spool",
            "--name",
            "nobody said why",
            "--material",
            "PLA",
            "--initial-g",
            "1000",
            "--actor",
            "operator",
            "--reason",
            " ",
        ],
    );
    assert_eq!(
        blank.code,
        Some(i32::from(Exit::Usage.status())),
        "{}",
        blank.said()
    );
    assert!(blank.err.contains("--reason"), "{}", blank.said());
    assert_eq!(
        world.proxy.received(),
        Vec::new(),
        "a change with an empty reason reached the server"
    );
}

/// The printer's history, paged and narrowed, carries every change the
/// operator made, each with the operator as its actor and their reason.
fn the_history_carries_every_change(world: &World, white: &str, grey: &str) {
    let whole = running::read(world, &["printer-history", "--limit", "1000"]);
    let whole = whole["events"].as_array().cloned().unwrap_or_default();
    assert_eq!(
        paged(world, "2"),
        whole,
        "paging the history by its cursor did not walk every event once, newest first"
    );
    for (kind, spool) in [
        ("spool_registered", white),
        ("spool_registered", grey),
        ("spool_loaded", white),
        ("spool_loaded", grey),
        ("spool_unloaded", grey),
        ("spool_adjusted", white),
        ("spool_retired", grey),
    ] {
        let named = |event: &Value| {
            event.pointer("/payload/spool_id").and_then(Value::as_str) == Some(spool)
                || event.pointer("/payload/spool/id").and_then(Value::as_str) == Some(spool)
        };
        let found = whole
            .iter()
            .find(|event| event["kind"] == json!(kind) && named(event))
            .unwrap_or_else(|| panic!("the history carries no `{kind}` of {spool}"));
        assert_eq!(
            found.pointer("/payload/reason"),
            Some(&json!(REASON)),
            "{found}"
        );
        assert_eq!(
            found.pointer("/payload/actor"),
            Some(&json!("operator")),
            "{found}"
        );
    }

    let narrowed = running::read(
        world,
        &[
            "printer-history",
            "--kinds",
            "spool_loaded",
            "--kinds",
            "spool_unloaded",
            "--spool-id",
            white,
        ],
    );
    let kinds: Vec<&str> = narrowed["events"]
        .as_array()
        .expect("a page carries events")
        .iter()
        .filter_map(|event| event["kind"].as_str())
        .collect();
    assert_eq!(
        kinds,
        vec!["spool_unloaded", "spool_loaded", "spool_loaded"],
        "the history narrowed to two kinds and one spool answered other events: {narrowed}"
    );
}

/// A start of this world's file through this program, with the check
/// overridden or not.
fn started(world: &World, overridden: bool) -> Ran {
    let file = world.printable_file();
    let manifest = walk::manifest_of("a spool journey", 1.3, &file).to_string();
    let print = world.print_id();
    let mut asked = vec![
        "start-print",
        "--print-id",
        &print,
        "--file-name",
        &file,
        "--manifest",
        &manifest,
    ];
    if overridden {
        asked.extend_from_slice(&["--override-filament-check", "true"]);
    }
    changed(world, &asked)
}

/// A print of this world's file, run on the spool loaded and cancelled before
/// anything could be charged for it: the machine's file has no analysis, so
/// the spool is marked as one whose figure is in doubt.
fn a_print_nothing_could_be_charged_for(world: &World) {
    world.wants(Reports::Operational);
    let ran = started(world, true);
    assert_eq!(
        ran.code,
        Some(0),
        "the overridden start did not start: {}",
        ran.said()
    );
    world.wants(Reports::Printing);
    let cancelled = changed(world, &["cancel", "--print-id", &world.print_id()]);
    assert_eq!(cancelled.code, Some(0), "{}", cancelled.said());
    world.settles_at(Reports::Operational, "cancel");
    // The read that finds the job over closes the print out.
    running::read(world, &["prints"]);
}

/// Three spools — one loaded on tool 0, one whose figure is in doubt, one
/// retired — are listed one line each, ahead of everything else the listing
/// prints; the retired one only when asked for. And a start refused because
/// the spool on its tool is unverified, or because the file has no analysis,
/// prints what to do about each.
pub fn the_listing_and_the_start_say_what_each_spool_holds(world: &World) {
    let doubted = registered(world, "PLA doubted", &[]);
    done(
        world,
        &["load-spool", "--spool-id", &doubted, "--tool", "0"],
    );
    a_print_nothing_could_be_charged_for(world);
    a_print_nothing_could_be_charged_for(world);
    let loaded = registered(world, "PLA loaded", &[]);
    done(world, &["load-spool", "--spool-id", &loaded, "--tool", "0"]);
    let retired = registered(world, "PLA retired", &[]);
    done(world, &["retire-spool", "--spool-id", &retired]);

    let lines = [
        "PLA retired | PLA | 1000.0 g of 1000.0 g left | not loaded | verified | retired",
        "PLA loaded | PLA | 1000.0 g of 1000.0 g left | on tool 0 | verified",
        "PLA doubted | PLA | 1000.0 g of 1000.0 g left | not loaded | unverified: \
         uncharged_print (+1 more)",
    ];
    let every = running::command(world, &["spools", "--include-retired", "true"]);
    assert_eq!(every.code, Some(0), "{}", every.said());
    let printed: Vec<&str> = every.out.lines().collect();
    for (index, line) in lines.iter().enumerate() {
        assert_eq!(
            printed.get(index).copied(),
            Some(format!("at_a_glance.{index}: {line}").as_str()),
            "the listing does not open with one line per spool: {}",
            every.out
        );
    }
    assert!(
        printed
            .get(lines.len())
            .is_some_and(|next| next.starts_with("pending") || next.starts_with("spools.")),
        "the listing carries a fourth line at a glance, or something before its spools: {}",
        every.out
    );
    let machine = running::read(world, &["spools", "--include-retired", "true"]);
    assert_eq!(machine["at_a_glance"], json!(lines), "{machine}");
    let unretired = running::read(world, &["spools"]);
    assert_eq!(unretired["at_a_glance"], json!(lines[1..]), "{unretired}");

    // The doubted spool back on tool 0: a start is refused for its doubt.
    done(
        world,
        &["load-spool", "--spool-id", &doubted, "--tool", "0"],
    );
    world.wants(Reports::Operational);
    let refused = started(world, false);
    assert_eq!(
        refused.code,
        Some(i32::from(Exit::Rejected.status())),
        "{}",
        refused.said()
    );
    for said in [
        "unverified_spool",
        "Spool `PLA doubted` on tool 0 is unverified (uncharged_print)",
        "adjust-spool",
        "--override-filament-check true",
    ] {
        assert!(
            refused.err.contains(said),
            "the note does not say {said:?}: {}",
            refused.err
        );
    }
    assert!(!refused.err.contains("Ask again inside"), "{}", refused.err);
    assert_eq!(world.reported_state(), Some(Reports::Operational));

    // A spool nothing doubts: the start waits for an analysis that never comes.
    done(world, &["load-spool", "--spool-id", &loaded, "--tool", "0"]);
    let refused = started(world, false);
    assert_eq!(
        refused.code,
        Some(i32::from(Exit::Rejected.status())),
        "{}",
        refused.said()
    );
    for said in [
        "analysis_unavailable".to_owned(),
        format!("no analysis of `{}`", world.printable_file()),
        format!("waited {ANALYSIS_WAIT_S} s"),
        "once OctoPrint has analysed the file".to_owned(),
        "--override-filament-check true".to_owned(),
    ] {
        assert!(
            refused.err.contains(&said),
            "the note does not say {said:?}: {}",
            refused.err
        );
    }
    assert_eq!(world.reported_state(), Some(Reports::Operational));
}

/// How long a start in the world these journeys run in waits for an analysis.
pub const ANALYSIS_WAIT_S: u64 = 1;

/// Run one statement on the database the running supervisor holds, over a
/// connection of the store's own, as a second writer would.
fn in_the_database(world: &World, statement: &str) {
    let path = world
        .root
        .path()
        .join("state")
        .join(printobserver_store_sqlite::DATABASE_FILE_NAME);
    printobserver_store_sqlite::connect(&path)
        .expect("the database opens")
        .execute_batch(statement)
        .unwrap_or_else(|error| panic!("`{statement}` was refused: {error}"));
}

/// Run one command the ledger refuses, and answer the tagged cause of its
/// refusal: exited `rejected`, with a `message` beside the cause.
fn cause_of(world: &World, asked: &[&str]) -> Value {
    let mut given = asked.to_vec();
    given.push("--json");
    let ran = changed(world, &given);
    assert_eq!(
        ran.code,
        Some(i32::from(Exit::Rejected.status())),
        "{asked:?} was not refused: {}",
        ran.said()
    );
    let answer: Value = printobserver_types::serde_json::from_str(&ran.out)
        .unwrap_or_else(|error| panic!("{asked:?} answered {error}: {}", ran.out));
    let refused = answer
        .pointer("/decision/rejected/spool_refused")
        .unwrap_or_else(|| panic!("{asked:?} was refused other than by the ledger: {answer}"));
    assert!(
        refused["message"]
            .as_str()
            .is_some_and(|message| !message.is_empty()),
        "{asked:?}'s refusal carries no message: {answer}"
    );
    refused["cause"].clone()
}

/// What an automation that never reads a refusal's message asks next, from
/// the cause alone: the scale reading it could not use given as the figure
/// instead, and a spool nothing registered replaced by the first one listed.
fn next_after(world: &World, cause: &Value, figure_g: &str) -> Vec<String> {
    let named = |field: &str| cause[field].as_str().unwrap_or_default().to_owned();
    match cause["cause"].as_str() {
        Some("tare_required") => [
            "adjust-spool",
            "--spool-id",
            &named("spool_id"),
            "--remaining-g",
            figure_g,
        ]
        .map(str::to_owned)
        .to_vec(),
        Some("unknown_spool") => {
            let first = running::read(world, &["spools"])["spools"][0]["id"]
                .as_str()
                .unwrap_or_default()
                .to_owned();
            ["load-spool", "--spool-id", &first, "--tool", "1"]
                .map(str::to_owned)
                .to_vec()
        }
        other => panic!("no next request is chosen for {other:?}"),
    }
}

/// Every refusal the ledger makes is answered through this program with its
/// tagged cause and the fields that cause names — an unknown or retired spool,
/// nothing to confirm, a tool the printer cannot have, a spool on no tool, a
/// missing tare or density, an impossible figure, a missing or conflicting
/// field, and a write the ledger itself refused — and an automation acting on
/// the cause alone, never the message, makes the next request that succeeds.
pub fn every_spool_refusal_is_answered_with_its_cause(world: &World) {
    let (unknown, loaded, shelved) = lifecycle_refusals(world);
    figure_and_field_refusals(world, &loaded);
    ledger_refusal(world, &loaded);
    acting_on_the_cause_alone(world, &unknown, &shelved);
}

/// The refusals a spool's place in its lifecycle earns: an unknown or retired
/// spool, nothing to confirm, a tool the printer cannot have, a spool on no
/// tool and a missing tare. Answers the unknown id, a spool loaded on tool 0
/// and a shelved spool with no tare, for the refusals that follow.
fn lifecycle_refusals(world: &World) -> (String, String, String) {
    let unknown = printobserver_types::EventId::new().to_string();
    assert_eq!(
        cause_of(
            world,
            &["load-spool", "--spool-id", &unknown, "--tool", "0"]
        ),
        json!({ "cause": "unknown_spool", "spool_id": unknown })
    );
    let retired = registered(world, "PLA used up", &[]);
    done(world, &["retire-spool", "--spool-id", &retired]);
    assert_eq!(
        cause_of(
            world,
            &["load-spool", "--spool-id", &retired, "--tool", "0"]
        ),
        json!({ "cause": "retired_spool", "spool_id": retired })
    );
    let loaded = registered(world, "PLA on tool 0", &["--tare-g", "250"]);
    done(world, &["load-spool", "--spool-id", &loaded, "--tool", "0"]);
    assert_eq!(
        cause_of(world, &["load-spool", "--spool-id", &loaded, "--tool", "0"]),
        json!({ "cause": "nothing_to_confirm", "tool": 0 })
    );
    assert_eq!(
        cause_of(
            world,
            &["load-spool", "--spool-id", &loaded, "--tool", "-1"]
        ),
        json!({ "cause": "unknown_tool", "tool": -1 })
    );
    let shelved = registered(world, "PLA untared", &[]);
    assert_eq!(
        cause_of(world, &["unload-spool", "--spool-id", &shelved]),
        json!({ "cause": "not_loaded", "spool_id": shelved })
    );
    assert_eq!(
        cause_of(
            world,
            &["adjust-spool", "--spool-id", &shelved, "--gross-g", "900"]
        ),
        json!({ "cause": "tare_required", "spool_id": shelved })
    );
    (unknown, loaded, shelved)
}

/// The refusals of what a request says rather than of the spool it names: a
/// missing density, an impossible figure, and a missing or conflicting field.
fn figure_and_field_refusals(world: &World, loaded: &str) {
    let register = |also: &[&str]| {
        let mut asked = vec!["register-spool"];
        asked.extend_from_slice(also);
        cause_of(world, &asked)
    };
    assert_eq!(
        register(&[
            "--name",
            "Exotic",
            "--material",
            "PEEK",
            "--initial-g",
            "500"
        ]),
        json!({ "cause": "density_required", "material": "PEEK" })
    );
    assert_eq!(
        register(&["--name", "Empty", "--material", "PLA", "--initial-g", "0"]),
        json!({ "cause": "invalid_adjustment_figure", "field": "initial_g", "value": 0.0 })
    );
    assert_eq!(
        cause_of(
            world,
            &["adjust-spool", "--spool-id", loaded, "--gross-g", "-10"]
        ),
        json!({ "cause": "invalid_adjustment_figure", "field": "gross_g", "value": -10.0 })
    );
    assert_eq!(
        register(&["--name", " ", "--material", "PLA", "--initial-g", "1000"]),
        json!({ "cause": "missing_field", "field": "name" })
    );
    assert_eq!(
        cause_of(world, &["adjust-spool", "--spool-id", loaded]),
        json!({ "cause": "missing_field", "field": "remaining_g" })
    );
    assert_eq!(
        cause_of(
            world,
            &[
                "adjust-spool",
                "--spool-id",
                loaded,
                "--remaining-g",
                "10",
                "--gross-g",
                "300",
            ]
        ),
        json!({ "cause": "conflicting_fields", "fields": ["remaining_g", "gross_g"] })
    );
}

/// A write the ledger itself refused is answered as that, naming no storage
/// constraint, and leaves the spool where it was.
fn ledger_refusal(world: &World, loaded: &str) {
    // The ledger refusing a write is the database refusing it: a rule put on
    // the spools table from outside, as a second writer racing this one would
    // leave the table, is one the store meets mid-write.
    in_the_database(
        world,
        "CREATE TRIGGER a_journey_refuses_every_spool_write BEFORE UPDATE ON spools \
         BEGIN SELECT RAISE(ABORT, 'spools.loaded_on_tool'); END",
    );
    let raced = cause_of(world, &["unload-spool", "--spool-id", loaded]);
    in_the_database(world, "DROP TRIGGER a_journey_refuses_every_spool_write");
    assert_eq!(raced, json!({ "cause": "ledger_refused" }));
    assert_eq!(
        running::read(world, &["spool", "--spool-id", loaded])["spool"]["lifecycle"],
        json!({ "state": "loaded", "tool": 0 }),
        "a refused write moved the spool"
    );
}

/// An automation reading only the cause makes the next request that succeeds.
fn acting_on_the_cause_alone(world: &World, unknown: &str, shelved: &str) {
    for (asked, figure) in [
        (
            vec!["adjust-spool", "--spool-id", shelved, "--gross-g", "900"],
            "650",
        ),
        (vec!["load-spool", "--spool-id", unknown, "--tool", "1"], ""),
    ] {
        let cause = cause_of(world, &asked);
        let next = next_after(world, &cause, figure);
        let next: Vec<&str> = next.iter().map(String::as_str).collect();
        done(world, &next);
    }
    assert_eq!(
        running::read(world, &["spool", "--spool-id", shelved])["spool"]["remaining_g"],
        json!(650.0),
        "the automation's next request did not correct the spool"
    );
}
