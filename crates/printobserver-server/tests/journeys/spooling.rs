//! The spool ledger on the one command surface: its four reads and its five
//! commands, as an operator and a supervision turn each reach them.
//!
//! Every journey drives the real router over HTTP and reads the outcome back
//! through the same surface — the spools, one spool, the printer's own history
//! and a filament check — so what is asserted is what a caller would see,
//! rather than what a store holds.

use printobserver_core::{ActionKind, SpoolPolicy, buffer_g, density_for, grams_for_length};
use printobserver_printer_api::ToolFilament;
use printobserver_types::serde_json::{Value, json};

use crate::binding::{agent, held_turn, sent};
use crate::world::{World, presenting, set, spool_body};

/// The four reads of the ledger, which these journeys drive.
pub const DRIVEN_THERE: [&str; 4] = ["spools", "spool", "printer_history", "preflight"];

/// The five spool commands, by the name the wire spells each.
const COMMANDS: [(&str, ActionKind); 5] = [
    ("register_spool", ActionKind::RegisterSpool),
    ("load_spool", ActionKind::LoadSpool),
    ("unload_spool", ActionKind::UnloadSpool),
    ("adjust_spool", ActionKind::AdjustSpool),
    ("retire_spool", ActionKind::RetireSpool),
];

/// The file the machine holds, and what its analysis says tool 0 feeds.
const FILE: &str = "benchy.gcode";

/// How much filament the file's analysis says tool 0 feeds, in millimetres.
const ANALYSED_MM: f64 = 4_000.0;

/// One operation's URL, with no value in its path.
pub fn plain(world: &World, operation: &str) -> String {
    world.at(&printobserver_server::operation(operation)
        .expect("the operation is declared")
        .full_path())
}

/// Register one spool as the operator, and answer its identifier.
pub async fn registered(world: &World, name: &str, initial_g: f64) -> String {
    let mut body = spool_body(ActionKind::RegisterSpool);
    body["name"] = json!(name);
    body["initial_g"] = json!(initial_g);
    let (status, answer) = world.post(&plain(world, "register_spool"), &body).await;
    assert_eq!(status, reqwest::StatusCode::OK, "{answer}");
    answer["spool"]["id"]
        .as_str()
        .expect("a registration answers the spool")
        .to_owned()
}

/// Load one spool onto one tool as the operator.
pub async fn loaded(world: &World, spool_id: &str, tool: i64) -> Value {
    let mut body = spool_body(ActionKind::LoadSpool);
    body["tool"] = json!(tool);
    let (status, answer) = world
        .post(&world.spool_url("load_spool", spool_id), &body)
        .await;
    assert_eq!(status, reqwest::StatusCode::OK, "{answer}");
    answer
}

/// Whether a figure an answer carries is this one, to the last few bits a sum
/// taken in another order differs in.
pub fn close(carried: &Value, expected: f64) -> bool {
    carried
        .as_f64()
        .is_some_and(|carried| (carried - expected).abs() < 1e-9)
}

/// The kinds of a page of events, in the order the page carries them.
pub fn kinds_of(page: &Value) -> Vec<String> {
    page["events"]
        .as_array()
        .expect("a page carries events")
        .iter()
        .map(|event| event["kind"].as_str().unwrap_or_default().to_owned())
        .collect()
}

/// An operator registers and loads a spool, and the ledger's four reads each
/// answer it: the listing with its one line, the spool with its own events,
/// the printer's history narrowed and paged, and a check of the file against
/// it that records nothing.
#[tokio::test(flavor = "multi_thread")]
async fn the_operator_registers_loads_and_reads_the_ledger_back() {
    let world = World::open().await;
    world.printer.holds_file(
        FILE,
        4_096,
        Some(vec![ToolFilament {
            tool: 0,
            length_mm: ANALYSED_MM,
            volume_cm3: None,
        }]),
    );
    let spool = registered(&world, "PLA white", 1_000.0).await;
    loaded(&world, &spool, 0).await;
    let other = registered(&world, "PETG black", 750.0).await;

    let (status, listing) = world.get(&plain(&world, "spools")).await;
    assert_eq!(status, reqwest::StatusCode::OK, "{listing}");
    assert_eq!(
        listing["at_a_glance"],
        json!([
            "PETG black | PLA | 750.0 g of 750.0 g left | not loaded | verified",
            "PLA white | PLA | 1000.0 g of 1000.0 g left | on tool 0 | verified",
        ]),
        "{listing}"
    );
    assert_eq!(listing["pending"], json!([]), "{listing}");

    let (status, read) = world.get(&world.spool_url("spool", &spool)).await;
    assert_eq!(status, reqwest::StatusCode::OK, "{read}");
    assert_eq!(read["spool"]["id"], json!(spool));
    assert_eq!(
        read["recent"]
            .as_array()
            .expect("a spool's read carries its events")
            .iter()
            .map(|event| event["kind"].as_str().unwrap_or_default())
            .collect::<Vec<_>>(),
        vec!["spool_loaded", "spool_registered"],
        "{read}"
    );
    let (status, unknown) = world
        .get(&world.spool_url("spool", &printobserver_core::SpoolId::new().to_string()))
        .await;
    assert_eq!(status, reqwest::StatusCode::NOT_FOUND, "{unknown}");

    // Narrowed by kind, given twice, and by spool.
    let history = plain(&world, "printer_history");
    let (status, narrowed) = world
        .get(&format!(
            "{history}?kinds=spool_registered&kinds=spool_loaded&spool_id={spool}"
        ))
        .await;
    assert_eq!(status, reqwest::StatusCode::OK, "{narrowed}");
    assert_eq!(
        kinds_of(&narrowed),
        vec!["spool_loaded", "spool_registered"]
    );
    assert!(narrowed.get("next").is_none(), "{narrowed}");

    paged_to_the_end(&world).await;
    checked_recording_nothing(&world, &spool).await;

    let _ = other;
    world.server.stop().await;
}

/// The printer's history, paged by the cursor each page answers, reaches every
/// event once and stops at the end; a limit above the maximum and a kind that
/// is not the printer's are each refused.
async fn paged_to_the_end(world: &World) {
    let history = plain(world, "printer_history");
    let mut pages = Vec::new();
    let mut cursor: Option<String> = None;
    loop {
        let url = cursor.as_ref().map_or_else(
            || format!("{history}?limit=1"),
            |before| format!("{history}?limit=1&before={before}"),
        );
        let (status, page) = world.get(&url).await;
        assert_eq!(status, reqwest::StatusCode::OK, "{page}");
        pages.extend(kinds_of(&page));
        match page.get("next").and_then(Value::as_str) {
            Some(next) => cursor = Some(next.to_owned()),
            None => break,
        }
        assert!(pages.len() < 10, "the cursor never reached the end");
    }
    assert_eq!(
        pages,
        vec!["spool_registered", "spool_loaded", "spool_registered"],
        "paging by the cursor did not walk every event once, newest first"
    );
    let (status, refused) = world.get(&format!("{history}?limit=1001")).await;
    assert_eq!(status, reqwest::StatusCode::BAD_REQUEST, "{refused}");
    let (status, refused) = world.get(&format!("{history}?kinds=camera_look")).await;
    assert_eq!(status, reqwest::StatusCode::BAD_REQUEST, "{refused}");
    // A filter given twice, or one the read does not take, is refused naming
    // it rather than one of them quietly dropped.
    let (status, refused) = world.get(&format!("{history}?limit=1&limit=2")).await;
    assert_eq!(status, reqwest::StatusCode::BAD_REQUEST, "{refused}");
    assert!(
        refused["error"]
            .as_str()
            .unwrap_or_default()
            .contains("`limit` was given more than once"),
        "{refused}"
    );
    let (status, refused) = world.get(&format!("{history}?print_id=anything")).await;
    assert_eq!(status, reqwest::StatusCode::BAD_REQUEST, "{refused}");
    assert!(
        refused["error"]
            .as_str()
            .unwrap_or_default()
            .contains("takes no `print_id`"),
        "{refused}"
    );
}

/// A check of the file against the spool on its tool answers its figures, and
/// records nothing and asks the machine for nothing.
async fn checked_recording_nothing(world: &World, spool: &str) {
    let history = plain(world, "printer_history");
    let (_, before) = world.get(&history).await;
    let (_, spools_before) = world
        .get(&format!("{}?include_retired=true", plain(world, "spools")))
        .await;
    let (status, checked) = world
        .get(&format!("{}?file_name={FILE}", plain(world, "preflight")))
        .await;
    assert_eq!(status, reqwest::StatusCode::OK, "{checked}");
    let required = grams_for_length(
        ANALYSED_MM,
        printobserver_core::DEFAULT_DIAMETER_MM,
        density_for("PLA").expect("PLA has a known density"),
    );
    let tool = &checked["preflight"]["tools"][0]["result"];
    assert_eq!(tool["verdict"], json!("passed"), "{checked}");
    assert_eq!(tool["spool"]["id"], json!(spool), "{checked}");
    assert_eq!(tool["spool"]["name"], json!("PLA white"), "{checked}");
    assert!(close(&tool["need"]["required_g"], required), "{checked}");
    assert!(
        close(
            &tool["need"]["buffer_g"],
            buffer_g(required, &SpoolPolicy::default())
        ),
        "{checked}"
    );
    assert_eq!(tool["remaining_g"], json!(1_000.0), "{checked}");
    assert_eq!(
        checked["preflight"]["verdict"],
        json!("passed"),
        "{checked}"
    );
    let (_, after) = world.get(&history).await;
    let (_, spools_after) = world
        .get(&format!("{}?include_retired=true", plain(world, "spools")))
        .await;
    assert_eq!(before, after, "a filament check wrote into the history");
    assert_eq!(
        spools_before, spools_after,
        "a filament check moved a spool"
    );
    assert!(
        world.printer.calls().is_empty(),
        "a read of the ledger asked the machine for something: {:?}",
        world.printer.calls()
    );
    let (status, refused) = world.get(&plain(world, "preflight")).await;
    assert_eq!(status, reqwest::StatusCode::BAD_REQUEST, "{refused}");
}

/// A supervision turn reads the ledger, and every spool command it asks for is
/// refused by the grants the default envelope gives the agent — and recorded.
///
/// Claiming anybody else, or the system, is refused before the grants are
/// asked, exactly as a print action is.
#[tokio::test(flavor = "multi_thread")]
async fn a_turn_reads_the_ledger_and_is_refused_every_command_by_its_grants() {
    let world = World::open().await;
    world.printer.holds_file(FILE, 4_096, None);
    let spool = registered(&world, "PLA white", 1_000.0).await;
    let (_, pass) = held_turn(&world).await;
    let turn = presenting(&pass.credential);

    for url in [
        plain(&world, "spools"),
        world.spool_url("spool", &spool),
        plain(&world, "printer_history"),
        format!("{}?file_name={FILE}", plain(&world, "preflight")),
    ] {
        let (status, body) = sent(turn.get(&url)).await;
        assert_eq!(status, reqwest::StatusCode::OK, "{url}: {body}");
    }

    let as_itself = agent(&pass.session_name);
    for (name, kind) in COMMANDS {
        let url = if kind == ActionKind::RegisterSpool {
            plain(&world, name)
        } else {
            world.spool_url(name, &spool)
        };
        let mut body = spool_body(kind);
        body["actor"] = as_itself.clone();
        let (status, answer) = sent(turn.post(&url).json(&body)).await;
        assert_eq!(status, reqwest::StatusCode::CONFLICT, "`{name}`: {answer}");
        assert_eq!(
            answer["decision"]["rejected"]["actor_may_not_request"],
            json!({ "actor_class": "agent", "action": name }),
            "`{name}` was not refused by the agent's grants: {answer}"
        );
        assert_eq!(
            kinds_of(&answer),
            vec!["spool_command_rejected"],
            "`{name}`'s refusal was not recorded: {answer}"
        );

        for (claimed, naming) in [
            (json!("operator"), "the operator"),
            (json!("system"), "system"),
        ] {
            let mut body = spool_body(kind);
            body["actor"] = claimed;
            let (status, refused) = sent(turn.post(&url).json(&body)).await;
            assert_eq!(
                status,
                reqwest::StatusCode::FORBIDDEN,
                "`{name}`: {refused}"
            );
            assert!(
                refused["error"]
                    .as_str()
                    .unwrap_or_default()
                    .contains(naming),
                "`{name}`'s refusal does not name {naming}: {refused}"
            );
        }
    }
    let (_, history) = world
        .get(&format!(
            "{}?kinds=spool_command_rejected",
            plain(&world, "printer_history")
        ))
        .await;
    assert_eq!(
        kinds_of(&history).len(),
        COMMANDS.len(),
        "a refusal the claim made was recorded, or one the grants made was not: {history}"
    );
    let (_, read) = world.get(&world.spool_url("spool", &spool)).await;
    assert!(
        read["spool"]["lifecycle"] == json!({ "state": "shelved" }),
        "a refused command moved the spool: {read}"
    );

    world.agent.release_turns();
    world.server.stop().await;
}

/// An operator under a configuration that grants no spool command is refused
/// every one by the grants, naming it — which is what an installation whose
/// configuration predates the ledger meets until it adds the five.
#[tokio::test(flavor = "multi_thread")]
async fn an_operator_granted_no_spool_command_is_refused_naming_it() {
    let world = World::configured(
        crate::printer::RecordingPrinter::printing(),
        crate::agent::StandInAgent::new(),
        |document| {
            set(
                document,
                "safety.actions.operator",
                toml::Value::Array(
                    ["pause", "resume", "cancel", "start_print"]
                        .iter()
                        .map(|name| toml::Value::String((*name).to_owned()))
                        .collect(),
                ),
            );
        },
    )
    .await;

    for (name, kind) in COMMANDS {
        let url = if kind == ActionKind::RegisterSpool {
            plain(&world, name)
        } else {
            world.spool_url(name, &printobserver_core::SpoolId::new().to_string())
        };
        let (status, answer) = world.post(&url, &spool_body(kind)).await;
        assert_eq!(status, reqwest::StatusCode::CONFLICT, "`{name}`: {answer}");
        assert_eq!(
            answer["decision"]["rejected"]["actor_may_not_request"],
            json!({ "actor_class": "operator", "action": name }),
            "`{name}`: {answer}"
        );
    }
    let (_, listing) = world.get(&plain(&world, "spools")).await;
    assert_eq!(listing["spools"], json!([]), "{listing}");
    world.server.stop().await;
}

/// A spool command's body is held to its command's own fields: the spool its
/// path names cannot be named again, and a field it does not take is refused.
#[tokio::test(flavor = "multi_thread")]
async fn a_spool_command_body_carries_its_own_fields_and_no_others() {
    let world = World::open().await;
    let spool = registered(&world, "PLA white", 1_000.0).await;

    let mut naming = spool_body(ActionKind::LoadSpool);
    naming["spool_id"] = json!(spool);
    let (status, refused) = world
        .post(&world.spool_url("load_spool", &spool), &naming)
        .await;
    assert_eq!(status, reqwest::StatusCode::BAD_REQUEST, "{refused}");

    let mut stray = spool_body(ActionKind::RegisterSpool);
    stray["factor"] = json!(1.1);
    let (status, refused) = world.post(&plain(&world, "register_spool"), &stray).await;
    assert_eq!(status, reqwest::StatusCode::BAD_REQUEST, "{refused}");
    assert!(
        refused["error"]
            .as_str()
            .unwrap_or_default()
            .contains("factor"),
        "{refused}"
    );

    let mut short = spool_body(ActionKind::RegisterSpool);
    short
        .as_object_mut()
        .expect("a body is a document")
        .remove("initial_g");
    let (status, refused) = world.post(&plain(&world, "register_spool"), &short).await;
    assert_eq!(status, reqwest::StatusCode::BAD_REQUEST, "{refused}");

    // A body that is no document of fields, one naming no actor, and one
    // naming an actor this system does not have: each refused before the
    // claim, the grants or the ledger are asked anything.
    let register = plain(&world, "register_spool");
    let (status, refused) = world.post(&register, &json!(["a list of fields"])).await;
    assert_eq!(status, reqwest::StatusCode::BAD_REQUEST, "{refused}");
    assert!(
        refused["error"]
            .as_str()
            .unwrap_or_default()
            .contains("a document of its fields"),
        "{refused}"
    );
    let mut nobody = spool_body(ActionKind::RegisterSpool);
    nobody
        .as_object_mut()
        .expect("a body is a document")
        .remove("actor");
    let (status, refused) = world.post(&register, &nobody).await;
    assert_eq!(status, reqwest::StatusCode::BAD_REQUEST, "{refused}");
    assert!(
        refused["error"]
            .as_str()
            .unwrap_or_default()
            .contains("needs `actor`"),
        "{refused}"
    );
    let mut stranger = spool_body(ActionKind::RegisterSpool);
    stranger["actor"] = json!("a passer-by");
    let (status, refused) = world.post(&register, &stranger).await;
    assert_eq!(status, reqwest::StatusCode::BAD_REQUEST, "{refused}");
    assert!(
        refused["error"]
            .as_str()
            .unwrap_or_default()
            .contains("not an actor this system has"),
        "{refused}"
    );

    let (_, listing) = world.get(&plain(&world, "spools")).await;
    assert_eq!(
        listing["spools"].as_array().map(Vec::len),
        Some(1),
        "a refused body registered a spool: {listing}"
    );
    let (_, history) = world
        .get(&format!(
            "{}?kinds=spool_command_rejected",
            plain(&world, "printer_history")
        ))
        .await;
    assert_eq!(
        history["events"],
        json!([]),
        "a body refused before the grants was recorded as a refused command: {history}"
    );
    world.server.stop().await;
}

/// The `[spools]` table is what the supervisor checks spools under, and left
/// out it is the supervision core's own defaults.
#[tokio::test(flavor = "multi_thread")]
async fn the_spools_table_is_what_the_supervisor_checks_under() {
    let world = World::open().await;
    assert_eq!(
        world.server.supervisor().config().spools,
        SpoolPolicy::default()
    );
    world.server.stop().await;

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
                    ("buffer_percent".to_owned(), toml::Value::Float(12.5)),
                    ("buffer_min_g".to_owned(), toml::Value::Integer(3)),
                    ("analysis_wait_seconds".to_owned(), toml::Value::Integer(0)),
                ])),
            );
        },
    )
    .await;
    assert_eq!(
        world.server.supervisor().config().spools,
        SpoolPolicy {
            preflight: printobserver_core::PreflightMode::Warn,
            buffer_percent: 12.5,
            buffer_min_g: 3.0,
            analysis_wait_s: 0,
        }
    );
    world.server.stop().await;
}
