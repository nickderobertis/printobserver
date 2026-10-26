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
    DetectorHealthChangedPayload, DetectorStatus, HeatedTool, PrintEndedPayload,
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

/// The failure detector's health, as the last check found it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema, Default)]
#[serde(
    crate = "printobserver_types::serde",
    rename_all = "snake_case",
    deny_unknown_fields
)]
#[schemars(crate = "printobserver_types::schemars")]
pub enum DetectorHealthStatus {
    /// It answered its health check.
    Available,
    /// It did not.
    Unavailable,
    /// Nothing checks it: no detector health address is configured, or no
    /// check has been made yet.
    #[default]
    Unchecked,
}

/// The failure detector's health, and since when it has been so.
///
/// Three states and no fourth: a detector nobody has checked has no "since",
/// and one that was checked always has one. On the wire it is the object the
/// status and context reads answer — `status`, and `since` that is `null`
/// exactly when the status is `unchecked` — and an object pairing them any
/// other way is refused rather than read.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum DetectorHealth {
    /// Nothing checks it: no detector health address is configured, or no
    /// check has been made yet.
    #[default]
    Unchecked,
    /// It answered its health check, from this instant on.
    Available {
        /// When it was first found answering.
        since: Timestamp,
    },
    /// It did not, from this instant on.
    Unavailable {
        /// When it was first found not answering.
        since: Timestamp,
    },
}

impl DetectorHealth {
    /// What the last check found.
    #[must_use]
    pub const fn status(&self) -> DetectorHealthStatus {
        match self {
            Self::Unchecked => DetectorHealthStatus::Unchecked,
            Self::Available { .. } => DetectorHealthStatus::Available,
            Self::Unavailable { .. } => DetectorHealthStatus::Unavailable,
        }
    }
}

/// The failure detector's health, and since when it has been so.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(crate = "printobserver_types::serde", deny_unknown_fields)]
#[schemars(crate = "printobserver_types::schemars", rename = "DetectorHealth")]
struct DetectorHealthWire {
    /// What the last check found.
    status: DetectorHealthStatus,
    /// When the status became what it is, `null` exactly while unchecked.
    #[serde(default)]
    since: Option<Timestamp>,
}

impl From<DetectorHealth> for DetectorHealthWire {
    fn from(health: DetectorHealth) -> Self {
        let since = match health {
            DetectorHealth::Unchecked => None,
            DetectorHealth::Available { since } | DetectorHealth::Unavailable { since } => {
                Some(since)
            }
        };
        Self {
            status: health.status(),
            since,
        }
    }
}

impl TryFrom<DetectorHealthWire> for DetectorHealth {
    type Error = &'static str;

    fn try_from(wire: DetectorHealthWire) -> Result<Self, Self::Error> {
        match (wire.status, wire.since) {
            (DetectorHealthStatus::Unchecked, None) => Ok(Self::Unchecked),
            (DetectorHealthStatus::Available, Some(since)) => Ok(Self::Available { since }),
            (DetectorHealthStatus::Unavailable, Some(since)) => Ok(Self::Unavailable { since }),
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
        DetectorHealthWire::from(*self).serialize(serializer)
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

impl JsonSchema for DetectorHealth {
    fn schema_name() -> std::borrow::Cow<'static, str> {
        DetectorHealthWire::schema_name()
    }

    fn json_schema(
        generator: &mut printobserver_types::schemars::SchemaGenerator,
    ) -> printobserver_types::schemars::Schema {
        DetectorHealthWire::json_schema(generator)
    }
}

impl Sample for DetectorHealth {
    fn sample_full() -> Self {
        Self::Unavailable {
            since: Timestamp::sample_full(),
        }
    }

    fn sample_minimal() -> Self {
        Self::Unchecked
    }

    fn sample_alternates() -> Vec<Self> {
        vec![Self::Available {
            since: Timestamp::sample_full(),
        }]
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
    /// The detector's health.
    detector: DetectorHealth,
    /// Turns an ending claimed, for the next watch round to answer.
    pending_turns: Vec<PendingTurn>,
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

    /// The failure detector's health, as the last check found it.
    #[must_use]
    pub fn detector_health(&self) -> DetectorHealth {
        self.watch_state().detector
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
                self.watch_state().pushes.reset = true;
                let during_print = self.running_print().await?.map(|print| print.id);
                let detail = match during_print {
                    Some(print_id) => format!("the printer pushed a reset during print {print_id}"),
                    None => "the printer pushed a reset while no print was running".to_owned(),
                };
                let draft = self.draft_of(
                    None,
                    system_source(),
                    &PrinterResetPayload {
                        during_print,
                        detail,
                    },
                )?;
                self.stores().events.append_event(draft).await?;
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
        let resolving = self.resolving().await;
        let observed = self.observe_job().await;
        if let Some(observed) = &observed {
            self.note_connection(&observed.job.state, observed.job.error.as_deref())
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
            let mut held = self.watch_state();
            (
                held.last_state.replace(state.clone()),
                held.channel_is_down(),
            )
        };
        let Some(previous) = previous else {
            return Ok(());
        };
        if previous == *state || !(is_down(&previous) || is_down(state)) {
            return Ok(());
        }
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
                from: previous,
                to: state.clone(),
                detail,
            },
        )?;
        self.stores().events.append_event(draft).await?;
        Ok(())
    }

    /// What follows a print's close-out: an `interrupted` or `failed` ending
    /// opens the unattended window and claims its agent a turn on the
    /// `print_ended`; any other ending closes the window.
    pub(crate) async fn after_close_out(&self, print_id: PrintId) -> Result<(), CoreError> {
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
            let unchanged = open.recorded.as_ref() == Some(&activity);
            open.recorded = Some(activity.clone());
            unchanged
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
            let due = open
                .turned_at
                .is_none_or(|turned| now.saturating_sub(turned) >= interval);
            if due {
                open.turned_at = Some(now);
            }
            due
        };
        if turn_due {
            self.claim_after_end(print_id, event).await?;
        }
        Ok(())
    }

    /// Ask the failure detector whether it is answering, during a print, and
    /// record each change; going unavailable claims the active print's agent a
    /// turn.
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
        {
            let mut held = self.watch_state();
            let previous = held.detector.status();
            if previous == health {
                return Ok(None);
            }
            let since = self.clock().now();
            held.detector = match status {
                DetectorStatus::Available => DetectorHealth::Available { since },
                DetectorStatus::Unavailable => DetectorHealth::Unavailable { since },
            };
            // The first check finding the detector up is where watching it
            // begins, not a change in it.
            if previous == DetectorHealthStatus::Unchecked
                && health == DetectorHealthStatus::Available
            {
                return Ok(None);
            }
        }
        let draft = self.draft_of(
            None,
            system_source(),
            &DetectorHealthChangedPayload { status, detail },
        )?;
        let event = self.stores().events.append_event(draft).await?;
        if status != DetectorStatus::Unavailable {
            return Ok(None);
        }
        let arrival = Arrival {
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
