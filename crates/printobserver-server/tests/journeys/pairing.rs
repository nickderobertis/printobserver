//! A start is refused when its file was sliced for another material than the
//! spool loaded, or when its build sheet does not suit that material — through
//! the real `start-print` route, whoever asks.
//!
//! The printer is the scripted `OctoPrint`, holding real `PrusaSlicer` G-code
//! from `tests/real-prints/` whose settings block names its `filament_type`;
//! the server reads that from the file's tail through the real adapter. Spools
//! are registered and loaded through the real spool routes. What is asserted
//! is what a client and the machine observe: the answer and its rejection,
//! whether `OctoPrint` was ever told to start, and the `preflight_checked` and
//! `print_started` the print's history holds. The agent's requests carry the
//! credential a held supervision turn was issued, as the agent's own do — and
//! a start, overrides and all, is the operator's alone, so the route refuses
//! the agent's before anything is decided.

use printobserver_core::store::PrintStore as _;
use printobserver_octoprint::standin::JobScript;
use printobserver_types::serde_json::{Value, json};

use crate::binding::agent;
use crate::watching::Watch;
use crate::world::{SECRET, failure_alert, presenting, set};

/// A recorded `PrusaSlicer` print, sliced for PLA.
const RECORDED: &[u8] =
    include_bytes!("../../../../tests/real-prints/fan-cut-bridge/fan-cut-test.gcode");

/// The setting the recording names its material by.
const SLICED_FOR_PLA: &str = "; filament_type = PLA\n";

/// The file sliced for PLA, for PETG, and naming no material.
const PLA_FILE: &str = "bridge-pla.gcode";
/// The same file, sliced for PETG.
const PETG_FILE: &str = "bridge-petg.gcode";
/// The same file, its `filament_type` line gone.
const UNNAMED_FILE: &str = "bridge-unnamed.gcode";

/// The provider identifier of the print a held turn is about.
const OBICO_PRINT: i64 = 4211;

/// The recording with its `filament_type` line replaced.
fn recorded_as(line: &str) -> Vec<u8> {
    let text = String::from_utf8_lossy(RECORDED);
    assert!(text.contains(SLICED_FOR_PLA), "the recording names PLA");
    text.replacen(SLICED_FOR_PLA, line, 1).into_bytes()
}

/// A server watching a scripted `OctoPrint` that holds the three files, its
/// configuration edited by a journey.
async fn watching(edit: impl FnOnce(&mut toml::Value)) -> Watch {
    let watch = Watch::start(edit, |config| {
        config.printer_poll = core::time::Duration::from_millis(200);
    })
    .await;
    watch
        .octoprint
        .holds_file(PLA_FILE, RECORDED.to_vec(), 1_000.0);
    watch
        .octoprint
        .holds_file(PETG_FILE, recorded_as("; filament_type = PETG\n"), 1_000.0);
    watch
        .octoprint
        .holds_file(UNNAMED_FILE, recorded_as(""), 1_000.0);
    watch
}

/// One URL of an operation, with its path's identifier filled in.
fn url_of(watch: &Watch, operation: &str, id: Option<(&str, &str)>) -> String {
    let mut path = printobserver_server::operation(operation)
        .expect("the operation is declared")
        .full_path();
    if let Some((placeholder, value)) = id {
        path = path.replace(placeholder, value);
    }
    format!("{}{path}", watch.server.base_url().trim_end_matches("/v1"))
}

/// Post one body, answering the status and the answer.
async fn posted(client: &reqwest::Client, url: &str, body: &Value) -> (reqwest::StatusCode, Value) {
    let response = client
        .post(url)
        .json(body)
        .send()
        .await
        .expect("the server answers");
    let status = response.status();
    (status, response.json().await.expect("a JSON answer"))
}

/// Register a full roll of one material and load it onto tool 0.
async fn loaded_with(watch: &Watch, material: &str) {
    let (status, registered) = posted(
        &watch.client,
        &url_of(watch, "register_spool", None),
        &json!({
            "name": format!("{material} roll"),
            "material": material,
            "initial_g": 1000.0,
            "reason": "a full roll goes on the printer",
            "actor": "operator",
        }),
    )
    .await;
    assert_eq!(status, reqwest::StatusCode::OK, "{registered}");
    let spool = registered["spool"]["id"]
        .as_str()
        .expect("the spool's identifier")
        .to_owned();
    let (status, loaded) = posted(
        &watch.client,
        &url_of(watch, "load_spool", Some(("{spool_id}", &spool))),
        &json!({ "tool": 0, "reason": "onto the only tool", "actor": "operator" }),
    )
    .await;
    assert_eq!(status, reqwest::StatusCode::OK, "{loaded}");
}

/// The manifest a start carries, declaring a sheet or none.
fn manifest(file: &str, sheet: Option<&str>) -> Value {
    let mut manifest = json!({
        "file_name": file,
        "material": "PLA",
        "nozzle_diameter_mm": 0.4,
        "slicer_profile": "0.20mm SPEED",
        "allowed": {},
        "metadata": {},
    });
    if let Some(sheet) = sheet {
        manifest["sheet"] = json!(sheet);
    }
    manifest
}

/// A start of one file on one sheet, by one actor, asking these overrides.
fn start(actor: &Value, file: &str, sheet: Option<&str>, overrides: &[&str]) -> Value {
    let mut body = json!({
        "file_name": file,
        "manifest": manifest(file, sheet),
        "reason": "the next print in the queue",
        "actor": actor,
    });
    for name in overrides {
        body[*name] = json!(true);
    }
    body
}

/// Ask the idle scripted printer to start, as the operator, against a print
/// identifier nothing holds — a start opens its own print.
async fn started_by_operator(watch: &Watch, body: &Value) -> (reqwest::StatusCode, Value) {
    watch.octoprint.reports("Operational", JobScript::idle());
    let print = printobserver_types::PrintId::new().to_string();
    posted(
        &watch.client,
        &url_of(watch, "start_print", Some(("{print_id}", &print))),
        body,
    )
    .await
}

/// How many times `OctoPrint` was told to start a job.
fn starts(watch: &Watch) -> usize {
    watch
        .octoprint
        .job_commands()
        .iter()
        .filter(|command| *command == "start")
        .count()
}

/// The rejection an answer carries.
fn rejection(answer: &Value) -> &Value {
    &answer["record"]["decision"]["rejected"]
}

/// One kind of event in one print's history.
async fn events_of(watch: &Watch, print_id: &str, kind: &str) -> Vec<Value> {
    watch
        .get(&format!("/prints/{print_id}/history?limit=500"))
        .await["events"]
        .as_array()
        .cloned()
        .unwrap_or_default()
        .into_iter()
        .filter(|event| event["kind"] == kind)
        .collect()
}

/// A turn held open about a print the scripted printer is running, answering
/// that print's identifier and a client presenting the turn's credential; the
/// printer then goes idle, so a start is valid from its state.
async fn held_turn(watch: &Watch) -> (String, reqwest::Client) {
    watch.octoprint.reports(
        "Printing",
        JobScript::printing(PLA_FILE, 450_468, 4_000, 60),
    );
    let print = watch
        .prints
        .open_print(Some(OBICO_PRINT), Some(PLA_FILE.to_owned()))
        .await
        .expect("the store opens a print");
    watch.agent.hold_turns();
    let status = reqwest::Client::new()
        .post(format!("{}?token={SECRET}", watch.server.ingress_url()))
        .header(reqwest::header::CONTENT_TYPE, "application/json")
        .body(failure_alert(OBICO_PRINT, "http://127.0.0.1:1/frame.jpg").to_string())
        .send()
        .await
        .expect("the ingress answers")
        .status();
    assert_eq!(status, reqwest::StatusCode::ACCEPTED);
    let pass = watch
        .agent
        .issued(1)
        .await
        .into_iter()
        .next()
        .expect("one pass");
    watch.octoprint.reports("Operational", JobScript::idle());
    (print.id.to_string(), presenting(&pass.credential))
}

/// The agent's start on the print its turn is about.
async fn started_by_agent(
    watch: &Watch,
    print: &str,
    turn: &reqwest::Client,
    file: &str,
    sheet: &str,
    overrides: &[&str],
) -> (reqwest::StatusCode, Value) {
    let actor = agent(&crate::agent::session_of(
        print.parse().expect("a print identifier"),
    ));
    posted(
        turn,
        &url_of(watch, "start_print", Some(("{print_id}", print))),
        &start(&actor, file, Some(sheet), overrides),
    )
    .await
}

/// An agent's start, overrides and all, refused by the route as the
/// operator's alone.
fn assert_forbidden_start(answer: &(reqwest::StatusCode, Value)) {
    assert_eq!(answer.0, reqwest::StatusCode::FORBIDDEN, "{}", answer.1);
    let said = answer.1["error"].as_str().unwrap_or_default();
    assert!(
        said.contains("start-print") && said.contains("operator's alone"),
        "{said}"
    );
}

/// A PLA file on a loaded PETG spool is refused naming both materials, and so
/// is the agent's override of it; the operator's override starts it and is
/// recorded as overridden; and a file naming no material is refused.
#[tokio::test(flavor = "multi_thread")]
async fn a_start_sliced_for_another_material_is_refused_unless_the_operator_overrides() {
    let watch = watching(|_| {}).await;
    loaded_with(&watch, "PETG").await;
    let operator = json!("operator");

    let (status, refused) = started_by_operator(
        &watch,
        &start(&operator, PLA_FILE, Some("textured-pei"), &[]),
    )
    .await;
    assert_eq!(status, reqwest::StatusCode::CONFLICT, "{refused}");
    let objection = &rejection(&refused)["material_refused"]["objection"];
    assert_eq!(objection["cause"], "mismatch", "{refused}");
    assert_eq!(
        (&objection["sliced_for"], &objection["loaded"]),
        (&json!("PLA"), &json!("PETG")),
        "{refused}"
    );
    assert_eq!(starts(&watch), 0, "OctoPrint was told to start");

    let (print, turn) = held_turn(&watch).await;
    let (status, by_agent) = started_by_agent(
        &watch,
        &print,
        &turn,
        PLA_FILE,
        "textured-pei",
        &["override_material_check"],
    )
    .await;
    watch.agent.release_turns();
    assert_forbidden_start(&(status, by_agent));
    assert_eq!(starts(&watch), 0, "the agent's override started a print");

    let (status, overridden) = started_by_operator(
        &watch,
        &start(
            &operator,
            PLA_FILE,
            Some("textured-pei"),
            &["override_material_check"],
        ),
    )
    .await;
    assert_eq!(status, reqwest::StatusCode::OK, "{overridden}");
    assert_eq!(starts(&watch), 1);
    let started = overridden["record"]["print_id"]
        .as_str()
        .expect("the start's print")
        .to_owned();
    for kind in ["preflight_checked", "print_started"] {
        let recorded = events_of(&watch, &started, kind).await;
        let material = &recorded
            .last()
            .unwrap_or_else(|| panic!("no {kind} on the started print"))["payload"]["preflight"]["material"];
        assert_eq!(material["verdict"], "overridden", "{kind}: {material}");
        assert_eq!(material["objection"]["sliced_for"], "PLA", "{kind}");
    }

    let (status, unnamed) = started_by_operator(
        &watch,
        &start(&operator, UNNAMED_FILE, Some("textured-pei"), &[]),
    )
    .await;
    assert_eq!(status, reqwest::StatusCode::CONFLICT, "{unnamed}");
    let objection = &rejection(&unnamed)["material_refused"]["objection"];
    assert_eq!(objection["cause"], "undeclared", "{unnamed}");
    assert_eq!(objection["file_name"], UNNAMED_FILE, "{unnamed}");
    assert_eq!(starts(&watch), 1);
    watch.server.stop().await;
}

/// A manifest declaring no sheet is refused naming `sheet`, even with both
/// overrides; each default pairing passes or is refused as the table says; and
/// a refused pairing passes on the operator's override, recorded as such, and
/// not on the agent's.
#[tokio::test(flavor = "multi_thread")]
async fn the_sheet_a_start_declares_must_suit_the_loaded_material() {
    let operator = json!("operator");
    for (material, file, refused_on, passes_on) in [
        ("PLA", PLA_FILE, "textured-pei", "smooth-pei"),
        ("PETG", PETG_FILE, "smooth-pei", "textured-pei"),
    ] {
        let watch = watching(|_| {}).await;
        loaded_with(&watch, material).await;

        let both = ["override_material_check", "override_sheet_check"];
        let (status, sheetless) =
            started_by_operator(&watch, &start(&operator, file, None, &both)).await;
        assert_eq!(status, reqwest::StatusCode::CONFLICT, "{sheetless}");
        assert_eq!(
            rejection(&sheetless),
            &json!("sheet_not_declared"),
            "{sheetless}"
        );
        assert_eq!(starts(&watch), 0);

        let (status, refused) =
            started_by_operator(&watch, &start(&operator, file, Some(refused_on), &[])).await;
        assert_eq!(status, reqwest::StatusCode::CONFLICT, "{refused}");
        let objection = &rejection(&refused)["sheet_refused"]["objection"];
        assert_eq!(objection["cause"], "not_allowed", "{refused}");
        assert_eq!(
            (&objection["sheet"], &objection["material"]),
            (&json!(refused_on), &json!(material)),
            "{refused}"
        );
        assert_eq!(objection["allowed"], json!([passes_on]), "{refused}");
        assert_eq!(starts(&watch), 0);

        let (print, turn) = held_turn(&watch).await;
        let (status, by_agent) = started_by_agent(
            &watch,
            &print,
            &turn,
            file,
            refused_on,
            &["override_sheet_check"],
        )
        .await;
        watch.agent.release_turns();
        assert_forbidden_start(&(status, by_agent));
        assert_eq!(starts(&watch), 0, "the agent's override started a print");

        let (status, passed) =
            started_by_operator(&watch, &start(&operator, file, Some(passes_on), &[])).await;
        assert_eq!(status, reqwest::StatusCode::OK, "{passed}");
        assert_eq!(starts(&watch), 1);
        let started = passed["record"]["print_id"].as_str().expect("a print");
        let checked = events_of(&watch, started, "print_started").await;
        let sheet = &checked.last().expect("a print_started")["payload"]["preflight"]["sheet"];
        assert_eq!(sheet["verdict"], "passed", "{sheet}");
        assert_eq!(sheet["sheet"], passes_on, "{sheet}");

        let (status, overridden) = started_by_operator(
            &watch,
            &start(&operator, file, Some(refused_on), &["override_sheet_check"]),
        )
        .await;
        assert_eq!(status, reqwest::StatusCode::OK, "{overridden}");
        assert_eq!(starts(&watch), 2);
        let started = overridden["record"]["print_id"].as_str().expect("a print");
        let checked = events_of(&watch, started, "print_started").await;
        let sheet = &checked.last().expect("a print_started")["payload"]["preflight"]["sheet"];
        assert_eq!(sheet["verdict"], "overridden", "{sheet}");
        assert_eq!(sheet["objection"]["sheet"], refused_on, "{sheet}");
        watch.server.stop().await;
    }
}

/// A configured table replaces the defaults: a pairing it allows passes, and
/// a material it has no entry for is refused — and passes on the operator's
/// override.
#[tokio::test(flavor = "multi_thread")]
async fn a_configured_sheet_table_replaces_the_defaults() {
    let operator = json!("operator");
    let table = |document: &mut toml::Value| {
        set(
            document,
            "sheets",
            toml::Value::Table(toml::Table::from_iter([(
                "allowed".to_owned(),
                toml::Value::Table(toml::Table::from_iter([(
                    "PETG".to_owned(),
                    toml::Value::Array(vec![toml::Value::from("smooth-pei")]),
                )])),
            )])),
        );
    };

    let petg = watching(table).await;
    loaded_with(&petg, "PETG").await;
    let (status, passed) =
        started_by_operator(&petg, &start(&operator, PETG_FILE, Some("smooth-pei"), &[])).await;
    assert_eq!(status, reqwest::StatusCode::OK, "{passed}");
    petg.server.stop().await;

    let pla = watching(table).await;
    loaded_with(&pla, "PLA").await;
    let (status, refused) =
        started_by_operator(&pla, &start(&operator, PLA_FILE, Some("smooth-pei"), &[])).await;
    assert_eq!(status, reqwest::StatusCode::CONFLICT, "{refused}");
    let objection = &rejection(&refused)["sheet_refused"]["objection"];
    assert_eq!(objection["cause"], "unlisted_material", "{refused}");
    assert_eq!(objection["material"], "PLA", "{refused}");
    let (status, overridden) = started_by_operator(
        &pla,
        &start(
            &operator,
            PLA_FILE,
            Some("smooth-pei"),
            &["override_sheet_check"],
        ),
    )
    .await;
    assert_eq!(status, reqwest::StatusCode::OK, "{overridden}");
    assert_eq!(starts(&pla), 1);
    pla.server.stop().await;
}

/// The read-only check reports the material result, and the sheet result
/// only when it is given a sheet; it records nothing and starts nothing.
#[tokio::test(flavor = "multi_thread")]
async fn the_read_only_check_reports_the_material_and_a_given_sheet() {
    let watch = watching(|_| {}).await;
    loaded_with(&watch, "PETG").await;

    let unsheeted = watch.get(&format!("/preflight?file_name={PLA_FILE}")).await;
    let material = &unsheeted["preflight"]["material"];
    assert_eq!(material["verdict"], "refused", "{unsheeted}");
    assert_eq!(material["objection"]["sliced_for"], "PLA", "{unsheeted}");
    assert!(unsheeted["preflight"].get("sheet").is_none(), "{unsheeted}");

    let sheeted = watch
        .get(&format!(
            "/preflight?file_name={PETG_FILE}&sheet=textured-pei"
        ))
        .await;
    assert_eq!(
        sheeted["preflight"]["material"]["verdict"], "passed",
        "{sheeted}"
    );
    assert_eq!(
        sheeted["preflight"]["sheet"]["verdict"], "passed",
        "{sheeted}"
    );
    assert_eq!(starts(&watch), 0);
    watch.server.stop().await;
}
