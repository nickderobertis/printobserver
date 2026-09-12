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
use crate::event::{EventBody, EventKind, EventPayload, EventRecord, EventSource};
use crate::file_name::FileName;
use crate::ids::{ActionId, EventId, ImageId, InterventionId, PrintId};
use crate::image::{ImageRecord, ImageRef};
use crate::intervention::{Intervention, InterventionOutcome};
use crate::manifest::JobManifest;
use crate::policy::{EffectiveBounds, PolicyDecision, RejectionReason, SafetyEnvelope};
use crate::print::{ManifestNarrowing, PrintRecord};
use crate::printer::PrinterState;
use crate::raw::RawBytes;
use crate::reported::{Range, Reported};
use crate::session::{SessionPhase, SupervisionSession};
use crate::timestamp::Timestamp;

/// A canonical value of one type, for the schema set and the round-trip corpus.
pub trait Sample: Sized {
    /// A value carrying every field the type declares.
    fn sample_full() -> Self;

    /// A value carrying every optional field absent.
    #[must_use]
    fn sample_minimal() -> Self {
        Self::sample_full()
    }

    /// One value per further arm, for a type whose wire form is a union.
    ///
    /// A union's arms declare fields the other arms do not, so a corpus of one
    /// value per type would leave those fields undriven. Every arm a type
    /// declares beyond the one `sample_full` carries belongs here.
    #[must_use]
    fn sample_alternates() -> Vec<Self> {
        Vec::new()
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
    /// One serialized value per further arm of a union.
    alternates: fn() -> Vec<Value>,
}

impl TypeContract {
    /// The contract of one type, under the name its schema is written under.
    ///
    /// This is how a crate other than this one declares a type of its own into
    /// the schema set: its `schemas` test lists its types through this and
    /// reconciles the files under `schemas/<crate>/` against them, walking the
    /// same round trip and the same absent-optional rule this crate's own
    /// contract tests walk.
    #[must_use]
    pub fn of<T: Sample + JsonSchema + Serialize + DeserializeOwned>(name: &'static str) -> Self {
        Self {
            name,
            schema: schema_of::<T>,
            full: || canonical(&T::sample_full()),
            minimal: || canonical(&T::sample_minimal()),
            round_trip: round_trip_as::<T>,
            alternates: || T::sample_alternates().iter().map(canonical).collect(),
        }
    }

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

    /// Every canonical value of this type: the full one, the minimal one, and
    /// one per further arm of a union.
    #[must_use]
    pub fn samples(&self) -> Vec<Value> {
        let mut values = vec![self.full(), self.minimal()];
        values.extend((self.alternates)());
        values
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

/// The member an event payload's schema carries naming the kind it is under.
pub const EVENT_KIND_MARKER: &str = "x-event-kind";

/// The JSON Schema one event payload emits, marked with the kind it is under.
///
/// The schema [`schema_of`] answers for the payload type, plus one top-level
/// member, `x-event-kind`, whose value is the type's own
/// [`KIND`](EventPayload::KIND). Nothing writes that member by hand: the crate
/// that owns the kind writes its schema file through this, and the clients'
/// kind-to-payload tables are generated from the marker.
///
/// # Panics
///
/// Panics if the generated schema is not an object, which it is by
/// construction for a payload type.
#[must_use]
pub fn event_schema_of<P: EventPayload>() -> Value {
    let mut schema = schema_of::<P>();
    schema
        .as_object_mut()
        .expect("a payload's schema is an object")
        .insert(EVENT_KIND_MARKER.to_owned(), Value::from(P::KIND));
    schema
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
        TypeContract::of::<$ty>($name)
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
        ActionId,
        ActionKind,
        ActionRecord,
        ActionRequest,
        Actor,
        ActorClass,
        Adjustable,
        AgentAssessment,
        Confidence,
        EffectiveBounds,
        EventId,
        EventBody,
        EventKind,
        EventRecord,
        EventSource,
        ExecutionOutcome,
        FileName,
        ImageId,
        ImageRecord,
        ImageRef,
        Intervention,
        InterventionId,
        InterventionOutcome,
        JobManifest,
        ManifestNarrowing,
        PolicyDecision,
        PrintAction,
        PrintId,
        PrintRecord,
        PrinterState,
        Range,
        RawBytes,
        RejectionReason,
        Reported<f64> => "Reported",
        SafetyEnvelope,
        SessionPhase,
        SupervisionSession,
        Timestamp,
    ]
}

/// The types the port crates own, which this crate declares none of.
///
/// Each names a shape one port's methods carry, or an event kind one port
/// declares, rather than a shape the whole workspace holds, so it is declared
/// by the crate whose methods carry it. Declaring one here would either
/// duplicate it or relocate it out of the port that owns it.
pub const PORT_OWNED_TYPES: [&str; 14] = [
    "PrinterError",
    "VisionError",
    "SupervisorError",
    "StoreError",
    "NormalizedAlert",
    "FetchedImage",
    "ProviderPrint",
    "MalformedExternalEventPayload",
    "TurnRequest",
    "TurnOutcome",
    "SupervisionSessionOpenedPayload",
    "SupervisionSessionClosedPayload",
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
        Self::new(1.0, Range::sample_full())
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
                event_id: event_id(),
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
        Self::new("sample_source")
    }
}

impl Sample for EventKind {
    fn sample_full() -> Self {
        Self::new("sample_event").expect("a fixed kind name")
    }
}

impl Sample for EventBody {
    fn sample_full() -> Self {
        Self {
            kind: EventKind::sample_full(),
            payload: serde_json::json!({ "detail": "a payload in the form its kind declares" }),
        }
    }
}

impl Sample for EventRecord {
    fn sample_full() -> Self {
        Self {
            id: event_id(),
            print_id: Some(print_id()),
            source: EventSource::sample_full(),
            received_at: instant(),
            image: Some(ImageRef::sample_full()),
            body: EventBody::sample_full(),
            raw: Some(RawBytes::sample_full()),
        }
    }

    fn sample_minimal() -> Self {
        Self {
            print_id: None,
            image: None,
            raw: None,
            ..Self::sample_full()
        }
    }
}

/// One field as a generated schema declares it.
///
/// This is how the contract tests read a type's fields: off the schema the type
/// generates rather than off a table maintained beside it.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct WireField {
    /// The field's name on the wire.
    pub name: String,
    /// What the field is: a referenced type's name, or a primitive's.
    pub descriptor: String,
    /// Whether an instance must carry it.
    pub required: bool,
}

/// The name a reference names.
fn referenced(reference: &str) -> String {
    reference.rsplit('/').next().unwrap_or(reference).to_owned()
}

/// The one non-null arm of a union, when there is exactly one.
fn sole_arm(arms: &[Value]) -> Option<&Value> {
    let mut kept = arms
        .iter()
        .filter(|arm| arm.get("type") != Some(&Value::from("null")));
    let first = kept.next()?;
    kept.next().is_none().then_some(first)
}

/// What one property schema declares the field to be.
fn descriptor(schema: &Value) -> String {
    // `schemars` emits the boolean schema `true` for a JSON value of any form,
    // and, where the value carries a description, the empty object schema.
    if schema == &Value::Bool(true) {
        return "any".to_owned();
    }
    if let Some(object) = schema.as_object()
        && object.keys().all(|key| key == "description")
    {
        return "any".to_owned();
    }
    if let Some(Value::String(reference)) = schema.get("$ref") {
        return referenced(reference);
    }
    for key in ["anyOf", "oneOf"] {
        if let Some(Value::Array(arms)) = schema.get(key) {
            return match sole_arm(arms) {
                Some(arm) => descriptor(arm),
                None => "enum".to_owned(),
            };
        }
    }
    let kind = match schema.get("type") {
        Some(Value::String(name)) => name.clone(),
        Some(Value::Array(names)) => names
            .iter()
            .filter_map(Value::as_str)
            .find(|name| *name != "null")
            .unwrap_or("unknown")
            .to_owned(),
        _ => "unknown".to_owned(),
    };
    match kind.as_str() {
        "array" => format!(
            "array:{}",
            schema
                .get("items")
                .map_or_else(|| "unknown".to_owned(), descriptor)
        ),
        "object" => match (
            schema.get("additionalProperties"),
            schema.get("patternProperties"),
        ) {
            (Some(value), _) if value.is_object() => format!("map:{}", descriptor(value)),
            (_, Some(Value::Object(patterns))) if patterns.len() == 1 => {
                let value = patterns.values().next().expect("one pattern");
                format!("map:{}", descriptor(value))
            }
            _ => "object".to_owned(),
        },
        other => other.to_owned(),
    }
}

/// The names one schema object marks required.
fn required_names(schema: &Value) -> Vec<&str> {
    schema
        .get("required")
        .and_then(Value::as_array)
        .map(|names| names.iter().filter_map(Value::as_str).collect())
        .unwrap_or_default()
}

/// Read the fields off one object schema into a map.
fn collect(schema: &Value, into: &mut BTreeMap<String, WireField>) {
    let required = required_names(schema);
    let Some(Value::Object(properties)) = schema.get("properties") else {
        return;
    };
    for (name, property) in properties {
        into.insert(
            name.clone(),
            WireField {
                name: name.clone(),
                descriptor: descriptor(property),
                required: required.contains(&name.as_str()),
            },
        );
    }
}

/// Every field one generated schema declares, in name order.
///
/// A schema whose type flattens a tagged enum declares some of its fields
/// inside the union's arms rather than beside them, so this reads both: a field
/// every arm requires is required, a field no arm declares is absent, and a
/// field the arms declare at differing types is `union`.
#[must_use]
pub fn wire_fields(schema: &Value) -> Vec<WireField> {
    let mut fields = BTreeMap::new();
    collect(schema, &mut fields);
    for key in ["anyOf", "oneOf", "allOf"] {
        let Some(Value::Array(arms)) = schema.get(key) else {
            continue;
        };
        let mut from_arms: BTreeMap<String, WireField> = BTreeMap::new();
        for arm in arms {
            let mut one = BTreeMap::new();
            collect(arm, &mut one);
            for (name, field) in one {
                from_arms
                    .entry(name)
                    .and_modify(|held| {
                        held.required &= field.required;
                        if held.descriptor != field.descriptor {
                            "union".clone_into(&mut held.descriptor);
                        }
                    })
                    .or_insert(field);
            }
        }
        fields.extend(from_arms);
    }
    fields.into_values().collect()
}
