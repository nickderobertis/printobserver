//! `printobserver-supervisor-api`.
//!
//! Owns: the port the supervising agent is reached through — the trait for
//! running one supervision turn and for closing a session, the two shapes those
//! methods carry, and that port's own error type.
//!
//! May depend on: `printobserver-types` only. A port that named an
//! implementation would stop being a port.
//!
//! # Opening a session is not a call of its own
//!
//! Opening or continuing is [`SupervisorPort::run_turn`]'s own doing, and the
//! [`SessionPhase`] it answers is what says which happened. A separate open
//! call would let a caller open a session it then never took a turn in.
//!
//! # Why the methods answer a boxed future
//!
//! Every method is asynchronous, and the trait is dyn-compatible and shareable
//! across threads, because the supervision core holds all four ports behind
//! `Arc<dyn Port>`. An `async fn` in a trait is not dyn-compatible, so each
//! method answers a [`BoxFuture`] instead.

use core::future::Future;
use core::pin::Pin;
use std::path::PathBuf;

use printobserver_types::schemars::JsonSchema;
use printobserver_types::serde::{Deserialize, Serialize};
use printobserver_types::{
    AgentAssessment, EventRecord, PrintId, SessionPhase, SupervisionSession,
};

/// A future this port's methods answer with, in the one shape a trait object
/// can carry.
pub type BoxFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

/// What one supervision turn is asked to consider.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(crate = "printobserver_types::serde", deny_unknown_fields)]
#[schemars(crate = "printobserver_types::schemars")]
pub struct TurnRequest {
    /// The print the turn is about.
    pub print_id: PrintId,
    /// The event that prompted the turn.
    pub event: EventRecord,
    /// The absolute path of the image to look at, when there is one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub image_path: Option<PathBuf>,
    /// The command the turn runs to read the print's context.
    pub context_command: String,
}

/// What one supervision turn answered with.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(crate = "printobserver_types::serde", deny_unknown_fields)]
#[schemars(crate = "printobserver_types::schemars")]
pub struct TurnOutcome {
    /// The session the turn ran in.
    pub session: SupervisionSession,
    /// Whether this turn opened that session or continued it.
    pub phase: SessionPhase,
    /// The agent's written record of the turn.
    pub assessment: AgentAssessment,
}

/// Why a supervision turn did not produce an assessment.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SupervisorError {
    /// The agent answered something that failed validation.
    InvalidAnswer {
        /// What was wrong with the answer.
        detail: String,
    },
    /// The harness refused the identity the turn ran under.
    IdentityRefused {
        /// What the harness said about it.
        detail: String,
    },
    /// The harness could not be reached, or would not run a turn.
    Unavailable {
        /// What went wrong.
        detail: String,
    },
    /// The turn took too long.
    TimedOut,
}

impl core::fmt::Display for SupervisorError {
    fn fmt(&self, formatter: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::InvalidAnswer { detail } => {
                write!(formatter, "the agent's answer failed validation: {detail}")
            }
            Self::IdentityRefused { detail } => {
                write!(formatter, "the harness refused the identity: {detail}")
            }
            Self::Unavailable { detail } => {
                write!(formatter, "the harness is unavailable: {detail}")
            }
            Self::TimedOut => formatter.write_str("the turn took too long"),
        }
    }
}

impl core::error::Error for SupervisorError {}

/// The port the supervising agent is reached through.
///
/// Every method is asynchronous, the trait is dyn-compatible, and it is
/// shareable across threads, because the supervision core holds it behind
/// `Arc<dyn SupervisorPort>`.
pub trait SupervisorPort: Send + Sync {
    /// Run one supervision turn, opening the session if it is not open.
    fn run_turn(&self, request: TurnRequest)
    -> BoxFuture<'_, Result<TurnOutcome, SupervisorError>>;

    /// Close the session watching one print.
    fn close_session(
        &self,
        print_id: PrintId,
        close_reason: String,
    ) -> BoxFuture<'_, Result<(), SupervisorError>>;
}
