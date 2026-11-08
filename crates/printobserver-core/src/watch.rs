//! The printer watch: what changed under a print while nobody asked.
//!
//! Every other read of the printer here is somebody's — a listing, a status, a
//! turn's context. The watch is nobody's: the server asks it to read the
//! machine every [`CoreConfig::printer_poll`](crate::CoreConfig::printer_poll),
//! whether or not a print is active, and it records what changed.
//!
//! * **The connection.** A read finding the printer's state changed into or
//!   out of `offline` or `error` — a serial disconnect, a reconnect, a printer
//!   error — is a [`PrinterConnectionChangedPayload`]. The printer's state is
//!   read from its job, never from a refusal: a printer that is not
//!   operational refuses a read of its own state, and that refusal is the
//!   adapter's to read as `offline`, never one recorded as a port failure.
//! * **How a print ended.** The watch's read settles the open prints as every
//!   other read does ([`crate::listing`]), and the close-out decides the
//!   outcome from what the printer pushed during the stop as well as from what
//!   a read shows ([`crate::accounting`]). An `interrupted` or `failed` ending
//!   gives the print's agent a turn on its `print_ended`, and opens the
//!   unattended window.
//! * **The printer acting on its own.** Within
//!   [`CoreConfig::unattended_window`](crate::CoreConfig::unattended_window)
//!   of an `interrupted` or `failed` ending, a read finding a heater target
//!   above zero, or the head moved, while no job runs is an
//!   [`UnattendedPrinterActivityPayload`]: once when it begins, and again only
//!   when what it reports changes. The head's position is asked for through a
//!   [`PositionProbe`] the server supplies, and only after this read found no
//!   job running. The print's agent gets a turn on it, at most one per
//!   [`CoreConfig::observation_interval`](crate::CoreConfig::observation_interval);
//!   one recorded inside that interval reaches the agent through its next
//!   turn's recent events.
//! * **A reset.** The printer's push channel says when it reset, one while
//!   idle included, which no read shows; each is a [`PrinterResetPayload`]
//!   ([`Supervisor::note_push`]).
//! * **The detector.** During a print, a [`DetectorProbe`] the server supplies
//!   is asked whether the failure detector is answering, and each change is a
//!   [`DetectorHealthChangedPayload`]; going unavailable gives the active
//!   print's agent a turn. The last status is what the status and context
//!   reads answer as [`DetectorHealth`].
//!
//! # Nothing here commands the machine
//!
//! The watch reads. A position is asked for through the probe, which is the
//! server's own: this crate names no method of the printer port but its reads,
//! and the chokepoint stands as [`crate::supervisor`] states it.

use std::time::Duration;

use printobserver_printer_api::{
    BoxFuture, PrinterErrorReason, PrinterPush, PrinterSnapshot, PrinterState,
};
use printobserver_types::contract::Sample;
use printobserver_types::schemars::JsonSchema;
use printobserver_types::serde::{Deserialize, Serialize};
use printobserver_types::{EventKind, EventPayload as _, EventRecord, PrintId, Timestamp};

use crate::error::CoreError;
use crate::events::PendingTurn;
use crate::inbox::Arrival;
use crate::kinds::{
    DetectorHealthChangedPayload, DetectorStatus, HeatedTool, PortFailureSite, PrintEndedPayload,
    PrinterConnectionChangedPayload, PrinterResetPayload, UnattendedPrinterActivityPayload,
    system_source,
};
use crate::listing::ObservedJob;
use crate::records::{PrintOutcome, PrintRecord};
use crate::store::HistoryQuery;
use crate::supervisor::Supervisor;

/// How often the printer watch reads the machine, when nothing configures it.
pub const DEFAULT_PRINTER_POLL: Duration = Duration::from_secs(15);

/// How long after an `interrupted` or `failed` ending the printer acting on
/// its own is recorded, when nothing configures it.
pub const DEFAULT_UNATTENDED_WINDOW: Duration = Duration::from_secs(3600);

/// How long a return to idle that would read as a cancel waits for the reset
/// the printer may still be pushing for it.
///
/// `OctoPrint` pushes a reset within about a third of a second of the
/// printer's own, and moves its state to idle within about two thirds; a read
/// can land between the two, and without the reset a firmware reset reads as
/// a cancel made at the printer.
pub const RESET_GRACE: Duration = Duration::from_secs(1);

/// What a detail says while the push channel is down.
pub const PUSH_CHANNEL_DOWN: &str =
    "the printer's push channel was down, so this was read from its state alone";

/// The kinds the printer watch writes with no print: the printer's own, read
/// into a print's recent events from the moment it opened.
pub const PRINTER_WATCH_KINDS: [&str; 4] = [
    PrinterConnectionChangedPayload::KIND,
    PrinterResetPayload::KIND,
    UnattendedPrinterActivityPayload::KIND,
    DetectorHealthChangedPayload::KIND,
];

/// Where the print head is.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct HeadPosition {
    /// X, in millimetres.
    pub x: f64,
    /// Y, in millimetres.
    pub y: f64,
    /// Z, in millimetres.
    pub z: f64,
}

/// What reads the print head's position for the watch.
///
/// The server's own: it asks the printer and reads the answer off the push
/// channel. Answers nothing when no position could be read.
pub trait PositionProbe: Send + Sync {
    /// Read where the head is now.
    fn read_position(&self) -> BoxFuture<'_, Option<HeadPosition>>;
}

/// What asks the failure detector whether it is answering.
pub trait DetectorProbe: Send + Sync {
    /// Answers what it was told: `Ok` when the detector answered its health
    /// check, `Err` when it did not, each in words.
    fn check(&self) -> BoxFuture<'_, Result<String, String>>;
}

/// The failure detector's status, as the status and context reads answer it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema, Default)]
#[serde(
    crate = "printobserver_types::serde",
    rename_all = "snake_case",
    deny_unknown_fields
)]
#[schemars(crate = "printobserver_types::schemars")]
pub enum DetectorHealthStatus {
    /// The detector is scoring the print, and no failure alert of its own has
    /// arrived for it.
    ScoringNothingFlagged,
    /// The detector is scoring the print, and a failure alert of its own has
    /// arrived for it.
    ScoringFlagged,
    /// No evidence that it is scoring the print, and it answered its health
    /// check: available, with no scores yet.
    Available,
    /// No evidence that it is scoring the print, and it did not answer its
    /// health check.
    Unavailable,
    /// No evidence of any kind: no detector health address is configured, or
    /// no check has been made yet, and nothing shows it scoring.
    #[default]
    Unchecked,
}

/// The failure detector's status, and since when it has been so.
///
/// Evidence that the detector is scoring the print comes first: while its own
/// record of the print's live prediction moved between two of this server's
/// reads within the last two observation intervals, it is scoring, whatever
/// its health check answers. Without that evidence its health check decides.
///
/// On the wire it is the object the status and context reads answer —
/// `status`; `since`, `null` exactly when the status is `unchecked`; the failing
/// health check named beside a scoring status as `probe_failure`; and, beside
/// any other status, why the detector's scoring is not being read as
/// `scoring_unread` — and an object pairing them any other way is refused
/// rather than read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DetectorHealth {
    /// No evidence of any kind: no detector health address is configured, or
    /// no check has been made yet, and nothing shows it scoring.
    Unchecked {
        /// Why the detector's scoring is not being read, when it is not.
        scoring_unread: Option<String>,
    },
    /// No evidence that it is scoring the print, and it answered its health
    /// check, from this instant on.
    Available {
        /// When it was first found answering.
        since: Timestamp,
        /// Why the detector's scoring is not being read, when it is not.
        scoring_unread: Option<String>,
    },
    /// No evidence that it is scoring the print, and it did not answer its
    /// health check, from this instant on.
    Unavailable {
        /// When it was first found not answering.
        since: Timestamp,
        /// Why the detector's scoring is not being read, when it is not.
        scoring_unread: Option<String>,
    },
    /// The detector is scoring the print, from this instant on.
    Scoring {
        /// When it was first found scoring it.
        since: Timestamp,
        /// Whether a failure alert of its own has arrived for the print.
        flagged: bool,
        /// What the detector's health check answered, when it is failing
        /// while the detector scores.
        probe_failure: Option<String>,
    },
}

impl Default for DetectorHealth {
    fn default() -> Self {
        Self::Unchecked {
            scoring_unread: None,
        }
    }
}

impl DetectorHealth {
    /// The status this is.
    #[must_use]
    pub const fn status(&self) -> DetectorHealthStatus {
        match self {
            Self::Unchecked { .. } => DetectorHealthStatus::Unchecked,
            Self::Available { .. } => DetectorHealthStatus::Available,
            Self::Unavailable { .. } => DetectorHealthStatus::Unavailable,
            Self::Scoring { flagged: false, .. } => DetectorHealthStatus::ScoringNothingFlagged,
            Self::Scoring { flagged: true, .. } => DetectorHealthStatus::ScoringFlagged,
        }
    }
}

/// The failure detector's status as the wire carries it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(crate = "printobserver_types::serde", deny_unknown_fields)]
struct DetectorHealthWire {
    /// What it is.
    status: DetectorHealthStatus,
    /// When the status became what it is, `null` exactly while unchecked —
    /// and always present, as the schema requires.
    #[serde(deserialize_with = "present")]
    since: Option<Timestamp>,
    /// The failing health check, beside a scoring status.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    probe_failure: Option<String>,
    /// Why the detector's scoring is not being read, beside any other status.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    scoring_unread: Option<String>,
}

/// Read a value that may be `null` but may not be missing: serde reads a
/// missing optional as `None` unless it is read through a function of its own.
fn present<'de, D: printobserver_types::serde::Deserializer<'de>>(
    deserializer: D,
) -> Result<Option<Timestamp>, D::Error> {
    Option::deserialize(deserializer)
}

impl From<DetectorHealth> for DetectorHealthWire {
    fn from(health: DetectorHealth) -> Self {
        let status = health.status();
        let (since, probe_failure, scoring_unread) = match health {
            DetectorHealth::Unchecked { scoring_unread } => (None, None, scoring_unread),
            DetectorHealth::Available {
                since,
                scoring_unread,
            }
            | DetectorHealth::Unavailable {
                since,
                scoring_unread,
            } => (Some(since), None, scoring_unread),
            DetectorHealth::Scoring {
                since,
                probe_failure,
                ..
            } => (Some(since), probe_failure, None),
        };
        Self {
            status,
            since,
            probe_failure,
            scoring_unread,
        }
    }
}

impl TryFrom<DetectorHealthWire> for DetectorHealth {
    type Error = &'static str;

    fn try_from(wire: DetectorHealthWire) -> Result<Self, Self::Error> {
        let DetectorHealthWire {
            status,
            since,
            probe_failure,
            scoring_unread,
        } = wire;
        let scoring = matches!(
            status,
            DetectorHealthStatus::ScoringNothingFlagged | DetectorHealthStatus::ScoringFlagged
        );
        if scoring && scoring_unread.is_some() {
            return Err("a scoring detector's scoring is being read");
        }
        if !scoring && probe_failure.is_some() {
            return Err("only a scoring detector names its failing health check beside it");
        }
        match (status, since) {
            (DetectorHealthStatus::Unchecked, None) => Ok(Self::Unchecked { scoring_unread }),
            (DetectorHealthStatus::Available, Some(since)) => Ok(Self::Available {
                since,
                scoring_unread,
            }),
            (DetectorHealthStatus::Unavailable, Some(since)) => Ok(Self::Unavailable {
                since,
                scoring_unread,
            }),
            (DetectorHealthStatus::ScoringNothingFlagged, Some(since)) => Ok(Self::Scoring {
                since,
                flagged: false,
                probe_failure,
            }),
            (DetectorHealthStatus::ScoringFlagged, Some(since)) => Ok(Self::Scoring {
                since,
                flagged: true,
                probe_failure,
            }),
            (DetectorHealthStatus::Unchecked, Some(_)) => {
                Err("an unchecked detector has no instant it became so")
            }
            (_, None) => Err("a checked detector carries the instant it became so"),
        }
    }
}

impl Serialize for DetectorHealth {
    fn serialize<S: printobserver_types::serde::Serializer>(
        &self,
        serializer: S,
    ) -> Result<S::Ok, S::Error> {
        DetectorHealthWire::from(self.clone()).serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for DetectorHealth {
    fn deserialize<D: printobserver_types::serde::Deserializer<'de>>(
        deserializer: D,
    ) -> Result<Self, D::Error> {
        let wire = DetectorHealthWire::deserialize(deserializer)?;
        Self::try_from(wire).map_err(printobserver_types::serde::de::Error::custom)
    }
}

/// One arm of the detector's status as a schema: its status, its `since`, and
/// the one optional text it may carry beside them.
fn health_arm(
    status: &str,
    doc: &str,
    since: &printobserver_types::serde_json::Value,
    beside: (&str, &str),
) -> printobserver_types::serde_json::Value {
    let (field, field_doc) = beside;
    let mut properties = printobserver_types::serde_json::Map::new();
    properties.insert(
        "status".to_owned(),
        printobserver_types::serde_json::json!({ "type": "string", "const": status }),
    );
    properties.insert("since".to_owned(), since.clone());
    properties.insert(
        field.to_owned(),
        printobserver_types::serde_json::json!({ "type": "string", "description": field_doc }),
    );
    printobserver_types::serde_json::json!({
        "type": "object",
        "description": doc,
        "properties": properties,
        "required": ["status", "since"],
        "additionalProperties": false,
    })
}

/// What `scoring_unread` says, in the schema.
const SCORING_UNREAD_DOC: &str = "Why the detector's scoring of the print is not being read, \
    when it is not: no Obico API is configured, no Obico printer is known yet, or the last read \
    failed.";

/// What `probe_failure` says, in the schema.
const PROBE_FAILURE_DOC: &str = "What the detector's health check answered, when it is \
    failing while the detector scores the print.";

/// The schema describes the five states as arms tagged by `status`, so a
/// generated client cannot pair a status with the wrong `since` either.
impl JsonSchema for DetectorHealth {
    fn schema_name() -> std::borrow::Cow<'static, str> {
        std::borrow::Cow::Borrowed("DetectorHealth")
    }

    fn json_schema(
        generator: &mut printobserver_types::schemars::SchemaGenerator,
    ) -> printobserver_types::schemars::Schema {
        let timestamp =
            printobserver_types::serde_json::to_value(generator.subschema_for::<Timestamp>())
                .unwrap_or_default();
        let unread = ("scoring_unread", SCORING_UNREAD_DOC);
        let failing = ("probe_failure", PROBE_FAILURE_DOC);
        let schema = printobserver_types::serde_json::json!({
            "title": "DetectorHealth",
            "description": "The failure detector's status, and since when it has been so: \
                `since` is `null` exactly while the status is `unchecked`. Evidence that the \
                detector is scoring the print comes before its health check.",
            "oneOf": [
                health_arm(
                    "scoring_nothing_flagged",
                    "The detector is scoring the print, from `since` on, and no failure alert \
                     of its own has arrived for it.",
                    &timestamp,
                    failing,
                ),
                health_arm(
                    "scoring_flagged",
                    "The detector is scoring the print, from `since` on, and a failure alert \
                     of its own has arrived for it.",
                    &timestamp,
                    failing,
                ),
                health_arm(
                    "available",
                    "No evidence that the detector is scoring the print, and it answered its \
                     health check, from `since` on: available, with no scores yet.",
                    &timestamp,
                    unread,
                ),
                health_arm(
                    "unavailable",
                    "No evidence that the detector is scoring the print, and it did not answer \
                     its health check, from `since` on.",
                    &timestamp,
                    unread,
                ),
                health_arm(
                    "unchecked",
                    "No evidence of any kind: no detector health address is configured, or no \
                     check has been made yet, and nothing shows the detector scoring.",
                    &printobserver_types::serde_json::json!({
                        "type": "null",
                        "description": "Nothing: an unchecked detector has no instant it became so.",
                    }),
                    unread,
                ),
            ],
        });
        printobserver_types::schemars::Schema::try_from(schema).unwrap_or_default()
    }
}

impl Sample for DetectorHealth {
    fn sample_full() -> Self {
        Self::Scoring {
            since: Timestamp::sample_full(),
            flagged: false,
            probe_failure: Some("the detector's health check answered 503".to_owned()),
        }
    }

    fn sample_minimal() -> Self {
        Self::Unchecked {
            scoring_unread: None,
        }
    }

    fn sample_alternates() -> Vec<Self> {
        vec![
            Self::Scoring {
                since: Timestamp::sample_full(),
                flagged: true,
                probe_failure: None,
            },
            Self::Available {
                since: Timestamp::sample_full(),
                scoring_unread: Some(
                    "no Obico printer is known yet: no [obico] printer_id is configured and Obico \
                     has posted nothing naming one since this server started"
                        .to_owned(),
                ),
            },
            Self::Unavailable {
                since: Timestamp::sample_full(),
                scoring_unread: None,
            },
            Self::Unchecked {
                scoring_unread: Some(
                    "no [obico] url and access_token are configured, so Obico's record of the \
                     print is not read"
                        .to_owned(),
                ),
            },
        ]
    }
}

/// What the detector's health check last found, with what it was answered.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) enum ProbeHealth {
    /// Nothing has checked it.
    #[default]
    Unchecked,
    /// It answered, from this instant on.
    Available {
        /// When it was first found answering.
        since: Timestamp,
    },
    /// It did not, from this instant on.
    Unavailable {
        /// When it was first found not answering.
        since: Timestamp,
        /// What it was answered, in words.
        detail: String,
    },
}

impl ProbeHealth {
    /// The status the check alone gives.
    const fn status(&self) -> DetectorHealthStatus {
        match self {
            Self::Unchecked => DetectorHealthStatus::Unchecked,
            Self::Available { .. } => DetectorHealthStatus::Available,
            Self::Unavailable { .. } => DetectorHealthStatus::Unavailable,
        }
    }
}

/// What the printer pushed since the last print started or was closed out:
/// a stop's push and the read that finds the stop can arrive in either order,
/// so what a print's run pushed is kept for its whole run.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct PushFacts {
    /// It pushed a reset.
    pub(crate) reset: bool,
    /// It pushed an error of its own.
    pub(crate) firmware_error: bool,
    /// The push channel was down at some point, so what it pushed is not the
    /// whole of what happened.
    pub(crate) gap: bool,
}

/// What the printer was doing on its own, as one read found it.
#[derive(Debug, Clone, PartialEq)]
struct Activity {
    /// Every tool whose target is above zero.
    tools: Vec<HeatedTool>,
    /// The bed's target, when above zero.
    bed: Option<f64>,
    /// Whether the head moved since the last position read.
    moved: Option<bool>,
}

impl Activity {
    /// Whether this is activity at all.
    fn is_active(&self) -> bool {
        !self.tools.is_empty() || self.bed.is_some() || self.moved == Some(true)
    }
}

/// The window after an unfinished ending in which activity is recorded.
#[derive(Debug, Clone)]
struct Unattended {
    /// The print that ended.
    print_id: PrintId,
    /// When, on the clock nothing steps, it ended.
    began: Duration,
    /// The activity last recorded, absent when none is under way.
    recorded: Option<Activity>,
    /// The position last read.
    position: Option<HeadPosition>,
    /// When, on the clock nothing steps, its agent was last given a turn on
    /// activity.
    turned_at: Option<Duration>,
}

/// What the watch holds between reads.
#[derive(Debug, Default)]
pub(crate) struct WatchState {
    /// The printer's state the last read found.
    last_state: Option<PrinterState>,
    /// What the printer pushed since the last print started or was closed out.
    pushes: PushFacts,
    /// The push channel.
    channel: Channel,
    /// The unattended window, while one is open.
    unattended: Option<Unattended>,
    /// What the detector's health check last found.
    pub(crate) probe: ProbeHealth,
    /// What the reads of the detector's own record of the print have found.
    pub(crate) scoring: crate::scoring::ScoringState,
    /// Turns an ending claimed, for the next watch round to answer.
    pending_turns: Vec<PendingTurn>,
    /// Resets the store refused to write down, for the next round to write.
    pending_resets: Vec<PrinterResetPayload>,
    /// Prints closed out whose ending was not yet followed up, because the
    /// store would not answer, for the next round to follow up.
    pending_ends: Vec<PrintId>,
    /// Activity turns the store would not answer for, for the next round to
    /// claim.
    pending_claims: Vec<(PrintId, EventRecord)>,
}

impl WatchState {
    /// Whether the push channel is anything but open now.
    const fn channel_is_down(&self) -> bool {
        !matches!(self.channel, Channel::Open)
    }
}

/// The printer's push channel, as the server last reported it.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
enum Channel {
    /// Nothing has reported on it yet.
    #[default]
    Unknown,
    /// It is open.
    Open,
    /// It is down, and why.
    Down(String),
}

/// What one round of the watch claimed.
#[derive(Debug, Default)]
pub struct Watched {
    /// The turns it claimed, each to be run by
    /// [`Supervisor::run_supervision`].
    pub turns: Vec<PendingTurn>,
}

/// Whether a state is one a connection change is recorded into or out of.
const fn is_down(state: &PrinterState) -> bool {
    matches!(state, PrinterState::Offline | PrinterState::Error)
}

/// Whether a job in this state is one the printer is running.
const fn job_runs(state: &PrinterState) -> bool {
    matches!(
        state,
        PrinterState::Printing | PrinterState::Paused | PrinterState::Cancelling
    )
}

/// What one snapshot says the heaters are driving towards.
fn heated(snapshot: &PrinterSnapshot) -> (Vec<HeatedTool>, Option<f64>) {
    let tools = snapshot
        .tools
        .iter()
        .enumerate()
        .filter_map(|(index, heater)| {
            let target = heater.target_c.as_ref()?.value();
            (target > 0.0).then(|| HeatedTool {
                tool: i64::try_from(index).unwrap_or(i64::MAX),
                target_c: target,
            })
        })
        .collect();
    let bed = snapshot
        .bed
        .as_ref()
        .and_then(|bed| bed.target_c.as_ref())
        .map(printobserver_types::Reported::value)
        .filter(|target| *target > 0.0);
    (tools, bed)
}

/// One activity in words.
fn activity_detail(activity: &Activity, channel_down: bool) -> String {
    let mut parts: Vec<String> = activity
        .tools
        .iter()
        .map(|tool| format!("tool {} is heating to {} °C", tool.tool, tool.target_c))
        .collect();
    if let Some(bed) = activity.bed {
        parts.push(format!("the bed is heating to {bed} °C"));
    }
    match activity.moved {
        Some(true) => parts.push("the print head moved".to_owned()),
        Some(false) => {}
        None => parts.push("the print head's position could not be read".to_owned()),
    }
    let mut detail = format!("with no job running, {}", parts.join(", "));
    if channel_down {
        detail.push_str("; ");
        detail.push_str(PUSH_CHANNEL_DOWN);
    }
    detail
}

impl Supervisor {
    /// What the watch holds, held for one read or write.
    pub(crate) fn watch_state(&self) -> std::sync::MutexGuard<'_, WatchState> {
        self.watch
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    /// What the printer pushed since the last print started or was closed out.
    pub(crate) fn push_facts(&self) -> PushFacts {
        let state = self.watch_state();
        let mut facts = state.pushes;
        facts.gap |= state.channel_is_down();
        facts
    }

    /// What the printer pushed, once a reset it may still be pushing for a
    /// return to idle has had [`RESET_GRACE`] to arrive — at once when the
    /// push channel is down, since nothing would arrive.
    pub(crate) async fn pushes_after_a_reset_could_arrive(&self) -> PushFacts {
        // A wait for something a real socket delivers, so it is measured on
        // the host's own clock rather than the one supervision is driven by.
        let started = std::time::Instant::now();
        loop {
            let facts = self.push_facts();
            if facts.reset
                || self.watch_state().channel_is_down()
                || started.elapsed() >= RESET_GRACE
                || crate::delay::Delay::new(Duration::from_millis(50))
                    .await
                    .is_err()
            {
                return facts;
            }
        }
    }

    /// Close any unattended window: a job is running.
    pub(crate) fn note_job_running(&self) {
        self.watch_state().unattended = None;
    }

    /// Forget what the printer pushed: a print starts, or one's close-out has
    /// read it.
    pub(crate) fn forget_pushes(&self) {
        let mut state = self.watch_state();
        let gap = state.channel_is_down();
        state.pushes = PushFacts {
            gap,
            ..PushFacts::default()
        };
    }

    /// The failure detector's status, for whichever print it is scoring.
    ///
    /// Evidence that the detector is scoring a print comes first
    /// ([`crate::scoring`]); without it, what its health check last found.
    #[must_use]
    pub fn detector_health(&self) -> DetectorHealth {
        self.detector_health_of(None)
    }

    /// The failure detector's status, as one print's status and context reads
    /// answer it: scoring only when the evidence is of that print.
    #[must_use]
    pub fn detector_health_for(&self, print_id: PrintId) -> DetectorHealth {
        self.detector_health_of(Some(print_id))
    }

    /// The failure detector's status, for one print or for whichever print it
    /// is scoring.
    fn detector_health_of(&self, print_id: Option<PrintId>) -> DetectorHealth {
        let window = self
            .config()
            .observation_interval
            .saturating_mul(crate::scoring::EVIDENCE_INTERVALS);
        let now = self.clock().elapsed();
        let held = self.watch_state();
        if let Some(scoring) = held.scoring.scoring(print_id, now, window) {
            return DetectorHealth::Scoring {
                since: scoring.since,
                flagged: scoring.flagged,
                probe_failure: match &held.probe {
                    ProbeHealth::Unavailable { detail, .. } => Some(detail.clone()),
                    ProbeHealth::Unchecked | ProbeHealth::Available { .. } => None,
                },
            };
        }
        let scoring_unread = held.scoring.unread().map(str::to_owned);
        match &held.probe {
            ProbeHealth::Unchecked => DetectorHealth::Unchecked { scoring_unread },
            ProbeHealth::Available { since } => DetectorHealth::Available {
                since: *since,
                scoring_unread,
            },
            ProbeHealth::Unavailable { since, .. } => DetectorHealth::Unavailable {
                since: *since,
                scoring_unread,
            },
        }
    }

    /// Report that the printer's push channel opened, or why it is down.
    ///
    /// While it is down an ending is decided from the printer's state alone,
    /// and every event the watch writes says so.
    pub fn note_push_channel(&self, open: Result<(), String>) {
        let mut state = self.watch_state();
        match open {
            Ok(()) => state.channel = Channel::Open,
            Err(detail) => {
                state.channel = Channel::Down(detail);
                state.pushes.gap = true;
            }
        }
    }

    /// Take one thing the printer pushed.
    ///
    /// A reset is recorded at once as a [`PrinterResetPayload`], naming the
    /// print it happened during when one was running; it and an error of the
    /// printer's own are kept for the close-out of the print they stopped.
    /// Terminal lines are the server's to read, and are ignored here.
    ///
    /// # Errors
    ///
    /// Returns the store's own error when the reset could not be recorded.
    pub async fn note_push(&self, push: PrinterPush) -> Result<(), CoreError> {
        match push {
            PrinterPush::Reset => {
                // A reset forgets where every axis is.
                self.distrust_z("the printer pushed a reset, and a reset forgets where Z is");
                // Which print was running is read before the close-out is told
                // of the reset: told first, it can close that print while the
                // read is still waiting on the store, and the reset would then
                // name no print. A close-out that reads the machine idle first
                // waits `RESET_GRACE` for this flag, so setting it a moment
                // later still reaches it.
                let running = self.running_print().await;
                self.watch_state().pushes.reset = true;
                // A reset is recorded whatever the store answers about which
                // print was running: the push is not sent again.
                let (during_print, detail) = match running {
                    Ok(Some(print)) => (
                        Some(print.id),
                        format!("the printer pushed a reset during print {}", print.id),
                    ),
                    Ok(None) => (
                        None,
                        "the printer pushed a reset while no print was running".to_owned(),
                    ),
                    Err(error) => (
                        None,
                        format!(
                            "the printer pushed a reset; which print was running could not be \
                             read: {error}"
                        ),
                    ),
                };
                let payload = PrinterResetPayload {
                    during_print,
                    detail,
                };
                if let Err(error) = self.write_reset(&payload).await {
                    // The push is not sent again, so what it said is kept for
                    // the next round to write down.
                    self.watch_state().pending_resets.push(payload);
                    return Err(error);
                }
                Ok(())
            }
            PrinterPush::Error {
                reason: PrinterErrorReason::Firmware,
            } => {
                self.watch_state().pushes.firmware_error = true;
                Ok(())
            }
            PrinterPush::Error {
                reason: PrinterErrorReason::Other(_),
            }
            | PrinterPush::Log { .. } => Ok(()),
        }
    }

    /// Write one reset down.
    async fn write_reset(&self, payload: &PrinterResetPayload) -> Result<(), CoreError> {
        let draft = self.draft_of(None, system_source(), payload)?;
        self.stores().events.append_event(draft).await?;
        Ok(())
    }

    /// Write down what an earlier round could not: the resets the store
    /// refused, the follow-up of endings and the activity turns it would not
    /// answer for. Each still refused is kept for the round after.
    async fn catch_up(&self) {
        let resets = std::mem::take(&mut self.watch_state().pending_resets);
        for payload in resets {
            if self.write_reset(&payload).await.is_err() {
                self.watch_state().pending_resets.push(payload);
            }
        }
        let ends = std::mem::take(&mut self.watch_state().pending_ends);
        for print_id in ends {
            if self.after_close_out(print_id).await.is_err() {
                self.watch_state().pending_ends.push(print_id);
            }
        }
        let claims = std::mem::take(&mut self.watch_state().pending_claims);
        for (print_id, event) in claims {
            self.claim_activity_turn(print_id, event).await;
        }
    }

    /// Follow up one print's close-out, or keep it for the next round when the
    /// store will not answer.
    pub(crate) async fn follow_up_end(&self, print_id: PrintId) {
        if self.after_close_out(print_id).await.is_err() {
            self.watch_state().pending_ends.push(print_id);
        }
    }

    /// The open print this supervisor last saw running, when there is one.
    async fn running_print(&self) -> Result<Option<PrintRecord>, CoreError> {
        Ok(self
            .stores()
            .prints
            .open_prints()
            .await?
            .into_iter()
            .find(|print| self.was_seen_running(print.id)))
    }

    /// Read the machine once, record what changed, and answer the turns that
    /// claimed.
    ///
    /// A printer whose job cannot be read changes nothing: an instance that
    /// does not answer says nothing about the machine behind it.
    ///
    /// # Errors
    ///
    /// Returns the store's own error when what changed could not be recorded.
    pub async fn watch_printer(&self, probe: &dyn PositionProbe) -> Result<Watched, CoreError> {
        self.catch_up().await;
        let resolving = self.resolving().await;
        let observed = self.observe_job().await;
        if let Some(observed) = &observed {
            self.note_connection(
                &observed.job.state,
                observed.job.error.as_ref().map(|error| error.text.as_str()),
            )
            .await?;
            self.settle_and_adopt(observed).await?;
        }
        drop(resolving);
        if let Some(observed) = &observed {
            self.watch_unattended(observed, probe).await?;
        }
        Ok(Watched {
            turns: std::mem::take(&mut self.watch_state().pending_turns),
        })
    }

    /// Record a change of the printer's state into or out of `offline` or
    /// `error`.
    async fn note_connection(
        &self,
        state: &PrinterState,
        error: Option<&str>,
    ) -> Result<(), CoreError> {
        let (previous, channel_down) = {
            let held = self.watch_state();
            (held.last_state.clone(), held.channel_is_down())
        };
        if is_down(state) && previous.as_ref() != Some(state) {
            // A printer that went offline or into error may have been
            // reset, moved or powered off since: where Z is is unknown.
            self.distrust_z(format!(
                "the printer went {state:?}, and Z may have moved since"
            ));
        }
        let changed = previous
            .as_ref()
            .filter(|previous| *previous != state && (is_down(previous) || is_down(state)));
        if let Some(previous) = changed {
            let reporting = error
                .filter(|error| !error.trim().is_empty())
                .map(|error| format!(", reporting {error:?}"))
                .unwrap_or_default();
            let mut detail = format!("the printer went from {previous:?} to {state:?}{reporting}");
            if channel_down {
                detail.push_str("; ");
                detail.push_str(PUSH_CHANNEL_DOWN);
            }
            let draft = self.draft_of(
                None,
                system_source(),
                &PrinterConnectionChangedPayload {
                    from: previous.clone(),
                    to: state.clone(),
                    detail,
                },
            )?;
            // Remembered only once it is written down: a change the store
            // refused is found again by the next read.
            self.stores().events.append_event(draft).await?;
        }
        self.watch_state().last_state = Some(state.clone());
        Ok(())
    }

    /// What follows a print's close-out: an `interrupted` or `failed` ending
    /// opens the unattended window and claims its agent a turn on the
    /// `print_ended`; any other ending closes the window.
    async fn after_close_out(&self, print_id: PrintId) -> Result<(), CoreError> {
        self.forget_pushes();
        let Some(ended) = self
            .stores()
            .events
            .history(HistoryQuery {
                print_id,
                kinds: vec![PrintEndedPayload::kind()],
                since: None,
                until: None,
                limit: Some(1),
            })
            .await?
            .into_iter()
            .next()
        else {
            return Ok(());
        };
        let Some(Ok(payload)) = ended.payload_as::<PrintEndedPayload>() else {
            return Ok(());
        };
        if !matches!(
            payload.outcome,
            PrintOutcome::Interrupted | PrintOutcome::Failed
        ) {
            self.watch_state().unattended = None;
            return Ok(());
        }
        self.distrust_z(format!(
            "print {print_id} ended {}, and where it left Z is unknown",
            if payload.outcome == PrintOutcome::Failed {
                "failed"
            } else {
                "interrupted"
            }
        ));
        self.watch_state().unattended = Some(Unattended {
            print_id,
            began: self.clock().elapsed(),
            recorded: None,
            position: None,
            turned_at: None,
        });
        self.claim_after_end(print_id, ended).await
    }

    /// Claim one ended print's agent a turn on one event about its end, or
    /// hand the event to the turn already running for it.
    async fn claim_after_end(
        &self,
        print_id: PrintId,
        event: EventRecord,
    ) -> Result<(), CoreError> {
        let Some(print) = self.stores().prints.print(print_id).await? else {
            return Ok(());
        };
        let arrival = Arrival {
            frame_captured_at: None,
            event,
            detection: None,
        };
        if self.inboxes().claim_or_hand_over(print_id, arrival.clone()) {
            self.watch_state()
                .pending_turns
                .push(PendingTurn::new(print, arrival, Vec::new()));
        }
        Ok(())
    }

    /// Record the printer acting on its own, while the unattended window is
    /// open and no job runs.
    async fn watch_unattended(
        &self,
        observed: &ObservedJob,
        probe: &dyn PositionProbe,
    ) -> Result<(), CoreError> {
        let window = self.config().unattended_window;
        let now = self.clock().elapsed();
        let (print_id, last_position, channel_down) = {
            let mut held = self.watch_state();
            let Some(open) = &held.unattended else {
                return Ok(());
            };
            if now.saturating_sub(open.began) > window {
                held.unattended = None;
                return Ok(());
            }
            (open.print_id, open.position, held.channel_is_down())
        };
        if job_runs(&observed.job.state) || is_down(&observed.job.state) {
            return Ok(());
        }
        let Ok(snapshot) = self.read_snapshot().await else {
            return Ok(());
        };
        if job_runs(&snapshot.connection) || is_down(&snapshot.connection) {
            return Ok(());
        }
        let (tools, bed) = heated(&snapshot);
        let position = probe.read_position().await;
        let moved = match (last_position, position) {
            (_, None) => None,
            (None, Some(_)) => Some(false),
            (Some(before), Some(after)) => Some(before != after),
        };
        let activity = Activity { tools, bed, moved };
        let unchanged = {
            let mut held = self.watch_state();
            let Some(open) = held
                .unattended
                .as_mut()
                .filter(|open| open.print_id == print_id)
            else {
                return Ok(());
            };
            if position.is_some() {
                open.position = position;
            }
            if !activity.is_active() {
                open.recorded = None;
                return Ok(());
            }
            open.recorded.as_ref() == Some(&activity)
        };
        if unchanged {
            return Ok(());
        }
        let draft = self.draft_of(
            None,
            system_source(),
            &UnattendedPrinterActivityPayload {
                last_print_id: print_id,
                tools: activity.tools.clone(),
                bed_target_c: activity.bed,
                moved: activity.moved,
                detail: activity_detail(&activity, channel_down),
            },
        )?;
        let event = self.stores().events.append_event(draft).await?;
        let interval = self.config().observation_interval;
        let turn_due = {
            let mut held = self.watch_state();
            let Some(open) = held.unattended.as_mut() else {
                return Ok(());
            };
            // Remembered only once it is written down: activity the store
            // refused is found again by the next read.
            open.recorded = Some(activity.clone());
            open.turned_at
                .is_none_or(|turned| now.saturating_sub(turned) >= interval)
        };
        if turn_due {
            self.claim_activity_turn(print_id, event).await;
        }
        Ok(())
    }

    /// Claim the turn one activity owes its print's agent, counting it against
    /// the interval once it is claimed; one the store would not answer for is
    /// kept for the next round to claim.
    async fn claim_activity_turn(&self, print_id: PrintId, event: EventRecord) {
        if self.claim_after_end(print_id, event.clone()).await.is_err() {
            self.watch_state().pending_claims.push((print_id, event));
            return;
        }
        let now = self.clock().elapsed();
        if let Some(open) = self
            .watch_state()
            .unattended
            .as_mut()
            .filter(|open| open.print_id == print_id)
        {
            open.turned_at = Some(now);
        }
    }

    /// Ask the failure detector whether it is answering, during a print, and
    /// record each change; going unavailable claims the active print's agent a
    /// turn — unless the detector is scoring that print, when its failing
    /// health check is recorded as a port failure against the change instead,
    /// and the print is still watched.
    ///
    /// No print open, no check: a detector nobody needs is not one to
    /// record.
    ///
    /// # Errors
    ///
    /// Returns the store's own error when a change could not be recorded.
    pub async fn check_detector(
        &self,
        probe: &dyn DetectorProbe,
    ) -> Result<Option<PendingTurn>, CoreError> {
        let Some(print) = self.stores().prints.open_prints().await?.into_iter().next() else {
            return Ok(None);
        };
        let (status, detail) = match probe.check().await {
            Ok(detail) => (DetectorStatus::Available, detail),
            Err(detail) => (DetectorStatus::Unavailable, detail),
        };
        let health = match status {
            DetectorStatus::Available => DetectorHealthStatus::Available,
            DetectorStatus::Unavailable => DetectorHealthStatus::Unavailable,
        };
        let previous = self.watch_state().probe.status();
        if previous == health {
            // The failure the status names beside a scoring detector is the
            // latest one it was answered.
            if let ProbeHealth::Unavailable { detail: held, .. } = &mut self.watch_state().probe {
                held.clone_from(&detail);
            }
            return Ok(None);
        }
        let since = self.clock().now();
        let found = match status {
            DetectorStatus::Available => ProbeHealth::Available { since },
            DetectorStatus::Unavailable => ProbeHealth::Unavailable {
                since,
                detail: detail.clone(),
            },
        };
        // The first check finding the detector up is where watching it begins,
        // not a change in it.
        if previous == DetectorHealthStatus::Unchecked && health == DetectorHealthStatus::Available
        {
            self.watch_state().probe = found;
            return Ok(None);
        }
        let draft = self.draft_of(
            None,
            system_source(),
            &DetectorHealthChangedPayload {
                status,
                detail: detail.clone(),
            },
        )?;
        let event = self.stores().events.append_event(draft).await?;
        // Remembered only once it is written down: a change the store refused
        // is found again by the next check.
        self.watch_state().probe = found;
        if status != DetectorStatus::Unavailable {
            return Ok(None);
        }
        if matches!(
            self.detector_health_for(print.id),
            DetectorHealth::Scoring { .. }
        ) {
            // The detector is still scoring the print, so the print is still
            // watched: the failing check is a port that failed, not an outage.
            self.record_port_failure_to(
                None,
                event.id,
                PortFailureSite::DetectorHealthCheck,
                detail,
            )
            .await;
            return Ok(None);
        }
        let arrival = Arrival {
            frame_captured_at: None,
            event,
            detection: None,
        };
        Ok(self
            .inboxes()
            .claim_or_hand_over(print.id, arrival.clone())
            .then(|| PendingTurn::new(print, arrival, Vec::new())))
    }
}

/// The kinds a print that has ended is still given a turn on: how it ended,
/// and what the printer did on its own after.
pub(crate) fn concerns_its_end(kind: &EventKind) -> bool {
    [
        PrintEndedPayload::KIND,
        UnattendedPrinterActivityPayload::KIND,
    ]
    .contains(&kind.as_str())
}

/// What one event says the watch noticed, in words, for the situation of the
/// turn it prompts; nothing for an event the watch did not raise.
pub(crate) fn noticed_of(event: &EventRecord) -> Option<String> {
    if let Some(Ok(ended)) = event.payload_as::<PrintEndedPayload>() {
        let outcome = match ended.outcome {
            PrintOutcome::Interrupted => "interrupted",
            PrintOutcome::Failed => "failed",
            PrintOutcome::Completed | PrintOutcome::Cancelled => return None,
        };
        return Some(format!(
            "{}: the print ended {outcome}, the printer reporting {:?} ({})",
            PrintEndedPayload::KIND,
            ended.state,
            ended.reason
        ));
    }
    if let Some(Ok(activity)) = event.payload_as::<UnattendedPrinterActivityPayload>() {
        return Some(format!(
            "{}: after print {} ended, {}",
            UnattendedPrinterActivityPayload::KIND,
            activity.last_print_id,
            activity.detail
        ));
    }
    if let Some(Ok(degraded)) = event.payload_as::<crate::kinds::CameraDegradedPayload>() {
        return Some(format!(
            "{}: the camera gave no usable frame in {} attempts, so there is no frame to look \
             at: {}",
            crate::kinds::CameraDegradedPayload::KIND,
            degraded.attempts,
            degraded.detail
        ));
    }
    if let Some(Ok(health)) = event.payload_as::<DetectorHealthChangedPayload>() {
        return Some(format!(
            "{}: the failure detector is {:?}, so nothing is watching the print for failures: {}",
            DetectorHealthChangedPayload::KIND,
            health.status,
            health.detail
        ));
    }
    None
}
