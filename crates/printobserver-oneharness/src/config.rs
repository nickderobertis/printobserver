//! What the port is built with, and the seam a caller watches a turn through.

use std::path::PathBuf;
use std::sync::Arc;

use oneharness_core::io::run::RunRequest;
use oneharness_core::io::runner::ProcessSupervisor;

/// How long one supervision turn is given, when the configuration names no
/// other bound. A turn looks at one picture and writes one paragraph; a turn
/// still running after this long is one nothing is waiting for.
pub const DEFAULT_TURN_TIMEOUT_S: u64 = 300;

/// How the supervising agent is reached, and where its sessions live.
///
/// Every path is absolute in production. Nothing here is read until the port is
/// built, and the skill and the template are read exactly then — which is what
/// makes editing either of them a restart rather than a rebuild.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SupervisorConfig {
    /// The state directory this port keeps its sessions under. It holds both
    /// this port's own per-print ledger and `OneHarness`'s session store, which
    /// is what a restart continues a conversation from.
    pub state_dir: PathBuf,
    /// The `PrintObserver` skill, sent as every turn's system prompt.
    pub skill_path: PathBuf,
    /// The committed prompt template, whose three slots one turn fills.
    pub prompt_template_path: PathBuf,
    /// The generated assessment schema the agent's answer is constrained by.
    /// The path is handed to `OneHarness`, which reads it per run.
    pub assessment_schema_path: PathBuf,
    /// The harness identity turns run on, as `OneHarness` names it.
    pub harness: String,
    /// The model, when one is pinned rather than left to the harness.
    pub model: Option<String>,
    /// The working directory each harness process runs in.
    pub working_dir: PathBuf,
    /// How long one turn is given.
    pub turn_timeout_s: u64,
    /// The harness binary, when it is not the one `OneHarness` resolves by name.
    pub harness_bin: Option<PathBuf>,
    /// Extra `KEY=VALUE` environment for each harness process.
    pub harness_env: Vec<String>,
}

/// Receives every run request this port builds.
///
/// Observation only: a request is handed over after it is complete and before
/// `OneHarness` receives it, and an observer cannot change it.
pub trait RunRequestObserver: Send + Sync {
    /// One run request, exactly as it will be handed to `OneHarness`.
    fn built(&self, request: &RunRequest);
}

/// Where a caller watches what a turn does: the requests this port builds, and
/// the processes `OneHarness` creates under them.
///
/// Both halves are the supervision a long-running host wants anyway — the
/// harness tree is what a watchdog reaps — and both are what a test observes a
/// turn through, so no test needs a double of the port to see either.
#[derive(Clone, Default)]
pub struct TurnSeam {
    /// Handed every run request this port builds, before `OneHarness` receives it.
    pub requests: Option<Arc<dyn RunRequestObserver>>,
    /// Handed every harness process `OneHarness` creates under the run.
    pub processes: Option<Arc<dyn ProcessSupervisor + Send + Sync>>,
}

impl core::fmt::Debug for TurnSeam {
    fn fmt(&self, formatter: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        formatter
            .debug_struct("TurnSeam")
            .field("requests", &self.requests.is_some())
            .field("processes", &self.processes.is_some())
            .finish()
    }
}
