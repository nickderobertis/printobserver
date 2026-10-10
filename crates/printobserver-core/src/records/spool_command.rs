//! The closed vocabulary of things an actor may do to the spool ledger.
//!
//! Each command is an action of its own kind, so a safety envelope grants it
//! and the history records it exactly as it does a print action, and none of
//! them asks anything of the machine.

use printobserver_types::schemars::JsonSchema;
use printobserver_types::serde::{Deserialize, Serialize};

use super::action::{ActionKind, Actor};
use super::ids::SpoolId;

/// The whole vocabulary of the spool ledger, and there is no other.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(
    crate = "printobserver_types::serde",
    tag = "command",
    rename_all = "snake_case",
    deny_unknown_fields
)]
#[schemars(crate = "printobserver_types::schemars")]
pub enum SpoolCommand {
    /// Register a spool.
    RegisterSpool {
        /// The operator's own label for it.
        name: String,
        /// The material, such as `PLA`.
        material: String,
        /// The filament's colour.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        color: Option<String>,
        /// The filament's diameter, in millimetres; 1.75 when absent.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        diameter_mm: Option<f64>,
        /// The material's density, in grams per cubic centimetre; the known
        /// density of the material when absent, and a material with none known
        /// is refused.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        density_g_cm3: Option<f64>,
        /// The net filament on the spool when full, in grams.
        initial_g: f64,
        /// The filament on it now, in grams; the full figure when absent.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        remaining_g: Option<f64>,
        /// The empty spool's own weight, in grams.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        tare_g: Option<f64>,
        /// Why the actor is asking for this.
        reason: String,
        /// Who is asking.
        actor: Actor,
    },
    /// Load a spool onto a tool.
    ///
    /// Onto an occupied tool it is a swap, and the spool there is unloaded in
    /// the same write. Naming the spool already on the tool confirms it was
    /// never swapped, which is accepted only while that tool holds a pending
    /// charge or that spool holds a filament-change mark.
    LoadSpool {
        /// The spool to load.
        spool_id: SpoolId,
        /// The tool, in the printer's own numbering.
        tool: i64,
        /// Why the actor is asking for this.
        reason: String,
        /// Who is asking.
        actor: Actor,
    },
    /// Unload a spool from its tool.
    UnloadSpool {
        /// The spool to unload.
        spool_id: SpoolId,
        /// Why the actor is asking for this.
        reason: String,
        /// Who is asking.
        actor: Actor,
    },
    /// Correct a spool's remaining figure, from exactly one of the two.
    AdjustSpool {
        /// The spool to correct.
        spool_id: SpoolId,
        /// The filament left, in grams.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        remaining_g: Option<f64>,
        /// A scale reading of the spool, in grams, which needs its tare.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        gross_g: Option<f64>,
        /// Why the actor is asking for this.
        reason: String,
        /// Who is asking.
        actor: Actor,
    },
    /// Retire a spool, after which it refuses every command.
    RetireSpool {
        /// The spool to retire.
        spool_id: SpoolId,
        /// Why the actor is asking for this.
        reason: String,
        /// Who is asking.
        actor: Actor,
    },
}

impl SpoolCommand {
    /// Which action this is, without its payload.
    #[must_use]
    pub const fn kind(&self) -> ActionKind {
        match self {
            Self::RegisterSpool { .. } => ActionKind::RegisterSpool,
            Self::LoadSpool { .. } => ActionKind::LoadSpool,
            Self::UnloadSpool { .. } => ActionKind::UnloadSpool,
            Self::AdjustSpool { .. } => ActionKind::AdjustSpool,
            Self::RetireSpool { .. } => ActionKind::RetireSpool,
        }
    }

    /// Why the actor asked for this.
    #[must_use]
    pub fn reason(&self) -> &str {
        match self {
            Self::RegisterSpool { reason, .. }
            | Self::LoadSpool { reason, .. }
            | Self::UnloadSpool { reason, .. }
            | Self::AdjustSpool { reason, .. }
            | Self::RetireSpool { reason, .. } => reason,
        }
    }

    /// Who asked for this.
    #[must_use]
    pub const fn actor(&self) -> &Actor {
        match self {
            Self::RegisterSpool { actor, .. }
            | Self::LoadSpool { actor, .. }
            | Self::UnloadSpool { actor, .. }
            | Self::AdjustSpool { actor, .. }
            | Self::RetireSpool { actor, .. } => actor,
        }
    }
}
