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
use super::ids::{ActionId, InterventionId};
use super::image::ImageRecord;
use super::intervention::{Intervention, InterventionOutcome};
use super::manifest::JobManifest;
use super::policy::{EffectiveBounds, PolicyDecision, RejectionReason, SafetyEnvelope};
use super::print::{ManifestNarrowing, PrintRecord};

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
            obico_print_id: Some(4211),
            file_name: Some("benchy.gcode".to_owned()),
            state: PrinterState::Printing,
            opened_at: Timestamp::sample_full(),
            ended_at: Some(later_instant()),
            end_reason: Some("finished".to_owned()),
            narrowings: vec![ManifestNarrowing::sample_full()],
        }
    }

    fn sample_minimal() -> Self {
        Self {
            id: PrintId::sample_full(),
            obico_print_id: None,
            file_name: None,
            state: PrinterState::Printing,
            opened_at: Timestamp::sample_full(),
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
            source_url: Some("https://obico.example/snapshots/1.jpg".to_owned()),
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
