//! `printobserver-core`.
//!
//! Owns: the supervision logic — the loop that turns printer state and vision
//! observations into supervisory decisions, and the policy that decides which
//! decisions are allowed to reach the printer — together with the event kinds
//! and source names that logic writes ([`kinds`]), the one aggregate a
//! caller and the agent both read, the print's context ([`context`]), and the
//! persistence interfaces it needs, one per aggregate ([`store`]).
//!
//! May depend on: `printobserver-types` and the three port crates
//! (`printobserver-printer-api`, `printobserver-vision-api`,
//! `printobserver-supervisor-api`), and NO implementation crate. This is the
//! rule the whole design rests on and `just check-repo` enforces it. The
//! persistence interfaces this crate needs are its own ([`store`]), one trait
//! per aggregate it persists, and the store implementation depends on this
//! crate to implement them.
//!
//! # The policy, which is the reason this layer exists
//!
//! A failure detector can already see a failure and a printer can already
//! drive the machine. What does not exist without this crate is the thing in between that
//! decides what an agent is allowed to do about it, and its whole value is in
//! being thin, bounded, and the **single** place that decision is made — so
//! that an operator's own commands are bounded by exactly the same rules an
//! agent's are.
//!
//! * **Effective bounds** are the configured [safety
//!   envelope](crate::records::SafetyEnvelope) intersected with the active
//!   print's manifest. A manifest may only narrow; a range wider than the
//!   envelope's is narrowed to the envelope's and the narrowing is recorded on
//!   the print. See [`bounds`].
//! * **Every action from every actor passes one decision**, taken by
//!   [`decision::decide`] and issued by the one function named in
//!   [`supervisor`]. There is no path by which an action reaches the printer
//!   without it, and that is structural: no other item of this crate names an
//!   action method of the printer port, and no item outside that one module can
//!   obtain the handle to call one on.
//! * **A request outside the bounds is rejected, not clamped**, and the
//!   rejection is persisted with the value asked for and the range allowed, so
//!   the caller can ask again inside the range.
//! * **A bounded intervention expires on its own.** Core reads the current time
//!   through an injected [`Clock`] rather than the system's, so expiry is
//!   driven by time passing and by nothing else. See [`expiry`].
//! * **Nothing in this crate, and nothing reachable from it, sends a command
//!   string to a printer.** The printer port declares no method that admits
//!   one: no method admits a byte sequence at all, and the only string anywhere
//!   in the closure its parameters reach is the one
//!   [`FileName`](printobserver_types::FileName) wraps.

pub mod accounting;
pub mod actions;
pub mod block_on;
pub mod bounds;
pub mod clock;
pub mod closing;
pub mod config;
pub mod context;
pub mod decision;
mod delay;
pub mod detector;
pub mod error;
pub mod events;
pub mod expiry;
pub mod filament;
pub mod inbox;
pub mod kinds;
pub mod listing;
pub mod look;
pub mod observation;
pub mod preflight;
pub mod records;
pub mod recovery;
pub mod spools;
pub mod store;
pub mod supervisor;
pub mod turn_access;
pub mod turn_lock;
pub mod unobserved;
pub mod upgrade;
pub mod watch;

pub use accounting::{ChargeRule, UNTRACKED_REASON, charge_rule};
pub use actions::{ActionOutcome, reported_value};
pub use block_on::block_on;
pub use bounds::{Bounds, effective_bounds};
pub use clock::{Clock, SystemClock, plus_seconds, seconds_between, unix_seconds};
pub use config::{
    CoreConfig, DEFAULT_DETECTOR_RESUME_GRACE, DEFAULT_EXPIRY_POLL, DEFAULT_OBSERVATION_INTERVAL,
    DEFAULT_RECENT_EVENTS, MAX_LOOK_WAIT_S, PRINT_ID_PLACEHOLDER, PreflightMode, SpoolPolicy,
};
pub use context::PrintContext;
pub use decision::{DecisionInput, adjustment, changes_the_machine, decide, valid_from};
pub use error::CoreError;
pub use events::{PendingTurn, Received, TERMINAL_STATES};
pub use expiry::{rejection_detail, restoring_action};
pub use filament::{
    DEFAULT_DIAMETER_MM, MATERIAL_DENSITIES, MarkClearing, buffer_g, density_for, grams_for_length,
    marks_cleared_by,
};
pub use inbox::{Arrival, Inboxes};
pub use kinds::{
    ActionExecutedPayload, ActionRejectedPayload, ActionRequestedPayload, AgentAssessmentPayload,
    CameraDegradedPayload, CameraLookPayload, ChargePendedPayload, DetectorHealthChangedPayload,
    DetectorStatus, FilamentChangeSignalledPayload, FilamentConsumedPayload,
    FileTallyRebaselinedPayload, FrameCause, HeatedTool, InterventionExpiredPayload,
    OperatorAcknowledgementPayload, PeriodicObservationPayload, PortFailurePayload,
    PortFailureSite, PositionFreshness, PreflightCheckedPayload, PrintEndedPayload,
    PrintStartedPayload, PrintUntrackedPayload, PrinterConnectionChangedPayload,
    PrinterResetPayload, RebaselineDisposition, RecoveryExecutedPayload, RecoveryRejectedPayload,
    RecoveryRequestedPayload, SignalPosition, SpoolAdjustedPayload, SpoolCommandRejectedPayload,
    SpoolLoadedPayload, SpoolRegisteredPayload, SpoolRetiredPayload, SpoolUnloadedPayload,
    SpoolUnverifiedPayload, TIMELINE_KINDS, UnaccountedPrintPayload,
    UnattendedPrinterActivityPayload, agent_source, operator_source, system_source,
};
pub use look::Look;
pub use observation::Observed;
pub use preflight::ANALYSIS_POLL;
pub use records::{
    AcknowledgementDisposition, ActionId, ActionKind, ActionRecord, ActionRequest, Actor,
    ActorClass, ChargeKey, ChargeMethod, ChargedFilament, ClearedMark, DoubtedSpool,
    EffectiveBounds, ExecutionOutcome, FilePosition, ImageRecord, Intervention, InterventionId,
    InterventionOutcome, JobManifest, LoadedSpool, ManifestNarrowing, PendingCharge, PendingTaken,
    PolicyDecision, Preflight, PreflightNeed, PreflightObjection, PreflightSpool, PreflightTool,
    PreflightToolResult, PreflightVerdict, PrintAction, PrintOutcome, PrintRecord, PrintSegmentKey,
    RejectionReason, SafetyEnvelope, ShortSpool, SpoolCommand, SpoolField, SpoolId, SpoolLifecycle,
    SpoolRecord, SpoolRefusalCause, SpoolTrust, UnanalysedSpool, UnverifiedMark, UnverifiedMarks,
    UnverifiedReason, UnverifiedReasonKind,
};
pub use records::{
    ActionAllowance, AwayFromBed, BedTemperature, DEFAULT_CLEARANCE_MARGIN_MM, DEFAULT_MAX_LIFT_MM,
    LastPrint, PausedPrint, RecoveryAction, RecoveryActionKind, RecoveryConfig, RecoveryDecision,
    RecoveryField, RecoveryGrants, RecoveryPosition, RecoveryRejection, RecoveryStatus,
    RecoveryTemperatures, ToolTemperature, ZHome, ZTrust,
};
pub use spools::SpoolCommandOutcome;
pub use store::{
    ActionStore, AuditPage, Charge, ChargeIdentity, ChargeRecord, DEFAULT_HISTORY_WINDOW,
    EventDraft, EventStore, FilamentStore, FileTally, HistoryQuery, ImageLookup, ImageStore,
    MAX_HISTORY_LIMIT, NewSpool, Pend, PrintStore, SessionStore, SettleOutcome, Settlement,
    SpoolChange, SpoolMutation, SpoolWrite, StoreError, Stores, TimelineQuery, TimelineStore,
    resolve_history_limit,
};
pub use supervisor::{Issued, RecoveryIssued, Supervisor};
pub use turn_access::{OpenedTurn, TurnAuthority};
pub use turn_lock::{TurnGuard, TurnLocks};
pub use watch::{
    DEFAULT_PRINTER_POLL, DEFAULT_UNATTENDED_WINDOW, DetectorHealth, DetectorHealthStatus,
    DetectorProbe, HeadPosition, PRINTER_WATCH_KINDS, PUSH_CHANNEL_DOWN, PositionProbe, Watched,
};
