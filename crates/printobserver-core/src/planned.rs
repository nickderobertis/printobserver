//! A print waiting at a change its own G-code plans.
//!
//! A single-nozzle print sliced with colour changes stops at each `M600` so a
//! person can load the next colour, and at an `M601` so a person can do
//! whatever the pause was planned for. To everything else this supervisor
//! watches that wait looks like a stall: the file position freezes, the parts
//! stop growing, and the firmware's idle timeout turns the nozzle target to
//! zero. So a planned stop is recognised, and the print is left alone until it
//! carries on.
//!
//! # Reached
//!
//! A change is reached when a filament-change signal arrives with the job at
//! it, or a read of the job finds its file position at the change's line or
//! just past it ([`GcodeIndex::change_waiting_at`]). Either writes one
//! [`FilamentChangePlannedReachedPayload`] per change of a print, and charges
//! the spool on tool 0 up to the change's own offset, with the
//! `filament_change_signalled` that is the accounting record of every change.
//! A filament-change signal arriving again for a change already reached —
//! `Obico` repeats one every ten minutes while the printer waits — writes
//! nothing of its own and starts nothing.
//!
//! # While it waits
//!
//! No supervision turn runs for the print: no alert's, no scheduled check's
//! and none the printer watch would give. A pause, a resume or an adjustment
//! from any actor is refused
//! [`PlannedChangeWaiting`](crate::records::RejectionReason::PlannedChangeWaiting);
//! a cancel and every spool command are not, because a person may abandon the
//! print or load the next spool while it waits.
//!
//! # Resumed
//!
//! When a read finds the file position past the change, one
//! [`FilamentChangePlannedResumedPayload`] is written with the camera's frame,
//! and that event is the print's next check, at once. The growth measurement
//! starts again from frames taken after it, and the print's next scheduled
//! check is one observation interval after it.
//!
//! # What is remembered, and where
//!
//! The store's history is the record; this supervisor keeps a copy of each
//! print's planned changes so an alert or a request asks nothing of the store
//! for a print whose file plans none. A print's copy is read from the history
//! the first time its index, once built, plans a change, and for every open
//! print as the server starts ([`Supervisor::restore_planned_changes`]).

use std::collections::{BTreeMap, BTreeSet};
use std::sync::PoisonError;
use std::time::Duration;

use printobserver_printer_api::{JobSnapshot, PrinterState};
use printobserver_types::{EventBody, EventKind, EventPayload, EventRecord, PrintId, Timestamp};

use crate::accounting::{Whereabouts, spool_on_tool};
use crate::clock::seconds_between;
use crate::error::CoreError;
use crate::events::PendingTurn;
use crate::gcode_index::{GcodeIndex, PlannedChange, PlannedChanges};
use crate::inbox::{AlertCameraFrame, Arrival};
use crate::kinds::{
    FilamentChangePlannedReachedPayload, FilamentChangePlannedResumedPayload, PlannedChangeTrigger,
};
use crate::look::Frame;
use crate::records::{ActionKind, FilePosition, PrintRecord, RejectionReason};
use crate::store::{HistoryQuery, MAX_HISTORY_LIMIT};
use crate::supervisor::Supervisor;

/// The actions a print waiting at a planned change refuses, whoever asks.
pub const REFUSED_WHILE_WAITING: [ActionKind; 7] = [
    ActionKind::Pause,
    ActionKind::Resume,
    ActionKind::SetFeedrateFactor,
    ActionKind::SetFlowrateFactor,
    ActionKind::SetToolTargetC,
    ActionKind::SetBedTargetC,
    ActionKind::SetFanPercent,
];

/// The change a print is waiting at, as its `reached` recorded it.
#[derive(Debug, Clone, PartialEq)]
pub struct Waiting {
    /// What the `reached` event recorded.
    pub reached: FilamentChangePlannedReachedPayload,
    /// When it was recorded.
    pub since: Timestamp,
}

impl Waiting {
    /// The refusal a pause, a resume or an adjustment is answered while it
    /// waits.
    #[must_use]
    pub fn refusal(&self) -> RejectionReason {
        let change = &self.reached.change;
        RejectionReason::PlannedChangeWaiting {
            change_index: change.index,
            layer: change.layer,
            z_mm: change.z_mm,
            color: change.color.clone(),
        }
    }
}

/// What this supervisor holds of one print's planned changes.
#[derive(Debug, Clone, Default)]
pub(crate) struct PlannedMemo {
    /// Every change of the print's reached so far, by index.
    reached: BTreeSet<u32>,
    /// The change it is waiting at now.
    waiting: Option<Waiting>,
}

/// One print's planned changes as its history records them, newest first.
fn memo_of(history: &[EventRecord]) -> PlannedMemo {
    let mut memo = PlannedMemo::default();
    let mut resumed = BTreeSet::new();
    for event in history.iter().rev() {
        if let Some(Ok(reached)) = event.payload_as::<FilamentChangePlannedReachedPayload>() {
            memo.reached.insert(reached.change.index);
            memo.waiting = Some(Waiting {
                reached,
                since: event.received_at,
            });
        } else if let Some(Ok(done)) = event.payload_as::<FilamentChangePlannedResumedPayload>() {
            resumed.insert(done.change.index);
        }
    }
    memo.waiting = memo
        .waiting
        .filter(|waiting| !resumed.contains(&waiting.reached.change.index));
    memo
}

/// The file position a job read reports for a print, when the job is that
/// print's file, printing or paused, and reports one.
fn position_of(print: &PrintRecord, job: &JobSnapshot) -> Option<u64> {
    let running = matches!(job.state, PrinterState::Printing | PrinterState::Paused);
    let same_file = print.file_name.is_none() || job.file_name == print.file_name;
    job.file_position_bytes
        .filter(|_| running && same_file)
        .and_then(|bytes| u64::try_from(bytes).ok())
}

/// Whether an index plans any change.
fn plans_changes(index: &GcodeIndex) -> bool {
    index.planned_change(0).is_some()
}

impl Supervisor {
    /// The memo of one print's planned changes, read from its history when
    /// this supervisor holds none and its index, built, plans a change.
    async fn planned_memo(&self, print_id: PrintId) -> Result<Option<PlannedMemo>, CoreError> {
        if let Some(memo) = self.planned_memos().get(&print_id) {
            return Ok(Some(memo.clone()));
        }
        let plans = self
            .gcode_indexes()
            .built(print_id)
            .is_some_and(|index| plans_changes(&index));
        if !plans {
            return Ok(None);
        }
        self.load_planned_memo(print_id).await.map(Some)
    }

    /// Read one print's planned changes from its history into the memo.
    async fn load_planned_memo(&self, print_id: PrintId) -> Result<PlannedMemo, CoreError> {
        let kinds = [
            FilamentChangePlannedReachedPayload::KIND,
            FilamentChangePlannedResumedPayload::KIND,
        ]
        .into_iter()
        .filter_map(|kind| EventKind::new(kind).ok())
        .collect();
        let history = self
            .stores()
            .events
            .history(HistoryQuery {
                print_id,
                kinds,
                since: None,
                until: None,
                limit: Some(MAX_HISTORY_LIMIT),
            })
            .await?;
        let memo = memo_of(&history);
        self.planned_memos().insert(print_id, memo.clone());
        Ok(memo)
    }

    /// The memos, held for one read or write.
    fn planned_memos(&self) -> std::sync::MutexGuard<'_, BTreeMap<PrintId, PlannedMemo>> {
        self.planned_memos
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
    }

    /// Read every open print's planned changes from the history, as a server
    /// starts: a print that was waiting at a change when the last one stopped
    /// is still waiting at it, and is left alone from the first read on.
    ///
    /// # Errors
    ///
    /// The store's own error when the open prints or a history could not be
    /// read.
    pub async fn restore_planned_changes(&self) -> Result<(), CoreError> {
        for print in self.stores().prints.open_prints().await? {
            self.load_planned_memo(print.id).await?;
        }
        Ok(())
    }

    /// The planned change one print is waiting at, when it is waiting at one.
    ///
    /// # Errors
    ///
    /// The store's own error when the print's history had to be read and
    /// could not be.
    pub async fn planned_change_waiting(
        &self,
        print_id: PrintId,
    ) -> Result<Option<Waiting>, CoreError> {
        Ok(self
            .planned_memo(print_id)
            .await?
            .and_then(|memo| memo.waiting))
    }

    /// One print's planned changes, the one it waits at and the colour
    /// expected where its job stands, as the status read answers them:
    /// nothing for a print whose file plans none or could not be read.
    ///
    /// # Errors
    ///
    /// The store's own error when the print's history could not be read.
    pub async fn planned_changes(
        &self,
        print: &PrintRecord,
        job: Option<&JobSnapshot>,
    ) -> Result<Option<PlannedChanges>, CoreError> {
        if print.ended_at.is_some() {
            return Ok(None);
        }
        let Some(index) = self.print_gcode_index(print.id).await else {
            return Ok(None);
        };
        if !plans_changes(&index) {
            return Ok(None);
        }
        let waiting = self.planned_change_waiting(print.id).await?;
        let position = job.and_then(|job| position_of(print, job));
        Ok(Some(PlannedChanges {
            changes: index.planned_changes(),
            segments: index.planned_segments(),
            waiting: waiting.map(|waiting| waiting.reached.change.index),
            expected_color: position.and_then(|position| {
                index.expected_color_at(i64::try_from(position).unwrap_or(i64::MAX))
            }),
        }))
    }

    /// Whether a print may be given a supervision turn now: not while it
    /// waits at a planned change.
    pub(crate) async fn turns_allowed(&self, print_id: PrintId) -> Result<bool, CoreError> {
        Ok(self.planned_change_waiting(print_id).await?.is_none())
    }

    /// Follow one read of a print's job against the changes its file plans:
    /// record a change it has reached, or one it has carried on from, and
    /// answer the check carrying on claims.
    ///
    /// Only a print whose index is already built is followed here; the check
    /// or the signal that builds it is what first reads the file.
    ///
    /// # Errors
    ///
    /// The store's own error when what changed could not be recorded.
    pub(crate) async fn follow_planned_changes(
        &self,
        print: &PrintRecord,
        job: &JobSnapshot,
    ) -> Result<Option<PendingTurn>, CoreError> {
        let Some(memo) = self.planned_memo(print.id).await? else {
            return Ok(None);
        };
        let Some(position) = position_of(print, job) else {
            return Ok(None);
        };
        if let Some(waiting) = memo.waiting {
            let change = &waiting.reached.change;
            if position > change.offset_bytes + crate::gcode_index::PLANNED_CHANGE_REACH_BYTES {
                return self
                    .resume_planned_change(print, waiting, job, position)
                    .await;
            }
            return Ok(None);
        }
        let Some(index) = self.gcode_indexes().built(print.id) else {
            return Ok(None);
        };
        if let Some(change) = index.change_waiting_at(position).cloned()
            && !memo.reached.contains(&change.index)
        {
            self.reach_planned_change(
                print,
                &index,
                change,
                PlannedChangeTrigger::Position,
                job,
                None,
            )
            .await?;
        }
        Ok(None)
    }

    /// Whether a filament-change signal is one about a planned change: one
    /// the print is already waiting at, which is answered with nothing more,
    /// or one the job stands at now, which is recorded reached. Answers
    /// whether it was, so a signal at no planned change is accounted as every
    /// unplanned one is.
    ///
    /// # Errors
    ///
    /// The store's own error when what it reached could not be recorded.
    pub(crate) async fn signal_at_planned_change(
        &self,
        print: &PrintRecord,
        source_kind: &EventKind,
    ) -> Result<bool, CoreError> {
        if self.planned_change_waiting(print.id).await?.is_some() {
            return Ok(true);
        }
        let Some(index) = self.print_gcode_index(print.id).await else {
            return Ok(false);
        };
        if !plans_changes(&index) {
            return Ok(false);
        }
        let Some(memo) = self.planned_memo(print.id).await? else {
            return Ok(false);
        };
        let Ok(job) = self.read_job().await else {
            return Ok(false);
        };
        let Some(change) = position_of(print, &job)
            .and_then(|position| index.change_waiting_at(position))
            .filter(|change| !memo.reached.contains(&change.index))
            .cloned()
        else {
            return Ok(false);
        };
        self.reach_planned_change(
            print,
            &index,
            change,
            PlannedChangeTrigger::Signal,
            &job,
            Some(source_kind),
        )
        .await?;
        Ok(true)
    }

    /// Record one planned change reached, and charge the spool feeding it up
    /// to the change's offset.
    async fn reach_planned_change(
        &self,
        print: &PrintRecord,
        index: &GcodeIndex,
        change: PlannedChange,
        trigger: PlannedChangeTrigger,
        job: &JobSnapshot,
        signal_kind: Option<&EventKind>,
    ) -> Result<(), CoreError> {
        let now = self.clock().now();
        let segments = index.planned_segments();
        let segment = |at: u32| segments.iter().find(|segment| segment.segment == at);
        let next = change.index + 1;
        let next_spool_id = self
            .stores()
            .prints
            .manifest(print.id)
            .await?
            .and_then(|manifest| {
                manifest
                    .segment_spools
                    .iter()
                    .find(|named| named.segment == next)
                    .map(|named| named.spool_id)
            });
        let loaded = self.stores().filament.loaded_spools().await?;
        let offset = i64::try_from(change.offset_bytes).unwrap_or(i64::MAX);
        let payload = FilamentChangePlannedReachedPayload {
            previous_color: segment(change.index).and_then(|segment| segment.color.clone()),
            trigger,
            position: FilePosition {
                bytes: job.file_position_bytes.unwrap_or(offset),
                size_bytes: job.size_bytes.filter(|size| *size > 0),
                observed_at: now,
            },
            spool_id: spool_on_tool(&loaded, 0).map(|spool| spool.id),
            next_spool_id,
            next_segment_filament_mm: segment(next).map_or(0.0, |segment| segment.filament_mm),
            change: change.clone(),
        };
        let event = self
            .append_system_event(print.id, EventBody::of(&payload)?)
            .await?;
        {
            let mut memos = self.planned_memos();
            let memo = memos.entry(print.id).or_default();
            memo.reached.insert(change.index);
            memo.waiting = Some(Waiting {
                reached: payload,
                since: event.received_at,
            });
        }
        let charged_at = FilePosition {
            bytes: offset,
            size_bytes: job.size_bytes.filter(|size| *size > 0),
            observed_at: now,
        };
        let source_kind = signal_kind.cloned().unwrap_or_else(|| event.kind().clone());
        self.charge_filament_change(
            print,
            &source_kind,
            Whereabouts::ReadNow(charged_at),
            Some(job.clone()),
            Some(change.index),
        )
        .await
    }

    /// Record a print carried on from the change it waited at, with the
    /// camera's frame, and claim the check that event is.
    async fn resume_planned_change(
        &self,
        print: &PrintRecord,
        waiting: Waiting,
        job: &JobSnapshot,
        position: u64,
    ) -> Result<Option<PendingTurn>, CoreError> {
        let now = self.clock().now();
        let payload = FilamentChangePlannedResumedPayload {
            change: waiting.reached.change.clone(),
            previous_color: waiting.reached.previous_color.clone(),
            waited_s: seconds_between(waiting.since, now).max(0),
            position: FilePosition {
                bytes: i64::try_from(position).unwrap_or(i64::MAX),
                size_bytes: job.size_bytes.filter(|size| *size > 0),
                observed_at: now,
            },
        };
        let fetched = self.fetch_configured_frame(Some(print.id)).await;
        let (fetched, degraded) = match fetched {
            Some((_, Frame::Degraded(degraded))) => (None, Some(degraded)),
            fetched => (fetched, None),
        };
        let event = self
            .write_with_frame(Some(print.id), &payload, fetched)
            .await?;
        if let Some(degraded) = degraded {
            self.append_system_event(print.id, EventBody::of(&degraded)?)
                .await?;
        }
        if let Some(memo) = self.planned_memos().get_mut(&print.id) {
            memo.waiting = None;
        }
        if let Some(images) = self.check_images.get() {
            images.restart_growth(print.id);
        }
        self.checked_after_change
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .insert(print.id, self.clock().elapsed());
        let arrival = Arrival {
            event,
            detection: None,
            frame_captured_at: None,
            camera_frame: AlertCameraFrame::NotAsked,
        };
        Ok(self
            .inboxes()
            .claim_or_hand_over(print.id, arrival.clone())
            .then(|| PendingTurn::new(print.clone(), arrival, Vec::new())))
    }

    /// Whether a print's scheduled check is due: not until one observation
    /// interval after the check its last planned change's resumption ran.
    pub(crate) fn observation_due(&self, print_id: PrintId) -> bool {
        let checked = self
            .checked_after_change
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .get(&print_id)
            .copied();
        checked.is_none_or(|checked| {
            self.clock().elapsed().saturating_sub(checked) >= self.config().observation_interval
        })
    }

    /// How long the periodic observation should wait before its next round:
    /// one interval, or less when a print's check after a planned change makes
    /// its next scheduled check due sooner.
    #[must_use]
    pub fn until_next_observation(&self) -> Duration {
        let interval = self.config().observation_interval;
        let now = self.clock().elapsed();
        self.checked_after_change
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .values()
            .map(|checked| (*checked + interval).saturating_sub(now))
            .filter(|left| !left.is_zero())
            .min()
            .map_or(interval, |left| left.min(interval))
    }

    /// Forget what this supervisor holds of a print's planned changes, once
    /// it has closed.
    pub(crate) fn forget_planned_changes(&self, print_id: PrintId) {
        self.planned_memos().remove(&print_id);
        self.checked_after_change
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .remove(&print_id);
    }
}
