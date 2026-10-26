//! The spool ledger's records say one thing at a time, over the real store.
//!
//! Every journey composes the real server over the real `SqliteStore`, drives
//! it over HTTP as an operator and the vision ingress do, and reads what it
//! wrote back through `spool`, `spools`, `preflight` and `printer_history`:
//! a retired spool is on no tool, a filament change's position carries its
//! own freshness or is absent, a consumption carries its whole calculation or
//! none of it, every tool of a filament check carries one result with that
//! verdict's evidence, and a change that writes no event writes none.

use printobserver_core::ActionKind;
use printobserver_printer_api::{PrinterState, ToolFilament};
use printobserver_types::Timestamp;
use printobserver_types::serde_json::{Value, json};

use crate::ledger_restarts::{
    FILE, FULL_G, SIZE_BYTES, WHOLE_MM, debited, filament_change, finishes, grams, look_at_the_job,
    of_kind, printing_world, spool_of, the_whole_history,
};
use crate::spooling::{close, loaded, plain, registered};
use crate::world::{SECRET, World, set, spool_body};

/// The kinds of the events one command's answer carries, in order.
fn kinds_answered(answer: &Value) -> Vec<String> {
    answer["events"]
        .as_array()
        .expect("a command answers its events")
        .iter()
        .map(|event| event["kind"].as_str().unwrap_or_default().to_owned())
        .collect()
}

/// Post one `Obico` filament-change notification, and wait until the printer's
/// history carries `signals` of them: a completion seen for an earlier
/// notification is not this one's.
async fn signal_a_filament_change(world: &World, signals: usize) {
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
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
    loop {
        let history = the_whole_history(world).await;
        if of_kind(&history, "filament_change_signalled").len() >= signals {
            return;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "the history never carried {signals} filament changes: {history:#?}"
        );
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    }
}

/// The keys one JSON object carries, sorted.
fn keys(object: &Value) -> Vec<String> {
    let mut found: Vec<String> = object
        .as_object()
        .map(|fields| fields.keys().cloned().collect())
        .unwrap_or_default();
    found.sort();
    found
}

/// Retiring a spool loaded mid-print charges it for what it fed up to then,
/// takes it off its tool and retires it, in one answer; read back, it is
/// retired and on no tool. What the tool feeds after it is recorded with no
/// charged spool at all, while the retired spool's own charge carries its
/// whole calculation.
#[tokio::test(flavor = "multi_thread")]
async fn retiring_a_loaded_spool_leaves_it_retired_and_on_no_tool() {
    let world = printing_world().await;
    let spool = registered(&world, "PLA nearly empty", FULL_G).await;
    loaded(&world, &spool, 0).await;
    world.printer.at_byte(1_024);
    look_at_the_job(&world).await;
    world.printer.at_byte(SIZE_BYTES / 2);

    let (status, retired) = world
        .post(
            &world.spool_url("retire_spool", &spool),
            &spool_body(ActionKind::RetireSpool),
        )
        .await;
    assert_eq!(status, reqwest::StatusCode::OK, "{retired}");
    assert_eq!(
        kinds_answered(&retired),
        ["filament_consumed", "spool_unloaded", "spool_retired"],
        "{retired}"
    );
    let read = spool_of(&world, &spool).await;
    assert_eq!(
        read["spool"]["lifecycle"]["state"],
        json!("retired"),
        "{read}"
    );
    assert_eq!(
        keys(&read["spool"]["lifecycle"]),
        ["at", "state"],
        "a retired spool still names a tool: {read}"
    );
    let (_, listing) = world.get(&plain(&world, "spools")).await;
    assert!(
        listing["spools"]
            .as_array()
            .is_some_and(|spools| spools.iter().all(|listed| listed["id"] != json!(spool))),
        "a retired spool is listed as live: {listing}"
    );

    finishes(&world);
    look_at_the_job(&world).await;
    let events = the_whole_history(&world).await;
    let consumed = of_kind(&events, "filament_consumed");
    assert_eq!(consumed.len(), 2, "{events:#?}");
    let charged = &consumed[0]["payload"]["charged"];
    assert_eq!(
        keys(charged),
        [
            "density_g_cm3",
            "diameter_mm",
            "grams",
            "remaining_after_g",
            "remaining_before_g",
            "spool_id",
        ],
        "a charged consumption does not carry its whole calculation: {}",
        consumed[0]
    );
    assert_eq!(charged["spool_id"], json!(spool));
    assert!(close(&charged["grams"], grams(WHOLE_MM / 2.0)), "{charged}");
    assert!(
        close(
            &charged["remaining_after_g"],
            FULL_G - grams(WHOLE_MM / 2.0)
        ),
        "{charged}"
    );
    let uncharged = &consumed[1]["payload"];
    for absent in [
        "charged",
        "spool_id",
        "grams",
        "diameter_mm",
        "density_g_cm3",
        "remaining_before_g",
        "remaining_after_g",
    ] {
        assert!(
            uncharged.get(absent).is_none(),
            "a consumption with no spool carries `{absent}`: {uncharged}"
        );
    }
    assert_eq!(debited(&events, &spool).len(), 1);
    world.server.stop().await;
}

/// A filament change signalled with a fresh position records that position
/// as `read_at_signal`; one signalled with none records no position and no
/// claim about one. A second signal of the same change marks nothing again,
/// and confirming the spool in place clears its mark with no event of the
/// clearing's own.
#[tokio::test(flavor = "multi_thread")]
async fn a_filament_change_records_its_position_whole_or_not_at_all() {
    let world = printing_world().await;
    let spool = registered(&world, "PLA white", FULL_G).await;
    loaded(&world, &spool, 0).await;
    world.printer.at_byte(SIZE_BYTES / 4);
    look_at_the_job(&world).await;

    signal_a_filament_change(&world, 1).await;
    signal_a_filament_change(&world, 2).await;
    let events = the_whole_history(&world).await;
    let signalled = of_kind(&events, "filament_change_signalled");
    assert_eq!(signalled.len(), 2, "{events:#?}");
    let first = &signalled[0]["payload"];
    assert!(
        first.get("position_read_at_signal").is_none(),
        "the freshness is still a field of its own: {first}"
    );
    assert_eq!(
        first["position"]["freshness"],
        json!("read_at_signal"),
        "{first}"
    );
    assert_eq!(
        first["position"]["position"]["bytes"],
        json!(SIZE_BYTES / 4),
        "{first}"
    );
    assert_eq!(
        of_kind(&events, "spool_unverified").len(),
        1,
        "a second signal of one change marked the spool again: {events:#?}"
    );

    let before = the_whole_history(&world).await.len();
    let confirmed = loaded(&world, &spool, 0).await;
    let answered = kinds_answered(&confirmed);
    assert_eq!(
        answered.last().map(String::as_str),
        Some("spool_loaded"),
        "{confirmed}"
    );
    assert!(
        answered
            .iter()
            .all(|kind| kind == "spool_loaded" || kind == "filament_consumed"),
        "clearing the mark wrote an event of its own: {confirmed}"
    );
    let spool_loaded = confirmed["events"]
        .as_array()
        .and_then(|written| written.last())
        .cloned()
        .unwrap_or_default();
    assert_eq!(
        spool_loaded["payload"]["cleared_marks"]
            .as_array()
            .map(Vec::len),
        Some(1),
        "the load does not name the mark it cleared: {confirmed}"
    );
    assert_eq!(
        the_whole_history(&world).await.len(),
        before + answered.len(),
        "the confirmation wrote events it did not answer"
    );
    assert_eq!(
        spool_of(&world, &spool).await["spool"]["trust"],
        json!({ "state": "verified" })
    );
    world.server.stop().await;

    let world = printing_world().await;
    let spool = registered(&world, "PLA grey", FULL_G).await;
    loaded(&world, &spool, 0).await;
    look_at_the_job(&world).await;
    signal_a_filament_change(&world, 1).await;
    let events = the_whole_history(&world).await;
    let signalled = of_kind(&events, "filament_change_signalled");
    assert_eq!(signalled.len(), 1, "{events:#?}");
    assert_eq!(
        keys(&signalled[0]["payload"]),
        ["source_kind", "spool_id", "tool"],
        "a change with no position carries a position or a claim about one: {}",
        signalled[0]
    );
    let read = spool_of(&world, &spool).await;
    assert_eq!(
        read["spool"]["trust"]["marks"][0]["reason"]["reason"],
        json!("filament_change_unconfirmed_position"),
        "{read}"
    );
    world.server.stop().await;
}

/// A filament change signalled while the job reports no position, after one
/// was recorded, records that recorded position as `last_recorded`,
/// and marks the spool as one whose figure may be overstated.
#[tokio::test(flavor = "multi_thread")]
async fn a_filament_change_with_only_a_recorded_position_says_it_is_not_fresh() {
    let world = printing_world().await;
    let spool = registered(&world, "PLA black", FULL_G).await;
    loaded(&world, &spool, 0).await;
    world.printer.at_byte(SIZE_BYTES / 4);
    look_at_the_job(&world).await;
    world.printer.reports_no_position();
    signal_a_filament_change(&world, 1).await;

    let events = the_whole_history(&world).await;
    let signalled = of_kind(&events, "filament_change_signalled");
    assert_eq!(
        signalled[0]["payload"]["position"]["freshness"],
        json!("last_recorded"),
        "{}",
        signalled[0]
    );
    assert_eq!(
        signalled[0]["payload"]["position"]["position"]["bytes"],
        json!(SIZE_BYTES / 4),
        "{}",
        signalled[0]
    );
    let read = spool_of(&world, &spool).await;
    assert_eq!(
        read["spool"]["trust"]["marks"][0]["reason"]["reason"],
        json!("filament_change_unconfirmed_position"),
        "{read}"
    );
    world.server.stop().await;
}

/// In mode `warn`, a start onto a spool too short for the file goes ahead,
/// and the check it records carries the tool's result as `warned` with the
/// objection it let through and that objection's evidence.
#[tokio::test(flavor = "multi_thread")]
async fn a_warned_start_records_the_objection_it_let_through() {
    let world = World::configured(
        crate::printer::RecordingPrinter::printing(),
        crate::agent::StandInAgent::new(),
        |document| {
            set(
                document,
                "spools",
                toml::Value::Table(toml::Table::from_iter([
                    (
                        "preflight".to_owned(),
                        toml::Value::String("warn".to_owned()),
                    ),
                    ("analysis_wait_seconds".to_owned(), toml::Value::Integer(0)),
                ])),
            );
        },
    )
    .await;
    world.printer.holds_file(
        FILE,
        SIZE_BYTES,
        Some(vec![ToolFilament {
            tool: 0,
            length_mm: WHOLE_MM,
            volume_cm3: None,
        }]),
    );
    let short = registered(&world, "PLA nearly out", 5.0).await;
    loaded(&world, &short, 0).await;
    let asked_against = world.open_print().await;
    world.printer.in_state(PrinterState::Operational);

    let manifest = printobserver_types::serde_json::to_value(
        <printobserver_core::JobManifest as printobserver_types::contract::Sample>::sample_full(),
    )
    .expect("a manifest renders");
    let start = json!({
        "reason": "a journey is asking",
        "actor": "operator",
        "file_name": FILE,
        "manifest": manifest,
    });
    let start_path = printobserver_server::operation("start_print")
        .expect("`start_print` is served")
        .full_path();
    let (status, answer) = world
        .post(&world.operation_url(&start_path, asked_against), &start)
        .await;
    assert_eq!(status, reqwest::StatusCode::OK, "{answer}");
    assert_eq!(answer["record"]["decision"], json!("accepted"), "{answer}");

    let events = the_whole_history(&world).await;
    let checked = of_kind(&events, "preflight_checked");
    let preflight = &checked.last().expect("the start's check is recorded")["payload"]["preflight"];
    assert_eq!(preflight["verdict"], json!("warned"), "{preflight}");
    let result = &preflight["tools"][0]["result"];
    assert_eq!(result["verdict"], json!("warned"), "{result}");
    let objection = &result["objection"];
    assert_eq!(objection["verdict"], json!("insufficient"), "{objection}");
    assert_eq!(
        objection["spool"],
        json!({ "id": short, "name": "PLA nearly out" })
    );
    assert!(close(&objection["remaining_g"], 5.0), "{objection}");
    assert!(
        close(&objection["need"]["required_g"], grams(WHOLE_MM)),
        "{objection}"
    );
    world.server.stop().await;
}

/// A filament check of a file feeding three tools — one holding enough, one
/// holding too little, one holding nothing tracked — answers one result per
/// tool, tagged by its verdict and carrying exactly that verdict's evidence;
/// and a file with no analysis answers that, naming the spool.
#[tokio::test(flavor = "multi_thread")]
async fn every_tool_of_a_filament_check_carries_one_result_with_its_evidence() {
    let world = World::open().await;
    world.printer.holds_file(
        FILE,
        SIZE_BYTES,
        Some(vec![
            ToolFilament {
                tool: 0,
                length_mm: WHOLE_MM,
                volume_cm3: None,
            },
            ToolFilament {
                tool: 1,
                length_mm: WHOLE_MM,
                volume_cm3: None,
            },
            ToolFilament {
                tool: 2,
                length_mm: WHOLE_MM,
                volume_cm3: None,
            },
        ]),
    );
    let enough = registered(&world, "PLA enough", FULL_G).await;
    let short = registered(&world, "PLA short", 5.0).await;
    loaded(&world, &enough, 0).await;
    loaded(&world, &short, 1).await;

    let check = |world: &World| format!("{}?file_name={FILE}", plain(world, "preflight"));
    let (status, checked) = world.get(&check(&world)).await;
    assert_eq!(status, reqwest::StatusCode::OK, "{checked}");
    let tools = checked["preflight"]["tools"]
        .as_array()
        .cloned()
        .unwrap_or_default();
    assert_eq!(tools.len(), 3, "{checked}");
    for tool in &tools {
        assert_eq!(keys(tool), ["result", "tool"], "{tool}");
    }
    let passed = &tools[0]["result"];
    assert_eq!(passed["verdict"], json!("passed"), "{checked}");
    assert_eq!(
        keys(passed),
        ["need", "remaining_g", "spool", "verdict"],
        "{passed}"
    );
    assert_eq!(
        passed["spool"],
        json!({ "id": enough, "name": "PLA enough" })
    );
    assert!(
        close(&passed["need"]["required_g"], grams(WHOLE_MM)),
        "{passed}"
    );
    let insufficient = &tools[1]["result"];
    assert_eq!(insufficient["verdict"], json!("insufficient"), "{checked}");
    assert_eq!(
        keys(insufficient),
        ["need", "remaining_g", "spool", "verdict"],
        "{insufficient}"
    );
    assert!(close(&insufficient["remaining_g"], 5.0), "{insufficient}");
    assert_eq!(
        tools[2]["result"],
        json!({ "verdict": "unchecked" }),
        "{checked}"
    );
    assert_eq!(checked["preflight"]["verdict"], json!("insufficient"));

    world.printer.holds_file(FILE, SIZE_BYTES, None);
    let (_, unanalysed) = world.get(&check(&world)).await;
    let results: Vec<Value> = unanalysed["preflight"]["tools"]
        .as_array()
        .map(|tools| tools.iter().map(|tool| tool["result"].clone()).collect())
        .unwrap_or_default();
    assert_eq!(results.len(), 2, "{unanalysed}");
    for result in &results {
        assert_eq!(result["verdict"], json!("no_analysis"), "{unanalysed}");
        assert_eq!(
            keys(result),
            ["remaining_g", "spool", "verdict"],
            "{result}"
        );
    }
    world.server.stop().await;
}
