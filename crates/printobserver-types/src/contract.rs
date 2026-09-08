//! This crate's own claim about the types it declares.
//!
//! Every type this crate declares as its own is listed in [`declared`], with
//! the JSON Schema it emits and a canonical value of it. This is what the
//! `printobserver-types:schemas` graph target generates the checked-in schemas
//! from, and what this crate's contract tests walk, so that a type cannot fall
//! out of the schema set or out of the round-trip corpus by being forgotten in
//! a list somebody maintains by hand.
//!
//! Each entry carries two values rather than one: `full`, which carries every
//! field the type declares, and `minimal`, which carries every optional field
//! absent. The second is what proves that an absent optional serializes as
//! absent rather than as `null`, a zero or an empty string.

use std::collections::BTreeMap;

use schemars::{JsonSchema, generate::SchemaSettings};
use serde::Serialize;
use serde::de::DeserializeOwned;
use serde_json::Value;

use crate::action::{
    AcknowledgementDisposition, ActionKind, ActionRecord, ActionRequest, Actor, ActorClass,
    ExecutionOutcome, PrintAction,
};
use crate::adjustable::Adjustable;
use crate::assessment::{AgentAssessment, Confidence};
use crate::context::PrintContext;
use crate::event::{
    ActionExecutedPayload, ActionRejectedPayload, ActionRequestedPayload, AgentAssessmentPayload,
    EventKind, EventPayload, EventRecord, EventSource, InterventionExpiredPayload,
    MalformedExternalEventPayload, ObicoFailureAlertPayload, ObicoNotificationType,
    ObicoPrinterNotificationPayload, OperatorAcknowledgementPayload,
    SupervisionSessionClosedPayload, SupervisionSessionOpenedPayload,
};
use crate::file_name::FileName;
use crate::ids::{ActionId, EventId, ImageId, InterventionId, PrintId};
use crate::image::{ImageRecord, ImageRef};
use crate::intervention::{Intervention, InterventionOutcome};
use crate::manifest::JobManifest;
use crate::obico::{
    ObicoEventType, ObicoFailureAlert, ObicoFailureEvent, ObicoFailureEventType,
    ObicoNotificationEvent, ObicoPrintInfo, ObicoPrinterInfo, ObicoPrinterNotification,
    ObicoTimestamp,
};
use crate::policy::{EffectiveBounds, PolicyDecision, RejectionReason, SafetyEnvelope};
use crate::print::{ManifestNarrowing, PrintRecord};
use crate::printer::{HeaterSnapshot, JobSnapshot, PrinterSnapshot, PrinterState};
use crate::raw::RawBytes;
use crate::reported::{
    COMPLETION_RANGE, FAN_PERCENT_RANGE, FEEDRATE_FACTOR_RANGE, FLOWRATE_FACTOR_RANGE,
    HEATER_ACTUAL_C_RANGE, HEATER_OFFSET_C_RANGE, HEATER_TARGET_C_RANGE, Range, Reported,
};
use crate::session::{SessionPhase, SupervisionSession};
use crate::timestamp::Timestamp;

/// One field this crate declares a plausibility range for.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RangedField {
    /// The type the field belongs to.
    pub type_name: &'static str,
    /// The field's own name.
    pub field: &'static str,
    /// The plausibility range declared for it.
    pub range: Range,
}

/// Every field this crate types as [`Reported<f64>`], with its declared range.
///
/// These are validity ranges on what a printer can plausibly report. They are
/// not the operator's [`SafetyEnvelope`], and nothing intersects, compares or
/// substitutes the two.
pub const RANGED_FIELDS: [RangedField; 7] = [
    RangedField {
        type_name: "PrinterSnapshot",
        field: "feedrate_factor",
        range: FEEDRATE_FACTOR_RANGE,
    },
    RangedField {
        type_name: "PrinterSnapshot",
        field: "flowrate_factor",
        range: FLOWRATE_FACTOR_RANGE,
    },
    RangedField {
        type_name: "PrinterSnapshot",
        field: "fan_percent",
        range: FAN_PERCENT_RANGE,
    },
    RangedField {
        type_name: "JobSnapshot",
        field: "completion",
        range: COMPLETION_RANGE,
    },
    RangedField {
        type_name: "HeaterSnapshot",
        field: "actual_c",
        range: HEATER_ACTUAL_C_RANGE,
    },
    RangedField {
        type_name: "HeaterSnapshot",
        field: "target_c",
        range: HEATER_TARGET_C_RANGE,
    },
    RangedField {
        type_name: "HeaterSnapshot",
        field: "offset_c",
        range: HEATER_OFFSET_C_RANGE,
    },
];

/// A canonical value of one type, for the schema set and the round-trip corpus.
pub trait Sample: Sized {
    /// A value carrying every field the type declares.
    fn sample_full() -> Self;

    /// A value carrying every optional field absent.
    #[must_use]
    fn sample_minimal() -> Self {
        Self::sample_full()
    }
}

/// One type this crate declares, with its schema and its canonical values.
#[derive(Debug, Clone, Copy)]
pub struct TypeContract {
    /// The type's own name, which is also its schema's file name.
    pub name: &'static str,
    /// The JSON Schema the type emits.
    schema: fn() -> Value,
    /// A value carrying every field the type declares, serialized.
    full: fn() -> Value,
    /// A value carrying every optional field absent, serialized.
    minimal: fn() -> Value,
    /// Parse a value as this type and serialize it back.
    round_trip: fn(Value) -> Result<Value, String>,
}

impl TypeContract {
    /// The JSON Schema this type emits.
    #[must_use]
    pub fn schema(&self) -> Value {
        (self.schema)()
    }

    /// A serialized value carrying every field the type declares.
    #[must_use]
    pub fn full(&self) -> Value {
        (self.full)()
    }

    /// A serialized value carrying every optional field absent.
    #[must_use]
    pub fn minimal(&self) -> Value {
        (self.minimal)()
    }

    /// Parse a value as this type and serialize the result back.
    ///
    /// # Errors
    ///
    /// Returns the parse or emission failure, as the message it carried.
    pub fn round_trip(&self, value: Value) -> Result<Value, String> {
        (self.round_trip)(value)
    }
}

/// The JSON Schema one type emits, under this crate's own generator settings.
///
/// Sub-schemas are referenced rather than inlined, so that a schema names the
/// types it is built from rather than flattening them into anonymous objects.
///
/// # Panics
///
/// Panics if the generated schema is not JSON, which it is by construction.
#[must_use]
pub fn schema_of<T: JsonSchema>() -> Value {
    let generator = SchemaSettings::draft2020_12().into_generator();
    serde_json::to_value(generator.into_root_schema_for::<T>())
        .expect("a generated schema is JSON by construction")
}

/// Serialize a value, reporting a refusal as the message it carried.
fn emit<T: Serialize>(value: &T) -> Result<Value, String> {
    serde_json::to_value(value).map_err(|error| error.to_string())
}

/// Parse a value as `T` and serialize it back.
fn round_trip_as<T: Serialize + DeserializeOwned>(value: Value) -> Result<Value, String> {
    let parsed: T = serde_json::from_value(value).map_err(|error| error.to_string())?;
    emit(&parsed)
}

/// Serialize a canonical value, which this crate's own types never refuse.
fn canonical<T: Serialize>(value: &T) -> Value {
    emit(value).expect("a canonical sample serializes")
}

/// One type's contract, under its own name or under a name given for it.
macro_rules! contract_of {
    ($ty:ty) => {
        contract_of!($ty, stringify!($ty))
    };
    ($ty:ty, $name:expr) => {
        TypeContract {
            name: $name,
            schema: schema_of::<$ty>,
            full: || canonical(&<$ty as Sample>::sample_full()),
            minimal: || canonical(&<$ty as Sample>::sample_minimal()),
            round_trip: round_trip_as::<$ty>,
        }
    };
}

/// Declare the contract of each named type.
macro_rules! declare {
    ($($ty:ty $(=> $name:literal)?),* $(,)?) => {
        vec![$( contract_of!($ty $(, $name)?) ),*]
    };
}

/// Every type this crate declares as its own.
#[must_use]
pub fn declared() -> Vec<TypeContract> {
    declare![
        AcknowledgementDisposition,
        ActionExecutedPayload,
        ActionId,
        ActionKind,
        ActionRecord,
        ActionRejectedPayload,
        ActionRequest,
        ActionRequestedPayload,
        Actor,
        ActorClass,
        Adjustable,
        AgentAssessment,
        AgentAssessmentPayload,
        Confidence,
        EffectiveBounds,
        EventId,
        EventKind,
        EventPayload,
        EventRecord,
        EventSource,
        ExecutionOutcome,
        FileName,
        HeaterSnapshot,
        ImageId,
        ImageRecord,
        ImageRef,
        Intervention,
        InterventionExpiredPayload,
        InterventionId,
        InterventionOutcome,
        JobManifest,
        JobSnapshot,
        MalformedExternalEventPayload,
        ManifestNarrowing,
        ObicoEventType,
        ObicoFailureAlert,
        ObicoFailureAlertPayload,
        ObicoFailureEvent,
        ObicoFailureEventType,
        ObicoNotificationEvent,
        ObicoNotificationType,
        ObicoPrintInfo,
        ObicoPrinterInfo,
        ObicoPrinterNotification,
        ObicoPrinterNotificationPayload,
        OperatorAcknowledgementPayload,
        PolicyDecision,
        PrintAction,
        PrintContext,
        PrintId,
        PrintRecord,
        PrinterSnapshot,
        PrinterState,
        Range,
        RawBytes,
        RejectionReason,
        Reported<f64> => "Reported",
        SafetyEnvelope,
        SessionPhase,
        SupervisionSession,
        SupervisionSessionClosedPayload,
        SupervisionSessionOpenedPayload,
        Timestamp,
    ]
}

/// The ten types the port crates own, which this crate declares none of.
///
/// Each names a shape one port's methods carry rather than a shape the domain
/// holds, so it is declared by the crate whose methods carry it. Declaring one
/// here would either duplicate it or relocate it out of the port that owns it.
pub const PORT_OWNED_TYPES: [&str; 10] = [
    "PrinterError",
    "VisionError",
    "SupervisorError",
    "StoreError",
    "NormalizedAlert",
    "FetchedImage",
    "TurnRequest",
    "TurnOutcome",
    "EventDraft",
    "HistoryQuery",
];

/// A fixed print identifier, so that a sample is the same on every run.
fn print_id() -> PrintId {
    "0191f0a0-0000-7000-8000-000000000001"
        .parse()
        .expect("a fixed version 7 identifier")
}

/// A fixed event identifier.
fn event_id() -> EventId {
    "0191f0a0-0000-7000-8000-000000000002"
        .parse()
        .expect("a fixed version 7 identifier")
}

/// A fixed image identifier.
fn image_id() -> ImageId {
    "0191f0a0-0000-7000-8000-000000000003"
        .parse()
        .expect("a fixed version 7 identifier")
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

/// A fixed instant, so that a sample is the same on every run.
fn instant() -> Timestamp {
    "2026-03-01T12:00:00Z"
        .parse()
        .expect("a fixed RFC 3339 instant")
}

/// A later fixed instant.
fn later_instant() -> Timestamp {
    "2026-03-01T12:30:00Z"
        .parse()
        .expect("a fixed RFC 3339 instant")
}

impl Sample for PrintId {
    fn sample_full() -> Self {
        print_id()
    }
}

impl Sample for EventId {
    fn sample_full() -> Self {
        event_id()
    }
}

impl Sample for ImageId {
    fn sample_full() -> Self {
        image_id()
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

impl Sample for Timestamp {
    fn sample_full() -> Self {
        instant()
    }
}

impl Sample for FileName {
    fn sample_full() -> Self {
        Self::new("benchy.gcode").expect("a name carrying no forbidden property")
    }
}

impl Sample for RawBytes {
    fn sample_full() -> Self {
        Self::new(b"{\"event\":{}}".to_vec())
    }
}

impl Sample for Range {
    fn sample_full() -> Self {
        Self::new(0.5, 1.5)
    }
}

impl Sample for Reported<f64> {
    fn sample_full() -> Self {
        Self::new(1.0, FEEDRATE_FACTOR_RANGE)
    }
}

impl Sample for Adjustable {
    fn sample_full() -> Self {
        Self::ToolTarget { tool: 0 }
    }
}

impl Sample for PrinterState {
    fn sample_full() -> Self {
        Self::Printing
    }
}

impl Sample for HeaterSnapshot {
    fn sample_full() -> Self {
        Self {
            actual_c: Some(Reported::new(214.5, HEATER_ACTUAL_C_RANGE)),
            target_c: Some(Reported::new(215.0, HEATER_TARGET_C_RANGE)),
            offset_c: Some(Reported::new(0.0, HEATER_OFFSET_C_RANGE)),
        }
    }

    fn sample_minimal() -> Self {
        Self {
            actual_c: None,
            target_c: None,
            offset_c: None,
        }
    }
}

impl Sample for PrinterSnapshot {
    fn sample_full() -> Self {
        Self {
            connection: PrinterState::Printing,
            tools: vec![HeaterSnapshot::sample_full()],
            bed: Some(HeaterSnapshot::sample_full()),
            chamber: Some(HeaterSnapshot::sample_full()),
            feedrate_factor: Some(Reported::new(1.0, FEEDRATE_FACTOR_RANGE)),
            flowrate_factor: Some(Reported::new(1.0, FLOWRATE_FACTOR_RANGE)),
            fan_percent: Some(Reported::new(100.0, FAN_PERCENT_RANGE)),
            observed_at: instant(),
        }
    }

    fn sample_minimal() -> Self {
        Self {
            connection: PrinterState::Printing,
            tools: vec![],
            bed: None,
            chamber: None,
            feedrate_factor: None,
            flowrate_factor: None,
            fan_percent: None,
            observed_at: instant(),
        }
    }
}

impl Sample for JobSnapshot {
    fn sample_full() -> Self {
        Self {
            file_name: Some("benchy.gcode".to_owned()),
            file_origin: Some("local".to_owned()),
            size_bytes: Some(4_194_304),
            estimated_print_time_s: Some(7200),
            completion: Some(Reported::new(0.42, COMPLETION_RANGE)),
            print_time_s: Some(3024),
            print_time_left_s: Some(4176),
            state: PrinterState::Printing,
            error: Some("none".to_owned()),
        }
    }

    fn sample_minimal() -> Self {
        Self {
            file_name: None,
            file_origin: None,
            size_bytes: None,
            estimated_print_time_s: None,
            completion: None,
            print_time_s: None,
            print_time_left_s: None,
            state: PrinterState::Printing,
            error: None,
        }
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
            id: print_id(),
            obico_print_id: Some(4211),
            file_name: Some("benchy.gcode".to_owned()),
            state: PrinterState::Printing,
            opened_at: instant(),
            ended_at: Some(later_instant()),
            end_reason: Some("finished".to_owned()),
            narrowings: vec![ManifestNarrowing::sample_full()],
        }
    }

    fn sample_minimal() -> Self {
        Self {
            id: print_id(),
            obico_print_id: None,
            file_name: None,
            state: PrinterState::Printing,
            opened_at: instant(),
            ended_at: None,
            end_reason: None,
            narrowings: vec![],
        }
    }
}

impl Sample for ImageRecord {
    fn sample_full() -> Self {
        Self {
            id: image_id(),
            print_id: print_id(),
            event_id: event_id(),
            source_url: Some("https://obico.example/snapshots/1.jpg".to_owned()),
            fetched_at: instant(),
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

impl Sample for ImageRef {
    fn sample_full() -> Self {
        Self {
            id: image_id(),
            sha256: "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855".to_owned(),
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
}

impl Sample for ActionRequest {
    fn sample_full() -> Self {
        Self {
            action: PrintAction::sample_full(),
            actor: Actor::sample_full(),
            requested_at: instant(),
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
            print_id: print_id(),
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
            print_id: print_id(),
            action_id: action_id(),
            adjustable: Adjustable::Feedrate,
            prior_value: Some(1.0),
            applied_value: 0.8,
            applied_at: instant(),
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

impl Sample for SessionPhase {
    fn sample_full() -> Self {
        Self::Created
    }
}

impl Sample for SupervisionSession {
    fn sample_full() -> Self {
        Self {
            print_id: print_id(),
            session_name: "print-0191f0a0".to_owned(),
            harness_identity: "printobserver-supervisor".to_owned(),
            created_at: instant(),
            last_turn_at: later_instant(),
            closed_at: Some(later_instant()),
            close_reason: Some("the print ended".to_owned()),
        }
    }

    fn sample_minimal() -> Self {
        Self {
            closed_at: None,
            close_reason: None,
            ..Self::sample_full()
        }
    }
}

impl Sample for Confidence {
    fn sample_full() -> Self {
        Self::Medium
    }
}

impl Sample for AgentAssessment {
    fn sample_full() -> Self {
        Self {
            summary: "the first layer is down and adhesion looks even".to_owned(),
            confidence: Confidence::Medium,
            should_continue: true,
            did: "slowed the feedrate to 80% for ten minutes".to_owned(),
            why: "the extrusion width was widening on the long edges".to_owned(),
            escalating: false,
        }
    }
}

impl Sample for EventSource {
    fn sample_full() -> Self {
        Self::Obico
    }
}

impl Sample for ObicoNotificationType {
    fn sample_full() -> Self {
        Self::Started
    }
}

impl Sample for EventKind {
    fn sample_full() -> Self {
        Self::ObicoFailureAlert
    }
}

impl Sample for ObicoFailureAlertPayload {
    fn sample_full() -> Self {
        Self {
            is_warning: true,
            print_paused: false,
            obico_print_id: Some(4211),
            file_name: Some("benchy.gcode".to_owned()),
        }
    }

    fn sample_minimal() -> Self {
        Self {
            obico_print_id: None,
            file_name: None,
            ..Self::sample_full()
        }
    }
}

impl Sample for ObicoPrinterNotificationPayload {
    fn sample_full() -> Self {
        Self {
            notification_type: ObicoNotificationType::Started,
            obico_print_id: Some(4211),
            file_name: Some("benchy.gcode".to_owned()),
        }
    }

    fn sample_minimal() -> Self {
        Self {
            obico_print_id: None,
            file_name: None,
            ..Self::sample_full()
        }
    }
}

impl Sample for MalformedExternalEventPayload {
    fn sample_full() -> Self {
        Self {
            detail: "the body is not JSON: expected value at line 1 column 1".to_owned(),
        }
    }
}

impl Sample for ActionRequestedPayload {
    fn sample_full() -> Self {
        Self {
            action_id: action_id(),
            action: PrintAction::sample_full(),
            actor: Actor::sample_full(),
        }
    }

    fn sample_minimal() -> Self {
        Self {
            action: PrintAction::sample_minimal(),
            ..Self::sample_full()
        }
    }
}

impl Sample for ActionExecutedPayload {
    fn sample_full() -> Self {
        Self {
            action_id: action_id(),
            intervention_id: Some(intervention_id()),
        }
    }

    fn sample_minimal() -> Self {
        Self {
            action_id: action_id(),
            intervention_id: None,
        }
    }
}

impl Sample for ActionRejectedPayload {
    fn sample_full() -> Self {
        Self {
            action_id: action_id(),
            decision: PolicyDecision::sample_full(),
        }
    }
}

impl Sample for InterventionExpiredPayload {
    fn sample_full() -> Self {
        Self {
            intervention_id: intervention_id(),
            adjustable: Adjustable::Feedrate,
            outcome: InterventionOutcome::Restored,
        }
    }
}

impl Sample for SupervisionSessionOpenedPayload {
    fn sample_full() -> Self {
        Self {
            session_name: "print-0191f0a0".to_owned(),
            harness_identity: "printobserver-supervisor".to_owned(),
        }
    }
}

impl Sample for SupervisionSessionClosedPayload {
    fn sample_full() -> Self {
        Self {
            session_name: "print-0191f0a0".to_owned(),
            close_reason: "the print ended".to_owned(),
        }
    }
}

impl Sample for AgentAssessmentPayload {
    fn sample_full() -> Self {
        Self {
            session_name: "print-0191f0a0".to_owned(),
            assessment: AgentAssessment::sample_full(),
        }
    }
}

impl Sample for OperatorAcknowledgementPayload {
    fn sample_full() -> Self {
        Self {
            acknowledged_event_id: event_id(),
            disposition: AcknowledgementDisposition::Watch,
        }
    }
}

impl Sample for EventPayload {
    fn sample_full() -> Self {
        Self::ObicoFailureAlert(ObicoFailureAlertPayload::sample_full())
    }

    fn sample_minimal() -> Self {
        Self::ObicoFailureAlert(ObicoFailureAlertPayload::sample_minimal())
    }
}

impl Sample for EventRecord {
    fn sample_full() -> Self {
        Self {
            id: event_id(),
            print_id: Some(print_id()),
            source: EventSource::Obico,
            received_at: instant(),
            image: Some(ImageRef::sample_full()),
            payload: EventPayload::sample_full(),
            raw: Some(RawBytes::sample_full()),
        }
    }

    fn sample_minimal() -> Self {
        Self {
            print_id: None,
            image: None,
            raw: None,
            payload: EventPayload::sample_minimal(),
            ..Self::sample_full()
        }
    }
}

impl Sample for PrintContext {
    fn sample_full() -> Self {
        Self {
            print: PrintRecord::sample_full(),
            printer: Some(PrinterSnapshot::sample_full()),
            job: Some(JobSnapshot::sample_full()),
            manifest: Some(JobManifest::sample_full()),
            bounds: EffectiveBounds::sample_full(),
            interventions: vec![Intervention::sample_full()],
            recent_events: vec![EventRecord::sample_full()],
            latest_image: Some(ImageRef::sample_full()),
        }
    }

    fn sample_minimal() -> Self {
        Self {
            print: PrintRecord::sample_minimal(),
            printer: None,
            job: None,
            manifest: None,
            bounds: EffectiveBounds::sample_full(),
            interventions: vec![],
            recent_events: vec![],
            latest_image: None,
        }
    }
}

impl Sample for ObicoFailureEventType {
    fn sample_full() -> Self {
        Self::PrintFailure
    }
}

impl Sample for ObicoEventType {
    fn sample_full() -> Self {
        Self::PrintStarted
    }
}

impl Sample for ObicoTimestamp {
    fn sample_full() -> Self {
        Self::Seconds(1_772_366_400.5)
    }
}

impl Sample for ObicoPrinterInfo {
    fn sample_full() -> Self {
        Self {
            id: 17,
            name: "Prusa MK4".to_owned(),
        }
    }
}

impl Sample for ObicoPrintInfo {
    fn sample_full() -> Self {
        Self {
            id: 4211,
            filename: "benchy.gcode".to_owned(),
            started_at: ObicoTimestamp::Seconds(1_772_366_400.5),
            ended_at: ObicoTimestamp::NotReported,
        }
    }
}

impl Sample for ObicoFailureEvent {
    fn sample_full() -> Self {
        Self {
            event_type: ObicoFailureEventType::PrintFailure,
            is_warning: true,
            print_paused: false,
        }
    }
}

impl Sample for ObicoNotificationEvent {
    fn sample_full() -> Self {
        Self {
            event_type: ObicoEventType::PrintStarted,
            is_warning: false,
            print_paused: false,
        }
    }
}

impl Sample for ObicoFailureAlert {
    fn sample_full() -> Self {
        Self {
            event: ObicoFailureEvent::sample_full(),
            printer: ObicoPrinterInfo::sample_full(),
            print: ObicoPrintInfo::sample_full(),
            img_url: "https://obico.example/snapshots/4211.jpg".to_owned(),
        }
    }
}

impl Sample for ObicoPrinterNotification {
    fn sample_full() -> Self {
        Self {
            event: ObicoNotificationEvent::sample_full(),
            printer: ObicoPrinterInfo::sample_full(),
            print: Some(ObicoPrintInfo::sample_full()),
            img_url: Some("https://obico.example/snapshots/4211.jpg".to_owned()),
        }
    }

    fn sample_minimal() -> Self {
        Self {
            print: None,
            img_url: None,
            ..Self::sample_full()
        }
    }
}
