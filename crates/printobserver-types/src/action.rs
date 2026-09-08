//! The closed vocabulary an actor may ask for, and the record of one request.
//!
//! [`PrintAction`] has no arbitrary-command variant, no G-code field and no
//! escape hatch, which is the whole point of the vocabulary being closed: it is
//! the reason an agent is allowed near a machine that can destroy itself. The
//! only free-form text any variant carries is the `reason` every action
//! carries, and the only caller-supplied text that reaches the printer's own
//! file API is a [`FileName`](crate::FileName), which validates itself.
//!
//! Filtration is deliberately not a variant: nobody has yet confirmed that the
//! target printer exposes filtration through `OctoPrint` at all, and an action
//! the machine cannot perform is one an agent can call that silently does
//! nothing.

use serde::{Deserialize, Serialize};

use schemars::JsonSchema;

use crate::file_name::FileName;
use crate::ids::{ActionId, EventId, PrintId};
use crate::manifest::JobManifest;
use crate::policy::PolicyDecision;
use crate::timestamp::Timestamp;

/// Who asked for something.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum Actor {
    /// The supervising agent, naming its session.
    Agent {
        /// The supervision session the agent is acting in.
        session_name: String,
    },
    /// A person.
    Operator,
    /// The supervisor itself.
    System,
}

impl Actor {
    /// The class this actor belongs to.
    #[must_use]
    pub const fn class(&self) -> ActorClass {
        match self {
            Self::Agent { .. } => ActorClass::Agent,
            Self::Operator => ActorClass::Operator,
            Self::System => ActorClass::System,
        }
    }
}

/// An actor class, which is what a safety envelope grants actions to.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize, JsonSchema,
)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum ActorClass {
    /// The supervising agent.
    Agent,
    /// A person.
    Operator,
    /// The supervisor itself.
    System,
}

/// What an operator's acknowledgement of a failure event asks for next.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum AcknowledgementDisposition {
    /// Carry on printing.
    Continue,
    /// Carry on printing, watched more closely.
    Watch,
    /// Stop the print.
    Stop,
}

/// One action of the closed vocabulary, named without its payload.
///
/// This is what a safety envelope grants and what a policy rejection names; the
/// payload lives on [`PrintAction`] itself.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize, JsonSchema,
)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum ActionKind {
    /// Pause the print.
    Pause,
    /// Resume the print.
    Resume,
    /// Cancel the print.
    Cancel,
    /// Start a print of a named file.
    StartPrint,
    /// Set the feedrate factor.
    SetFeedrateFactor,
    /// Set the flowrate factor.
    SetFlowrateFactor,
    /// Set a tool's target temperature.
    SetToolTargetC,
    /// Set the bed's target temperature.
    SetBedTargetC,
    /// Set the fan percentage.
    SetFanPercent,
    /// Acknowledge a failure event.
    AcknowledgeFailure,
}

/// The whole vocabulary an actor may ask for, and there is no other.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "action", rename_all = "snake_case", deny_unknown_fields)]
pub enum PrintAction {
    /// Pause the print.
    Pause {
        /// Why the actor is asking for this.
        reason: String,
        /// Who is asking.
        actor: Actor,
    },
    /// Resume the print.
    Resume {
        /// Why the actor is asking for this.
        reason: String,
        /// Who is asking.
        actor: Actor,
    },
    /// Cancel the print.
    Cancel {
        /// Why the actor is asking for this.
        reason: String,
        /// Who is asking.
        actor: Actor,
    },
    /// Start a print of a named file, with a manifest.
    StartPrint {
        /// The file to print, validated as a name a file API can be asked for.
        file_name: FileName,
        /// The manifest this print is bounded by.
        manifest: JobManifest,
        /// Why the actor is asking for this.
        reason: String,
        /// Who is asking.
        actor: Actor,
    },
    /// Set the feedrate factor, where one means one hundred percent.
    SetFeedrateFactor {
        /// The multiplier asked for.
        factor: f64,
        /// How long the change stands for, in whole seconds.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        duration_s: Option<i64>,
        /// Why the actor is asking for this.
        reason: String,
        /// Who is asking.
        actor: Actor,
    },
    /// Set the flowrate factor, where one means one hundred percent.
    SetFlowrateFactor {
        /// The multiplier asked for.
        factor: f64,
        /// How long the change stands for, in whole seconds.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        duration_s: Option<i64>,
        /// Why the actor is asking for this.
        reason: String,
        /// Who is asking.
        actor: Actor,
    },
    /// Set one tool's target temperature.
    SetToolTargetC {
        /// The tool, in the printer's own numbering.
        tool: i64,
        /// The target temperature asked for, in degrees Celsius.
        target_c: f64,
        /// How long the change stands for, in whole seconds.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        duration_s: Option<i64>,
        /// Why the actor is asking for this.
        reason: String,
        /// Who is asking.
        actor: Actor,
    },
    /// Set the bed's target temperature.
    SetBedTargetC {
        /// The target temperature asked for, in degrees Celsius.
        target_c: f64,
        /// How long the change stands for, in whole seconds.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        duration_s: Option<i64>,
        /// Why the actor is asking for this.
        reason: String,
        /// Who is asking.
        actor: Actor,
    },
    /// Set the part-cooling fan percentage.
    SetFanPercent {
        /// The percentage asked for.
        percent: f64,
        /// How long the change stands for, in whole seconds.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        duration_s: Option<i64>,
        /// Why the actor is asking for this.
        reason: String,
        /// Who is asking.
        actor: Actor,
    },
    /// Acknowledge a failure event, with a disposition.
    AcknowledgeFailure {
        /// The event being acknowledged.
        event_id: EventId,
        /// What to do next.
        disposition: AcknowledgementDisposition,
        /// Why the actor is asking for this.
        reason: String,
        /// Who is asking.
        actor: Actor,
    },
}

impl PrintAction {
    /// Which action this is, without its payload.
    #[must_use]
    pub const fn kind(&self) -> ActionKind {
        match self {
            Self::Pause { .. } => ActionKind::Pause,
            Self::Resume { .. } => ActionKind::Resume,
            Self::Cancel { .. } => ActionKind::Cancel,
            Self::StartPrint { .. } => ActionKind::StartPrint,
            Self::SetFeedrateFactor { .. } => ActionKind::SetFeedrateFactor,
            Self::SetFlowrateFactor { .. } => ActionKind::SetFlowrateFactor,
            Self::SetToolTargetC { .. } => ActionKind::SetToolTargetC,
            Self::SetBedTargetC { .. } => ActionKind::SetBedTargetC,
            Self::SetFanPercent { .. } => ActionKind::SetFanPercent,
            Self::AcknowledgeFailure { .. } => ActionKind::AcknowledgeFailure,
        }
    }

    /// Why the actor asked for this.
    #[must_use]
    pub fn reason(&self) -> &str {
        match self {
            Self::Pause { reason, .. }
            | Self::Resume { reason, .. }
            | Self::Cancel { reason, .. }
            | Self::StartPrint { reason, .. }
            | Self::SetFeedrateFactor { reason, .. }
            | Self::SetFlowrateFactor { reason, .. }
            | Self::SetToolTargetC { reason, .. }
            | Self::SetBedTargetC { reason, .. }
            | Self::SetFanPercent { reason, .. }
            | Self::AcknowledgeFailure { reason, .. } => reason,
        }
    }

    /// Who asked for this.
    #[must_use]
    pub const fn actor(&self) -> &Actor {
        match self {
            Self::Pause { actor, .. }
            | Self::Resume { actor, .. }
            | Self::Cancel { actor, .. }
            | Self::StartPrint { actor, .. }
            | Self::SetFeedrateFactor { actor, .. }
            | Self::SetFlowrateFactor { actor, .. }
            | Self::SetToolTargetC { actor, .. }
            | Self::SetBedTargetC { actor, .. }
            | Self::SetFanPercent { actor, .. }
            | Self::AcknowledgeFailure { actor, .. } => actor,
        }
    }
}

/// What happened when an accepted action reached the printer.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum ExecutionOutcome {
    /// The printer took it.
    Succeeded,
    /// The printer did not, for this reason.
    Failed {
        /// Why it failed.
        reason: String,
    },
}

/// One request an actor made, at the instant it made it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ActionRequest {
    /// What was asked for.
    pub action: PrintAction,
    /// Who asked.
    pub actor: Actor,
    /// When they asked.
    pub requested_at: Timestamp,
}

/// One request, the decision taken on it, and what became of it.
///
/// `executed_at` and `outcome` are both optional and are absent together: a
/// record is written when the decision is taken, which is before — and, for a
/// rejected request, instead of — anything reaching the printer.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ActionRecord {
    /// This record's identifier, minted by the store.
    pub id: ActionId,
    /// The print this request was made against.
    pub print_id: PrintId,
    /// The request itself.
    pub request: ActionRequest,
    /// The decision policy took on it.
    pub decision: PolicyDecision,
    /// When it reached the printer, if it did.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub executed_at: Option<Timestamp>,
    /// What the printer made of it, if it reached the printer.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub outcome: Option<ExecutionOutcome>,
}
