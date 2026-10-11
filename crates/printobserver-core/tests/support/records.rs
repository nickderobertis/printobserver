//! Every record this domain declares into the schema set, with its canonical
//! values.
//!
//! Shared by the `schemas` test, which reconciles the checked-in schema files
//! against them, the `records` test, which walks their wire forms, and the
//! `vocabulary` test, which holds the action vocabulary closed — so the three
//! read one list rather than each keeping its own.

use printobserver_core::{
    AcknowledgementDisposition, ActionId, ActionKind, ActionRecord, ActionRequest, Actor,
    ActorClass, ChargeKey, ChargeMethod, ClearedMark, EffectiveBounds, ExecutionOutcome,
    FilePosition, ImageRecord, Intervention, InterventionId, InterventionOutcome, JobManifest,
    LoadedSpool, ManifestNarrowing, PendingCharge, PendingTaken, PolicyDecision, Preflight,
    PreflightTool, PreflightVerdict, PrintAction, PrintOutcome, PrintRecord, RejectionReason,
    SafetyEnvelope, SpoolCommand, SpoolId, SpoolRecord, SpoolTrust, UnverifiedMark,
    UnverifiedReason,
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
        TypeContract::of::<ChargeKey>("ChargeKey"),
        TypeContract::of::<ChargeMethod>("ChargeMethod"),
        TypeContract::of::<ClearedMark>("ClearedMark"),
        TypeContract::of::<EffectiveBounds>("EffectiveBounds"),
        TypeContract::of::<ExecutionOutcome>("ExecutionOutcome"),
        TypeContract::of::<FilePosition>("FilePosition"),
        TypeContract::of::<ImageRecord>("ImageRecord"),
        TypeContract::of::<Intervention>("Intervention"),
        TypeContract::of::<InterventionId>("InterventionId"),
        TypeContract::of::<InterventionOutcome>("InterventionOutcome"),
        TypeContract::of::<JobManifest>("JobManifest"),
        TypeContract::of::<LoadedSpool>("LoadedSpool"),
        TypeContract::of::<ManifestNarrowing>("ManifestNarrowing"),
        TypeContract::of::<PendingCharge>("PendingCharge"),
        TypeContract::of::<PendingTaken>("PendingTaken"),
        TypeContract::of::<PolicyDecision>("PolicyDecision"),
        TypeContract::of::<Preflight>("Preflight"),
        TypeContract::of::<PreflightTool>("PreflightTool"),
        TypeContract::of::<PreflightVerdict>("PreflightVerdict"),
        TypeContract::of::<PrintAction>("PrintAction"),
        TypeContract::of::<PrintOutcome>("PrintOutcome"),
        TypeContract::of::<PrintRecord>("PrintRecord"),
        TypeContract::of::<RejectionReason>("RejectionReason"),
        TypeContract::of::<SafetyEnvelope>("SafetyEnvelope"),
        TypeContract::of::<SpoolCommand>("SpoolCommand"),
        TypeContract::of::<SpoolId>("SpoolId"),
        TypeContract::of::<SpoolRecord>("SpoolRecord"),
        TypeContract::of::<SpoolTrust>("SpoolTrust"),
        TypeContract::of::<UnverifiedMark>("UnverifiedMark"),
        TypeContract::of::<UnverifiedReason>("UnverifiedReason"),
    ]
}
