//! The canonical values of every record this domain declares.
//!
//! One value carrying every field, one carrying every optional field absent,
//! and one per further arm of a union: what the schema set and the round-trip
//! corpus are walked over. They are here rather than beside each record so
//! that the records read as the data they are, and so that the fixed
//! identifiers and instants the samples share are written once.

use std::collections::BTreeMap;

use printobserver_printer_api::{Adjustable, PrinterState};
use printobserver_types::contract::Sample;
use printobserver_types::{EventId, FileName, ImageId, PrintId, Range, Timestamp};

use super::action::{
    AcknowledgementDisposition, ActionKind, ActionRecord, ActionRequest, Actor, ActorClass,
    ExecutionOutcome, PrintAction,
};
use super::charge::{
    ChargeKey, ChargeMethod, FilePosition, PendingCharge, PendingTaken, PrintOutcome,
};
use super::ids::{ActionId, InterventionId, SpoolId};
use super::image::ImageRecord;
use super::intervention::{Intervention, InterventionOutcome};
use super::manifest::JobManifest;
use super::policy::{EffectiveBounds, PolicyDecision, RejectionReason, SafetyEnvelope};
use super::preflight::{Preflight, PreflightTool, PreflightVerdict};
use super::print::{ManifestNarrowing, PrintRecord};
use super::spool::{
    ClearedMark, LoadedSpool, SpoolRecord, SpoolTrust, UnverifiedMark, UnverifiedReason,
};
use super::spool_command::SpoolCommand;

/// A later fixed instant than the one the type crate's sample carries.
fn later_instant() -> Timestamp {
    "2026-03-01T12:30:00Z"
        .parse()
        .expect("a fixed RFC 3339 instant")
}

/// A fixed action identifier.
fn action_id() -> ActionId {
    "0191f0a0-0000-7000-8000-000000000004"
        .parse()
        .expect("a fixed version 7 identifier")
}

/// A fixed intervention identifier.
fn intervention_id() -> InterventionId {
    "0191f0a0-0000-7000-8000-000000000005"
        .parse()
        .expect("a fixed version 7 identifier")
}

/// A fixed spool identifier.
fn spool_id() -> SpoolId {
    "0191f0a0-0000-7000-8000-000000000006"
        .parse()
        .expect("a fixed version 7 identifier")
}

/// A second fixed spool identifier, for the spool a swap replaced.
fn replaced_spool_id() -> SpoolId {
    "0191f0a0-0000-7000-8000-000000000007"
        .parse()
        .expect("a fixed version 7 identifier")
}

impl Sample for SpoolId {
    fn sample_full() -> Self {
        spool_id()
    }
}

impl Sample for ActionId {
    fn sample_full() -> Self {
        action_id()
    }
}

impl Sample for InterventionId {
    fn sample_full() -> Self {
        intervention_id()
    }
}

impl Sample for ManifestNarrowing {
    fn sample_full() -> Self {
        Self {
            adjustable: Adjustable::Feedrate,
            requested: Range::new(0.2, 4.0),
            applied: Range::new(0.5, 1.5),
        }
    }
}

impl Sample for PrintRecord {
    fn sample_full() -> Self {
        Self {
            id: PrintId::sample_full(),
            provider_print_id: Some(4211),
            file_name: Some("benchy.gcode".to_owned()),
            state: PrinterState::Printing,
            opened_at: Timestamp::sample_full(),
            job_started_at: Some(Timestamp::sample_full()),
            job_print_time_s: Some(3024),
            ended_at: Some(later_instant()),
            end_reason: Some("finished".to_owned()),
            narrowings: vec![ManifestNarrowing::sample_full()],
        }
    }

    fn sample_minimal() -> Self {
        Self {
            id: PrintId::sample_full(),
            provider_print_id: None,
            file_name: None,
            state: PrinterState::Printing,
            opened_at: Timestamp::sample_full(),
            job_started_at: None,
            job_print_time_s: None,
            ended_at: None,
            end_reason: None,
            narrowings: vec![],
        }
    }
}

impl Sample for ImageRecord {
    fn sample_full() -> Self {
        Self {
            id: ImageId::sample_full(),
            print_id: PrintId::sample_full(),
            event_id: EventId::sample_full(),
            source_url: Some("https://detector.example/snapshots/1.jpg".to_owned()),
            fetched_at: Timestamp::sample_full(),
            content_type: "image/jpeg".to_owned(),
            byte_len: 51_200,
            sha256: "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855".to_owned(),
            relative_path: "images/0191f0a0/1.jpg".to_owned(),
        }
    }

    fn sample_minimal() -> Self {
        Self {
            source_url: None,
            ..Self::sample_full()
        }
    }
}

impl Sample for Actor {
    fn sample_full() -> Self {
        Self::Agent {
            session_name: "print-0191f0a0".to_owned(),
        }
    }
}

impl Sample for ActorClass {
    fn sample_full() -> Self {
        Self::Agent
    }
}

impl Sample for ActionKind {
    fn sample_full() -> Self {
        Self::SetFeedrateFactor
    }
}

impl Sample for AcknowledgementDisposition {
    fn sample_full() -> Self {
        Self::Watch
    }
}

impl Sample for ExecutionOutcome {
    fn sample_full() -> Self {
        Self::Failed {
            reason: "the printer refused the command".to_owned(),
        }
    }
}

impl Sample for PrintAction {
    fn sample_full() -> Self {
        Self::SetFeedrateFactor {
            factor: 0.8,
            duration_s: Some(600),
            reason: "the first layer is going down fast".to_owned(),
            actor: Actor::sample_full(),
        }
    }

    fn sample_minimal() -> Self {
        Self::SetFeedrateFactor {
            factor: 0.8,
            duration_s: None,
            reason: "the first layer is going down fast".to_owned(),
            actor: Actor::sample_full(),
        }
    }

    fn sample_alternates() -> Vec<Self> {
        let reason = || "the print needs it".to_owned();
        vec![
            Self::Pause {
                reason: reason(),
                actor: Actor::sample_full(),
            },
            Self::Resume {
                reason: reason(),
                actor: Actor::sample_full(),
            },
            Self::Cancel {
                reason: reason(),
                actor: Actor::sample_full(),
            },
            Self::StartPrint {
                file_name: FileName::sample_full(),
                manifest: JobManifest::sample_full(),
                override_filament_check: true,
                reason: reason(),
                actor: Actor::sample_full(),
            },
            Self::SetFlowrateFactor {
                factor: 1.05,
                duration_s: Some(300),
                reason: reason(),
                actor: Actor::sample_full(),
            },
            Self::SetToolTargetC {
                tool: 0,
                target_c: 215.0,
                duration_s: Some(300),
                reason: reason(),
                actor: Actor::sample_full(),
            },
            Self::SetBedTargetC {
                target_c: 60.0,
                duration_s: Some(300),
                reason: reason(),
                actor: Actor::sample_full(),
            },
            Self::SetFanPercent {
                percent: 80.0,
                duration_s: Some(300),
                reason: reason(),
                actor: Actor::sample_full(),
            },
            Self::AcknowledgeFailure {
                event_id: EventId::sample_full(),
                disposition: AcknowledgementDisposition::Watch,
                reason: reason(),
                actor: Actor::Operator,
            },
            Self::ClosePrint {
                outcome: PrintOutcome::Interrupted,
                reason: "the printer was switched off mid-print".to_owned(),
                actor: Actor::Operator,
            },
        ]
    }
}

impl Sample for ActionRequest {
    fn sample_full() -> Self {
        Self {
            action: PrintAction::sample_full(),
            actor: Actor::sample_full(),
            requested_at: Timestamp::sample_full(),
        }
    }

    fn sample_minimal() -> Self {
        Self {
            action: PrintAction::sample_minimal(),
            ..Self::sample_full()
        }
    }
}

impl Sample for ActionRecord {
    fn sample_full() -> Self {
        Self {
            id: action_id(),
            print_id: PrintId::sample_full(),
            request: ActionRequest::sample_full(),
            decision: PolicyDecision::Accepted,
            executed_at: Some(later_instant()),
            outcome: Some(ExecutionOutcome::Succeeded),
        }
    }

    fn sample_minimal() -> Self {
        Self {
            request: ActionRequest::sample_minimal(),
            executed_at: None,
            outcome: None,
            ..Self::sample_full()
        }
    }
}

/// The allowed map both the manifest and the envelope carry.
fn allowed_ranges() -> BTreeMap<Adjustable, Range> {
    BTreeMap::from([
        (Adjustable::Feedrate, Range::new(0.5, 1.5)),
        (Adjustable::ToolTarget { tool: 0 }, Range::new(190.0, 230.0)),
    ])
}

impl Sample for JobManifest {
    fn sample_full() -> Self {
        Self {
            file_name: "benchy.gcode".to_owned(),
            material: "PLA".to_owned(),
            nozzle_diameter_mm: 0.4,
            slicer_profile: "0.2mm standard".to_owned(),
            allowed: allowed_ranges(),
            metadata: BTreeMap::from([("layer_height_mm".to_owned(), "0.2".to_owned())]),
        }
    }
}

impl Sample for SafetyEnvelope {
    fn sample_full() -> Self {
        Self {
            allowed: allowed_ranges(),
            actions: BTreeMap::from([(
                ActorClass::Agent,
                vec![ActionKind::Pause, ActionKind::SetFeedrateFactor],
            )]),
            agent_min_interval_s: 60,
        }
    }
}

impl Sample for EffectiveBounds {
    fn sample_full() -> Self {
        Self {
            allowed: allowed_ranges(),
        }
    }
}

impl Sample for RejectionReason {
    fn sample_full() -> Self {
        Self::OutOfBounds {
            adjustable: Adjustable::Feedrate,
            requested: 4.0,
            allowed: Range::new(0.5, 1.5),
        }
    }

    fn sample_alternates() -> Vec<Self> {
        vec![
            Self::InsufficientFilament {
                tool: 0,
                spool_id: spool_id(),
                spool_name: "Galaxy black".to_owned(),
                required_g: 120.0,
                buffer_g: 12.0,
                remaining_g: 80.0,
                unverified: Some(UnverifiedReason::sample_full()),
            },
            Self::InsufficientFilament {
                tool: 0,
                spool_id: spool_id(),
                spool_name: "Galaxy black".to_owned(),
                required_g: 120.0,
                buffer_g: 12.0,
                remaining_g: 80.0,
                unverified: None,
            },
            Self::UnverifiedSpool {
                tool: 0,
                spool_id: spool_id(),
                spool_name: "Galaxy black".to_owned(),
                reason: UnverifiedReason::sample_full(),
            },
            Self::AnalysisUnavailable {
                file_name: "benchy.gcode".to_owned(),
                waited_s: 300,
            },
            Self::SpoolRefused {
                detail: "spool 0191f0a0 is retired, so it takes no command".to_owned(),
            },
            Self::PrintAlreadyEnded,
            Self::PrintStillActive {
                state: PrinterState::Paused,
            },
            Self::InterventionStillActive {
                intervention_id: InterventionId::sample_full(),
            },
        ]
    }
}

impl Sample for PolicyDecision {
    fn sample_full() -> Self {
        Self::Rejected(RejectionReason::sample_full())
    }
}

impl Sample for InterventionOutcome {
    fn sample_full() -> Self {
        Self::Superseded {
            by: intervention_id(),
        }
    }
}

impl Sample for Intervention {
    fn sample_full() -> Self {
        Self {
            id: intervention_id(),
            print_id: PrintId::sample_full(),
            action_id: action_id(),
            adjustable: Adjustable::Feedrate,
            prior_value: Some(1.0),
            applied_value: 0.8,
            applied_at: Timestamp::sample_full(),
            expires_at: later_instant(),
            restored_at: Some(later_instant()),
            outcome: InterventionOutcome::Restored,
        }
    }

    fn sample_minimal() -> Self {
        Self {
            prior_value: None,
            restored_at: None,
            outcome: InterventionOutcome::StillActive,
            ..Self::sample_full()
        }
    }
}

impl Sample for UnverifiedReason {
    fn sample_full() -> Self {
        Self::UnchargedPrint {
            print_id: PrintId::sample_full(),
        }
    }

    fn sample_alternates() -> Vec<Self> {
        vec![
            Self::UnaccountedPrint {
                file_name: "benchy.gcode".to_owned(),
                printed_at: Timestamp::sample_full(),
            },
            Self::FilamentChangeUnconfirmed {
                print_id: PrintId::sample_full(),
            },
            Self::FilamentChangeUnconfirmedPosition {
                print_id: PrintId::sample_full(),
            },
            Self::TallyReset {
                file_name: "benchy.gcode".to_owned(),
                origin: "local".to_owned(),
            },
        ]
    }
}

impl Sample for UnverifiedMark {
    fn sample_full() -> Self {
        Self {
            reason: UnverifiedReason::sample_full(),
            since: Timestamp::sample_full(),
        }
    }
}

impl Sample for SpoolTrust {
    fn sample_full() -> Self {
        Self::Unverified {
            marks: vec![
                UnverifiedMark::sample_full(),
                UnverifiedMark {
                    reason: UnverifiedReason::FilamentChangeUnconfirmed {
                        print_id: PrintId::sample_full(),
                    },
                    since: later_instant(),
                },
            ],
        }
    }

    fn sample_alternates() -> Vec<Self> {
        vec![Self::Verified]
    }
}

impl Sample for SpoolRecord {
    fn sample_full() -> Self {
        Self {
            id: spool_id(),
            name: "Galaxy black".to_owned(),
            material: "PLA".to_owned(),
            color: Some("black".to_owned()),
            diameter_mm: 1.75,
            density_g_cm3: 1.24,
            initial_g: 1_000.0,
            remaining_g: 742.5,
            tare_g: Some(250.0),
            loaded_on_tool: Some(0),
            trust: SpoolTrust::sample_full(),
            registered_at: Timestamp::sample_full(),
            updated_at: later_instant(),
            retired_at: None,
        }
    }

    fn sample_minimal() -> Self {
        Self {
            color: None,
            tare_g: None,
            loaded_on_tool: None,
            trust: SpoolTrust::Verified,
            retired_at: None,
            ..Self::sample_full()
        }
    }

    fn sample_alternates() -> Vec<Self> {
        vec![Self {
            remaining_g: 12.5,
            loaded_on_tool: None,
            retired_at: Some(later_instant()),
            ..Self::sample_full()
        }]
    }
}

impl Sample for ClearedMark {
    fn sample_full() -> Self {
        Self {
            spool_id: replaced_spool_id(),
            reason: UnverifiedReason::FilamentChangeUnconfirmed {
                print_id: PrintId::sample_full(),
            },
        }
    }
}

impl Sample for LoadedSpool {
    fn sample_full() -> Self {
        Self {
            tool: 0,
            spool_id: spool_id(),
        }
    }
}

impl Sample for PrintOutcome {
    fn sample_full() -> Self {
        Self::Completed
    }

    fn sample_alternates() -> Vec<Self> {
        vec![Self::Cancelled, Self::Failed, Self::Interrupted]
    }
}

impl Sample for ChargeMethod {
    fn sample_full() -> Self {
        Self::GcodeExtrusion
    }
}

impl Sample for ChargeKey {
    fn sample_full() -> Self {
        Self::Print {
            print_id: PrintId::sample_full(),
            tool: 0,
            segment: 1,
        }
    }

    fn sample_alternates() -> Vec<Self> {
        vec![Self::Unobserved {
            origin: "local".to_owned(),
            file_name: "benchy.gcode".to_owned(),
            generation: 2,
            print_number: 7,
            tool: 0,
        }]
    }
}

impl Sample for FilePosition {
    fn sample_full() -> Self {
        Self {
            bytes: 1_761_607,
            size_bytes: Some(4_194_304),
            observed_at: Timestamp::sample_full(),
        }
    }

    fn sample_minimal() -> Self {
        Self {
            size_bytes: None,
            ..Self::sample_full()
        }
    }
}

impl Sample for PendingCharge {
    fn sample_full() -> Self {
        Self {
            charge_key: ChargeKey::sample_full(),
            print_id: PrintId::sample_full(),
            tool: 0,
            length_mm: 1_204.5,
            method: ChargeMethod::GcodeExtrusion,
            analysed_length_mm: Some(5_120.0),
            position: Some(FilePosition::sample_full()),
            note: Some("fed after the filament change at layer 12".to_owned()),
            pended_at: later_instant(),
        }
    }

    fn sample_minimal() -> Self {
        Self {
            method: ChargeMethod::FileFraction,
            analysed_length_mm: None,
            position: None,
            note: None,
            ..Self::sample_full()
        }
    }
}

impl Sample for PendingTaken {
    fn sample_full() -> Self {
        Self {
            charge_key: ChargeKey::sample_full(),
            print_id: PrintId::sample_full(),
        }
    }
}

impl Sample for PreflightVerdict {
    fn sample_full() -> Self {
        Self::Insufficient
    }
}

impl Sample for PreflightTool {
    fn sample_full() -> Self {
        Self {
            tool: 0,
            spool_id: Some(spool_id()),
            spool_name: Some("Galaxy black".to_owned()),
            required_g: Some(120.0),
            buffer_g: Some(12.0),
            remaining_g: Some(80.0),
            marks: vec![UnverifiedMark::sample_full()],
            verdict: PreflightVerdict::Insufficient,
        }
    }

    fn sample_minimal() -> Self {
        Self {
            tool: 1,
            spool_id: None,
            spool_name: None,
            required_g: None,
            buffer_g: None,
            remaining_g: None,
            marks: Vec::new(),
            verdict: PreflightVerdict::Unchecked,
        }
    }
}

impl Sample for Preflight {
    fn sample_full() -> Self {
        Self {
            file_name: "benchy.gcode".to_owned(),
            tools: vec![
                PreflightTool::sample_full(),
                PreflightTool::sample_minimal(),
            ],
            verdict: PreflightVerdict::Insufficient,
            checked_at: Timestamp::sample_full(),
        }
    }
}

impl Sample for SpoolCommand {
    fn sample_full() -> Self {
        Self::RegisterSpool {
            name: "Galaxy black".to_owned(),
            material: "PLA".to_owned(),
            color: Some("black".to_owned()),
            diameter_mm: Some(1.75),
            density_g_cm3: Some(1.24),
            initial_g: 1_000.0,
            remaining_g: Some(742.5),
            tare_g: Some(250.0),
            reason: "a new spool went on the shelf".to_owned(),
            actor: Actor::Operator,
        }
    }

    fn sample_minimal() -> Self {
        Self::RegisterSpool {
            name: "Galaxy black".to_owned(),
            material: "PLA".to_owned(),
            color: None,
            diameter_mm: None,
            density_g_cm3: None,
            initial_g: 1_000.0,
            remaining_g: None,
            tare_g: None,
            reason: "a new spool went on the shelf".to_owned(),
            actor: Actor::Operator,
        }
    }

    fn sample_alternates() -> Vec<Self> {
        let reason = || "the spool on the shelf changed".to_owned();
        vec![
            Self::LoadSpool {
                spool_id: spool_id(),
                tool: 0,
                reason: reason(),
                actor: Actor::Operator,
            },
            Self::UnloadSpool {
                spool_id: spool_id(),
                reason: reason(),
                actor: Actor::Operator,
            },
            Self::AdjustSpool {
                spool_id: spool_id(),
                remaining_g: Some(700.0),
                gross_g: None,
                reason: reason(),
                actor: Actor::Operator,
            },
            Self::AdjustSpool {
                spool_id: spool_id(),
                remaining_g: None,
                gross_g: Some(950.0),
                reason: reason(),
                actor: Actor::sample_full(),
            },
            Self::RetireSpool {
                spool_id: spool_id(),
                reason: reason(),
                actor: Actor::Operator,
            },
        ]
    }
}
