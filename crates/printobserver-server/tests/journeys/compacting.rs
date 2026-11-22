//! A print's session is compacted while the supervisor is idle, and a new
//! event always wins over a compaction.
//!
//! The server here is the real composition, configured from each committed
//! install template, with the real supervisor port and `OneHarness` engine and
//! only the paid provider process replaced by this crate's responder over
//! `OneHarness`'s published mock harness. Each scripted turn reports its model
//! calls' usage as Claude Code streams it, and a compaction answers Claude
//! Code's own `/compact` stream. What happened is read off the print's ledger,
//! the run requests the port built, and what each harness process recorded as
//! it started.

use core::num::NonZeroU64;
use core::time::Duration;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use oneharness_core::domain::lean::CLAUDE_WINDOW_RULE;
use printobserver_core::store::Stores;
use printobserver_obico::{ObicoVision, ObicoVisionConfig};
use printobserver_octoprint::OctoPrintPrinter;
use printobserver_octoprint::standin::{JobScript, ScriptedOctoPrint};
use printobserver_oneharness::{
    CompactionEnd, EnvAssignment, OneharnessSupervisor, RecordedCompaction, RecordedTurn,
    RunRequestObserver, SESSIONS_DIRECTORY, TurnSeam, TurnTimeout,
};
use printobserver_server::{Ports, Running, Server, ServerConfig, agent_config};
use printobserver_store_sqlite::SqliteStore;
use printobserver_types::serde_json::{Value, json};
use printobserver_types::{PrintId, Timestamp};
use tempfile::TempDir;

use crate::installed_detector::INSTALLERS;
use crate::lean_checks::{Requests, answering, configured, copy_tree, marked_frame};
use crate::world::{OPERATOR, SECRET, committed_skill, presenting};

/// The real slice the scripted instance serves.
const SLICE: &[u8] = include_bytes!("../fixtures/real-prints/bubbler-guard-and-pad.gcode");

/// The name it is held under.
const FILE: &str = "bubbler-guard-and-pad.gcode";

/// How long a journey waits for something the server does on its own clock.
const PATIENCE: Duration = Duration::from_secs(90);

/// What every install template sets the harness's own compaction to.
const AUTO_COMPACT: u64 = 200_000;

/// What every install template sets the idle compaction to.
const IDLE_COMPACT: u64 = 150_000;

/// The session id the scripted turns answer under.
const SESSION: &str = "SID-COMPACT";

/// The session id a completed compaction's fork answers under.
const FORK: &str = "SID-COMPACT-FORK";

/// The most an arriving event's turn may be held up by cancelling a running
/// compaction: from the event's arrival to its turn's start.
const CANCEL_BOUND: Duration = Duration::from_secs(5);

/// How long after `from` a turn started, by the ledger's own clock.
fn after(from: Timestamp, turn: &RecordedTurn) -> Duration {
    let started = turn
        .started_at
        .expect("a turn this build recorded has a start");
    (*started.as_utc() - *from.as_utc())
        .to_std()
        .expect("the turn started after the event arrived")
}

/// The context size the scripted compaction reports it left.
const AFTER: u64 = 12_000;

/// One scripted model call's usage: input, cache written, cache read, output.
type Usage = (u64, u64, u64, u64);

/// A turn whose one call leaves a conversation of `size` tokens.
const fn sized(size: u64) -> Usage {
    (1, 999, size - 1_100, 100)
}

/// One harness invocation the responder is scripted for, in order.
enum Step {
    /// A supervision turn answering `continue`, its one call of this usage.
    Turn(Usage),
    /// A compaction that completes.
    Compaction,
}

/// A turn's stream-json output: one model call with this usage, then the
/// result carrying a quiet answer.
fn turn_stream(usage: Usage) -> String {
    let (input, written, read, output) = usage;
    [
        json!({
            "type": "assistant",
            "session_id": SESSION,
            "message": {
                "id": "msg_0",
                "model": "claude-test",
                "content": [{ "type": "text", "text": "judging the frame" }],
                "usage": {
                    "input_tokens": input,
                    "cache_creation_input_tokens": written,
                    "cache_read_input_tokens": read,
                    "output_tokens": output,
                },
            },
        }),
        json!({
            "type": "result",
            "subtype": "success",
            "is_error": false,
            "result": answering("continue", "the walls are clean").to_string(),
            "session_id": SESSION,
        }),
    ]
    .map(|line| line.to_string())
    .join("\n")
}

/// Claude Code's `/compact` stream, compacting a fork of `before` tokens.
fn compaction_stream(before: u64) -> String {
    [
        json!({ "type": "system", "subtype": "status", "status": "compacting", "session_id": FORK }),
        json!({
            "type": "system", "subtype": "status", "status": null,
            "compact_result": "success", "session_id": FORK,
        }),
        json!({
            "type": "system", "subtype": "compact_boundary", "session_id": FORK,
            "compact_metadata": { "trigger": "manual", "pre_tokens": before, "post_tokens": AFTER },
        }),
        json!({
            "type": "result", "subtype": "success", "is_error": false, "result": "",
            "session_id": FORK,
            "model_usage_since_last_result": {
                "claude-test": {
                    "inputTokens": 5, "outputTokens": 2_000,
                    "cacheReadInputTokens": before, "cacheCreationInputTokens": 0,
                    "costUSD": 0.1,
                },
            },
        }),
    ]
    .map(|line| line.to_string())
    .join("\n")
}

/// The environment `OneHarness` hands Claude Code for `auto_compact_tokens`.
fn translated(tokens: u64) -> Vec<(String, String)> {
    CLAUDE_WINDOW_RULE.environment(tokens).0
}

/// One server configured from one installer's template, and what watches it.
struct Compacting {
    root: TempDir,
    server: Running,
    octoprint: ScriptedOctoPrint,
    camera: crate::http_host::Host,
    agent: Arc<OneharnessSupervisor>,
    requests: Arc<Requests>,
    client: reqwest::Client,
}

impl Compacting {
    /// A server configured from `installer`'s template, as `edit` leaves it,
    /// whose harness answers `steps` in order and quietly from then on.
    async fn open(installer: &str, edit: fn(&mut toml::Value), steps: &[Step]) -> Self {
        Self::open_as(installer, edit, steps, None, Harness::AsPublished).await
    }

    /// The same, each turn and compaction given `timeout` when one is named.
    async fn open_timed(
        installer: &str,
        edit: fn(&mut toml::Value),
        steps: &[Step],
        timeout: Option<TurnTimeout>,
    ) -> Self {
        Self::open_as(installer, edit, steps, timeout, Harness::AsPublished).await
    }

    /// The same, its harness processes started as `harness` says.
    async fn open_as(
        installer: &str,
        edit: fn(&mut toml::Value),
        steps: &[Step],
        timeout: Option<TurnTimeout>,
        harness: Harness,
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
        let camera = crate::http_host::image_host(marked_frame()).await;

        let path = configured(installer, &root, &octoprint, &camera.url(), 3_600);
        let text = std::fs::read_to_string(&path).expect("the configuration reads");
        let mut document: toml::Value = toml::from_str(&text).expect("it is a document");
        edit(&mut document);
        std::fs::write(&path, toml::to_string(&document).expect("it renders"))
            .expect("the configuration is writable");
        let config = ServerConfig::load(&path)
            .unwrap_or_else(|error| panic!("{installer}'s configuration reads: {error}"));

        let (agent, requests) = scripted(&config, root.path(), steps, timeout, harness);
        let agent = Arc::new(agent);
        let server = Server::start_with(
            config.clone(),
            Ports {
                printer: Arc::new(OctoPrintPrinter::new(config.octoprint.clone())),
                stores: Stores::of(Arc::new(
                    SqliteStore::open(&config.state_dir).expect("the store opens"),
                )),
                vision: Arc::new(ObicoVision::new(ObicoVisionConfig::default()).expect("built")),
                agent: Arc::clone(&agent) as Arc<dyn printobserver_supervisor_api::SupervisorPort>,
            },
        )
        .await
        .unwrap_or_else(|error| panic!("{installer}'s server starts: {error}"));
        Self {
            root,
            server,
            octoprint,
            camera,
            agent,
            requests,
            client: presenting(OPERATOR),
        }
    }

    /// A scratch path under this journey's root.
    fn path(&self, name: &str) -> PathBuf {
        self.root.path().join(name)
    }

    /// One JSON read of the versioned surface, as the operator.
    async fn get(&self, path: &str) -> Value {
        let response = self
            .client
            .get(format!("{}{path}", self.server.base_url()))
            .send()
            .await
            .expect("the server answers");
        response.json().await.expect("a JSON answer")
    }

    /// The print the scripted job is, once the server has adopted it.
    async fn active(&self) -> PrintId {
        let deadline = tokio::time::Instant::now() + PATIENCE;
        loop {
            if let Some(active) = self.get("/prints").await["active"].as_str() {
                return active.parse().expect("a print id");
            }
            assert!(tokio::time::Instant::now() < deadline, "no job was adopted");
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
    }

    /// Post one failure alert about the running job, as Obico does.
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

    /// The turns the print's ledger records.
    fn turns(&self, print: &PrintId) -> Vec<RecordedTurn> {
        self.agent.recorded_turns(print).expect("the ledger reads")
    }

    /// The compactions the print's ledger records.
    fn compactions(&self, print: &PrintId) -> Vec<RecordedCompaction> {
        self.agent
            .recorded_compactions(print)
            .expect("the ledger reads")
    }

    /// Wait until `condition` holds of the print's ledger.
    async fn until(&self, what: &str, condition: impl Fn() -> bool) {
        let deadline = tokio::time::Instant::now() + PATIENCE;
        while !condition() {
            assert!(tokio::time::Instant::now() < deadline, "never: {what}");
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
    }

    /// Post an alert and wait until the print's ledger records `count` turns.
    async fn turn(&self, print: &PrintId, count: usize) {
        self.alert().await;
        self.until(&format!("{count} turns ended"), || {
            self.turns(print).len() >= count
        })
        .await;
    }

    /// What each harness process recorded as it started, in order.
    fn seen(&self) -> Vec<Value> {
        std::fs::read_to_string(self.path("seen.json.lines"))
            .unwrap_or_default()
            .lines()
            .map(|line| printobserver_types::serde_json::from_str(line).expect("a document"))
            .collect()
    }

    /// Every compaction the responder was started for, as it began.
    fn compactions_begun(&self) -> Vec<Value> {
        std::fs::read_to_string(self.path("compact.log"))
            .unwrap_or_default()
            .lines()
            .map(|line| printobserver_types::serde_json::from_str(line).expect("a document"))
            .collect()
    }
}

/// The agent composed from `config` as the server composes it, its harness the
/// responder scripted to answer `steps` in order.
fn scripted(
    config: &ServerConfig,
    root: &Path,
    steps: &[Step],
    timeout: Option<TurnTimeout>,
    harness: Harness,
) -> (OneharnessSupervisor, Arc<Requests>) {
    let mut composed = agent_config(config).expect("the agent is composed");
    if let Some(timeout) = timeout {
        composed.turn_timeout = timeout;
    }
    composed.harness_bin = Some(match harness {
        Harness::AsPublished => responder(),
        #[cfg(unix)]
        Harness::IgnoringTerm => ignoring_term(root),
    });
    let watched: Vec<String> = translated(AUTO_COMPACT)
        .into_iter()
        .map(|(name, _)| name)
        .collect();
    let mut env = vec![
        format!("MOCK_ATTEMPT_FILE={}", root.join("attempts").display()),
        format!(
            "PRINTOBSERVER_RESPONDER_SEEN={}",
            root.join("seen.json").display()
        ),
        format!("PRINTOBSERVER_RESPONDER_SEEN_ENV={}", watched.join(",")),
        format!(
            "PRINTOBSERVER_RESPONDER_SEEN_STORE={}",
            config.state_dir.join("harness-sessions").display()
        ),
        format!(
            "PRINTOBSERVER_RESPONDER_COMPACT_LOG={}",
            root.join("compact.log").display()
        ),
        format!(
            "PRINTOBSERVER_RESPONDER_COMPACT_HOLD={}",
            root.join("compact-hold").display()
        ),
        format!("MOCK_STDOUT={}", turn_stream(sized(2_000))),
    ];
    for (index, step) in steps.iter().enumerate() {
        let stdout = match step {
            Step::Turn(usage) => turn_stream(*usage),
            Step::Compaction => compaction_stream(IDLE_COMPACT),
        };
        env.push(format!("MOCK_STDOUT_{}={stdout}", index + 1));
    }
    for assignment in env {
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

/// How a journey's harness processes are started.
#[derive(Clone, Copy)]
enum Harness {
    /// The responder over the published mock harness, as it is: a process
    /// asked to exit with `SIGTERM` does.
    AsPublished,
    /// The same responder started with `SIGTERM` ignored, so a compaction asked
    /// to exit never does, and holds until it is killed.
    #[cfg(unix)]
    IgnoringTerm,
}

/// The responder over the published mock harness.
fn responder() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_printobserver-server-responder"))
}

/// A program under `root` that starts [`responder`] with `SIGTERM` ignored: an
/// ignored signal stays ignored across `exec`, so the responder inherits it
/// from the shell that starts it.
#[cfg(unix)]
fn ignoring_term(root: &Path) -> PathBuf {
    use std::os::unix::fs::PermissionsExt as _;
    let wrapper = root.join("ignoring-term.sh");
    std::fs::write(
        &wrapper,
        format!(
            "#!/bin/sh\ntrap '' TERM\nexec '{}' \"$@\"\n",
            responder().display()
        ),
    )
    .expect("the wrapper is writable");
    std::fs::set_permissions(&wrapper, std::fs::Permissions::from_mode(0o755))
        .expect("the wrapper is executable");
    wrapper
}

/// The configuration as the template writes it.
fn as_written(_: &mut toml::Value) {}

/// The configuration with neither compaction key.
fn without_compaction(document: &mut toml::Value) {
    let supervisor = document["supervisor"]
        .as_table_mut()
        .expect("a supervisor table");
    supervisor.remove("auto_compact_tokens");
    supervisor.remove("idle_compact_tokens");
}

/// The configuration checking every second, and reading the printer as
/// often as it admits.
fn checking_every_second(document: &mut toml::Value) {
    crate::world::set(
        document,
        "supervisor.observation_interval_s",
        toml::Value::Integer(1),
    );
    crate::world::set(
        document,
        "supervisor.printer_poll_s",
        toml::Value::Integer(5),
    );
}

/// The configuration reading the printer as often as it admits.
fn polling_often(document: &mut toml::Value) {
    crate::world::set(
        document,
        "supervisor.printer_poll_s",
        toml::Value::Integer(5),
    );
}

/// Whether a harness process was started for a compaction.
fn is_compaction(seen: &Value) -> bool {
    seen["arguments"]
        .as_array()
        .is_some_and(|arguments| arguments.iter().any(|argument| argument == "/compact"))
}

/// The environment one harness process recorded for the variables
/// `auto_compact_tokens` translates to.
fn compaction_env(seen: &Value) -> Vec<(String, Option<String>)> {
    translated(AUTO_COMPACT)
        .into_iter()
        .map(|(name, _)| {
            let value = seen["env"][&name].as_str().map(ToOwned::to_owned);
            (name, value)
        })
        .collect()
}

/// No turn and compaction of the print overlap: each turn's span, from its
/// start to its end, lies wholly outside every compaction's.
fn none_overlap(turns: &[RecordedTurn], compactions: &[RecordedCompaction]) {
    for turn in turns {
        let started = turn
            .started_at
            .expect("a turn this build recorded has a start");
        for compaction in compactions {
            assert!(
                turn.ran_at <= compaction.started_at || compaction.ended_at <= started,
                "a turn ({started} to {}) overlapped a compaction ({} to {})",
                turn.ran_at,
                compaction.started_at,
                compaction.ended_at
            );
        }
    }
}

/// Every installer's configuration carries both compaction keys, and a server
/// built from it compacts a print's session while idle once a turn leaves it
/// at the idle threshold and not below. Every supervision turn's run request
/// carries the template's `auto_compact_tokens` and its harness process the
/// environment `OneHarness` translates it to; the compaction and the probe
/// carry neither.
#[tokio::test(flavor = "multi_thread")]
async fn every_installers_server_compacts_at_the_idle_threshold_and_not_below() {
    for installer in INSTALLERS {
        let journey = Compacting::open(
            installer,
            as_written,
            &[
                Step::Turn(sized(IDLE_COMPACT - 1)),
                Step::Turn(sized(IDLE_COMPACT)),
                Step::Compaction,
                Step::Turn(sized(AFTER + 4_000)),
            ],
        )
        .await;
        let config = ServerConfig::load(journey.path("config.toml")).expect("it reads");
        assert_eq!(
            (config.auto_compact_tokens, config.idle_compact_tokens),
            (NonZeroU64::new(AUTO_COMPACT), NonZeroU64::new(IDLE_COMPACT)),
            "{installer}'s configuration does not carry both compaction keys"
        );
        let print = journey.active().await;

        journey.turn(&print, 1).await;
        journey.turn(&print, 2).await;
        journey
            .until("the idle compaction ended", || {
                !journey.compactions(&print).is_empty()
            })
            .await;
        journey.turn(&print, 3).await;

        let turns = journey.turns(&print);
        let compactions = journey.compactions(&print);
        assert_eq!(
            compactions.len(),
            1,
            "{installer}: a turn below the threshold, or one after the compaction, compacted: \
             {compactions:?}"
        );
        let compaction = &compactions[0];
        assert_eq!(compaction.ended, CompactionEnd::Completed, "{compaction:?}");
        assert_eq!(compaction.size_before, IDLE_COMPACT);
        assert_eq!(compaction.reported_before, Some(IDLE_COMPACT));
        assert_eq!(compaction.reported_after, Some(AFTER));
        assert_eq!(compaction.usage.cache_read_tokens, Some(IDLE_COMPACT));
        assert!(
            turns[1].ran_at <= compaction.started_at
                && compaction.ended_at <= turns[2].started_at.expect("a start"),
            "{installer}: the compaction did not run in the gap after the second turn"
        );
        none_overlap(&turns, &compactions);

        let requests = journey.requests.all();
        assert_eq!(requests.len(), 3, "one run request per turn");
        for request in &requests {
            assert_eq!(
                request.auto_compact_tokens,
                NonZeroU64::new(AUTO_COMPACT),
                "{installer}: a supervision turn's run request lost auto_compact_tokens"
            );
        }
        let expected: Vec<(String, Option<String>)> = translated(AUTO_COMPACT)
            .into_iter()
            .map(|(name, value)| (name, Some(value)))
            .collect();
        let seen = journey.seen();
        let (compacted, turned): (Vec<&Value>, Vec<&Value>) =
            seen.iter().partition(|seen| is_compaction(seen));
        assert_eq!((turned.len(), compacted.len()), (3, 1), "{seen:?}");
        for turn in turned {
            assert_eq!(
                compaction_env(turn),
                expected,
                "{installer}: a turn's harness process was not handed what auto_compact_tokens \
                 translates to"
            );
        }
        assert!(
            compaction_env(compacted[0])
                .iter()
                .all(|(_, value)| value.is_none()),
            "{installer}: the compaction's harness process carried auto_compact_tokens"
        );

        journey
            .agent
            .probe("compacting", "Answer with the assessment.")
            .expect("the probe runs");
        let probed = journey.requests.all();
        assert_eq!(
            probed
                .last()
                .expect("the probe's request")
                .auto_compact_tokens,
            None,
            "{installer}: the probe, which is no supervision turn, carried auto_compact_tokens"
        );
        let probe_seen = journey.seen();
        assert!(
            compaction_env(probe_seen.last().expect("the probe's process"))
                .iter()
                .all(|(_, value)| value.is_none()),
            "{installer}: the probe's harness process carried auto_compact_tokens"
        );
        journey.server.stop().await;
    }
}

/// A configuration without either key sets nothing on a turn, hands the
/// harness process nothing, and never compacts while idle.
#[tokio::test(flavor = "multi_thread")]
async fn a_configuration_without_the_keys_carries_nothing() {
    let journey = Compacting::open(
        INSTALLERS[0],
        without_compaction,
        &[
            Step::Turn(sized(IDLE_COMPACT * 2)),
            Step::Turn(sized(IDLE_COMPACT * 2)),
        ],
    )
    .await;
    let config = ServerConfig::load(journey.path("config.toml")).expect("it reads");
    assert_eq!(
        (config.auto_compact_tokens, config.idle_compact_tokens),
        (None, None)
    );
    let print = journey.active().await;
    journey.turn(&print, 1).await;
    journey.turn(&print, 2).await;

    assert!(journey.compactions(&print).is_empty());
    assert!(journey.compactions_begun().is_empty());
    for request in journey.requests.all() {
        assert_eq!(request.auto_compact_tokens, None);
    }
    for seen in journey.seen() {
        assert!(
            compaction_env(&seen)
                .iter()
                .all(|(_, value)| value.is_none()),
            "a harness process was handed a compaction setting nobody configured"
        );
    }
    journey.server.stop().await;
}

/// An event that arrives while a compaction runs cancels it: the compaction's
/// harness process exits promptly, and that event's turn is issued once it has,
/// inside the bound and before any compaction of the session completes, in the
/// session as it was, and the compaction is tried again after that turn and
/// completes.
#[tokio::test(flavor = "multi_thread")]
async fn an_event_during_a_compaction_cancels_it_and_its_turn_goes_first() {
    // The cancelled compaction's process is ended while it holds, before the
    // published responder counts it, so it takes no scripted step of its own.
    let journey = Compacting::open(
        INSTALLERS[0],
        as_written,
        &[
            Step::Turn(sized(IDLE_COMPACT + 5_000)),
            Step::Turn(sized(IDLE_COMPACT + 9_000)),
            Step::Compaction,
            Step::Turn(sized(AFTER + 4_000)),
        ],
    )
    .await;
    let hold = journey.path("compact-hold");
    std::fs::write(&hold, "held").expect("the hold is writable");
    let print = journey.active().await;

    journey.turn(&print, 1).await;
    journey
        .until("the compaction began", || {
            journey.compactions_begun().len() == 1
        })
        .await;
    let store = journey.seen().last().expect("the compaction's process")["store"].clone();
    assert!(
        is_compaction(journey.seen().last().expect("a process")),
        "the last process started was not the compaction"
    );
    assert!(
        journey.compactions(&print).is_empty(),
        "it is still running"
    );

    let arrived = Timestamp::now();
    journey.turn(&print, 2).await;
    std::fs::remove_file(&hold).expect("the hold is released");
    journey
        .until("the retried compaction ended", || {
            journey.compactions(&print).len() == 2
        })
        .await;
    journey.turn(&print, 3).await;

    let turns = journey.turns(&print);
    let compactions = journey.compactions(&print);
    assert_eq!(compactions.len(), 2, "{compactions:?}");
    let (cancelled, retried) = (&compactions[0], &compactions[1]);
    assert_eq!(cancelled.ended, CompactionEnd::Cancelled, "{cancelled:?}");
    assert_eq!(cancelled.size_before, IDLE_COMPACT + 5_000);
    assert_eq!(retried.ended, CompactionEnd::Completed, "{retried:?}");
    assert_eq!(retried.size_before, IDLE_COMPACT + 9_000);

    let second = turns[1].started_at.expect("a start");
    let delay = after(arrived, &turns[1]);
    eprintln!("from the event's arrival to its turn's start: {delay:?}");
    assert!(
        delay < CANCEL_BOUND,
        "cancelling the compaction held the event's turn up {delay:?}"
    );
    assert!(
        cancelled.ended_at <= second,
        "the event's turn started before the compaction's harness process had exited"
    );
    assert!(
        second < retried.started_at && turns[1].ran_at <= retried.started_at,
        "the event's turn waited on a compaction instead of going first"
    );
    none_overlap(&turns, &compactions);

    let seen = journey.seen();
    let turned: Vec<&Value> = seen.iter().filter(|seen| !is_compaction(seen)).collect();
    assert_eq!(
        turned[1]["store"], store,
        "the cancelled compaction changed the session's stored files"
    );
    assert_eq!(journey.compactions_begun().len(), 2);
    journey.server.stop().await;
}

/// An alert that arrives while a compaction runs whose harness process ignores
/// `SIGTERM` and never exits on its own: cancelling the compaction kills it,
/// and the alert's turn starts once its process has exited and inside the
/// bound, in the session as it was, its answer accepted.
#[cfg(unix)]
#[tokio::test(flavor = "multi_thread")]
async fn a_compaction_that_ignores_the_request_to_exit_is_killed_and_the_alert_goes_first() {
    let journey = Compacting::open_as(
        INSTALLERS[0],
        as_written,
        &[
            Step::Turn(sized(IDLE_COMPACT + 5_000)),
            Step::Turn(sized(AFTER + 4_000)),
        ],
        None,
        Harness::IgnoringTerm,
    )
    .await;
    // Held for the journey's whole length: the compaction cannot end on its own.
    let hold = journey.path("compact-hold");
    std::fs::write(&hold, "held for good").expect("the hold is writable");
    let print = journey.active().await;

    journey.turn(&print, 1).await;
    journey
        .until("the compaction began", || {
            journey.compactions_begun().len() == 1
        })
        .await;
    let store = journey.seen().last().expect("the compaction's process")["store"].clone();
    let arrived = Timestamp::now();
    journey.turn(&print, 2).await;

    let turns = journey.turns(&print);
    let compactions = journey.compactions(&print);
    assert_eq!(compactions.len(), 1, "{compactions:?}");
    let killed = &compactions[0];
    assert_eq!(killed.ended, CompactionEnd::Cancelled, "{killed:?}");
    assert!(
        hold.exists(),
        "the compaction was released rather than killed"
    );
    let second = &turns[1];
    assert!(
        killed.ended_at <= second.started_at.expect("a start"),
        "the alert's turn started before the compaction's harness process had exited"
    );
    let delay = after(arrived, second);
    eprintln!("from the alert's arrival to its turn's start: {delay:?}");
    assert!(
        delay < CANCEL_BOUND,
        "killing the compaction held the alert's turn up {delay:?}"
    );
    assert_eq!(second.failure, None, "the alert's turn failed: {second:?}");
    none_overlap(&turns, &compactions);

    let seen = journey.seen();
    let turned: Vec<&Value> = seen.iter().filter(|seen| !is_compaction(seen)).collect();
    assert_eq!(
        turned[1]["store"], store,
        "the killed compaction changed the session's stored files"
    );
    journey.server.stop().await;
}

/// A scheduled check that comes due while a compaction runs waits for it, and
/// runs once it ends: the compaction completes, and the check's turn starts
/// after it, the wait written down with the compaction.
#[tokio::test(flavor = "multi_thread")]
async fn a_scheduled_check_waits_for_a_running_compaction_and_then_runs() {
    let journey = Compacting::open(
        INSTALLERS[0],
        checking_every_second,
        &[
            Step::Turn(sized(IDLE_COMPACT + 5_000)),
            Step::Compaction,
            Step::Turn(sized(AFTER + 4_000)),
        ],
    )
    .await;
    let hold = journey.path("compact-hold");
    std::fs::write(&hold, "held").expect("the hold is writable");
    let print = journey.active().await;
    journey
        .until("the first scheduled check's turn ended", || {
            !journey.turns(&print).is_empty()
        })
        .await;
    journey
        .until("the compaction began", || {
            journey.compactions_begun().len() == 1
        })
        .await;
    // Several rounds of a one-second check come due while it holds.
    tokio::time::sleep(Duration::from_secs(4)).await;
    assert_eq!(
        journey.turns(&print).len(),
        1,
        "a scheduled check ran while the compaction did"
    );
    assert!(
        journey.compactions(&print).is_empty(),
        "it is still running"
    );

    std::fs::remove_file(&hold).expect("the hold is released");
    journey
        .until("the waiting check's turn ended", || {
            journey.turns(&print).len() >= 2
        })
        .await;

    let turns = journey.turns(&print);
    let compactions = journey.compactions(&print);
    assert_eq!(compactions.len(), 1, "{compactions:?}");
    let compaction = &compactions[0];
    assert_eq!(compaction.ended, CompactionEnd::Completed, "{compaction:?}");
    let waited = compaction
        .scheduled_check_waited_ms
        .expect("the waiting check is written down");
    assert!(waited >= 3_000, "the check waited {waited} ms");
    assert!(
        compaction.ended_at <= turns[1].started_at.expect("a start"),
        "the scheduled check's turn started before the compaction ended"
    );
    none_overlap(&turns, &compactions);
    let observed = journey
        .get(&format!("/prints/{print}/history?limit=1000"))
        .await["events"]
        .as_array()
        .expect("events")
        .iter()
        .filter(|event| event["kind"] == "periodic_observation")
        .count();
    assert!(observed >= 2, "the second turn was not a scheduled check's");
    journey.server.stop().await;
}

/// A compaction that hangs past its own timeout is ended there, and releases
/// the scheduled check waiting on it, which then runs in the session as it was.
#[tokio::test(flavor = "multi_thread")]
async fn a_compaction_hung_past_its_timeout_releases_the_waiting_check() {
    let timeout = 3;
    let journey = Compacting::open_timed(
        INSTALLERS[0],
        checking_every_second,
        &[
            Step::Turn(sized(IDLE_COMPACT + 5_000)),
            Step::Turn(sized(AFTER + 4_000)),
        ],
        Some(TurnTimeout::new(timeout).expect("a bound")),
    )
    .await;
    std::fs::write(journey.path("compact-hold"), "held for good").expect("the hold is writable");
    let print = journey.active().await;
    journey
        .until("the waiting check's turn ended", || {
            journey.turns(&print).len() >= 2
        })
        .await;

    let turns = journey.turns(&print);
    let compactions = journey.compactions(&print);
    assert_eq!(compactions.len(), 1, "{compactions:?}");
    let compaction = &compactions[0];
    assert_eq!(compaction.ended, CompactionEnd::Failed, "{compaction:?}");
    assert!(
        compaction.scheduled_check_waited_ms.is_some(),
        "no scheduled check waited on it: {compaction:?}"
    );
    assert!(
        compaction.ended_at <= turns[1].started_at.expect("a start"),
        "the waiting check ran before the hung compaction was ended"
    );
    let ran = journey.compactions_begun().len();
    assert_eq!(ran, 1, "the hung compaction was not the only one");
    none_overlap(&turns, &compactions);
    journey.server.stop().await;
}

/// The printer pushing a reset that ends the print, while its session is
/// compacting, cancels the compaction at once rather than waiting on it.
#[tokio::test(flavor = "multi_thread")]
async fn a_print_ending_during_a_compaction_cancels_it() {
    let journey = Compacting::open(
        INSTALLERS[0],
        polling_often,
        &[Step::Turn(sized(IDLE_COMPACT + 5_000))],
    )
    .await;
    std::fs::write(journey.path("compact-hold"), "held").expect("the hold is writable");
    let print = journey.active().await;
    journey.turn(&print, 1).await;
    journey
        .until("the compaction began", || {
            journey.compactions_begun().len() == 1
        })
        .await;

    journey.octoprint.pushes_event("PrinterReset", &json!({}));
    journey.octoprint.reports(
        "Operational",
        JobScript {
            file: Some((FILE.to_owned(), i64::try_from(SLICE.len()).expect("a size"))),
            ..JobScript::idle()
        },
    );
    journey
        .until("the compaction ended", || {
            !journey.compactions(&print).is_empty()
        })
        .await;
    let compaction = &journey.compactions(&print)[0];
    assert_eq!(compaction.ended, CompactionEnd::Cancelled, "{compaction:?}");
    assert_eq!(compaction.scheduled_check_waited_ms, None);
    journey.server.stop().await;
}

/// A person's request while the print's session is compacting claims no turn,
/// so it is answered without waiting on the compaction, and leaves it running
/// to complete.
#[tokio::test(flavor = "multi_thread")]
async fn an_operator_request_during_a_compaction_neither_waits_nor_cancels() {
    let journey = Compacting::open(
        INSTALLERS[0],
        as_written,
        &[Step::Turn(sized(IDLE_COMPACT + 5_000)), Step::Compaction],
    )
    .await;
    let hold = journey.path("compact-hold");
    std::fs::write(&hold, "held").expect("the hold is writable");
    let print = journey.active().await;
    journey.turn(&print, 1).await;
    journey
        .until("the compaction began", || {
            journey.compactions_begun().len() == 1
        })
        .await;

    let path = printobserver_server::operation("set_fan_percent")
        .expect("`set_fan_percent` is served")
        .path
        .replace("{print_id}", &print.to_string());
    let asked = tokio::time::timeout(
        Duration::from_secs(10),
        journey
            .client
            .post(format!("{}{path}", journey.server.base_url()))
            .json(&json!({
                "percent": 80.0,
                "reason": "a person turned the fan up",
                "actor": "operator",
            }))
            .send(),
    )
    .await
    .expect("the request waited on the compaction")
    .expect("the server answers");
    assert!(asked.status().is_success(), "{}", asked.status());
    assert!(
        journey.compactions(&print).is_empty(),
        "the person's request ended the compaction"
    );
    assert_eq!(journey.turns(&print).len(), 1, "the request claimed a turn");

    std::fs::remove_file(&hold).expect("the hold is released");
    journey
        .until("the compaction ended", || {
            !journey.compactions(&print).is_empty()
        })
        .await;
    let compaction = &journey.compactions(&print)[0];
    assert_eq!(compaction.ended, CompactionEnd::Completed, "{compaction:?}");
    journey.server.stop().await;
}

/// Zero and a negative count are each refused for both compaction keys, the
/// refusal naming the key.
#[test]
fn a_token_count_below_one_is_refused_naming_its_key() {
    let root = TempDir::new().expect("a root");
    let state = root.path().join("state");
    copy_tree(
        committed_skill().parent().expect("the skill's directory"),
        &state.join("skills").join("printobserver"),
    );
    for (key, field) in [
        (
            "auto_compact_tokens",
            printobserver_server::ConfigField::AutoCompactTokens,
        ),
        (
            "idle_compact_tokens",
            printobserver_server::ConfigField::IdleCompactTokens,
        ),
    ] {
        for value in ["0", "-1", "-200000"] {
            let text = written_template(&root)
                .lines()
                .map(|line| {
                    if line.starts_with(&format!("{key} = ")) {
                        format!("{key} = {value}")
                    } else {
                        line.to_owned()
                    }
                })
                .collect::<Vec<_>>()
                .join("\n");
            let refused = ServerConfig::parse(&text, &root.path().join("config.toml"))
                .expect_err("a count below one was accepted");
            assert_eq!(refused.field(), Some(field), "{key} = {value}: {refused}");
            assert!(
                refused.to_string().contains(&format!("supervisor.{key}")),
                "{key} = {value} was refused without naming it: {refused}"
            );
        }
    }
}

/// A ledger written before compactions were recorded reads unchanged: its
/// sessions and turns as they were, and no compaction.
#[test]
fn a_ledger_from_before_compactions_reads_unchanged() {
    let root = TempDir::new().expect("a root");
    let print = PrintId::new();
    let at = Timestamp::now();
    let written = json!({
        "schema_version": "2",
        "print_id": print,
        "sessions": [{
            "print_id": print,
            "session_name": format!("print-{print}"),
            "harness_identity": "claude-code",
            "created_at": at,
            "last_turn_at": at,
        }],
        "turns": [{
            "session_name": format!("print-{print}"),
            "ran_at": at,
            "calls": [{ "input_tokens": 3, "output_tokens": 90 }],
        }],
    });
    let ledgers = root.path().join("state").join(SESSIONS_DIRECTORY);
    std::fs::create_dir_all(&ledgers).expect("a ledger directory");
    std::fs::write(ledgers.join(format!("{print}.json")), written.to_string())
        .expect("the ledger is writable");
    let config = agent_config_for_reading(&root);
    assert_eq!(config.idle_compact_tokens, NonZeroU64::new(IDLE_COMPACT));
    let agent = OneharnessSupervisor::open(config).expect("the agent opens");

    assert!(
        agent
            .recorded_compactions(&print)
            .expect("it reads")
            .is_empty()
    );
    let sessions = agent.recorded_sessions(&print).expect("it reads");
    assert_eq!(sessions.len(), 1);
    assert_eq!(sessions[0].session_name, format!("print-{print}"));
    let turns = agent.recorded_turns(&print).expect("it reads");
    assert_eq!(turns.len(), 1);
    assert_eq!(turns[0].ran_at, at);
    assert_eq!(turns[0].started_at, None);
    assert_eq!(turns[0].calls[0].output_tokens, Some(90));
}

/// The agent's configuration as the first installer's template composes it,
/// under `root`.
fn agent_config_for_reading(root: &TempDir) -> printobserver_oneharness::SupervisorConfig {
    copy_tree(
        committed_skill().parent().expect("the skill's directory"),
        &root
            .path()
            .join("state")
            .join("skills")
            .join("printobserver"),
    );
    let path = root.path().join("config.toml");
    std::fs::write(&path, written_template(root)).expect("written");
    agent_config(&ServerConfig::load(&path).expect("the template reads")).expect("composed")
}

/// The first installer's template under `root`, with what an operator fills
/// in filled in.
fn written_template(root: &TempDir) -> String {
    let state = root.path().join("state");
    let text = crate::installed_detector::template(INSTALLERS[0], &state);
    let mut document: toml::Value = toml::from_str(&text).expect("the template is a document");
    for (dotted, value) in [
        ("listen", "127.0.0.1:0"),
        ("octoprint.url", "http://127.0.0.1:9"),
        ("octoprint.api_key", "a-key-nothing-reads"),
        ("ingress.shared_secret", SECRET),
    ] {
        crate::world::set(&mut document, dotted, toml::Value::String(value.to_owned()));
    }
    toml::to_string(&document).expect("it renders")
}
