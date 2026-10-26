//! Every record this domain declares into the schema set, with its canonical
//! values.
//!
//! Shared by the `schemas` test, which reconciles the checked-in schema files
//! against them, the `records` test, which walks their wire forms, and the
//! `vocabulary` test, which holds the action vocabulary closed — so the three
//! read one list rather than each keeping its own.

use printobserver_core::{
    AcknowledgementDisposition, ActionId, ActionKind, ActionRecord, ActionRequest, Actor,
    ActorClass, ChargeKey, ChargeMethod, ChargedFilament, ClearedMark, DoubtedSpool,
    EffectiveBounds, ExecutionOutcome, FilePosition, ImageRecord, Intervention, InterventionId,
    InterventionOutcome, JobManifest, LoadedSpool, ManifestNarrowing, PendingCharge, PendingTaken,
    PolicyDecision, Preflight, PreflightNeed, PreflightObjection, PreflightSpool, PreflightTool,
    PreflightToolResult, PreflightVerdict, PrintAction, PrintOutcome, PrintRecord, PrintSegmentKey,
    RejectionReason, SafetyEnvelope, ShortSpool, SpoolCommand, SpoolField, SpoolId, SpoolLifecycle,
    SpoolRecord, SpoolRefusalCause, SpoolTrust, UnanalysedSpool, UnverifiedMark, UnverifiedReason,
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
        TypeContract::of::<ChargedFilament>("ChargedFilament"),
        TypeContract::of::<ClearedMark>("ClearedMark"),
        TypeContract::of::<DoubtedSpool>("DoubtedSpool"),
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
        TypeContract::of::<PreflightNeed>("PreflightNeed"),
        TypeContract::of::<PreflightObjection>("PreflightObjection"),
        TypeContract::of::<PreflightSpool>("PreflightSpool"),
        TypeContract::of::<PreflightTool>("PreflightTool"),
        TypeContract::of::<PreflightToolResult>("PreflightToolResult"),
        TypeContract::of::<PreflightVerdict>("PreflightVerdict"),
        TypeContract::of::<PrintAction>("PrintAction"),
        TypeContract::of::<PrintOutcome>("PrintOutcome"),
        TypeContract::of::<PrintRecord>("PrintRecord"),
        TypeContract::of::<PrintSegmentKey>("PrintSegmentKey"),
        TypeContract::of::<RejectionReason>("RejectionReason"),
        TypeContract::of::<SafetyEnvelope>("SafetyEnvelope"),
        TypeContract::of::<ShortSpool>("ShortSpool"),
        TypeContract::of::<SpoolCommand>("SpoolCommand"),
        TypeContract::of::<SpoolField>("SpoolField"),
        TypeContract::of::<SpoolId>("SpoolId"),
        TypeContract::of::<SpoolLifecycle>("SpoolLifecycle"),
        TypeContract::of::<SpoolRecord>("SpoolRecord"),
        TypeContract::of::<SpoolRefusalCause>("SpoolRefusalCause"),
        TypeContract::of::<SpoolTrust>("SpoolTrust"),
        TypeContract::of::<UnanalysedSpool>("UnanalysedSpool"),
        TypeContract::of::<UnverifiedMark>("UnverifiedMark"),
        TypeContract::of::<UnverifiedReason>("UnverifiedReason"),
    ]
}
