//! The closed vocabulary for moving the printer after a print fails, and what
//! it is checked against.
//!
//! [`RecoveryAction`] is the whole of it: lifting the nozzle away from the bed,
//! cooling every heater, setting the part fan, heating one tool, parking the
//! head, homing X and Y, and homing Z. Like [`PrintAction`](super::PrintAction)
//! it has no G-code field and no escape hatch; unlike it, it names no print,
//! because what it moves is the printer whatever the last print left on its
//! bed. Each request is refused with one [`RecoveryRejection`] or carried out,
//! and either way is recorded with the printer's events.
//!
//! What a recovery is checked against is the printer's own facts —
//! [`RecoveryConfig`], the operator's statement of the machine — and who may
//! ask for which action, [`RecoveryGrants`]. Both are configuration rather
//! than code.

use printobserver_printer_api::{Footprint, PrinterState};
use printobserver_types::contract::Sample;
use printobserver_types::schemars::JsonSchema;
use printobserver_types::serde::{Deserialize, Serialize};
use printobserver_types::{PrintId, Range, Timestamp};

use super::action::{ActorClass, ExecutionOutcome};

/// One recovery action, named without its fields: what a grant names and what
/// a refusal of an ungranted request names.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize, JsonSchema,
)]
#[serde(
    crate = "printobserver_types::serde",
    rename_all = "snake_case",
    deny_unknown_fields
)]
#[schemars(crate = "printobserver_types::schemars")]
pub enum RecoveryActionKind {
    /// Move the nozzle away from the bed.
    RecoverLift,
    /// Every heater's target to zero.
    RecoverCool,
    /// Set the part-cooling fan.
    RecoverFan,
    /// Heat one tool.
    RecoverHeat,
    /// Move the head to the configured park place.
    RecoverPark,
    /// Home X and Y.
    RecoverHomeXy,
    /// Home Z.
    RecoverHomeZ,
}

impl RecoveryActionKind {
    /// Every recovery action, in the order the vocabulary declares them.
    pub const ALL: [Self; 7] = [
        Self::RecoverLift,
        Self::RecoverCool,
        Self::RecoverFan,
        Self::RecoverHeat,
        Self::RecoverPark,
        Self::RecoverHomeXy,
        Self::RecoverHomeZ,
    ];

    /// The action's name, as the wire and its operation spell it.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::RecoverLift => "recover_lift",
            Self::RecoverCool => "recover_cool",
            Self::RecoverFan => "recover_fan",
            Self::RecoverHeat => "recover_heat",
            Self::RecoverPark => "recover_park",
            Self::RecoverHomeXy => "recover_home_xy",
            Self::RecoverHomeZ => "recover_home_z",
        }
    }

    /// Whether this action moves the head across the bed or heats a nozzle
    /// over it, and so needs Z trusted and the nozzle clear of the part.
    #[must_use]
    pub const fn needs_clearance(self) -> bool {
        matches!(
            self,
            Self::RecoverHeat | Self::RecoverPark | Self::RecoverHomeXy
        )
    }
}

impl core::fmt::Display for RecoveryActionKind {
    fn fmt(&self, formatter: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        formatter.write_str(self.as_str())
    }
}

/// The whole recovery vocabulary, and there is no other.
///
/// The two assertions are the operator's word for what printobserver cannot
/// see: `clearance_asserted` that the nozzle is clear of the part, and
/// `bed_clear_asserted` that homing Z will meet nothing on the bed. Each is
/// false when left out.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(
    crate = "printobserver_types::serde",
    tag = "action",
    rename_all = "snake_case",
    deny_unknown_fields
)]
#[schemars(crate = "printobserver_types::schemars")]
pub enum RecoveryAction {
    /// Move the nozzle away from the bed by `mm`.
    RecoverLift {
        /// How far, in millimetres: above zero and at most
        /// `recovery.max_lift_mm`.
        mm: f64,
    },
    /// Set every heater's target to zero.
    RecoverCool {},
    /// Set the part-cooling fan.
    RecoverFan {
        /// The percentage, from 0 to 100.
        percent: f64,
    },
    /// Heat one tool, within the safety envelope's range for it.
    RecoverHeat {
        /// The tool, in the printer's own numbering.
        tool: i64,
        /// The target, in degrees Celsius.
        target_c: f64,
        /// The operator's word that the nozzle is clear of the part.
        // llmlint: ignore[invalid_states_unrepresentable] suppressions.toml has the reason.
        #[serde(default)]
        clearance_asserted: bool,
    },
    /// Move the head to `recovery.park_xy`.
    RecoverPark {
        /// The operator's word that the nozzle is clear of the part.
        // llmlint: ignore[invalid_states_unrepresentable] suppressions.toml has the reason.
        #[serde(default)]
        clearance_asserted: bool,
    },
    /// Home X and Y.
    RecoverHomeXy {
        /// The operator's word that the nozzle is clear of the part.
        // llmlint: ignore[invalid_states_unrepresentable] suppressions.toml has the reason.
        #[serde(default)]
        clearance_asserted: bool,
    },
    /// Home Z.
    RecoverHomeZ {
        /// The operator's word that homing Z will meet nothing on the bed.
        // llmlint: ignore[invalid_states_unrepresentable] suppressions.toml has the reason.
        #[serde(default)]
        bed_clear_asserted: bool,
    },
}

impl RecoveryAction {
    /// Which action this is, without its fields.
    #[must_use]
    pub const fn kind(&self) -> RecoveryActionKind {
        match self {
            Self::RecoverLift { .. } => RecoveryActionKind::RecoverLift,
            Self::RecoverCool {} => RecoveryActionKind::RecoverCool,
            Self::RecoverFan { .. } => RecoveryActionKind::RecoverFan,
            Self::RecoverHeat { .. } => RecoveryActionKind::RecoverHeat,
            Self::RecoverPark { .. } => RecoveryActionKind::RecoverPark,
            Self::RecoverHomeXy { .. } => RecoveryActionKind::RecoverHomeXy,
            Self::RecoverHomeZ { .. } => RecoveryActionKind::RecoverHomeZ,
        }
    }

    /// Whether the request carries the operator's word that the nozzle is
    /// clear of the part.
    #[must_use]
    pub const fn clearance_asserted(&self) -> bool {
        match self {
            Self::RecoverHeat {
                clearance_asserted, ..
            }
            | Self::RecoverPark { clearance_asserted }
            | Self::RecoverHomeXy { clearance_asserted } => *clearance_asserted,
            Self::RecoverLift { .. }
            | Self::RecoverCool {}
            | Self::RecoverFan { .. }
            | Self::RecoverHomeZ { .. } => false,
        }
    }
}

/// One field of a recovery action a bound is held to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(crate = "printobserver_types::serde", rename_all = "snake_case")]
#[schemars(crate = "printobserver_types::schemars")]
pub enum RecoveryField {
    /// A lift's distance, in millimetres.
    Mm,
    /// The fan's percentage.
    Percent,
    /// A heat's tool.
    Tool,
    /// A heat's target, in degrees Celsius.
    TargetC,
    /// The operator's word that the nozzle is clear of the part.
    ClearanceAsserted,
    /// The operator's word that homing Z will meet nothing on the bed.
    BedClearAsserted,
}

/// Why a recovery request was refused, each a variant of its own so a
/// consumer tells them apart by matching rather than by reading words.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(
    crate = "printobserver_types::serde",
    rename_all = "snake_case",
    deny_unknown_fields
)]
#[schemars(crate = "printobserver_types::schemars")]
pub enum RecoveryRejection {
    /// The asking actor's class is not granted this action.
    ActorMayNotRequest {
        /// The action asked for.
        action: RecoveryActionKind,
    },
    /// A print is running, or being cancelled: nothing may move the printer
    /// under it.
    PrintRunning {
        /// The state the printer reported.
        state: PrinterState,
    },
    /// The printer could not be read, or reports itself offline or in error.
    PrinterUnreachable,
    /// Nothing printobserver did has told it where Z is, so no move that
    /// depends on the nozzle's height above the part may be made.
    ZUntrusted,
    /// The nozzle is not far enough above the tallest part the last print
    /// laid down.
    InsufficientClearance {
        /// Where the printer reports the nozzle, in millimetres.
        nozzle_z_mm: f64,
        /// The tallest part the last print laid down, in millimetres.
        tallest_mm: f64,
        /// The clearance asked above it, in millimetres.
        margin_mm: f64,
    },
    /// A value the request carries is outside the range allowed for it.
    OutOfBounds {
        /// The field.
        field: RecoveryField,
        /// The value asked for.
        requested: f64,
        /// The range allowed, inclusive.
        allowed: Range,
    },
    /// This action needs an assertion the request did not make.
    AssertionRequired {
        /// The assertion it needs.
        field: RecoveryField,
    },
    /// The configuration states nothing of this printer's recovery facts, so
    /// no recovery action can be checked.
    RecoveryNotConfigured,
    /// The printer cannot do what this action needs: a move its adapter does
    /// not support, or a nozzle position it cannot read.
    PrinterCannot {
        /// The action asked for.
        action: RecoveryActionKind,
        /// What the printer cannot do, in words.
        detail: String,
    },
}

/// Which way along Z moves the nozzle away from the bed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(crate = "printobserver_types::serde", rename_all = "snake_case")]
#[schemars(crate = "printobserver_types::schemars")]
pub enum AwayFromBed {
    /// Up the axis: Z increasing.
    ZPlus,
    /// Down the axis: Z decreasing.
    ZMinus,
}

/// The printer's own facts a recovery is checked against, as the operator
/// states them.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RecoveryConfig {
    /// Which way along Z moves the nozzle away from the bed.
    pub away_from_bed: AwayFromBed,
    /// The highest Z the machine can reach, in millimetres.
    pub max_z_mm: f64,
    /// The longest lift one request may ask for, in millimetres.
    pub max_lift_mm: f64,
    /// How far above the tallest part the nozzle must be, in millimetres.
    pub clearance_margin_mm: f64,
    /// Where the head is parked, in X and Y.
    pub park_xy: [f64; 2],
    /// Where in X and Y the machine homes Z.
    pub z_home_xy: [f64; 2],
    /// Whether homing Z brings a gantry down over the whole bed.
    // llmlint: ignore[invalid_states_unrepresentable] suppressions.toml has the reason.
    pub z_home_sweeps_bed: bool,
}

/// The lift a request may ask for when nothing configures it, in millimetres.
pub const DEFAULT_MAX_LIFT_MM: f64 = 50.0;

/// The clearance asked above the tallest part when nothing configures it, in
/// millimetres.
pub const DEFAULT_CLEARANCE_MARGIN_MM: f64 = 5.0;

/// Who may ask for which recovery action, and whose assertions count.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecoveryGrants {
    /// The actions the operator may ask for.
    pub operator: Vec<RecoveryActionKind>,
    /// The actions the agent may ask for.
    pub agent: Vec<RecoveryActionKind>,
    /// The actor classes whose word that the nozzle or the bed is clear
    /// counts. The agent is never among them.
    pub asserting: Vec<ActorClass>,
}

impl RecoveryGrants {
    /// Whether one actor class may ask for one action. The system asks for
    /// none.
    #[must_use]
    pub fn grants(&self, class: ActorClass, action: RecoveryActionKind) -> bool {
        match class {
            ActorClass::Operator => self.operator.contains(&action),
            ActorClass::Agent => self.agent.contains(&action),
            ActorClass::System => false,
        }
    }

    /// Whether one actor class's assertion counts. The agent's never does:
    /// an assertion is a person's word for what they can see.
    #[must_use]
    pub fn assertion_counts(&self, class: ActorClass) -> bool {
        class != ActorClass::Agent && self.asserting.contains(&class)
    }
}

impl Default for RecoveryGrants {
    /// The operator holds every action, and the agent none; only the
    /// operator's assertions count.
    fn default() -> Self {
        Self {
            operator: RecoveryActionKind::ALL.to_vec(),
            agent: Vec::new(),
            asserting: vec![ActorClass::Operator],
        }
    }
}

/// Declare every field of a shape required, as the null it may be: these
/// shapes send each field always, `null` where there is nothing to report,
/// and the derive would otherwise read an `Option` as a field that may be
/// absent.
fn every_field_is_sent(schema: &mut printobserver_types::schemars::Schema) {
    let named: Vec<printobserver_types::serde_json::Value> = schema
        .get("properties")
        .and_then(printobserver_types::serde_json::Value::as_object)
        .map(|properties| {
            properties
                .keys()
                .map(|name| printobserver_types::serde_json::Value::from(name.as_str()))
                .collect()
        })
        .unwrap_or_default();
    schema.insert(
        "required".to_owned(),
        printobserver_types::serde_json::Value::Array(named),
    );
}

/// Whether Z is trusted, and since when or why not.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(
    crate = "printobserver_types::serde",
    rename_all = "snake_case",
    deny_unknown_fields
)]
#[schemars(crate = "printobserver_types::schemars")]
pub enum ZTrust {
    /// Z is where printobserver last put it, from this instant on: a `home_z`
    /// it executed, or a print it started.
    TrustedSince(Timestamp),
    /// Z is not trusted, and why.
    Untrusted(String),
}

/// Where the print head is, in millimetres, as the printer reported it.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(crate = "printobserver_types::serde", deny_unknown_fields)]
#[schemars(crate = "printobserver_types::schemars")]
pub struct RecoveryPosition {
    /// X.
    pub x: f64,
    /// Y.
    pub y: f64,
    /// Z.
    pub z: f64,
}

/// One tool heater, as a recovery read reports it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(crate = "printobserver_types::serde", deny_unknown_fields)]
#[schemars(crate = "printobserver_types::schemars", transform = every_field_is_sent)]
pub struct ToolTemperature {
    /// The tool, in the printer's own numbering.
    pub tool: i64,
    /// What it reads, in degrees Celsius, `null` when not reported.
    pub actual_c: Option<f64>,
    /// What it is driving towards, in degrees Celsius, `null` when not
    /// reported.
    pub target_c: Option<f64>,
}

/// The bed heater, as a recovery read reports it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(crate = "printobserver_types::serde", deny_unknown_fields)]
#[schemars(crate = "printobserver_types::schemars", transform = every_field_is_sent)]
pub struct BedTemperature {
    /// What it reads, in degrees Celsius, `null` when not reported.
    pub actual_c: Option<f64>,
    /// What it is driving towards, in degrees Celsius, `null` when not
    /// reported.
    pub target_c: Option<f64>,
}

/// Every heater, as a recovery read reports it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(crate = "printobserver_types::serde", deny_unknown_fields)]
#[schemars(crate = "printobserver_types::schemars", transform = every_field_is_sent)]
pub struct RecoveryTemperatures {
    /// Each tool.
    pub tools: Vec<ToolTemperature>,
    /// The bed, `null` when the printer reports none.
    pub bed: Option<BedTemperature>,
}

/// What the last print laid down, read from its own G-code up to the last
/// file position recorded for it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(crate = "printobserver_types::serde", deny_unknown_fields)]
#[schemars(crate = "printobserver_types::schemars")]
pub struct LastPrint {
    /// The print.
    pub print_id: PrintId,
    /// The tallest part it laid down, in millimetres.
    pub tallest_mm: f64,
    /// Where in X and Y it laid material.
    pub footprint: Footprint,
}

/// The print paused under the printer, and whether resuming it is still safe.
///
/// A recovery action executed while it was paused moved the head, and
/// `OctoPrint` restores no position on resume, so the rest of the layer would
/// print at the wrong height: such a print is not resumable.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(crate = "printobserver_types::serde", deny_unknown_fields)]
#[schemars(crate = "printobserver_types::schemars")]
pub struct PausedPrint {
    /// The print.
    pub print_id: PrintId,
    /// Whether it may be resumed.
    // Marked as a two-valued fact, so a client generated from it carries this
    // directive and its reason too.
    #[schemars(extend("x-two-valued-fact" = true))]
    // llmlint: ignore[invalid_states_unrepresentable] suppressions.toml has the reason.
    pub resumable: bool,
}

/// Where the machine homes Z, and whether doing so sweeps the bed.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(crate = "printobserver_types::serde", deny_unknown_fields)]
#[schemars(crate = "printobserver_types::schemars")]
pub struct ZHome {
    /// Where in X Z is homed, in millimetres.
    pub x: f64,
    /// Where in Y Z is homed, in millimetres.
    pub y: f64,
    /// Whether homing Z brings a gantry down over the whole bed.
    // Marked as a two-valued fact, so a client generated from it carries this
    // directive and its reason too.
    #[schemars(extend("x-two-valued-fact" = true))]
    // llmlint: ignore[invalid_states_unrepresentable] suppressions.toml has the reason.
    pub sweeps_bed: bool,
}

/// Whether one recovery action is allowed now, and the refusal it would meet
/// when it is not.
#[derive(Debug, Clone, PartialEq)]
pub struct ActionAllowance {
    /// The action.
    pub action: RecoveryActionKind,
    /// Its refusal, absent when it is allowed.
    pub refusal: Option<RecoveryRejection>,
}

/// The wire form of [`ActionAllowance`]: `allowed`, and a refusal exactly when
/// it is false.
#[derive(Serialize, Deserialize)]
#[serde(crate = "printobserver_types::serde", deny_unknown_fields)]
struct AllowanceWire {
    /// The action.
    action: RecoveryActionKind,
    /// Whether it is allowed now.
    // llmlint: ignore[invalid_states_unrepresentable] suppressions.toml has the reason.
    allowed: bool,
    /// Its refusal, present exactly when it is not allowed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    refusal: Option<RecoveryRejection>,
}

impl Serialize for ActionAllowance {
    fn serialize<S: printobserver_types::serde::Serializer>(
        &self,
        serializer: S,
    ) -> Result<S::Ok, S::Error> {
        AllowanceWire {
            action: self.action,
            allowed: self.refusal.is_none(),
            refusal: self.refusal.clone(),
        }
        .serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for ActionAllowance {
    fn deserialize<D: printobserver_types::serde::Deserializer<'de>>(
        deserializer: D,
    ) -> Result<Self, D::Error> {
        let wire = AllowanceWire::deserialize(deserializer)?;
        match (wire.allowed, wire.refusal) {
            (true, None) => Ok(Self {
                action: wire.action,
                refusal: None,
            }),
            (false, Some(refusal)) => Ok(Self {
                action: wire.action,
                refusal: Some(refusal),
            }),
            (true, Some(_)) => Err(printobserver_types::serde::de::Error::custom(
                "an allowed action carries no refusal",
            )),
            (false, None) => Err(printobserver_types::serde::de::Error::custom(
                "a refused action carries its refusal",
            )),
        }
    }
}

/// The schema is the wire form's own object: `allowed`, and `refusal` present
/// exactly when it is false — stated in the schema itself, and held to the
/// same verdicts as reading one back by `tests/records.rs`.
impl JsonSchema for ActionAllowance {
    fn schema_name() -> std::borrow::Cow<'static, str> {
        std::borrow::Cow::Borrowed("ActionAllowance")
    }

    fn json_schema(
        generator: &mut printobserver_types::schemars::SchemaGenerator,
    ) -> printobserver_types::schemars::Schema {
        let to_value = |schema: printobserver_types::schemars::Schema| {
            printobserver_types::serde_json::to_value(schema).unwrap_or_default()
        };
        let action = to_value(generator.subschema_for::<RecoveryActionKind>());
        let refusal = to_value(generator.subschema_for::<RecoveryRejection>());
        let schema = printobserver_types::serde_json::json!({
            "title": "ActionAllowance",
            "description": "Whether one recovery action is allowed now: `allowed`, and the \
                refusal it would meet, present exactly when it is not.",
            "type": "object",
            "properties": {
                "action": action,
                "allowed": {
                    "type": "boolean",
                    "description": "Whether it is allowed now.",
                    "x-paired-with": "refusal",
                },
                "refusal": refusal,
            },
            "required": ["action", "allowed"],
            "additionalProperties": false,
            // The pairing its reading holds it to: a refused action carries
            // its refusal, and an allowed one carries none.
            "if": { "properties": { "allowed": { "const": false } } },
            "then": { "required": ["refusal"] },
            "else": { "not": { "required": ["refusal"] } },
        });
        printobserver_types::schemars::Schema::try_from(schema).unwrap_or_default()
    }
}

/// What a recovery read answers: everything a recovery is checked against,
/// and what each action would meet now.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(crate = "printobserver_types::serde", deny_unknown_fields)]
#[schemars(crate = "printobserver_types::schemars", transform = every_field_is_sent)]
pub struct RecoveryStatus {
    /// The state the printer reports; `offline` when it could not be read.
    pub state: PrinterState,
    /// Whether Z is trusted.
    pub z_trust: ZTrust,
    /// Where the head is, read only while Z is trusted; `null` otherwise or
    /// when it could not be read.
    pub position: Option<RecoveryPosition>,
    /// Every heater.
    pub temperatures: RecoveryTemperatures,
    /// What the last print laid down, `null` when there is no print with a
    /// file to read or its file could not be read.
    pub last_print: Option<LastPrint>,
    /// The print paused under the printer, `null` when none is.
    pub paused_print: Option<PausedPrint>,
    /// Where Z is homed and whether that sweeps the bed, `null` when
    /// recovery is not configured.
    pub z_home: Option<ZHome>,
    /// Each action, and whether it is allowed now.
    pub actions: Vec<ActionAllowance>,
}

/// What became of a recovery request: carried out with the printer's outcome,
/// or refused — one or the other, never both and never neither.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(
    crate = "printobserver_types::serde",
    rename_all = "snake_case",
    deny_unknown_fields
)]
#[schemars(crate = "printobserver_types::schemars")]
pub enum RecoveryDecision {
    /// It reached the printer, and this is what the printer made of it.
    Executed(ExecutionOutcome),
    /// It was refused, and reached nothing.
    Rejected(RecoveryRejection),
}

impl Sample for RecoveryActionKind {
    fn sample_full() -> Self {
        Self::RecoverLift
    }
}

impl Sample for RecoveryAction {
    fn sample_full() -> Self {
        Self::RecoverHeat {
            tool: 0,
            target_c: 180.0,
            clearance_asserted: true,
        }
    }

    fn sample_minimal() -> Self {
        Self::RecoverCool {}
    }

    fn sample_alternates() -> Vec<Self> {
        vec![
            Self::RecoverLift { mm: 10.0 },
            Self::RecoverFan { percent: 50.0 },
            Self::RecoverPark {
                clearance_asserted: false,
            },
            Self::RecoverHomeXy {
                clearance_asserted: false,
            },
            Self::RecoverHomeZ {
                bed_clear_asserted: true,
            },
        ]
    }
}

impl Sample for RecoveryField {
    fn sample_full() -> Self {
        Self::Mm
    }
}

impl Sample for RecoveryRejection {
    fn sample_full() -> Self {
        Self::InsufficientClearance {
            nozzle_z_mm: 3.0,
            tallest_mm: 6.0,
            margin_mm: 5.0,
        }
    }

    fn sample_minimal() -> Self {
        Self::ZUntrusted
    }

    fn sample_alternates() -> Vec<Self> {
        vec![
            Self::ActorMayNotRequest {
                action: RecoveryActionKind::RecoverHomeZ,
            },
            Self::PrintRunning {
                state: PrinterState::Printing,
            },
            Self::PrinterUnreachable,
            Self::OutOfBounds {
                field: RecoveryField::Mm,
                requested: 80.0,
                allowed: Range::new(0.0, 50.0),
            },
            Self::AssertionRequired {
                field: RecoveryField::BedClearAsserted,
            },
            Self::RecoveryNotConfigured,
            Self::PrinterCannot {
                action: RecoveryActionKind::RecoverPark,
                detail: "this printer cannot move its head to a place".to_owned(),
            },
        ]
    }
}

impl Sample for ZTrust {
    fn sample_full() -> Self {
        Self::TrustedSince(Timestamp::sample_full())
    }

    fn sample_minimal() -> Self {
        Self::Untrusted("the server started, and has homed no Z since".to_owned())
    }
}

impl Sample for RecoveryPosition {
    fn sample_full() -> Self {
        Self {
            x: 100.0,
            y: 100.0,
            z: 12.0,
        }
    }
}

impl Sample for ToolTemperature {
    fn sample_full() -> Self {
        Self {
            tool: 0,
            actual_c: Some(180.0),
            target_c: Some(180.0),
        }
    }

    fn sample_minimal() -> Self {
        Self {
            tool: 0,
            actual_c: None,
            target_c: None,
        }
    }
}

impl Sample for BedTemperature {
    fn sample_full() -> Self {
        Self {
            actual_c: Some(60.0),
            target_c: Some(0.0),
        }
    }

    fn sample_minimal() -> Self {
        Self {
            actual_c: None,
            target_c: None,
        }
    }
}

impl Sample for RecoveryTemperatures {
    fn sample_full() -> Self {
        Self {
            tools: vec![ToolTemperature::sample_full()],
            bed: Some(BedTemperature::sample_full()),
        }
    }

    fn sample_minimal() -> Self {
        Self {
            tools: Vec::new(),
            bed: None,
        }
    }
}

impl Sample for LastPrint {
    fn sample_full() -> Self {
        Self {
            print_id: PrintId::sample_full(),
            tallest_mm: 6.0,
            footprint: Footprint::sample_full(),
        }
    }
}

impl Sample for PausedPrint {
    fn sample_full() -> Self {
        Self {
            print_id: PrintId::sample_full(),
            resumable: false,
        }
    }
}

impl Sample for ZHome {
    fn sample_full() -> Self {
        Self {
            x: 125.0,
            y: 105.0,
            sweeps_bed: true,
        }
    }
}

impl Sample for ActionAllowance {
    fn sample_full() -> Self {
        Self {
            action: RecoveryActionKind::RecoverPark,
            refusal: Some(RecoveryRejection::ZUntrusted),
        }
    }

    fn sample_minimal() -> Self {
        Self {
            action: RecoveryActionKind::RecoverLift,
            refusal: None,
        }
    }
}

impl Sample for RecoveryStatus {
    fn sample_full() -> Self {
        Self {
            state: PrinterState::Paused,
            z_trust: ZTrust::sample_full(),
            position: Some(RecoveryPosition::sample_full()),
            temperatures: RecoveryTemperatures::sample_full(),
            last_print: Some(LastPrint::sample_full()),
            paused_print: Some(PausedPrint::sample_full()),
            z_home: Some(ZHome::sample_full()),
            actions: vec![
                ActionAllowance::sample_minimal(),
                ActionAllowance::sample_full(),
            ],
        }
    }

    fn sample_minimal() -> Self {
        Self {
            state: PrinterState::Operational,
            z_trust: ZTrust::sample_minimal(),
            position: None,
            temperatures: RecoveryTemperatures::sample_minimal(),
            last_print: None,
            paused_print: None,
            z_home: None,
            actions: Vec::new(),
        }
    }
}

impl Sample for RecoveryDecision {
    fn sample_full() -> Self {
        Self::Rejected(RecoveryRejection::sample_full())
    }

    fn sample_alternates() -> Vec<Self> {
        vec![
            Self::Executed(ExecutionOutcome::Succeeded),
            Self::Executed(ExecutionOutcome::Failed {
                reason: "the printer refused the request with 409: Printer is busy".to_owned(),
            }),
        ]
    }
}
