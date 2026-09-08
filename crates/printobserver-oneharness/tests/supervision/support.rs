//! What every journey in this suite drives the port with.
//!
//! Nothing here is a double of the port. The seam these helpers hang off is the
//! port's own [`TurnSeam`]: the requests it built, and the processes
//! `OneHarness` created under them. Everything from the run request down —
//! argument construction, the session store, schema validation and the
//! validate-and-re-prompt loop — is the real `OneHarness`, and the only thing
//! replaced is the paid provider process, by the responder `OneHarness`
//! publishes and this repository ships a binary for.

use std::fs;
use std::future::Future;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::{Arc, Mutex};
use std::task::{Context, Poll, Wake, Waker};

use oneharness_core::io::run::RunRequest;
use oneharness_core::io::runner::ProcessSupervisor;
use printobserver_oneharness::{
    DEFAULT_TURN_TIMEOUT_S, OneharnessSupervisor, RunRequestObserver, SupervisorConfig, TurnSeam,
};
use printobserver_supervisor_api::TurnRequest;
use printobserver_types::{
    EventId, EventPayload, EventRecord, EventSource, PrintId, Timestamp, serde_json,
};

/// The harness every journey runs on unless it is about a second identity.
pub const HARNESS: &str = "claude-code";

/// A second harness identity, for the journey about a session that cannot
/// migrate between them.
pub const OTHER_HARNESS: &str = "codex";

/// Unparks the thread that is blocking on a future.
struct Unpark(std::thread::Thread);

impl Wake for Unpark {
    fn wake(self: Arc<Self>) {
        self.0.unpark();
    }
}

/// Run one future to completion on the calling thread.
///
/// The port's methods are asynchronous because the supervision core holds every
/// port behind one shared trait object; this suite needs no runtime beyond a
/// thread that parks.
pub fn block_on<F: Future>(future: F) -> F::Output {
    let waker = Waker::from(Arc::new(Unpark(std::thread::current())));
    let mut context = Context::from_waker(&waker);
    let mut future = Box::pin(future);
    loop {
        match future.as_mut().poll(&mut context) {
            Poll::Ready(value) => return value,
            Poll::Pending => std::thread::park(),
        }
    }
}

/// This crate's own directory.
fn crate_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

/// The repository root.
fn repo_root() -> PathBuf {
    crate_dir().join("..").join("..")
}

/// The generated assessment schema's own checked-in artifact — the file the
/// contracts' generation target writes and refuses the tree over when it no
/// longer matches the types.
pub fn generated_assessment_schema() -> PathBuf {
    repo_root()
        .join("schemas")
        .join("printobserver-types")
        .join("AgentAssessment.json")
}

/// The committed prompt template.
pub fn template_path() -> PathBuf {
    crate_dir().join("assets").join("turn-prompt.md")
}

/// The committed `PrintObserver` skill.
pub fn skill_path() -> PathBuf {
    crate_dir().join("assets").join("printobserver-skill.md")
}

/// The test-only responder this repository ships for `OneHarness` to resolve
/// to. It delegates to `OneHarness`'s own published deterministic responder.
pub fn responder() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_printobserver-oneharness-responder"))
}

/// A scratch tree for one journey: the state directory the port keeps its
/// sessions in, the directory turns run in, and room for scripted files.
pub struct Fixture {
    /// The scratch root, removed and recreated as the journey starts.
    root: PathBuf,
}

impl Fixture {
    /// A scratch tree named for the journey that owns it.
    pub fn new(tag: &str) -> Self {
        let root = std::env::temp_dir().join(format!(
            "printobserver-oneharness-{tag}-{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(root.join("state")).expect("a state directory");
        fs::create_dir_all(root.join("work")).expect("a working directory");
        Self { root }
    }

    /// Where the port keeps its sessions — both its own ledger and
    /// `OneHarness`'s session store.
    pub fn state_dir(&self) -> PathBuf {
        self.root.join("state")
    }

    /// A scratch path under this journey's own tree.
    pub fn path(&self, name: &str) -> PathBuf {
        self.root.join(name)
    }
}

/// The configuration a journey drives the port with, constrained by the schema
/// at `schema`.
pub fn config(
    fixture: &Fixture,
    harness: &str,
    schema: &Path,
    env: Vec<String>,
) -> SupervisorConfig {
    SupervisorConfig {
        state_dir: fixture.state_dir(),
        skill_path: skill_path(),
        prompt_template_path: template_path(),
        assessment_schema_path: schema.to_path_buf(),
        harness: harness.to_owned(),
        model: None,
        working_dir: fixture.root.join("work"),
        turn_timeout_s: DEFAULT_TURN_TIMEOUT_S,
        harness_bin: Some(responder()),
        harness_env: env,
    }
}

/// What one journey saw the port do: every run request it built, and every
/// process `OneHarness` created under them.
#[derive(Debug, Default)]
pub struct Watch {
    /// Every run request, in the order the port built them.
    requests: Mutex<Vec<RunRequest>>,
    /// Every process created under a run, by the program it was spawned with.
    programs: Mutex<Vec<PathBuf>>,
}

impl Watch {
    /// Every run request the port has built so far.
    pub fn requests(&self) -> Vec<RunRequest> {
        self.requests
            .lock()
            .expect("the watch is not poisoned")
            .clone()
    }

    /// Every process created under a run so far, by its program.
    pub fn programs(&self) -> Vec<PathBuf> {
        self.programs
            .lock()
            .expect("the watch is not poisoned")
            .clone()
    }
}

impl RunRequestObserver for Watch {
    fn built(&self, request: &RunRequest) {
        self.requests
            .lock()
            .expect("the watch is not poisoned")
            .push(request.clone());
    }
}

impl ProcessSupervisor for Watch {
    fn spawning(&self, command: &mut Command) {
        self.programs
            .lock()
            .expect("the watch is not poisoned")
            .push(PathBuf::from(command.get_program()));
    }
}

/// The port, built from a configuration and watched.
pub fn port(config: SupervisorConfig, watch: &Arc<Watch>) -> OneharnessSupervisor {
    OneharnessSupervisor::observed(
        config,
        TurnSeam {
            requests: Some(watch.clone()),
            processes: Some(watch.clone()),
        },
    )
    .expect("the port is built from the committed skill and template")
}

/// An assessment conforming to the generated schema, whose summary says which
/// answer it is.
pub fn assessment(summary: &str, confidence: &str) -> String {
    serde_json::json!({
        "summary": summary,
        "confidence": confidence,
        "should_continue": true,
        "did": "read the event, the picture and the print's context",
        "why": "the first layer is down and the walls are clean",
        "escalating": false,
    })
    .to_string()
}

/// One harness response document: the answer as the final text, beside the
/// session id the harness exposes for continuation.
///
/// Both spellings of that id are written, because the responder keeps only the
/// one the harness it is standing in for would really emit — `session_id` for
/// Claude Code, `thread_id` for Codex — and drops the other. One document
/// therefore scripts either identity without the journey knowing which.
fn document(session_id: &str, answer: &str) -> String {
    serde_json::json!({
        "result": answer,
        "session_id": session_id,
        "thread_id": session_id,
    })
    .to_string()
}

/// The environment that scripts the responder to give the same answer every
/// time it is invoked.
pub fn always(session_id: &str, answer: &str) -> Vec<String> {
    vec![format!("MOCK_STDOUT={}", document(session_id, answer))]
}

/// The environment that scripts the responder to give a different answer on
/// each successive invocation, and the last of them from then on.
pub fn in_turn(session_id: &str, answers: &[&str], counter: &Path) -> Vec<String> {
    let mut env = vec![format!("MOCK_ATTEMPT_FILE={}", counter.display())];
    for (index, answer) in answers.iter().enumerate() {
        env.push(format!(
            "MOCK_STDOUT_{}={}",
            index + 1,
            document(session_id, answer)
        ));
    }
    let last = answers.last().copied().unwrap_or_default();
    env.push(format!("MOCK_STDOUT={}", document(session_id, last)));
    env
}

/// One event of a print, at the instant it arrived.
pub fn event(print_id: PrintId, payload: EventPayload) -> EventRecord {
    EventRecord {
        id: EventId::new(),
        print_id: Some(print_id),
        source: EventSource::Obico,
        received_at: Timestamp::now(),
        image: None,
        payload,
        raw: None,
    }
}

/// One turn about one event of one print.
pub fn turn(print_id: PrintId, event: EventRecord, image: Option<PathBuf>) -> TurnRequest {
    TurnRequest {
        print_id,
        event,
        image_path: image,
        context_command: format!("printobserver context --print {print_id}"),
    }
}
