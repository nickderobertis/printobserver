//! A print the detector paused, and the resume an adjustment earns it.
//!
//! A detector that pauses a print reacts faster than any turn can, so the
//! pause is left to it; what it cannot do is decide whether the print can be
//! saved. When the supervising agent decides it can — and asks for an
//! adjustment while the print is still held by the detector's pause — the
//! adjustment is applied at once and the print is resumed with it, so that the
//! change takes effect as the print moves again rather than waiting for a
//! person to notice the pause.
//!
//! # The resume is the supervisor's, through the policy
//!
//! The agent is not granted resume: a print it may resume is a print whose
//! pause it may overrule, and a pause a person made is not one it should.
//! The resume here is requested as [`Actor::System`], through
//! [`Supervisor::request_action`] like every other action, so it is recorded
//! with its reason and refused — and the print left paused — when the envelope
//! does not grant the system resume.
//!
//! # When the resume happens
//!
//! [`CoreConfig::detector_resume_grace`](crate::CoreConfig) after the agent's
//! last such adjustment, or when its turn ends, whichever comes first — so
//! that two or three changes asked for one after the other reach the print
//! together. An agent that acknowledges the detection with the disposition
//! `stop` has decided a person should look, and the pause is then left alone;
//! so is one somebody else has already resumed or cancelled.
//!
//! # The detector is told
//!
//! Once the print has resumed, the detector is told its detection was handled,
//! which is what re-arms it for the rest of the print; a failure to tell it is
//! recorded against the detection's own event.

use std::collections::BTreeMap;

use printobserver_types::{EventId, PrintId, Timestamp};
use printobserver_vision_api::Detection;

use crate::clock::plus_seconds;
use crate::kinds::PortFailureSite;
use crate::records::{Actor, PrintAction};
use crate::supervisor::Supervisor;

/// One print the detector paused, and what the agent has asked of it since.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct DetectorPause {
    /// The event the detection arrived as.
    pub event_id: EventId,
    /// What the detector did.
    pub detection: Detection,
    /// The reasons of the adjustments asked for while it was paused, in order.
    pub adjustments: Vec<String>,
    /// When the print is to be resumed, once an adjustment has been asked for.
    pub resume_due: Option<Timestamp>,
}

/// Every print the detector has paused and nobody has resumed yet.
pub(crate) type DetectorPauses = BTreeMap<PrintId, DetectorPause>;

impl Supervisor {
    /// Remember that the detector paused one print, under one event.
    pub(crate) fn note_detector_pause(
        &self,
        print_id: PrintId,
        event_id: EventId,
        detection: Detection,
    ) {
        self.detector_pauses().insert(
            print_id,
            DetectorPause {
                event_id,
                detection,
                adjustments: Vec::new(),
                resume_due: None,
            },
        );
    }

    /// Note an adjustment the agent asked for while the detector's pause held
    /// the print, and schedule the resume it earns.
    pub(crate) fn note_adjustment_while_paused(&self, print_id: PrintId, reason: &str) {
        let now = self.clock().now();
        let grace =
            i64::try_from(self.config().detector_resume_grace.as_secs()).unwrap_or(i64::MAX);
        let due = plus_seconds(now, grace).unwrap_or(now);
        if let Some(pause) = self.detector_pauses().get_mut(&print_id) {
            pause.adjustments.push(reason.to_owned());
            pause.resume_due = Some(due);
        }
    }

    /// Leave the detector's pause alone from now on: the agent decided a
    /// person should look, somebody else moved the print on, or it ended.
    pub(crate) fn forget_detector_pause(&self, print_id: PrintId) {
        self.detector_pauses().remove(&print_id);
    }

    /// Whether the detector's pause is one this supervisor is still holding
    /// for one print.
    pub(crate) fn detector_paused(&self, print_id: PrintId) -> bool {
        self.detector_pauses().contains_key(&print_id)
    }

    /// Resume every print whose resume has come due.
    pub async fn resume_due_detector_pauses(&self) {
        let now = self.clock().now();
        let due: Vec<(PrintId, DetectorPause)> = {
            let mut pauses = self.detector_pauses();
            let ids: Vec<PrintId> = pauses
                .iter()
                .filter(|(_, pause)| pause.resume_due.is_some_and(|at| at <= now))
                .map(|(id, _)| *id)
                .collect();
            ids.into_iter()
                .filter_map(|id| pauses.remove(&id).map(|pause| (id, pause)))
                .collect()
        };
        for (print_id, pause) in due {
            self.resume_detector_pause(print_id, pause).await;
        }
    }

    /// Resume one print at once when the agent adjusted it under the
    /// detector's pause, because the turn that asked has ended.
    pub(crate) async fn resume_adjusted_detector_pause(&self, print_id: PrintId) {
        let taken = {
            let mut pauses = self.detector_pauses();
            match pauses.get(&print_id) {
                Some(pause) if !pause.adjustments.is_empty() => pauses.remove(&print_id),
                _ => None,
            }
        };
        if let Some(pause) = taken {
            self.resume_detector_pause(print_id, pause).await;
        }
    }

    /// Resume one print the detector paused, and tell the detector.
    ///
    /// A resume the policy refuses, or the printer does, is recorded by the
    /// request itself, and the detector is then told nothing: the print is
    /// still paused, and a re-armed detector over a paused print would alert
    /// on nothing.
    async fn resume_detector_pause(&self, print_id: PrintId, pause: DetectorPause) {
        let reason = format!(
            "the detector paused this print and the supervising agent adjusted it while \
             paused ({}); resuming so the adjustment takes effect",
            pause.adjustments.join("; ")
        );
        let resumed = self
            .request_action(
                print_id,
                PrintAction::Resume {
                    reason,
                    actor: Actor::System,
                },
            )
            .await;
        if !resumed.is_ok_and(|outcome| matches!(outcome.executed, Some(Ok(())))) {
            return;
        }
        if let Err(error) = self.vision().clear_detection(pause.detection).await {
            self.record_port_failure(
                print_id,
                pause.event_id,
                PortFailureSite::DetectorAcknowledgement,
                error.to_string(),
            )
            .await;
        }
    }
}
