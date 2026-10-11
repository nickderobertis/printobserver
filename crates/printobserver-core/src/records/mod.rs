//! The supervision domain's own records, and the identifiers it mints.
//!
//! What this domain persists and serves is declared here: the print
//! ([`print`]), the action an actor asks for and the record of it
//! ([`action`]), the bounded intervention an action opens ([`intervention`]),
//! the manifest a job declares ([`manifest`]), the safety envelope and the
//! decision policy takes ([`policy`]), the image stored beside an event
//! ([`image`]), and the filament ledger — a registered spool and how far its
//! figure can be trusted ([`spool`]), the commands that change one
//! ([`spool_command`]), what a print took from one ([`charge`]) and the check
//! made before a print starts ([`preflight`]) — with the three identifiers
//! only this domain keys records by ([`ids`]), minted through the type crate's exported rule. Each is declared
//! into the schema set under `schemas/printobserver-core/` by this crate's own
//! `schemas` test, through the type crate's [`TypeContract::of`]; nothing
//! central lists them.
//!
//! [`TypeContract::of`]: printobserver_types::contract::TypeContract::of

pub mod action;
pub mod charge;
pub mod ids;
pub mod image;
pub mod intervention;
pub mod manifest;
pub mod policy;
pub mod preflight;
pub mod print;
mod samples;
pub mod spool;
pub mod spool_command;

// The printer vocabulary these records carry — the state a print is in, and
// the adjustable a narrowing, an envelope's bound and an intervention name —
// re-exported beside them, so that a consumer of the records reaches what a
// record's field holds without naming the printer port itself.
pub use printobserver_printer_api::{Adjustable, PrinterState};

pub use action::{
    AcknowledgementDisposition, ActionKind, ActionRecord, ActionRequest, Actor, ActorClass,
    ExecutionOutcome, PrintAction,
};
pub use charge::{
    ChargeKey, ChargeMethod, FilePosition, PendingCharge, PendingTaken, PrintOutcome,
};
pub use ids::{ActionId, InterventionId, SpoolId};
pub use image::ImageRecord;
pub use intervention::{Intervention, InterventionOutcome};
pub use manifest::JobManifest;
pub use policy::{EffectiveBounds, PolicyDecision, RejectionReason, SafetyEnvelope};
pub use preflight::{Preflight, PreflightTool, PreflightVerdict};
pub use print::{ManifestNarrowing, PrintRecord};
pub use spool::{
    ClearedMark, LoadedSpool, SpoolRecord, SpoolTrust, UnverifiedMark, UnverifiedReason,
    UnverifiedReasonKind,
};
pub use spool_command::SpoolCommand;
