//! Every record this domain declares into the schema set, with its canonical
//! values.
//!
//! Shared by the `schemas` test, which reconciles the checked-in schema files
//! against them, the `records` test, which walks their wire forms, and the
//! `vocabulary` test, which holds the action vocabulary closed — so the three
//! read one list rather than each keeping its own.

use printobserver_core::{
    AcknowledgementDisposition, ActionId, ActionKind, ActionRecord, ActionRequest, Actor,
    ActorClass, EffectiveBounds, ExecutionOutcome, ImageRecord, Intervention, InterventionId,
    InterventionOutcome, JobManifest, ManifestNarrowing, PolicyDecision, PrintAction, PrintRecord,
    RejectionReason, SafetyEnvelope,
};
use printobserver_types::contract::TypeContract;

/// Every record this domain declares into the schema set.
pub fn declared() -> Vec<TypeContract> {
    vec![
        TypeContract::of::<AcknowledgementDisposition>("AcknowledgementDisposition"),
        TypeContract::of::<ActionId>("ActionId"),
        TypeContract::of::<ActionKind>("ActionKind"),
        TypeContract::of::<ActionRecord>("ActionRecord"),
        TypeContract::of::<ActionRequest>("ActionRequest"),
        TypeContract::of::<Actor>("Actor"),
        TypeContract::of::<ActorClass>("ActorClass"),
        TypeContract::of::<EffectiveBounds>("EffectiveBounds"),
        TypeContract::of::<ExecutionOutcome>("ExecutionOutcome"),
        TypeContract::of::<ImageRecord>("ImageRecord"),
        TypeContract::of::<Intervention>("Intervention"),
        TypeContract::of::<InterventionId>("InterventionId"),
        TypeContract::of::<InterventionOutcome>("InterventionOutcome"),
        TypeContract::of::<JobManifest>("JobManifest"),
        TypeContract::of::<ManifestNarrowing>("ManifestNarrowing"),
        TypeContract::of::<PolicyDecision>("PolicyDecision"),
        TypeContract::of::<PrintAction>("PrintAction"),
        TypeContract::of::<PrintRecord>("PrintRecord"),
        TypeContract::of::<RejectionReason>("RejectionReason"),
        TypeContract::of::<SafetyEnvelope>("SafetyEnvelope"),
    ]
}
