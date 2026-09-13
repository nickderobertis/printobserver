//! The supervision domain's own records, and the identifiers it mints.
//!
//! What this domain persists and serves is declared here: the print
//! ([`print`]), the action an actor asks for and the record of it
//! ([`action`]), the bounded intervention an action opens ([`intervention`]),
//! the manifest a job declares ([`manifest`]), the safety envelope and the
//! decision policy takes ([`policy`]), and the image stored beside an event
//! ([`image`]) — with the two identifiers only this domain keys records by
//! ([`ids`]), minted through the type crate's exported rule. Each is declared
//! into the schema set under `schemas/printobserver-core/` by this crate's own
//! `schemas` test, through the type crate's [`TypeContract::of`]; nothing
//! central lists them.
//!
//! [`TypeContract::of`]: printobserver_types::contract::TypeContract::of

pub mod action;
pub mod ids;
pub mod image;
pub mod intervention;
pub mod manifest;
pub mod policy;
pub mod print;
mod samples;

pub use action::{
    AcknowledgementDisposition, ActionKind, ActionRecord, ActionRequest, Actor, ActorClass,
    ExecutionOutcome, PrintAction,
};
pub use ids::{ActionId, InterventionId};
pub use image::ImageRecord;
pub use intervention::{Intervention, InterventionOutcome};
pub use manifest::JobManifest;
pub use policy::{EffectiveBounds, PolicyDecision, RejectionReason, SafetyEnvelope};
pub use print::{ManifestNarrowing, PrintRecord};
