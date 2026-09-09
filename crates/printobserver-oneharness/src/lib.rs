//! `printobserver-oneharness`.
//!
//! Owns: the oneharness adapter — the one implementation of
//! `printobserver-supervisor-api` that drives an agentic coding harness through
//! `OneHarness`'s own Rust crate API, in process.
//!
//! May depend on: `printobserver-types` and `printobserver-supervisor-api`,
//! plus whatever it needs to drive oneharness. Never another implementation
//! crate, and never `printobserver-core`.
//!
//! # In process, and never by spawning a command
//!
//! Every turn goes through [`oneharness_core::io::run::run_supervised`], which
//! returns the report to this process. Nothing here builds a command, and
//! nothing here names the `oneharness` program: a check over this crate's own
//! sources refuses any use of a process-spawning interface at all, whatever
//! executable it would name. Processes still exist during a turn — `OneHarness`
//! spawns the harness itself — and [`TurnSeam::processes`] is where a caller
//! sees each one.
//!
//! # Nothing about a live session is held in memory
//!
//! A print's session is a file under the configured state directory, and so is
//! `OneHarness`'s own session store. That is what lets the supervisor be
//! restarted between two events of one print and have the second event continue
//! the first's conversation: this port rebuilt from the state directory alone
//! continues exactly where it left off.
//!
//! # What a prompt may say is one committed file
//!
//! The system prompt is the `PrintObserver` skill, read from the tree when the
//! port is built. The prompt for a turn is the committed template at
//! [`SupervisorConfig::prompt_template_path`] with its three slots filled — the
//! triggering event, the materialized path of its image, and the context
//! command — and nothing else. This port composes no sentence of its own, so
//! what a prompt may say is one small file a person reads.
//!
//! # The answer is constrained by the generated schema
//!
//! [`SupervisorConfig::assessment_schema`] names the assessment schema the
//! contracts generate, and the path is what reaches `OneHarness`: this crate
//! carries no schema of its own and embeds none, so the constraint the agent
//! answers under is the one the types declare.

mod config;
mod ledger;
mod prompt;
mod turn;

pub use config::{
    AssessmentSchema, ConfigError, EnvAssignment, HarnessIdentity, ModelName, RunReportObserver,
    RunRequestObserver, SupervisorConfig, TurnSeam, TurnTimeout,
};
pub use ledger::{LedgerFormat, RecordedTurn, SESSIONS_DIRECTORY, SessionName};
pub use prompt::{
    CONTEXT_COMMAND_SLOT, EVENT_SLOT, IMAGE_SLOT, NO_IMAGE, PromptTemplate, SLOTS, TemplateError,
};
pub use turn::{HARNESS_SESSIONS_DIRECTORY, OneharnessSupervisor, TurnReport};
