//! Every check is lean, and a quiet one is acknowledged from its answer.
//!
//! The server here is the real composition, configured from each committed
//! install template: the `OctoPrint` adapter against the scripted `OctoPrint`
//! serving a real slice, the durable store, a camera serving a 1920x1080 frame
//! with a mark in each corner, and the real supervisor port and `OneHarness`
//! engine with only the paid provider process replaced by this crate's
//! responder over `OneHarness`'s published mock harness. What a check's turn
//! was handed is read off both sides of the process boundary: the run request
//! the port built, and the arguments and standard input the harness process
//! received — the prompt and the image `OneHarness` attached to it.

use core::time::Duration;
use std::io::Cursor;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use oneharness_core::domain::lean::ToolClass;
use oneharness_core::io::run::RunRequest;
use printobserver_core::store::Stores;
use printobserver_obico::{ObicoVision, ObicoVisionConfig};
use printobserver_octoprint::OctoPrintPrinter;
use printobserver_octoprint::standin::{JobScript, ScriptedOctoPrint};
use printobserver_oneharness::{EnvAssignment, OneharnessSupervisor, RunRequestObserver, TurnSeam};
use printobserver_server::{Ports, Running, Server, ServerConfig, agent_config};
use printobserver_store_sqlite::SqliteStore;
use printobserver_types::serde_json::{Value, json};
use tempfile::TempDir;

use crate::installed_detector::{INSTALLERS, template};
use crate::world::{OPERATOR, SECRET, committed_skill, operator_verifier, presenting, set};

/// The real slice the scripted instance serves.
const SLICE: &[u8] = include_bytes!("../fixtures/real-prints/bubbler-guard-and-pad.gcode");

/// The name it is held under.
const FILE: &str = "bubbler-guard-and-pad.gcode";

/// How long a journey waits for something the server does on its own clock.
const PATIENCE: Duration = Duration::from_secs(90);

/// The camera's frame size.
const CAMERA: (u32, u32) = (1920, 1080);

/// The size every check's image is.
const CHECK: (u32, u32) = (1280, 720);

/// How wide each corner's mark is, in camera pixels.
const MARK: u32 = 150;

/// A camera frame: a checkerboard of two greys, which no blank-frame bound
/// reads as a camera showing nothing, with a pure red mark in each corner, so a
/// crop loses a mark and a frame scaled whole keeps all four.
fn marked_frame() -> Vec<u8> {
    marked_frame_of(CAMERA)
}

/// A marked frame of one size.
fn marked_frame_of((width, height): (u32, u32)) -> Vec<u8> {
    let frame = image::RgbImage::from_fn(width, height, |x, y| {
        let left_or_right = x < MARK || x >= width - MARK;
        let top_or_bottom = y < MARK || y >= height - MARK;
        if left_or_right && top_or_bottom {
            image::Rgb([255, 0, 0])
        } else if (x / 40 + y / 40) % 2 == 0 {
            image::Rgb([60, 60, 60])
        } else {
            image::Rgb([190, 190, 190])
        }
    });
    let mut jpeg = Vec::new();
    image::codecs::jpeg::JpegEncoder::new_with_quality(&mut jpeg, 95)
        .encode_image(&frame)
        .expect("the frame encodes");
    jpeg
}

/// Whether one pixel is the mark's red.
fn red(pixel: image::Rgb<u8>) -> bool {
    let [r, g, b] = pixel.0;
    r > 200 && g < 60 && b < 60
}

/// Hold a check's image to the whole frame scaled: 1280x720, a mark in every
/// corner, and the middle of every edge grey.
fn is_the_whole_frame_scaled(bytes: &[u8], what: &str) {
    let decoded = image::load_from_memory(bytes)
        .unwrap_or_else(|error| panic!("{what} is no image: {error}"))
        .to_rgb8();
    assert_eq!(decoded.dimensions(), CHECK, "{what} is not 1280x720");
    let (width, height) = CHECK;
    let inset = 20;
    for (x, y) in [
        (inset, inset),
        (width - inset, inset),
        (inset, height - inset),
        (width - inset, height - inset),
    ] {
        assert!(
            red(*decoded.get_pixel(x, y)),
            "{what} lost the corner mark at ({x}, {y}): it was cropped"
        );
    }
    for (x, y) in [
        (width / 2, inset),
        (inset, height / 2),
        (width / 2, height / 2),
    ] {
        assert!(
            !red(*decoded.get_pixel(x, y)),
            "{what} is marked at ({x}, {y}), where the frame is grey"
        );
    }
}

/// Every run request the port built, in order.
#[derive(Default)]
struct Requests(Mutex<Vec<RunRequest>>);

impl RunRequestObserver for Requests {
    fn built(&self, request: &RunRequest) {
        self.0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .push(request.clone());
    }
}

impl Requests {
    fn all(&self) -> Vec<RunRequest> {
        self.0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone()
    }
}

/// An assessment the scripted agent answers, as Claude Code streams one: one
/// model call, then the result.
fn streamed(answer: &Value) -> String {
    [
        json!({
            "type": "assistant",
            "session_id": "SID-LEAN",
            "message": {
                "id": "msg_0",
                "model": "claude-test",
                "content": [{ "type": "text", "text": "judging the frame" }],
                "usage": {
                    "input_tokens": 3,
                    "cache_creation_input_tokens": 4_000,
                    "cache_read_input_tokens": 0,
                    "output_tokens": 90,
                },
            },
        }),
        json!({
            "type": "result",
            "subtype": "success",
            "is_error": false,
            "result": answer.to_string(),
            "session_id": "SID-LEAN",
        }),
    ]
    .map(|line| line.to_string())
    .join("\n")
}

/// An assessment answering with a disposition and a reason.
fn answering(disposition: &str, why: &str) -> Value {
    json!({
        "summary": "the walls are going down clean",
        "confidence": "high",
        "should_continue": disposition != "stop",
        "did": "judged the frame",
        "why": why,
        "escalating": false,
        "disposition": disposition,
    })
}

/// Copy one directory tree into another.
fn copy_tree(from: &Path, to: &Path) {
    std::fs::create_dir_all(to).expect("the copy's directory is writable");
    for entry in std::fs::read_dir(from).expect("the directory is readable") {
        let path = entry.expect("an entry").path();
        let target = to.join(path.file_name().expect("a file name"));
        if path.is_dir() {
            copy_tree(&path, &target);
        } else {
            std::fs::copy(&path, &target).expect("a file is copyable");
        }
    }
}

/// The configuration `installer`'s template writes, its `[camera]` example
/// taken up and what an operator fills in filled in, written under `root`.
fn configured(
    installer: &str,
    root: &TempDir,
    octoprint: &ScriptedOctoPrint,
    camera: &crate::http_host::Host,
    interval_s: i64,
) -> PathBuf {
    let state = root.path().join("state");
    let text: String = template(installer, &state)
        .lines()
        .map(|line| {
            if line.starts_with("# [camera]") || line.starts_with("# snapshot_url = ") {
                line.trim_start_matches("# ").to_owned()
            } else {
                line.to_owned()
            }
        })
        .collect::<Vec<_>>()
        .join("\n");
    let mut document: toml::Value = toml::from_str(&text).expect("the template is a document");
    let mut fill = |dotted: &str, value: toml::Value| set(&mut document, dotted, value);
    fill("listen", toml::Value::String("127.0.0.1:0".to_owned()));
    fill("octoprint.url", toml::Value::String(octoprint.base_url()));
    fill(
        "octoprint.api_key",
        toml::Value::String(octoprint.api_key()),
    );
    fill(
        "ingress.shared_secret",
        toml::Value::String(SECRET.to_owned()),
    );
    fill("camera.snapshot_url", toml::Value::String(camera.url()));
    fill(
        "supervisor.observation_interval_s",
        toml::Value::Integer(interval_s),
    );
    document
        .as_table_mut()
        .expect("the configuration is a table")
        .insert(
            "api".to_owned(),
            toml::Value::Table(toml::Table::from_iter([(
                "credential_verifier".to_owned(),
                toml::Value::String(operator_verifier()),
            )])),
        );
    let path = root.path().join("config.toml");
    std::fs::write(&path, toml::to_string(&document).expect("it renders"))
        .expect("the configuration is writable");
    path
}

/// The agent composed from `config` as the server composes it, its harness the
/// responder scripted to answer `answers` in turn after issuing `actions`, and
/// what watches the run requests it builds.
fn scripted_agent(
    config: &ServerConfig,
    root: &Path,
    answers: &[Value],
    actions: &Value,
) -> (OneharnessSupervisor, Arc<Requests>) {
    let mut composed = agent_config(config).expect("the agent is composed");
    composed.harness_bin = Some(PathBuf::from(env!(
        "CARGO_BIN_EXE_printobserver-server-responder"
    )));
    let mut scripted = vec![
        format!("MOCK_ATTEMPT_FILE={}", root.join("attempts").display()),
        format!(
            "PRINTOBSERVER_RESPONDER_LOG={}",
            root.join("responder.log").display()
        ),
        format!(
            "PRINTOBSERVER_RESPONDER_SEEN={}",
            root.join("seen.json").display()
        ),
        format!("PRINTOBSERVER_RESPONDER_ACTIONS={actions}"),
        format!(
            "PRINTOBSERVER_RESPONDER_HOLD={}",
            root.join("hold").display()
        ),
        format!(
            "MOCK_STDOUT={}",
            streamed(answers.last().expect("an answer"))
        ),
    ];
    for (index, answer) in answers.iter().enumerate() {
        scripted.push(format!("MOCK_STDOUT_{}={}", index + 1, streamed(answer)));
    }
    for assignment in scripted {
        composed
            .harness_env
            .push(EnvAssignment::new(&assignment).expect("an assignment"));
    }
    let requests = Arc::new(Requests::default());
    let agent = OneharnessSupervisor::observed(
        composed,
        TurnSeam {
            requests: Some(Arc::clone(&requests) as Arc<dyn RunRequestObserver>),
            ..TurnSeam::default()
        },
    )
    .expect("the agent opens");
    (agent, requests)
}

/// One server, configured from one installer's template, and what watches it.
struct Lean {
    root: TempDir,
    server: Running,
    _octoprint: ScriptedOctoPrint,
    camera: crate::http_host::Host,
    requests: Arc<Requests>,
    client: reqwest::Client,
}

impl Lean {
    /// A server configured from `installer`'s template, its `[camera]` example
    /// taken up and checking every `interval_s` seconds, whose agent answers
    /// each turn in `answers` in turn, the last from then on, after issuing
    /// `actions`.
    async fn open(installer: &str, interval_s: i64, answers: &[Value], actions: &Value) -> Self {
        Self::open_with_frame(installer, interval_s, answers, actions, marked_frame()).await
    }

    /// The same, with a camera answering `frame`.
    async fn open_with_frame(
        installer: &str,
        interval_s: i64,
        answers: &[Value],
        actions: &Value,
        frame: Vec<u8>,
    ) -> Self {
        let root = TempDir::new().expect("a journey's own root");
        let state = root.path().join("state");
        copy_tree(
            committed_skill().parent().expect("the skill's directory"),
            &state.join("skills").join("printobserver"),
        );
        let octoprint = ScriptedOctoPrint::start();
        octoprint.holds_file(FILE, SLICE.to_vec(), 1_000.0);
        let size = i64::try_from(SLICE.len()).expect("a file's size");
        octoprint.reports("Printing", JobScript::printing(FILE, size, size / 2, 600));
        let camera = crate::http_host::image_host(frame).await;

        let path = configured(installer, &root, &octoprint, &camera, interval_s);
        let config = ServerConfig::load(&path)
            .unwrap_or_else(|error| panic!("{installer}'s configuration reads: {error}"));

        let (agent, requests) = scripted_agent(&config, root.path(), answers, actions);
        let server = Server::start_with(
            config.clone(),
            Ports {
                printer: Arc::new(OctoPrintPrinter::new(config.octoprint.clone())),
                stores: Stores::of(Arc::new(
                    SqliteStore::open(&config.state_dir).expect("the store opens"),
                )),
                vision: Arc::new(ObicoVision::new(ObicoVisionConfig::default()).expect("built")),
                agent: Arc::new(agent) as Arc<dyn printobserver_supervisor_api::SupervisorPort>,
            },
        )
        .await
        .unwrap_or_else(|error| panic!("{installer}'s server starts: {error}"));
        Self {
            root,
            server,
            _octoprint: octoprint,
            camera,
            requests,
            client: presenting(OPERATOR),
        }
    }

    /// One JSON read of the versioned surface, as the operator.
    async fn get(&self, path: &str) -> Value {
        let response = self
            .client
            .get(format!("{}{path}", self.server.base_url()))
            .send()
            .await
            .expect("the server answers");
        let status = response.status();
        let body: Value = response.json().await.expect("a JSON answer");
        assert_eq!(status, reqwest::StatusCode::OK, "{path}: {body}");
        body
    }

    /// The print the scripted job is, once the server has adopted it.
    async fn active(&self) -> String {
        let deadline = tokio::time::Instant::now() + PATIENCE;
        loop {
            if let Some(active) = self.get("/prints").await["active"].as_str() {
                return active.to_owned();
            }
            assert!(tokio::time::Instant::now() < deadline, "no job was adopted");
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
    }

    /// The print's events of one kind, oldest first.
    async fn history(&self, print_id: &str, kind: &str) -> Vec<Value> {
        let mut found: Vec<Value> = self
            .get(&format!("/prints/{print_id}/history?limit=1000"))
            .await["events"]
            .as_array()
            .expect("a history answers its events")
            .iter()
            .filter(|event| event["kind"] == json!(kind))
            .cloned()
            .collect();
        found.reverse();
        found
    }

    /// Wait until `count` turns on the print have ended, answered or failed.
    async fn turns_ended(&self, print_id: &str, count: usize) {
        let deadline = tokio::time::Instant::now() + PATIENCE;
        loop {
            let ended = self.history(print_id, "agent_assessment").await.len()
                + self
                    .history(print_id, "port_failure")
                    .await
                    .iter()
                    .filter(|failure| failure["payload"]["site"] == "supervision_turn")
                    .count();
            if ended >= count {
                return;
            }
            if tokio::time::Instant::now() >= deadline {
                let history = self
                    .get(&format!("/prints/{print_id}/history?limit=1000"))
                    .await;
                let events = self.get("/events?limit=50").await;
                panic!("{ended} of {count} turns ended: {history}; every event: {events}");
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
    }

    /// Post one failure alert about the running job, carrying the camera's
    /// frame, as Obico does.
    async fn alert(&self) {
        let mut alert = crate::world::failure_alert(4211, &self.camera.url());
        alert["print"]["filename"] = json!(FILE);
        let status = reqwest::Client::new()
            .post(format!("{}?token={SECRET}", self.server.ingress_url()))
            .header(reqwest::header::CONTENT_TYPE, "application/json")
            .body(alert.to_string())
            .send()
            .await
            .expect("the ingress answers")
            .status();
        assert_eq!(status, reqwest::StatusCode::ACCEPTED);
    }

    /// What the harness process of each run was started with and handed, in
    /// order.
    fn seen(&self) -> Vec<Value> {
        std::fs::read_to_string(self.root.path().join("seen.json.lines"))
            .unwrap_or_default()
            .lines()
            .map(|line| printobserver_types::serde_json::from_str(line).expect("a document"))
            .collect()
    }

    /// Every context the responder read, in order.
    fn contexts_read(&self) -> Vec<Value> {
        std::fs::read_to_string(self.root.path().join("responder.log"))
            .unwrap_or_default()
            .lines()
            .filter_map(|line| printobserver_types::serde_json::from_str::<Value>(line).ok())
            .filter(|done| done["operation"] == "context")
            .map(|done| done["body"]["context"].clone())
            .collect()
    }
}

/// The prompt and the image the harness process was handed on its standard
/// input, as `OneHarness`'s stream-json user frame carries them.
fn handed(seen: &Value) -> (String, Vec<u8>) {
    use base64::Engine as _;

    let stdin = seen["stdin"]
        .as_str()
        .expect("the harness was handed input");
    let frame: Value =
        printobserver_types::serde_json::from_str(stdin.lines().next().expect("one frame"))
            .unwrap_or_else(|error| panic!("the input is no stream-json frame ({error}): {stdin}"));
    let blocks = frame["message"]["content"]
        .as_array()
        .expect("the frame carries content");
    let text = blocks
        .iter()
        .find_map(|block| block["text"].as_str())
        .expect("the frame carries the prompt")
        .to_owned();
    let images: Vec<&Value> = blocks
        .iter()
        .filter(|block| block["type"] == "image")
        .collect();
    assert_eq!(
        images.len(),
        1,
        "a check was handed {} images",
        images.len()
    );
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(
            images[0]["source"]["data"]
                .as_str()
                .expect("the image carries its bytes"),
        )
        .expect("the image's bytes are base64");
    (text, bytes)
}

/// The facts a prompt carries, read from its own section.
fn facts_in(prompt: &str) -> Value {
    let (_, after) = prompt
        .split_once("## The print's facts")
        .unwrap_or_else(|| panic!("the prompt carries no facts:\n{prompt}"));
    let (_, block) = after
        .split_once("```json\n")
        .expect("the facts are a block");
    let (facts, _) = block.split_once("\n```").expect("the block closes");
    printobserver_types::serde_json::from_str(facts).expect("the facts are JSON")
}

/// Hold every field of `facts`, at every depth, to a field `answer` carries:
/// the facts add back nothing the reviewed context answer leaves out.
fn adds_nothing_to(facts: &Value, answer: &Value, at: &str) {
    match (facts, answer) {
        (Value::Object(fields), Value::Object(answered)) => {
            for (name, value) in fields {
                let carried = answered.get(name).unwrap_or_else(|| {
                    panic!("the facts carry `{at}.{name}`, which the context answer does not")
                });
                adds_nothing_to(value, carried, &format!("{at}.{name}"));
            }
        }
        (Value::Array(items), Value::Array(answered)) => {
            for (index, item) in items.iter().enumerate() {
                // An event is matched by its identifier, because a later read
                // carries newer events ahead of those the facts were cut to.
                let carried = match item.get("id") {
                    Some(id) => answered
                        .iter()
                        .find(|carried| carried.get("id") == Some(id)),
                    None => answered.get(index),
                }
                .unwrap_or_else(|| {
                    panic!("the facts carry `{at}[{index}]`, which the context answer does not")
                });
                adds_nothing_to(item, carried, &format!("{at}[{index}]"));
            }
        }
        _ => {}
    }
}

/// One check's run: what the port built, what the harness process was started
/// with and handed, and the context it read.
struct Run {
    request: RunRequest,
    seen: Value,
    read: Value,
}

impl Lean {
    /// Every run so far, in order, that has been built, started and has read
    /// its context.
    fn runs(&self) -> Vec<Run> {
        let requests = self.requests.all();
        let seen = self.seen();
        let read = self.contexts_read();
        // Checks go on while a journey reads, so a run still under way — built
        // but not yet started, or started but not yet read — is not one yet.
        requests
            .into_iter()
            .zip(seen)
            .zip(read)
            .map(|((request, seen), read)| Run {
                request,
                seen,
                read,
            })
            .collect()
    }
}

/// The run of the check on an event of one kind.
fn check_on<'runs>(runs: &'runs [Run], kind: &str) -> &'runs Run {
    runs.iter()
        .find(|run| {
            handed(&run.seen)
                .0
                .contains(&format!("\"kind\": \"{kind}\""))
        })
        .unwrap_or_else(|| panic!("no check ran on a {kind}"))
}

/// One check's run, from what the port built and the harness was handed:
/// lean, carrying the facts the context answered and no frame path, and shown
/// the whole frame scaled.
fn is_a_lean_check(run: &Run, installer: &str, stored_frames: &[String]) {
    let Run {
        request,
        seen,
        read,
    } = run;
    assert_eq!(
        request.tool_set,
        Some(vec![ToolClass::Shell, ToolClass::Read]),
        "{installer}"
    );
    assert_eq!(request.load_skills, Some(false), "{installer}");
    assert_eq!(request.load_mcp, Some(false), "{installer}");
    assert!(
        !request
            .passthrough
            .iter()
            .any(|argument| argument == "--tools"),
        "{installer}: the port passed a tool list of its own: {:?}",
        request.passthrough
    );
    let arguments: Vec<&str> = seen["arguments"]
        .as_array()
        .expect("the arguments")
        .iter()
        .filter_map(Value::as_str)
        .collect();
    for expected in [
        ["--tools", "Bash", "Read"].as_slice(),
        ["--disable-slash-commands"].as_slice(),
        ["--strict-mcp-config"].as_slice(),
    ] {
        assert!(
            arguments
                .windows(expected.len())
                .any(|window| window == expected),
            "{installer}: the harness was not started with {expected:?}: {arguments:?}"
        );
    }

    let (prompt, image) = handed(seen);
    is_the_whole_frame_scaled(&image, &format!("{installer}'s check image"));
    for stored in stored_frames {
        assert!(
            !prompt.contains(stored.as_str()),
            "{installer}: the prompt names the frame's path {stored}"
        );
    }
    assert!(
        !prompt.contains(".jpg") && !prompt.contains("check-images"),
        "{installer}: the prompt names an image file:\n{prompt}"
    );

    let facts = facts_in(&prompt);
    let events = facts["recent_events"]
        .as_array()
        .expect("the facts carry their events");
    assert!(
        !events
            .iter()
            .any(|event| event["kind"] == "agent_assessment"),
        "{installer}: the facts carry events from before the agent's last answer: {facts}"
    );
    adds_nothing_to(&facts, read, "facts");
    for field in ["layer", "z_mm", "types", "final_z_mm", "minutes_left"] {
        assert!(
            !facts["slicer"][field].is_null(),
            "{installer}: the facts carry no slicer {field}: {facts}"
        );
        assert_eq!(
            facts["slicer"][field], read["slicer"][field],
            "{installer}: the facts' {field} is not the context answer's"
        );
    }
    assert_eq!(
        facts["detector"], read["detector"],
        "{installer}: the facts' detector status is not the context answer's"
    );
}

/// Each install template configures a server whose every check — the
/// scheduled one and an alert's — is a lean run: the shell and file reading
/// alone, no skills and no MCP, translated by `OneHarness` for the harness
/// process; whose prompt carries the print's facts as the context answers them
/// and names no frame; and whose one image is the whole camera frame scaled to
/// 1280x720. A look at the printer answers the frame alike, and the operator's
/// image read serves the stored frame at the camera's size.
#[tokio::test(flavor = "multi_thread")]
async fn every_check_is_lean_and_shown_the_whole_frame_scaled() {
    for installer in INSTALLERS {
        let lean = Lean::open(
            installer,
            1,
            &[answering("continue", "the walls are clean")],
            &json!([{ "operation": "context", "body": {} }]),
        )
        .await;
        let print_id = lean.active().await;
        lean.turns_ended(&print_id, 1).await;
        lean.alert().await;
        let deadline = tokio::time::Instant::now() + PATIENCE;
        while lean
            .history(&print_id, "obico_failure_alert")
            .await
            .is_empty()
            || lean.runs().len() < 2
        {
            assert!(
                tokio::time::Instant::now() < deadline,
                "{installer}: the alert was never checked"
            );
            tokio::time::sleep(Duration::from_millis(100)).await;
        }

        let mut frames = Vec::new();
        for kind in ["periodic_observation", "obico_failure_alert"] {
            for event in lean.history(&print_id, kind).await {
                let Some(id) = event["image"]["id"].as_str() else {
                    continue;
                };
                let answer = lean.get(&format!("/images/{id}")).await;
                frames.push(answer["path"].as_str().expect("a stored path").to_owned());
            }
        }
        assert!(!frames.is_empty(), "{installer}: no check stored a frame");
        for stored in &frames {
            let size = image::ImageReader::new(Cursor::new(
                std::fs::read(stored).expect("the stored frame reads"),
            ))
            .with_guessed_format()
            .expect("a format")
            .into_dimensions()
            .expect("dimensions");
            assert_eq!(
                size, CAMERA,
                "{installer}: a stored frame is not the camera's size"
            );
        }
        let runs = lean.runs();
        is_a_lean_check(check_on(&runs, "periodic_observation"), installer, &frames);
        is_a_lean_check(check_on(&runs, "obico_failure_alert"), installer, &frames);
        lean.server.stop().await;
    }
}

/// A look with no print named answers the whole frame scaled, as every check is
/// shown it, while the operator's image read serves the frame the camera gave.
#[tokio::test(flavor = "multi_thread")]
async fn a_look_answers_the_whole_frame_scaled_and_the_image_read_the_stored_one() {
    let lean = Lean::open(
        INSTALLERS[0],
        3_600,
        &[answering("continue", "fine")],
        &json!([]),
    )
    .await;
    let look = lean.get("/look").await;
    let path = look["image_path"]
        .as_str()
        .unwrap_or_else(|| panic!("the look answers no frame: {look}"));
    is_the_whole_frame_scaled(
        &std::fs::read(path).expect("the look's frame reads"),
        "the printer look",
    );
    let frame = look["frame"]["id"]
        .as_str()
        .expect("the look's frame record");
    let image = lean.get(&format!("/images/{frame}")).await;
    let stored = std::fs::read(image["path"].as_str().expect("a stored path"))
        .expect("the stored frame reads");
    assert_eq!(
        stored,
        marked_frame(),
        "the image read is not the camera's frame"
    );
    lean.server.stop().await;
}

/// The acknowledgements of one print, oldest first, as `(event, disposition,
/// reason, actor)`.
async fn acknowledgements(lean: &Lean, print_id: &str) -> Vec<(String, String, String, Value)> {
    lean.history(print_id, "action_requested")
        .await
        .iter()
        .map(|event| &event["payload"]["action"])
        .filter(|action| action["action"] == "acknowledge_failure")
        .map(|action| {
            (
                action["event_id"].as_str().unwrap_or_default().to_owned(),
                action["disposition"]
                    .as_str()
                    .unwrap_or_default()
                    .to_owned(),
                action["reason"].as_str().unwrap_or_default().to_owned(),
                action["actor"].clone(),
            )
        })
        .collect()
}

/// The acknowledgements of one print once there are at least `count`.
///
/// A turn's assessment is written before the acknowledgement its answer
/// makes, so a turn having ended does not yet say its acknowledgement is there.
async fn acknowledgements_reaching(
    lean: &Lean,
    print_id: &str,
    count: usize,
) -> Vec<(String, String, String, Value)> {
    let deadline = tokio::time::Instant::now() + PATIENCE;
    loop {
        let acknowledged = acknowledgements(lean, print_id).await;
        if acknowledged.len() >= count {
            return acknowledged;
        }
        assert!(
            tokio::time::Instant::now() < deadline,
            "{} of {count} acknowledgements were made: {acknowledged:?}",
            acknowledged.len()
        );
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
}

/// The alerts each turn was about, oldest first.
async fn checked_events(lean: &Lean, print_id: &str) -> Vec<String> {
    lean.history(print_id, "obico_failure_alert")
        .await
        .iter()
        .filter_map(|event| event["id"].as_str())
        .map(ToOwned::to_owned)
        .collect()
}

/// A turn that runs no command is acknowledged from its answer, for each
/// disposition: its event, the answer's disposition and `why`, and the agent in
/// the turn's own session.
#[tokio::test(flavor = "multi_thread")]
async fn a_quiet_turn_is_acknowledged_from_its_answer() {
    let dispositions = ["continue", "watch", "stop"];
    let answers: Vec<Value> = dispositions
        .iter()
        .map(|disposition| answering(disposition, &format!("judged it {disposition}")))
        .collect();
    let lean = Lean::open(INSTALLERS[0], 3_600, &answers, &json!([])).await;
    let print_id = lean.active().await;
    for count in 1..=dispositions.len() {
        lean.alert().await;
        lean.turns_ended(&print_id, count).await;
    }

    let events = checked_events(&lean, &print_id).await;
    let acknowledged = acknowledgements_reaching(&lean, &print_id, dispositions.len()).await;
    assert_eq!(acknowledged.len(), dispositions.len(), "{acknowledged:?}");
    let session = json!({ "agent": { "session_name": format!("print-{print_id}") } });
    for ((event, disposition), (acked, given, reason, actor)) in
        events.iter().zip(dispositions).zip(&acknowledged)
    {
        assert_eq!(
            acked, event,
            "an acknowledgement is not of its turn's event"
        );
        assert_eq!(given, disposition);
        assert_eq!(reason, &format!("judged it {disposition}"));
        assert_eq!(
            actor, &session,
            "the acknowledgement is not the turn's agent's"
        );
    }
    lean.server.stop().await;
}

/// A person's request taken while a turn runs, and so recorded before that
/// turn's answer, is in the next turn's facts, which leave out the look that
/// turn took itself.
#[tokio::test(flavor = "multi_thread")]
async fn a_persons_request_during_a_turn_reaches_the_next_turns_facts() {
    let lean = Lean::open(
        INSTALLERS[0],
        3_600,
        &[answering("watch", "judged it")],
        &json!([{ "operation": "look", "body": {} }]),
    )
    .await;
    let print_id = lean.active().await;
    let hold = lean.root.path().join("hold");
    std::fs::write(&hold, b"").expect("the hold is writable");
    lean.alert().await;
    let deadline = tokio::time::Instant::now() + PATIENCE;
    while lean.seen().is_empty() {
        assert!(tokio::time::Instant::now() < deadline, "no turn started");
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    let path = printobserver_server::operation("set_fan_percent")
        .expect("`set_fan_percent` is served")
        .path
        .replace("{print_id}", &print_id);
    let asked = lean
        .client
        .post(format!("{}{path}", lean.server.base_url()))
        .json(&json!({
            "percent": 80.0,
            "reason": "a person turned the fan up",
            "actor": "operator",
        }))
        .send()
        .await
        .expect("the server answers");
    assert!(asked.status().is_success(), "{}", asked.status());
    std::fs::remove_file(&hold).expect("the hold is removable");
    lean.turns_ended(&print_id, 1).await;

    let persons = |event: &Value| {
        event["kind"] == "action_requested" && event["payload"]["actor"] == "operator"
    };
    let requested = lean.history(&print_id, "action_requested").await;
    let person = requested
        .iter()
        .find(|event| persons(event))
        .expect("the person's request is recorded");
    let answer = &lean.history(&print_id, "agent_assessment").await[0];
    assert!(
        person["received_at"].as_str() < answer["received_at"].as_str(),
        "the request was not taken during the turn: {person} {answer}"
    );
    assert!(
        !lean.history(&print_id, "camera_look").await.is_empty(),
        "the turn took no look"
    );

    lean.alert().await;
    lean.turns_ended(&print_id, 2).await;
    let (prompt, _) = handed(&lean.seen()[1]);
    let facts = facts_in(&prompt);
    let events = facts["recent_events"]
        .as_array()
        .expect("the facts carry their events");
    assert!(
        events.iter().any(persons),
        "the person's request is not in the next turn's facts: {facts}"
    );
    for left_out in ["camera_look", "agent_assessment"] {
        assert!(
            !events.iter().any(|event| event["kind"] == left_out),
            "the facts carry a `{left_out}` the agent already saw: {facts}"
        );
    }
    lean.server.stop().await;
}

/// A turn that acknowledged its event by command leaves that acknowledgement
/// alone, and its answer adds none.
#[tokio::test(flavor = "multi_thread")]
async fn a_turn_that_acknowledged_by_command_leaves_only_its_own() {
    let lean = Lean::open(
        INSTALLERS[0],
        3_600,
        &[answering("stop", "the answer's own reason")],
        &json!([
            { "operation": "look", "body": {} },
            {
                "operation": "acknowledge_failure",
                "body": {
                    "event_id": "{event_id}",
                    "disposition": "watch",
                    "reason": "acknowledged by command",
                },
            },
        ]),
    )
    .await;
    let print_id = lean.active().await;
    // A print's turns run one at a time, so once a second turn has ended the
    // first one's record is whole, and an acknowledgement its answer made
    // late would be there to see.
    for count in 1..=2 {
        lean.alert().await;
        lean.turns_ended(&print_id, count).await;
    }

    let events = checked_events(&lean, &print_id).await;
    let first: Vec<_> = acknowledgements(&lean, &print_id)
        .await
        .into_iter()
        .filter(|(event, ..)| *event == events[0])
        .collect();
    assert_eq!(first.len(), 1, "{first:?}");
    assert_eq!(first[0].1, "watch");
    assert_eq!(first[0].2, "acknowledged by command");
    lean.server.stop().await;
}

/// An answer with no valid disposition is refused: the turn is recorded as
/// failed naming the field, and nothing is acknowledged.
#[tokio::test(flavor = "multi_thread")]
async fn an_answer_without_a_valid_disposition_is_refused() {
    let mut invalid = answering("continue", "fine");
    invalid["disposition"] = json!("carry on");
    let lean = Lean::open(INSTALLERS[0], 3_600, &[invalid], &json!([])).await;
    let print_id = lean.active().await;
    lean.alert().await;
    lean.turns_ended(&print_id, 1).await;

    let failures: Vec<Value> = lean
        .history(&print_id, "port_failure")
        .await
        .into_iter()
        .filter(|failure| failure["payload"]["site"] == "supervision_turn")
        .collect();
    assert_eq!(failures.len(), 1, "{failures:?}");
    let detail = failures[0]["payload"]["detail"]
        .as_str()
        .unwrap_or_default();
    assert!(detail.contains("disposition"), "{detail}");
    assert!(lean.history(&print_id, "agent_assessment").await.is_empty());
    assert!(acknowledgements(&lean, &print_id).await.is_empty());
    lean.server.stop().await;
}

/// A camera frame that already fits a check is shown as stored: a look answers
/// the stored frame's own path, holding the camera's bytes, and makes no copy.
#[tokio::test(flavor = "multi_thread")]
async fn a_frame_that_already_fits_is_shown_as_stored() {
    let fitting = marked_frame_of(CHECK);
    let lean = Lean::open_with_frame(
        INSTALLERS[0],
        3_600,
        &[answering("continue", "fine")],
        &json!([]),
        fitting.clone(),
    )
    .await;
    let look = lean.get("/look").await;
    let shown = look["image_path"]
        .as_str()
        .unwrap_or_else(|| panic!("the look answers no frame: {look}"));
    let frame = look["frame"]["id"]
        .as_str()
        .expect("the look's frame record");
    let stored = lean.get(&format!("/images/{frame}")).await;
    assert_eq!(
        Some(shown),
        stored["path"].as_str(),
        "a fitting frame was copied"
    );
    assert_eq!(std::fs::read(shown).expect("the frame reads"), fitting);
    lean.server.stop().await;
}

/// Whether a decoded image carries the red mark in each of its four corners.
fn marked_in_every_corner(image: &image::RgbImage) -> bool {
    let (width, height) = image.dimensions();
    let inset = 10;
    [
        (inset, inset),
        (width - inset, inset),
        (inset, height - inset),
        (width - inset, height - inset),
    ]
    .into_iter()
    .all(|(x, y)| red(*image.get_pixel(x, y)))
}

/// A frame of another shape than a check's is scaled whole to fit it, its
/// shape kept and every corner in the image; a second check of the same stored
/// frame is shown the image the first made, without making it again.
#[test]
fn a_frame_of_any_shape_is_scaled_whole_once_and_reused() {
    use printobserver_core::CheckImages as _;

    let root = TempDir::new().expect("a journey's own root");
    let stored = root.path().join("images").join("01").join("digest");
    std::fs::create_dir_all(stored.parent().expect("a directory")).expect("the image directory");
    std::fs::write(&stored, marked_frame_of((2400, 1800))).expect("the stored frame");
    let images = printobserver_server::WholeFrames::under(root.path());

    let shown = images
        .check_image(None, &stored)
        .expect("a check image is made");
    assert_ne!(
        shown, stored,
        "a frame larger than a check's was not scaled"
    );
    let decoded = image::load_from_memory(&std::fs::read(&shown).expect("the image reads"))
        .expect("the check image decodes")
        .to_rgb8();
    assert_eq!(
        decoded.dimensions(),
        (960, 720),
        "the frame's shape was not kept"
    );
    assert!(marked_in_every_corner(&decoded), "the frame was cropped");

    // What a later check is shown is the image already made, read as it is.
    std::fs::write(&shown, b"made once").expect("the made image is writable");
    let again = images
        .check_image(None, &stored)
        .expect("the image is shown again");
    assert_eq!(again, shown);
    assert_eq!(std::fs::read(&again).expect("it reads"), b"made once");
}

/// A frame that cannot be read, or a check image that cannot be written, is
/// refused naming which: what core then shows the check is the stored frame.
#[test]
fn a_check_image_that_cannot_be_made_is_refused_naming_why() {
    use printobserver_core::CheckImages as _;

    let root = TempDir::new().expect("a journey's own root");
    let images = printobserver_server::WholeFrames::under(root.path());
    let missing = root.path().join("images").join("02").join("gone");
    let refused = images
        .check_image(None, &missing)
        .expect_err("a frame that is not there was made into an image");
    assert!(refused.contains("is unreadable"), "{refused}");

    let stored = root.path().join("frame.jpg");
    std::fs::write(&stored, marked_frame_of(CAMERA)).expect("the stored frame");
    std::fs::write(
        root.path()
            .join(printobserver_server::CHECK_IMAGES_DIRECTORY),
        b"a file where the directory goes",
    )
    .expect("the blocking file");
    let refused = images
        .check_image(None, &stored)
        .expect_err("an image was written where no directory can be");
    assert!(refused.contains("cannot be"), "{refused}");
}

/// A server whose check images cannot be written shows a look the stored
/// frame, at the camera's size, rather than nothing.
#[tokio::test(flavor = "multi_thread")]
async fn a_look_whose_check_image_cannot_be_made_is_shown_the_stored_frame() {
    let lean = Lean::open(
        INSTALLERS[0],
        3_600,
        &[answering("continue", "fine")],
        &json!([]),
    )
    .await;
    std::fs::write(
        lean.root
            .path()
            .join("state")
            .join(printobserver_server::CHECK_IMAGES_DIRECTORY),
        b"a file where the directory goes",
    )
    .expect("the blocking file");
    let look = lean.get("/look").await;
    let shown = look["image_path"]
        .as_str()
        .unwrap_or_else(|| panic!("the look answers no frame: {look}"));
    let frame = look["frame"]["id"]
        .as_str()
        .expect("the look's frame record");
    let stored = lean.get(&format!("/images/{frame}")).await;
    assert_eq!(Some(shown), stored["path"].as_str());
    assert_eq!(
        std::fs::read(shown).expect("the frame reads"),
        marked_frame()
    );
    lean.server.stop().await;
}

/// The probe, run with arguments it cannot read, says which on its error stream,
/// prints nothing on its output and exits 2; one whose configuration cannot be
/// loaded exits 1, naming why.
#[test]
fn the_probe_refuses_what_it_cannot_run_naming_why() {
    let root = TempDir::new().expect("a journey's own root");
    let probe = |arguments: &[&str]| {
        std::process::Command::new(env!("CARGO_BIN_EXE_printobserver-server-probe"))
            .args(arguments)
            .output()
            .expect("the probe runs")
    };
    let missing = root.path().join("absent.toml");
    let missing = missing.to_str().expect("a path");
    for (arguments, said) in [
        (vec![], "--config is required"),
        (vec!["--config", missing], "--session is required"),
        (
            vec!["--config", missing, "--session", "s", "--colour", "red"],
            "is no option",
        ),
        (
            vec!["--config", missing, "--config", missing],
            "is given twice",
        ),
        (vec!["--config", missing, "--session", ""], "takes a value"),
    ] {
        let done = probe(&arguments);
        assert_eq!(done.status.code(), Some(2), "{arguments:?}");
        assert!(done.stdout.is_empty(), "{arguments:?} printed an answer");
        let stderr = String::from_utf8_lossy(&done.stderr);
        assert!(stderr.contains(said), "{arguments:?}: {stderr}");
    }
    let done = probe(&["--config", missing, "--session", "s", "--prompt", "p"]);
    assert_eq!(done.status.code(), Some(1));
    assert!(done.stdout.is_empty());
    assert!(String::from_utf8_lossy(&done.stderr).contains("absent.toml"));

    // A usage file it cannot read stops it before it reads any configuration.
    let unread = root.path().join("no-usage.txt");
    let unread = unread.to_str().expect("a path");
    let done = probe(&[
        "--config",
        missing,
        "--session",
        "s",
        "--prompt",
        "p",
        "--usage",
        unread,
    ]);
    assert_eq!(done.status.code(), Some(1));
    assert!(done.stdout.is_empty(), "it printed an answer");
    let stderr = String::from_utf8_lossy(&done.stderr);
    assert!(
        stderr.contains("no-usage.txt is unreadable") && !stderr.contains("absent.toml"),
        "{stderr}"
    );
}

/// A responder handed standard input it cannot read whole stops saying so,
/// rather than acting on part of a prompt.
#[test]
fn the_responder_refuses_input_it_cannot_read() {
    use std::io::Write as _;

    let mut responder =
        std::process::Command::new(env!("CARGO_BIN_EXE_printobserver-server-responder"))
            .args(["--input-format", "stream-json"])
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .spawn()
            .expect("the responder starts");
    responder
        .stdin
        .take()
        .expect("its standard input is piped")
        .write_all(&[0xff, 0xfe, 0x00])
        .expect("the input is written");
    let done = responder.wait_with_output().expect("the responder ends");
    assert_eq!(done.status.code(), Some(1));
    assert!(done.stdout.is_empty(), "it answered");
    let stderr = String::from_utf8_lossy(&done.stderr);
    assert!(
        stderr.contains("its standard input could not be read"),
        "{stderr}"
    );
}

/// A check image that cannot be written into its directory is refused naming
/// why, and leaves no partial image behind.
#[cfg(unix)]
#[test]
fn a_check_image_that_cannot_be_written_is_refused_and_leaves_nothing() {
    use std::os::unix::fs::PermissionsExt as _;

    use printobserver_core::CheckImages as _;

    let root = TempDir::new().expect("a journey's own root");
    let stored = root.path().join("frame.jpg");
    std::fs::write(&stored, marked_frame_of(CAMERA)).expect("the stored frame");
    let directory = root
        .path()
        .join(printobserver_server::CHECK_IMAGES_DIRECTORY);
    std::fs::create_dir_all(&directory).expect("the check images' directory");
    std::fs::set_permissions(&directory, std::fs::Permissions::from_mode(0o555))
        .expect("the directory is made read-only");

    let refused = printobserver_server::WholeFrames::under(root.path())
        .check_image(None, &stored)
        .expect_err("an image was written into a read-only directory");
    std::fs::set_permissions(&directory, std::fs::Permissions::from_mode(0o755))
        .expect("the directory is made writable again");
    assert!(refused.contains("cannot be written"), "{refused}");
    assert_eq!(
        std::fs::read_dir(&directory)
            .expect("the directory reads")
            .count(),
        0,
        "a partial image was left behind"
    );
}

/// A check image written and then refused its place is refused naming why, and
/// what was written beside it is taken away rather than left for a reader.
#[test]
fn a_check_image_refused_its_place_leaves_no_partial_image() {
    use printobserver_core::CheckImages as _;

    let root = TempDir::new().expect("a journey's own root");
    let stored = root.path().join("01").join("digest");
    std::fs::create_dir_all(stored.parent().expect("a directory")).expect("the image directory");
    std::fs::write(&stored, marked_frame_of(CAMERA)).expect("the stored frame");
    // A directory, not empty, where the scaled image would be put.
    let directory = root
        .path()
        .join(printobserver_server::CHECK_IMAGES_DIRECTORY);
    let taken = directory.join(format!("01-digest-{}x{}.jpg", CHECK.0, CHECK.1));
    std::fs::create_dir_all(taken.join("occupied")).expect("the occupied place");

    let refused = printobserver_server::WholeFrames::under(root.path())
        .check_image(None, &stored)
        .expect_err("an image was put where a directory stands");
    assert!(refused.contains("cannot be written"), "{refused}");
    let left: Vec<String> = std::fs::read_dir(&directory)
        .expect("the directory reads")
        .map(|entry| {
            entry
                .expect("an entry")
                .file_name()
                .to_string_lossy()
                .into_owned()
        })
        .collect();
    assert_eq!(
        left,
        [format!("01-digest-{}x{}.jpg", CHECK.0, CHECK.1)],
        "{left:?}"
    );
}
