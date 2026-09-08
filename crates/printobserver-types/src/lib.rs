//! `printobserver-types`.
//!
//! Owns: the shared domain vocabulary every other crate speaks — printer and
//! job identity, observed printer state, vision observations, supervisory
//! decisions, and their serialized forms. It holds data and total functions
//! over that data, never I/O.
//!
//! May depend on: no crate of this workspace. It is the root of the graph, so a
//! type it does not hold is a type the rest of the workspace cannot agree on.
//!
//! # One authoritative source per restated fact
//!
//! The types here are declared once and imported by every consumer rather than
//! re-declared. The JSON Schemas the clients and the agent's structured answer
//! are validated against are **generated from these types** by the
//! `printobserver-types:schemas` graph target, which the gate runs: a schema
//! checked into `schemas/` that no longer matches what these types generate
//! fails that target. Nothing transcribes a schema by hand, and nothing
//! restates a field.
//!
//! [`contract`] is where that generation reads the declared set from: it is
//! this crate's own claim about which types it declares and what a canonical
//! value of each looks like.
//!
//! # The representations, stated once
//!
//! * **Identifiers.** Every identifier this system mints is a UUID version 7,
//!   minted by the store, held as a distinct newtype per record, and serialized
//!   as its lowercase hyphenated string. See [`ids`].
//! * **Numbers.** Every temperature, factor, multiplier, percentage and
//!   fraction is a 64-bit float. Every byte count and every integer identifier
//!   an external system supplies is a 64-bit signed integer. Every duration is
//!   whole seconds as a 64-bit signed integer.
//! * **Timestamps.** Every timestamp is an instant in UTC, serialized as an RFC
//!   3339 string with a zero offset. See [`Timestamp`].
//! * **Optionality.** An optional field absent means *the source did not report
//!   it*, and is never rendered as a zero, an empty string, or a default.
//!   Absent serializes as absent and parses back as absent.
//! * **Declared ranges.** A numeric field this crate declares a range for is
//!   typed [`Reported<f64>`]. See [`reported`] for the whole rule, including
//!   why a non-finite value is out of range and why one is refused on emission.
//!
//! # Why `serde`, `schemars` and `serde_json` are re-exported
//!
//! The four port crates depend on this crate and on nothing else, and each of
//! them declares request and answer shapes that cross a process boundary and so
//! must derive the same traits these types derive. They reach the derive macros
//! through [`serde`], [`schemars`] and [`serde_json`] here rather than by
//! declaring a dependency of their own.

pub mod action;
pub mod adjustable;
pub mod assessment;
pub mod context;
pub mod contract;
pub mod event;
pub mod file_name;
pub mod ids;
pub mod image;
pub mod intervention;
pub mod manifest;
pub mod obico;
pub mod policy;
pub mod print;
pub mod printer;
pub mod raw;
pub mod reported;
pub mod session;
pub mod timestamp;

pub use action::{
    AcknowledgementDisposition, ActionKind, ActionRecord, ActionRequest, Actor, ActorClass,
    ExecutionOutcome, PrintAction,
};
pub use adjustable::{Adjustable, AdjustableError};
pub use assessment::{AgentAssessment, Confidence};
pub use context::PrintContext;
pub use contract::{RANGED_FIELDS, RangedField, Sample, TypeContract, WireField, declared, wire_fields};
pub use event::{
    ActionExecutedPayload, ActionRejectedPayload, ActionRequestedPayload, AgentAssessmentPayload,
    EventKind, EventPayload, EventRecord, EventSource, InterventionExpiredPayload,
    MalformedExternalEventPayload, ObicoFailureAlertPayload, ObicoNotificationType,
    ObicoPrinterNotificationPayload, OperatorAcknowledgementPayload,
    SupervisionSessionClosedPayload, SupervisionSessionOpenedPayload,
};
pub use file_name::{FileName, FileNameError, FileNameRefusal, SEPARATORS};
pub use ids::{ActionId, EventId, IdentifierError, ImageId, InterventionId, PrintId};
pub use image::{ImageRecord, ImageRef};
pub use intervention::{Intervention, InterventionOutcome};
pub use manifest::JobManifest;
pub use obico::{
    ObicoEventType, ObicoFailureAlert, ObicoFailureEvent, ObicoFailureEventType,
    ObicoNotificationEvent, ObicoPrintInfo, ObicoPrinterInfo, ObicoPrinterNotification,
    ObicoTimestamp,
};
pub use policy::{EffectiveBounds, PolicyDecision, RejectionReason, SafetyEnvelope};
pub use print::{ManifestNarrowing, PrintRecord};
pub use printer::{HeaterSnapshot, JobSnapshot, PrinterSnapshot, PrinterState};
pub use raw::RawBytes;
pub use reported::{
    COMPLETION_RANGE, FAN_PERCENT_RANGE, FEEDRATE_FACTOR_RANGE, FLOWRATE_FACTOR_RANGE,
    HEATER_ACTUAL_C_RANGE, HEATER_OFFSET_C_RANGE, HEATER_TARGET_C_RANGE, Range, Reported,
};
pub use session::{SessionPhase, SupervisionSession};
pub use timestamp::{Timestamp, TimestampError};

pub use schemars;
pub use serde;
pub use serde_json;
