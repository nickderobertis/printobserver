//! Every check of a print is told what the slicer planned at the print's
//! current height, from one read of its G-code.
//!
//! The server here is the real composition: the `OctoPrint` adapter against
//! the scripted `OctoPrint`, the durable store, and the real `OneHarness`
//! engine with only the paid provider process replaced by this crate's
//! responder. The scripted instance serves a real `PrusaSlicer` slice
//! (`tests/fixtures/real-prints/bubbler-guard-and-pad.gcode`, the plain copy of
//! the file the `bubbler-guard-and-pad-pla-healthy` case printed, byte for
//! byte — a plain copy because that case's files are Git LFS pointers until
//! the real-print suite fetches them), and moves the job's file position
//! through it. Each scheduled check runs a turn, and the responder does what a
//! supervising agent is told to: it reads the print's context through the
//! running API with its turn's own credential, and writes down what it was
//! answered. So what a check's turn was handed is read off the far side of
//! the process boundary, and how often the file was read is read off the far
//! side of the printer's — the scripted instance counts every download it
//! answered from the first byte — and how often an index was built is read
//! off the print's own history, through the running server.
//!
//! The values asserted at each position were worked out from the file apart
//! from the reader: its `;LAYER_CHANGE` and `;Z:` lines, the `;TYPE:` in force
//! under each move of the layer that feeds filament while it travels, the last
//! `M73 P.. R..` and `M106` lines the position has passed, and the extremes of
//! every move laying material between `M486 S0` and `M486 S-1`.

use core::time::Duration;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use printobserver_core::store::Stores;
use printobserver_obico::{ObicoVision, ObicoVisionConfig};
use printobserver_octoprint::OctoPrintPrinter;
use printobserver_octoprint::standin::{JobScript, ScriptedOctoPrint};
use printobserver_oneharness::{EnvAssignment, OneharnessSupervisor};
use printobserver_server::{Ports, Running, Server, ServerConfig, agent_config};
use printobserver_store_sqlite::SqliteStore;
use printobserver_types::Timestamp;
use printobserver_types::serde_json::{Value, json};
use tempfile::TempDir;

use crate::world::{OPERATOR, document, presenting, set, write};

/// The real slice the scripted instance serves, byte for byte.
const BUBBLER: &[u8] = include_bytes!("../fixtures/real-prints/bubbler-guard-and-pad.gcode");

/// The name the first print's file is held under.
const FIRST: &str = "bubbler-guard-and-pad.gcode";

/// The name the second print's file is held under: the same slice, a second
/// file on the instance.
const SECOND: &str = "bubbler-guard-and-pad-again.gcode";

/// How long the scripted instance holds every download, so that a check and a
/// context read can be made to arrive while one build is under way.
const SLOW_LINK: Duration = Duration::from_secs(3);

/// How long a journey waits for something the server does on its own clock.
const PATIENCE: Duration = Duration::from_secs(90);

/// What the file plans at one position, as worked out from the file.
struct Planned {
    /// The job's file position.
    position: i64,
    /// The layer, counting the first as 1.
    layer: u64,
    /// Its height.
    z_mm: f64,
    /// What the layer lays material as, in the order the file first names each.
    types: &'static [&'static str],
    /// The slicer's minutes left by the position.
    minutes: i64,
    /// The part-cooling fan the file had set by the position, in percent.
    fan: f64,
}

/// Inside layer 23, which prints the support under the guard's overhang.
const IN_SUPPORT: Planned = Planned {
    position: 246_536,
    layer: 23,
    z_mm: 4.6,
    types: &[
        "Support material",
        "Perimeter",
        "External perimeter",
        "Internal infill",
    ],
    minutes: 142,
    fan: 99.0,
};

/// Inside layer 60, which prints the first overhang perimeter.
const IN_OVERHANG: Planned = Planned {
    position: 523_686,
    layer: 60,
    z_mm: 9.0,
    types: &[
        "Perimeter",
        "External perimeter",
        "Solid infill",
        "Internal infill",
        "Overhang perimeter",
    ],
    minutes: 135,
    fan: 99.0,
};

/// Inside layer 63, which bridges over the overhang.
const IN_BRIDGE: Planned = Planned {
    position: 554_564,
    layer: 63,
    z_mm: 9.4,
    types: &[
        "Perimeter",
        "External perimeter",
        "Solid infill",
        "Internal infill",
        "Overhang perimeter",
        "Bridge infill",
    ],
    minutes: 135,
    fan: 99.0,
};

/// Inside layer 400, a plain wall high up the guard.
const HIGH_UP: Planned = Planned {
    position: 3_695_119,
    layer: 400,
    z_mm: 75.6,
    types: &["External perimeter"],
    minutes: 62,
    fan: 100.0,
};

/// The file's layer count and final height.
const LAYERS: u64 = 732;

/// The final layer's height, in millimetres.
const FINAL_Z_MM: f64 = 142.0;

/// The one object the file labels, and the bed area its material covers.
fn objects() -> Value {
    json!([{
        "name": "bubbler_guard_and_pad.stl",
        "x_mm": [83.0, 167.3],
        "y_mm": [86.8, 133.3],
    }])
}

/// The plan a context should carry at one position, with no change made
/// through this program.
fn plan_of(planned: &Planned) -> Value {
    json!({
        "layer": planned.layer,
        "layers": LAYERS,
        "z_mm": planned.z_mm,
        "final_z_mm": FINAL_Z_MM,
        "types": planned.types,
        "minutes_left": planned.minutes,
        "flow_factor": { "file": 1.0 },
        "fan_percent": { "file": planned.fan },
        "objects": objects(),
    })
}

/// What a G-code line looks like, any one of which in what a turn was handed
/// is the file itself reaching the agent.
const GCODE_TEXT: [&str; 6] = [";LAYER_CHANGE", ";TYPE:", ";Z:", "G1 X", "M73 P", "M486 S"];

/// A server composed as `Server::start` composes it, over the scripted
/// instance, with the provider process the responder.
struct Indexing {
    /// The journey's root.
    root: TempDir,
    /// The configuration, which a restart starts again from.
    config: ServerConfig,
    /// The server, serving.
    server: Running,
    /// The scripted `OctoPrint`.
    octoprint: ScriptedOctoPrint,
    /// The operator's client.
    client: reqwest::Client,
}

impl Indexing {
    /// A server checking every second, over a scripted instance holding the
    /// files given and printing the first of them at a position.
    async fn open(files: &[(&str, &[u8])], position: i64) -> Self {
        let octoprint = ScriptedOctoPrint::start();
        for (name, bytes) in files {
            octoprint.holds_file(name, bytes.to_vec(), 1_000.0);
        }
        octoprint.delays_downloads(SLOW_LINK);
        let (first, bytes) = files[0];
        octoprint.reports("Printing", printing(first, bytes, position));
        let root = TempDir::new().expect("a journey's own root");
        let mut configured = document(root.path(), &octoprint.base_url());
        set(
            &mut configured,
            "octoprint.api_key",
            toml::Value::String(octoprint.api_key()),
        );
        set(
            &mut configured,
            "supervisor.observation_interval_s",
            toml::Value::Integer(1),
        );
        let config = ServerConfig::load(write(root.path(), &configured))
            .expect("the journey's configuration is accepted");
        let server = start(&config, root.path()).await;
        Self {
            root,
            config,
            server,
            octoprint,
            client: presenting(OPERATOR),
        }
    }

    /// Stop this server and start another over the same state directory.
    async fn restart(self) -> Self {
        let Self {
            root,
            config,
            server,
            octoprint,
            client,
        } = self;
        server.stop().await;
        let server = start(&config, root.path()).await;
        Self {
            root,
            config,
            server,
            octoprint,
            client,
        }
    }

    /// One JSON read of the versioned surface, and when it was sent and
    /// answered.
    async fn get(&self, path: &str) -> (Value, Timestamp, Timestamp) {
        let sent = Timestamp::now();
        let response = self
            .client
            .get(format!("{}{path}", self.server.base_url()))
            .send()
            .await
            .expect("the server answers");
        let status = response.status();
        let body: Value = response.json().await.expect("a JSON answer");
        assert_eq!(status, reqwest::StatusCode::OK, "{path}: {body}");
        (body, sent, Timestamp::now())
    }

    /// The print the scripted job is, once the server has adopted it.
    async fn active(&self, other_than: Option<&str>) -> String {
        let deadline = std::time::Instant::now() + PATIENCE;
        loop {
            let (listed, _, _) = self.get("/prints").await;
            if let Some(active) = listed["active"].as_str()
                && Some(active) != other_than
            {
                return active.to_owned();
            }
            assert!(
                std::time::Instant::now() < deadline,
                "the running job was never adopted: {listed}"
            );
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
    }

    /// The print's context, as the operator reads it, and when it was asked
    /// and answered.
    async fn context(&self, print_id: &str) -> (Value, Timestamp, Timestamp) {
        let (answer, sent, answered) = self.get(&format!("/prints/{print_id}/context")).await;
        (answer["context"].clone(), sent, answered)
    }

    /// The print's events of one kind, newest first, read through the server.
    async fn history(&self, print_id: &str, kind: &str) -> Vec<Value> {
        let (answer, _, _) = self
            .get(&format!("/prints/{print_id}/history?limit=1000"))
            .await;
        answer["events"]
            .as_array()
            .expect("a history answers its events")
            .iter()
            .filter(|event| event["kind"] == json!(kind))
            .cloned()
            .collect()
    }

    /// How many indexes the server built for a print, by its own record.
    async fn builds(&self, print_id: &str) -> usize {
        self.history(print_id, "gcode_indexed").await.len()
    }

    /// Every context the responder was answered, in the order its turns read
    /// them.
    fn turns(&self) -> Vec<Value> {
        std::fs::read_to_string(responder_log(self.root.path()))
            .unwrap_or_default()
            .lines()
            .filter(|line| !line.trim().is_empty())
            .map(|line| {
                let done: Value = printobserver_types::serde_json::from_str(line)
                    .unwrap_or_else(|error| panic!("the responder wrote {line:?}: {error}"));
                assert_eq!(done["operation"], json!("context"), "{done}");
                assert_eq!(done["status"], json!(200), "{done}");
                done["body"]["context"].clone()
            })
            .collect()
    }

    /// Move the job to a position, and wait for a scheduled check's turn to
    /// read the print's context there; answer what that turn was handed.
    async fn checked_at(&self, print_id: &str, file: &str, bytes: &[u8], position: i64) -> Value {
        self.octoprint
            .reports("Printing", printing(file, bytes, position));
        let deadline = std::time::Instant::now() + PATIENCE;
        loop {
            if let Some(turn) = self.turns().into_iter().rev().find(|turn| {
                turn["print"]["id"] == json!(print_id)
                    && turn["job"]["file_position_bytes"] == json!(position)
            }) {
                self.handed_no_gcode(&turn);
                return turn;
            }
            assert!(
                std::time::Instant::now() < deadline,
                "no check's turn read the context at byte {position}"
            );
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
    }

    /// Wait for this many more scheduled checks of a print to have run their
    /// turns past the ones counted already.
    async fn more_checks(&self, print_id: &str, more: usize) {
        let of_the_print = |turns: Vec<Value>| {
            turns
                .into_iter()
                .filter(|turn| turn["print"]["id"] == json!(print_id))
                .count()
        };
        let before = of_the_print(self.turns());
        let deadline = std::time::Instant::now() + PATIENCE;
        while of_the_print(self.turns()) < before + more {
            assert!(
                std::time::Instant::now() < deadline,
                "{more} more checks of {print_id} never ran their turns"
            );
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
    }

    /// Set the part-cooling fan as the operator, through the server, and read
    /// the print's context after it.
    async fn operator_sets_the_fan(&self, print_id: &str, percent: f64) -> Value {
        let response = self
            .client
            .post(format!(
                "http://{}{}",
                self.server.address(),
                printobserver_server::operation("set_fan_percent")
                    .expect("the fan is an operation")
                    .full_path()
                    .replace("{print_id}", print_id)
            ))
            .json(&json!({
                "reason": "the bridge needs more cooling",
                "actor": "operator",
                "percent": percent,
            }))
            .send()
            .await
            .expect("the server answers");
        assert_eq!(response.status(), reqwest::StatusCode::OK);
        self.context(print_id).await.0
    }

    /// Neither the context a turn read nor the prompt its harness was started
    /// with carries a line of G-code.
    fn handed_no_gcode(&self, turn: &Value) {
        let seen = std::fs::read_to_string(responder_seen(self.root.path()))
            .expect("the harness wrote down what it was started with");
        let read = turn.to_string();
        for marker in GCODE_TEXT {
            assert!(
                !read.contains(marker),
                "a turn's context carried G-code ({marker}): {read}"
            );
            assert!(
                !seen.contains(marker),
                "a turn's prompt carried G-code ({marker}): {seen}"
            );
        }
    }
}

/// The scripted job printing one file at a position.
fn printing(file: &str, bytes: &[u8], position: i64) -> JobScript {
    JobScript::printing(
        file,
        i64::try_from(bytes.len()).expect("a file's size"),
        position,
        600,
    )
}

/// Where the responder appends what it did, beside the state directory so a
/// restart goes on appending to it.
fn responder_log(root: &Path) -> PathBuf {
    root.join("responder.log")
}

/// Where the responder writes what it was last started with.
fn responder_seen(root: &Path) -> PathBuf {
    root.join("responder-seen.json")
}

/// Start the real server over the real four, the agent composed exactly as
/// `Server::start` composes it with only its harness the responder, which
/// reads the print's context on every turn.
async fn start(config: &ServerConfig, root: &Path) -> Running {
    let stores = Stores::of(Arc::new(
        SqliteStore::open(&config.state_dir).expect("the store opens"),
    ));
    let mut composed = agent_config(config).expect("the agent is composed");
    composed.harness_bin = Some(PathBuf::from(env!(
        "CARGO_BIN_EXE_printobserver-server-responder"
    )));
    let answer = json!({
        "result": json!({
            "summary": "the layer is going down as the slicer planned it",
            "confidence": "high",
            "should_continue": true,
            "did": "read the print's context",
            "why": "nothing in it called for a change",
            "escalating": false,
        })
        .to_string(),
        "session_id": "SID-INDEXING",
        "thread_id": "SID-INDEXING",
    });
    for assignment in [
        format!("MOCK_STDOUT={answer}"),
        format!(
            "PRINTOBSERVER_RESPONDER_LOG={}",
            responder_log(root).display()
        ),
        format!(
            "PRINTOBSERVER_RESPONDER_SEEN={}",
            responder_seen(root).display()
        ),
        format!(
            "PRINTOBSERVER_RESPONDER_ACTIONS={}",
            json!([{ "operation": "context", "body": {} }])
        ),
    ] {
        composed
            .harness_env
            .push(EnvAssignment::new(&assignment).expect("an assignment"));
    }
    Server::start_with(
        config.clone(),
        Ports {
            printer: Arc::new(OctoPrintPrinter::new(config.octoprint.clone()))
                as Arc<dyn printobserver_printer_api::PrinterPort>,
            stores,
            vision: Arc::new(
                ObicoVision::new(ObicoVisionConfig::default()).expect("the adapter is built"),
            ),
            agent: Arc::new(OneharnessSupervisor::open(composed).expect("the agent opens"))
                as Arc<dyn printobserver_supervisor_api::SupervisorPort>,
        },
    )
    .await
    .expect("the server starts against the scripted instance")
}

/// When an event was received, as the server recorded it.
fn received_at(event: &Value) -> Timestamp {
    printobserver_types::serde_json::from_value(event["received_at"].clone())
        .expect("an event carries when it was received")
}

/// One read and one build serve every check and every context read of a
/// print, the first of them a scheduled check and a context read arriving
/// while that one build is under way; each check's turn is handed the plan at
/// its own position — a support layer's and an overhang layer's types among
/// them — and no line of the file. A second print is read and built once of
/// its own, and a server restarted part-way through a print builds once more
/// and then never again.
#[tokio::test(flavor = "multi_thread")]
async fn one_read_of_a_prints_gcode_plans_every_check_of_it() {
    let world = Indexing::open(&[(FIRST, BUBBLER), (SECOND, BUBBLER)], IN_SUPPORT.position).await;
    let print_id = world.active(None).await;

    // The first scheduled check runs as the job is adopted, and the one
    // download is held for three seconds: whichever of the check and this read
    // arrives first starts the build, and the other arrives while it is held.
    let (context, sent, answered) = world.context(&print_id).await;
    assert_eq!(context["slicer"], plan_of(&IN_SUPPORT));
    let first_check = world
        .history(&print_id, "periodic_observation")
        .await
        .into_iter()
        .last()
        .expect("a scheduled check ran while the file was read");
    let at = received_at(&first_check);
    let held_from = Timestamp::from_unix_seconds(
        sent.as_utc().timestamp() - i64::try_from(SLOW_LINK.as_secs()).expect("seconds"),
    )
    .expect("a moment");
    assert!(
        held_from < at && at <= answered,
        "the first check, at {at:?}, did not arrive while the one download was held for the \
         read sent at {sent:?} and answered by {answered:?}"
    );
    assert!(
        answered.as_utc().timestamp_millis() - sent.as_utc().timestamp_millis() > 1_000,
        "the read was answered without waiting on the held download"
    );

    let turn = world
        .checked_at(&print_id, FIRST, BUBBLER, IN_SUPPORT.position)
        .await;
    assert_eq!(turn["slicer"], plan_of(&IN_SUPPORT));
    for planned in [&IN_OVERHANG, &IN_BRIDGE] {
        let turn = world
            .checked_at(&print_id, FIRST, BUBBLER, planned.position)
            .await;
        assert_eq!(
            turn["slicer"],
            plan_of(planned),
            "at {}",
            planned.position
        );
        let (read, _, _) = world.context(&print_id).await;
        assert_eq!(read["slicer"], plan_of(planned));
    }

    // A change made through this program stands beside what the file set,
    // until the file sets the same thing again.
    let changed = world.operator_sets_the_fan(&print_id, 80.0).await;
    assert_eq!(
        changed["slicer"]["fan_percent"],
        json!({ "file": IN_BRIDGE.fan, "printobserver": 80.0 })
    );
    let turn = world
        .checked_at(&print_id, FIRST, BUBBLER, HIGH_UP.position)
        .await;
    assert_eq!(
        turn["slicer"],
        plan_of(&HIGH_UP),
        "the file's own later fan command did not take over from the change"
    );

    world.more_checks(&print_id, 2).await;
    assert_eq!(world.octoprint.whole_downloads(FIRST), 1);
    assert_eq!(world.builds(&print_id).await, 1);

    let world = world.restart().await;
    let turn = world
        .checked_at(&print_id, FIRST, BUBBLER, IN_OVERHANG.position)
        .await;
    assert_eq!(turn["slicer"], plan_of(&IN_OVERHANG));
    world.more_checks(&print_id, 3).await;
    world.context(&print_id).await;
    assert_eq!(
        world.octoprint.whole_downloads(FIRST),
        2,
        "a restarted server did not build its own index once"
    );
    assert_eq!(world.builds(&print_id).await, 2);

    world
        .octoprint
        .reports("Printing", printing(SECOND, BUBBLER, IN_BRIDGE.position));
    let second = world.active(Some(&print_id)).await;
    let turn = world
        .checked_at(&second, SECOND, BUBBLER, IN_BRIDGE.position)
        .await;
    assert_eq!(turn["slicer"], plan_of(&IN_BRIDGE));
    world.more_checks(&second, 2).await;
    world.context(&second).await;
    assert_eq!(world.octoprint.whole_downloads(SECOND), 1);
    assert_eq!(world.builds(&second).await, 1);
    assert_eq!(world.octoprint.whole_downloads(FIRST), 2);
    assert_eq!(world.builds(&print_id).await, 2);
    world.server.stop().await;
}

/// The same slice with its `;TYPE:` and `M73` lines taken out: every check
/// still runs, and the context says the extrusion types and the time left are
/// unknown while it goes on naming the layers the file does mark.
#[tokio::test(flavor = "multi_thread")]
async fn a_file_with_no_types_or_estimates_says_so_and_is_checked_all_the_same() {
    let stripped: Vec<u8> = String::from_utf8(BUBBLER.to_vec())
        .expect("the slice is text")
        .split_inclusive('\n')
        .filter(|line| !line.starts_with(";TYPE:") && !line.starts_with("M73 "))
        .collect::<String>()
        .into_bytes();
    let world = Indexing::open(&[("unlabelled.gcode", &stripped)], 200_000).await;
    world.octoprint.delays_downloads(Duration::ZERO);
    let print_id = world.active(None).await;

    let turn = world
        .checked_at(&print_id, "unlabelled.gcode", &stripped, 200_000)
        .await;
    world.more_checks(&print_id, 2).await;

    let plan = &turn["slicer"];
    assert_eq!(
        plan["unknown"],
        json!(["types", "minutes_left"])
    );
    assert!(plan.get("types").is_none(), "{plan}");
    assert!(plan.get("minutes_left").is_none(), "{plan}");
    assert_eq!(plan["final_z_mm"], json!(FINAL_Z_MM));
    assert_eq!(plan["layers"], json!(LAYERS));
    assert!(
        world.history(&print_id, "periodic_observation").await.len() >= 3,
        "the checks did not go on"
    );
    assert_eq!(world.builds(&print_id).await, 1);
    world.server.stop().await;
}
