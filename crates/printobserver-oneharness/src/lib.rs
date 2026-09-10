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

/// The `PrintObserver` skill this crate ships, as bytes in the built artifact.
///
/// The port reads its system prompt from a *path*, because editing the skill on
/// a running host is a restart rather than a rebuild. An installed program has
/// no checkout to read that path out of, so the composition root writes this
/// constant to a file under its state directory and points the port at it —
/// which is what makes an install that configures no skill of its own run the
/// committed one rather than none.
///
/// It is `include_str!` of this crate's own committed asset rather than a copy,
/// so the file `just check-repo`'s supervisor checks read and the bytes an
/// installed program runs are one thing.
pub const DEFAULT_SKILL: &str = include_str!("../assets/printobserver-skill.md");

/// The committed prompt template this crate ships, for the same reason
/// [`DEFAULT_SKILL`] is here.
pub const DEFAULT_TURN_PROMPT: &str = include_str!("../assets/turn-prompt.md");

/// Where the reference documents sit, relative to the skill's own directory.
///
/// The skill links to them by this path and nothing else, so one relative link
/// resolves the same way in the checkout — where `assets/reference` is the
/// repository's own `docs/reference` — and on an installed host, where the
/// composition root writes them beside the skill it materialized.
// llmlint: ignore[invalid_states_unrepresentable] This is the immutable literal "reference", not a field or caller-supplied path. No API can assign an absolute or traversing value to this constant; the docs bundle check holds its consumers to the declared layout.
pub const REFERENCE_DIRECTORY: &str = "reference";

/// Every reference document the skill links to, as bytes in the built artifact.
///
/// [`DEFAULT_SKILL`] is deliberately short and links out for everything else,
/// which is a promise an installed program has to keep: a link to a file no
/// install carries is worse than no link at all. So the documents travel with
/// the skill, and the composition root writes them beside it — an installed
/// host has no checkout to read them out of.
///
/// Each entry is the path the document is materialized at, relative to the
/// skill's own directory, and its bytes. `just check-repo` holds this array to
/// the document set `repo-policy.toml` declares, in both directions, so a
/// document the skill may link to is one this array carries.
// llmlint: ignore[invalid_states_unrepresentable] This immutable array contains seven literal bundle paths, not caller-constructed entries. The docs bundle check compares every path with the validated policy set in both directions, and the installed-assets journey opens the materialized references.
pub const DEFAULT_REFERENCES: [(&str, &str); 7] = [
    (
        "reference/api-and-clients.md",
        include_str!("../assets/reference/api-and-clients.md"),
    ),
    (
        "reference/architecture.md",
        include_str!("../assets/reference/architecture.md"),
    ),
    (
        "reference/command-surface.md",
        include_str!("../assets/reference/command-surface.md"),
    ),
    (
        "reference/common-operations.md",
        include_str!("../assets/reference/common-operations.md"),
    ),
    (
        "reference/intervention-policy.md",
        include_str!("../assets/reference/intervention-policy.md"),
    ),
    (
        "reference/schemas.md",
        include_str!("../assets/reference/schemas.md"),
    ),
    (
        "reference/testing.md",
        include_str!("../assets/reference/testing.md"),
    ),
];

pub use config::{
    AssessmentSchema, ConfigError, EnvAssignment, HarnessIdentity, ModelName, RunReportObserver,
    RunRequestObserver, SupervisorConfig, TurnSeam, TurnTimeout,
};
pub use ledger::{LedgerFormat, RecordedTurn, SESSIONS_DIRECTORY, SessionName};
pub use prompt::{
    CONTEXT_COMMAND_SLOT, EVENT_SLOT, IMAGE_SLOT, NO_IMAGE, PromptTemplate, SLOTS, TemplateError,
};
pub use turn::{HARNESS_SESSIONS_DIRECTORY, OneharnessSupervisor, TurnReport};
