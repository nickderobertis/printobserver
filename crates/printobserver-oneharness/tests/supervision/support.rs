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
use std::fs::File;
use std::future::Future;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::{Arc, Mutex};
use std::task::{Context, Poll, Wake, Waker};

use oneharness_core::domain::report::RunReport;
use oneharness_core::io::run::RunRequest;
use oneharness_core::io::runner::ProcessSupervisor;
use printobserver_oneharness::{
    AssessmentSchema, EnvAssignment, HarnessIdentity, OneharnessSupervisor, RunReportObserver,
    RunRequestObserver, SupervisorConfig, TurnSeam, TurnTimeout,
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

/// The file the checked-in schema tree is read and written under.
///
/// One journey here **writes** to that tree: the assessment-schema journey
/// changes the checked-in artifact on disk, drives an answer that was accepted
/// before, and puts the artifact back, which is what proves the port reads that
/// artifact at run time rather than validating against a copy of its bytes.
/// Everything else that names the artifact reads it, in this suite and in
/// `printobserver-types`'s `schemas` suite, and all of it runs at the same time:
/// the test runner gives each test its own process, and `nx run-many` drives one
/// project's tests while another's are still going.
///
/// The file sits under `target`, which is per-worktree and ignored, so two
/// checkouts on one machine never block each other.
/// `repo-policy.toml`'s `supervisor.schema_lock` is where the name comes from,
/// and `just check-repo` holds every holder it declares to that one name: two
/// suites that locked two different files would be back to no lock at all.
fn schema_lock_file() -> File {
    let directory = repo_root().join("target");
    fs::create_dir_all(&directory).expect("the target directory is writable");
    File::create(directory.join("printobserver-schemas.lock"))
        .expect("the schema lock file is creatable")
}

/// Hold the checked-in schema tree still while this journey **changes** it.
///
/// Exclusive, so nothing reads the tree while it is half-changed. The lock is
/// the operating system's own, so the kernel releases it when the handle goes:
/// a journey that panics, or is killed, leaves nothing holding it.
pub fn schema_lock() -> File {
    let file = schema_lock_file();
    file.lock().expect("the schema lock is takeable");
    file
}

/// Hold the checked-in schema tree still while this journey **reads** it.
///
/// Shared, so the journeys that only read the artifact still run beside each
/// other and only the one that changes it waits for them. Every journey here
/// that drives a turn against the checked-in artifact takes this, because an
/// answer it accepts is only the answer that artifact admits.
pub fn schema_read_lock() -> File {
    let file = schema_lock_file();
    file.lock_shared().expect("the schema lock is shareable");
    file
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

/// One assessment schema a journey constrains an answer by.
pub fn schema(path: &Path) -> AssessmentSchema {
    AssessmentSchema::at(path)
        .unwrap_or_else(|error| panic!("a journey names a schema to constrain an answer: {error}"))
}

/// One harness identity a journey names.
pub fn identity(name: &str) -> HarnessIdentity {
    HarnessIdentity::new(name).expect("a journey names a harness")
}

/// One `KEY=VALUE` assignment a journey scripts the responder with.
pub fn assignment(text: &str) -> EnvAssignment {
    EnvAssignment::new(text)
        .unwrap_or_else(|error| panic!("a journey scripts the responder: {error}"))
}

/// The configuration a journey drives the port with, constrained by the schema
/// at `schema`.
pub fn config(
    fixture: &Fixture,
    harness: &str,
    constraint: &Path,
    env: Vec<EnvAssignment>,
) -> SupervisorConfig {
    SupervisorConfig {
        state_dir: fixture.state_dir(),
        skill_path: skill_path(),
        prompt_template_path: template_path(),
        assessment_schema: schema(constraint),
        harness: identity(harness),
        model: None,
        working_dir: fixture.root.join("work"),
        turn_timeout: TurnTimeout::DEFAULT,
        harness_bin: Some(responder()),
        harness_env: env,
    }
}

/// What one journey saw the port do: every run request it built, every process
/// `OneHarness` created under them, and every report it answered.
#[derive(Debug, Default)]
pub struct Watch {
    /// Every run request, in the order the port built them.
    requests: Mutex<Vec<RunRequest>>,
    /// Every process created under a run, by the program it was spawned with.
    programs: Mutex<Vec<PathBuf>>,
    /// Every report `OneHarness` answered, in the order the runs finished.
    reports: Mutex<Vec<RunReport>>,
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

    /// Every report `OneHarness` has answered so far.
    pub fn reports(&self) -> Vec<RunReport> {
        self.reports
            .lock()
            .expect("the watch is not poisoned")
            .clone()
    }
}

impl RunReportObserver for Watch {
    fn answered(&self, report: &RunReport) {
        self.reports
            .lock()
            .expect("the watch is not poisoned")
            .push(report.clone());
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
            reports: Some(watch.clone()),
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
pub fn always(session_id: &str, answer: &str) -> Vec<EnvAssignment> {
    vec![assignment(&format!(
        "MOCK_STDOUT={}",
        document(session_id, answer)
    ))]
}

/// The environment that scripts the responder to give a different answer on
/// each successive invocation, and the last of them from then on.
pub fn in_turn(session_id: &str, answers: &[&str], counter: &Path) -> Vec<EnvAssignment> {
    let mut env = vec![assignment(&format!(
        "MOCK_ATTEMPT_FILE={}",
        counter.display()
    ))];
    for (index, answer) in answers.iter().enumerate() {
        env.push(assignment(&format!(
            "MOCK_STDOUT_{}={}",
            index + 1,
            document(session_id, answer)
        )));
    }
    let last = answers.last().copied().unwrap_or_default();
    env.push(assignment(&format!(
        "MOCK_STDOUT={}",
        document(session_id, last)
    )));
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
        context_command: format!("printobserver context --print-id {print_id}"),
    }
}
