//! A registered spool, and how far its remaining figure can be trusted.
//!
//! A spool's remaining figure is only worth having when it can be explained, so
//! a spool carries every reason its figure is in doubt rather than one flag: a
//! mark is added when something happened that could not be charged exactly,
//! and is cleared only by the command that settles that doubt. The rule for
//! which command clears which mark is implemented once, by
//! [`marks_cleared_by`](crate::filament::marks_cleared_by).

use printobserver_types::schemars::JsonSchema;
use printobserver_types::serde::de::Error as _;
use printobserver_types::serde::{Deserialize, Deserializer, Serialize};
use printobserver_types::{PrintId, Timestamp};

use super::ids::SpoolId;

/// One spool the operator registered, and what is known of it now.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(crate = "printobserver_types::serde", deny_unknown_fields)]
#[schemars(crate = "printobserver_types::schemars")]
pub struct SpoolRecord {
    /// This spool's identifier, minted when it was registered.
    pub id: SpoolId,
    /// The operator's own label for the spool; never empty.
    pub name: String,
    /// The material, as given and trimmed, such as `PLA`; never empty.
    pub material: String,
    /// The filament's colour, when the operator gave one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub color: Option<String>,
    /// The filament's diameter, in millimetres; above zero.
    pub diameter_mm: f64,
    /// The material's density, in grams per cubic centimetre; above zero.
    pub density_g_cm3: f64,
    /// The net filament on the spool when it was full, in grams; above zero.
    pub initial_g: f64,
    /// The filament left, in grams.
    ///
    /// Always the initial figure less every charge plus every correction. A
    /// charge is never refused or clamped, so this may go below zero.
    pub remaining_g: f64,
    /// The empty spool's own weight, in grams, when the operator gave it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tare_g: Option<f64>,
    /// The tool the spool is loaded on, when it is loaded.
    // llmlint: ignore[invalid_states_unrepresentable] suppressions.toml has the reason.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub loaded_on_tool: Option<i64>,
    /// How far the remaining figure can be trusted.
    pub trust: SpoolTrust,
    /// When the spool was registered.
    pub registered_at: Timestamp,
    /// When anything about the spool last changed.
    pub updated_at: Timestamp,
    /// When the spool was retired, if it has been.
    // llmlint: ignore[invalid_states_unrepresentable] suppressions.toml has the reason.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub retired_at: Option<Timestamp>,
}

/// How far a spool's remaining figure can be trusted.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(
    crate = "printobserver_types::serde",
    tag = "state",
    rename_all = "snake_case",
    deny_unknown_fields
)]
#[schemars(crate = "printobserver_types::schemars")]
pub enum SpoolTrust {
    /// Every charge against the spool was taken exactly.
    Verified,
    /// At least one thing happened that could not be charged exactly.
    Unverified {
        /// Every reason the figure is in doubt, oldest first, at most one per
        /// distinct reason; never empty.
        // llmlint: ignore[invalid_states_unrepresentable] suppressions.toml has the reason.
        #[serde(deserialize_with = "at_least_one_mark")]
        #[schemars(length(min = 1))]
        marks: Vec<UnverifiedMark>,
    },
}

/// Read an unverified spool's marks, refusing none: a spool holding no mark is
/// `verified`, and one written as unverified with nothing to say why is not a
/// state this ledger has.
fn at_least_one_mark<'de, D: Deserializer<'de>>(
    deserializer: D,
) -> Result<Vec<UnverifiedMark>, D::Error> {
    let marks = Vec::<UnverifiedMark>::deserialize(deserializer)?;
    if marks.is_empty() {
        return Err(D::Error::custom(
            "an unverified spool carries at least one mark; one with none is verified",
        ));
    }
    Ok(marks)
}

/// One reason a spool's figure is in doubt, and since when.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(crate = "printobserver_types::serde", deny_unknown_fields)]
#[schemars(crate = "printobserver_types::schemars")]
pub struct UnverifiedMark {
    /// Why the figure is in doubt.
    pub reason: UnverifiedReason,
    /// When the spool was marked for it.
    pub since: Timestamp,
}

/// Why a spool's figure is in doubt.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(
    crate = "printobserver_types::serde",
    tag = "reason",
    rename_all = "snake_case",
    deny_unknown_fields
)]
#[schemars(crate = "printobserver_types::schemars")]
pub enum UnverifiedReason {
    /// A print ran on the spool's tool and nothing could be charged, because
    /// the file had no analysis — an SD-card file, for one.
    UnchargedPrint {
        /// The print nothing could be charged for.
        print_id: PrintId,
    },
    /// A print this system never saw ran and did not complete.
    UnaccountedPrint {
        /// The file that was printed.
        file_name: String,
        /// When it was printed.
        printed_at: Timestamp,
    },
    /// A filament change was signalled and nobody has said which spool is now
    /// loaded. The spool was charged up to a position read when the signal was
    /// handled.
    FilamentChangeUnconfirmed {
        /// The print the change was signalled during.
        print_id: PrintId,
    },
    /// A filament change was signalled, nobody has said which spool is now
    /// loaded, and no fresh position could be read at the signal: the spool was
    /// charged only up to the last recorded position, so its figure may be
    /// overstated.
    FilamentChangeUnconfirmedPosition {
        /// The print the change was signalled during.
        print_id: PrintId,
    },
    /// A file's print tally was reset, and it cannot be told which of the
    /// prints since the re-upload were already settled.
    TallyReset {
        /// The file whose tally was reset.
        file_name: String,
        /// Where the file lives, in the printer's own vocabulary.
        origin: String,
    },
}

/// The five reasons a spool may be marked for, without their payloads.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(crate = "printobserver_types::serde", rename_all = "snake_case")]
pub enum UnverifiedReasonKind {
    /// [`UnverifiedReason::UnchargedPrint`].
    UnchargedPrint,
    /// [`UnverifiedReason::UnaccountedPrint`].
    UnaccountedPrint,
    /// [`UnverifiedReason::FilamentChangeUnconfirmed`].
    FilamentChangeUnconfirmed,
    /// [`UnverifiedReason::FilamentChangeUnconfirmedPosition`].
    FilamentChangeUnconfirmedPosition,
    /// [`UnverifiedReason::TallyReset`].
    TallyReset,
}

impl UnverifiedReason {
    /// Which reason this is, without its payload.
    #[must_use]
    pub const fn kind(&self) -> UnverifiedReasonKind {
        match self {
            Self::UnchargedPrint { .. } => UnverifiedReasonKind::UnchargedPrint,
            Self::UnaccountedPrint { .. } => UnverifiedReasonKind::UnaccountedPrint,
            Self::FilamentChangeUnconfirmed { .. } => {
                UnverifiedReasonKind::FilamentChangeUnconfirmed
            }
            Self::FilamentChangeUnconfirmedPosition { .. } => {
                UnverifiedReasonKind::FilamentChangeUnconfirmedPosition
            }
            Self::TallyReset { .. } => UnverifiedReasonKind::TallyReset,
        }
    }
}

/// One mark a spool command cleared, and the spool it was cleared from.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(crate = "printobserver_types::serde", deny_unknown_fields)]
#[schemars(crate = "printobserver_types::schemars")]
pub struct ClearedMark {
    /// The spool the mark was cleared from.
    pub spool_id: SpoolId,
    /// The reason that no longer stands.
    pub reason: UnverifiedReason,
}

/// One spool loaded on one tool, as a print started.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(crate = "printobserver_types::serde", deny_unknown_fields)]
#[schemars(crate = "printobserver_types::schemars")]
pub struct LoadedSpool {
    /// The tool, in the printer's own numbering.
    pub tool: i64,
    /// The spool loaded on it.
    pub spool_id: SpoolId,
}
