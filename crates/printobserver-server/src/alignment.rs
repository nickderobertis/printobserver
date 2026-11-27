//! Each print's first-layer alignment verdict and its camera view: whether the
//! camera calibration's projection lands on that print's printed first layer,
//! and the correction every later drawing of the print goes through.
//!
//! A projection can be wrong with the camera unmoved, and a camera that has
//! shifted a little is ordinary on a real printer, so each print judges its
//! own. Once the print's first object layer is down, every scheduled check's
//! own stored frame is judged ([`Alignments::judge`]), at most once a check
//! and with no camera fetch of its own, until one passes: the layer clearly
//! found, on a frame the toolhead does not cover
//! ([`printobserver_panels::judge_first_layer`]). A covered or unclear frame
//! is inconclusive and the next check's is judged again. A check taken while
//! the printer reports the print paused is not judged: its bed is lowered off
//! the plan's height. Only a frame of another size or stream fails, and that
//! holds for the rest of the print.
//!
//! A pass's found shift becomes the print's correction, a translation every
//! later drawing goes through, kept with the landmarks found agreeing on that
//! frame as its reference. Every later frame's view is judged against that
//! reference ([`printobserver_panels::judge_found`]); one showing the camera
//! moved again sets the alignment back to pending, to take a fresh correction
//! from a later check. Nothing is kept across prints: a sheet sits a few
//! millimetres apart from one print to the next.
//!
//! Every judgement is written into the print's history, naming the stored
//! frame it was made on ([`crate::judgements`]).
//!
//! The verdict is written to `<state_dir>/alignment/<print id>.json` and the
//! correction, the last inconclusive reason and the last check's image to
//! `<state_dir>/camera-view/<print id>.json`, so they stand through a restart
//! while the print is open, and are deleted once the print closes. A verdict
//! file an earlier release left failed on its offset is read as pending and
//! judged again.
//!
//! Which check draws the toolpath on its image reads the verdict through
//! [`Alignments::print_alignment`] and draws only for a print that `Passed`.
//!
//! Beside each verdict is kept the frame the print's last check was given,
//! which the next check's image shows as the previous one
//! ([`Alignments::previous_check`]): written under
//! `<state_dir>/previous-check/<print id>.json`, standing through a restart
//! while the print is open, and swept with the verdict once it closes.
//!
//! And beside that, the frames of the print's recent checks with the height
//! the plan had reached at each ([`Alignments::checked_frames`]): written
//! under `<state_dir>/checked-frames/<print id>.json`, kept and swept the same
//! way, so that a check can measure whether the parts grew since one far
//! enough below it ([`Alignments::growth_reference`]).

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use printobserver_core::PlanPosition;
use printobserver_core::store::PrintStore;
use printobserver_panels::{
    AlignmentInputs, CameraCalibration, ComposeError, FirstLayerInputs, FirstLayerVerdict,
    FrameField, InconclusiveReason, ReferenceLandmark, ViewCorrection, ViewJudgement, ViewOutcome,
    judge_first_layer, judge_found, landmarks_in_frame,
};
use printobserver_supervisor_api::{ImageFallback, ImageKind};
use printobserver_types::contract::Sample;
use printobserver_types::schemars::JsonSchema;
use printobserver_types::serde::{Deserialize, Serialize};
use printobserver_types::{EventBody, EventPayload, PrintId, Timestamp};

/// Why a print's alignment failed.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(
    crate = "printobserver_types::serde",
    tag = "failure",
    rename_all = "snake_case"
)]
#[schemars(crate = "printobserver_types::schemars")]
pub enum AlignmentFailure {
    /// The printed first layer lies further from its projection than the
    /// calibration allows. No longer written: a frame that does not clearly
    /// find the layer is inconclusive, and a verdict file an earlier release
    /// left holding this is read as pending.
    Offset {
        /// How far, in calibration pixels.
        offset_px: f64,
    },
    /// The camera's frames are not the calibration's resolution.
    Resolution {
        /// The calibration's, `[width, height]`.
        expected: [u32; 2],
        /// The frame's.
        got: [u32; 2],
    },
    /// The camera configured is not the stream the calibration was fitted on.
    Stream,
}

/// One print's first-layer alignment verdict, as it is kept under the state
/// directory — which the real-print suite's record reads.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(
    crate = "printobserver_types::serde",
    tag = "verdict",
    rename_all = "snake_case"
)]
#[schemars(crate = "printobserver_types::schemars")]
pub enum PrintAlignment {
    /// Nothing can be checked: no calibration is configured, or the print's
    /// G-code lays no object.
    Unavailable,
    /// The first object layer is not down yet, or no frame of it has been
    /// judged.
    Pending,
    /// The projected first layer lands on the printed one.
    Passed {
        /// How far it lies from it, in calibration pixels.
        offset_px: f64,
        /// The first-layer correction applied to every later drawing of the
        /// print, `[dx, dy]` calibration pixels: the shift the layer was found
        /// at. `[0, 0]` in a verdict an earlier release wrote.
        #[serde(default)]
        correction_px: [f64; 2],
    },
    /// It does not, or the frames cannot be judged against the calibration.
    Failed {
        /// Why.
        reason: AlignmentFailure,
    },
}

impl Sample for PrintAlignment {
    fn sample_full() -> Self {
        Self::Passed {
            offset_px: 14.6,
            correction_px: [14.0, 4.0],
        }
    }

    fn sample_minimal() -> Self {
        Self::Pending
    }

    fn sample_alternates() -> Vec<Self> {
        vec![
            Self::Unavailable,
            Self::Failed {
                reason: AlignmentFailure::Offset { offset_px: 48.5 },
            },
            Self::Failed {
                reason: AlignmentFailure::Resolution {
                    expected: [1920, 1080],
                    got: [1280, 720],
                },
            },
            Self::Failed {
                reason: AlignmentFailure::Stream,
            },
        ]
    }
}

/// The frame one print's last check was given, and when it was captured.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(crate = "printobserver_types::serde")]
pub struct PreviousCheck {
    /// The stored frame's path.
    pub frame: PathBuf,
    /// When it was captured.
    pub captured_at: printobserver_types::Timestamp,
}

/// One check's frame and the height the plan had reached as it was captured.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(crate = "printobserver_types::serde")]
pub struct CheckedFrame {
    /// The stored frame's path.
    pub frame: PathBuf,
    /// When it was captured.
    pub captured_at: printobserver_types::Timestamp,
    /// The print's height in its plan then, in millimetres.
    pub print_z_mm: f64,
}

/// The most checked frames one print keeps; past it the oldest are dropped.
const MAX_CHECKED_FRAMES: usize = 512;

/// The alignment verdicts of every print, what decides them, the frame each
/// print's last check was given, and its recent checks' frames and heights.
pub struct Alignments {
    /// Where each print's verdict is kept.
    directory: PathBuf,
    /// Where each print's last check's frame is named.
    previous_directory: PathBuf,
    /// Where each print's recent checks' frames and heights are kept.
    checked_directory: PathBuf,
    /// Where each print's correction, last inconclusive reason and last
    /// check's image are kept.
    view_directory: PathBuf,
    /// The camera's calibration, when one is configured.
    calibration: Option<Arc<CameraCalibration>>,
    /// The prints, to tell which are open.
    prints: Arc<dyn PrintStore>,
}

impl core::fmt::Debug for Alignments {
    fn fmt(&self, formatter: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        formatter
            .debug_struct("Alignments")
            .field("directory", &self.directory)
            .field("calibrated", &self.calibration.is_some())
            .finish_non_exhaustive()
    }
}

/// The directory verdicts are kept in, under a state directory.
#[must_use]
pub fn verdict_dir(state_dir: &Path) -> PathBuf {
    state_dir.join("alignment")
}

/// The directory each print's last check's frame is named in, under a state
/// directory.
#[must_use]
pub fn previous_check_dir(state_dir: &Path) -> PathBuf {
    state_dir.join("previous-check")
}

/// The directory each print's recent checks' frames and heights are kept in,
/// under a state directory.
#[must_use]
pub fn checked_frames_dir(state_dir: &Path) -> PathBuf {
    state_dir.join("checked-frames")
}

/// The directory each print's correction, last inconclusive reason and last
/// check's image are kept in, under a state directory.
#[must_use]
pub fn camera_view_dir(state_dir: &Path) -> PathBuf {
    state_dir.join("camera-view")
}

impl Alignments {
    /// The verdicts kept under one state directory.
    ///
    /// # Errors
    ///
    /// The filesystem's own error when the directory cannot be made.
    pub fn new(
        state_dir: &Path,
        calibration: Option<Arc<CameraCalibration>>,
        prints: Arc<dyn PrintStore>,
    ) -> std::io::Result<Self> {
        let directory = verdict_dir(state_dir);
        std::fs::create_dir_all(&directory)?;
        let previous_directory = previous_check_dir(state_dir);
        std::fs::create_dir_all(&previous_directory)?;
        let checked_directory = checked_frames_dir(state_dir);
        std::fs::create_dir_all(&checked_directory)?;
        let view_directory = camera_view_dir(state_dir);
        std::fs::create_dir_all(&view_directory)?;
        Ok(Self {
            directory,
            previous_directory,
            checked_directory,
            view_directory,
            calibration,
            prints,
        })
    }

    /// Where one print's verdict is kept.
    fn path_of(&self, print_id: PrintId) -> PathBuf {
        self.directory.join(format!("{print_id}.json"))
    }

    /// The camera calibration in force, when one is configured.
    #[must_use]
    pub fn calibration(&self) -> Option<&CameraCalibration> {
        self.calibration.as_deref()
    }

    /// Where one print's last check's frame is named.
    fn previous_path_of(&self, print_id: PrintId) -> PathBuf {
        self.previous_directory.join(format!("{print_id}.json"))
    }

    /// The frame one print's last check was given; nothing before its first
    /// check, or when the record cannot be read back.
    #[must_use]
    pub fn previous_check(&self, print_id: PrintId) -> Option<PreviousCheck> {
        std::fs::read(self.previous_path_of(print_id))
            .ok()
            .and_then(|bytes| printobserver_types::serde_json::from_slice(&bytes).ok())
    }

    /// Name the frame one print's check was given, which its next check shows
    /// as the previous. A record the filesystem refuses for a moment is
    /// replaced once the refusal passes ([`replace_file`]); one it goes on
    /// refusing is said on the log and leaves the record before it standing,
    /// so the next check shows an older frame as the previous rather than
    /// none. No test makes the refusal last: that needs the state directory
    /// made to refuse writes beneath a running server, which refuses the
    /// store's own writes first. What a check makes of a record it cannot use
    /// — an empty panel 3 — is proved by
    /// `a_lost_previous_frame_empties_panel_3_and_the_record_goes_with_the_print`.
    pub fn record_check(&self, print_id: PrintId, check: &PreviousCheck) {
        let Ok(bytes) = printobserver_types::serde_json::to_vec(check) else {
            return;
        };
        if let Err(error) = replace_file(&self.previous_path_of(print_id), &bytes) {
            eprintln!(
                "printobserver: print {print_id}: the last check's frame could not be recorded, so the next check shows an older one or none: {error}"
            );
        }
    }

    /// Where one print's recent checks' frames and heights are kept.
    fn checked_path_of(&self, print_id: PrintId) -> PathBuf {
        self.checked_directory.join(format!("{print_id}.json"))
    }

    /// One print's recent checks' frames and heights, oldest first; none
    /// before its first check with a height, or when the record cannot be
    /// read back.
    #[must_use]
    pub fn checked_frames(&self, print_id: PrintId) -> Vec<CheckedFrame> {
        std::fs::read(self.checked_path_of(print_id))
            .ok()
            .and_then(|bytes| printobserver_types::serde_json::from_slice(&bytes).ok())
            .unwrap_or_default()
    }

    /// Forget one print's checked frames, so its growth is measured only from
    /// checks made after now: the print waited at a planned change, and a frame
    /// from before the wait would count the wait as no growth. A record the
    /// filesystem will not remove is said on the log and left, so a later check
    /// measures from a frame before the wait, which says no more than that a
    /// check could not tell.
    pub fn restart_growth(&self, print_id: PrintId) {
        let path = self.checked_path_of(print_id);
        if let Err(error) = std::fs::remove_file(&path)
            && error.kind() != std::io::ErrorKind::NotFound
        {
            eprintln!(
                "printobserver: print {print_id}: its checked frames could not be forgotten after a planned change, so a later check may measure growth across the wait: {error}"
            );
        }
    }

    /// The frame a check at `print_z_mm` measures growth from: the newest
    /// checked frame of the print at least `rise_mm` below it in the plan.
    #[must_use]
    pub fn growth_reference(
        &self,
        print_id: PrintId,
        print_z_mm: f64,
        rise_mm: f64,
    ) -> Option<CheckedFrame> {
        reference_in(&self.checked_frames(print_id), print_z_mm, rise_mm).cloned()
    }

    /// Keep one check's frame and height among the print's, keeping only what
    /// a later check at `rise_mm` above some kept height can still measure
    /// from: the newest frame at least that far below this one and every
    /// frame after it. A height below the last kept one is a plan that went
    /// back down, as a print of one object after another does, and starts the
    /// record again. A record the filesystem refuses is said on the log and
    /// leaves the one before standing, as [`record_check`](Self::record_check)
    /// does.
    pub fn record_checked_frame(&self, print_id: PrintId, checked: CheckedFrame, rise_mm: f64) {
        let kept = kept_with(self.checked_frames(print_id), checked, rise_mm);
        let Ok(bytes) = printobserver_types::serde_json::to_vec(&kept) else {
            return;
        };
        if let Err(error) = replace_file(&self.checked_path_of(print_id), &bytes) {
            eprintln!(
                "printobserver: print {print_id}: a check's frame and height could not be recorded, so a later check measures growth from an older one or none: {error}"
            );
        }
    }

    /// One print's verdict as it stands.
    #[must_use]
    pub fn print_alignment(&self, print_id: PrintId) -> PrintAlignment {
        if self.calibration.is_none() {
            return PrintAlignment::Unavailable;
        }
        match std::fs::read(self.path_of(print_id))
            .ok()
            .and_then(|bytes| printobserver_types::serde_json::from_slice(&bytes).ok())
        {
            // A failure on the offset is what an earlier release wrote for a
            // frame that did not find the layer, which is now inconclusive.
            Some(PrintAlignment::Failed {
                reason: AlignmentFailure::Offset { .. },
            })
            | None => PrintAlignment::Pending,
            Some(verdict) => verdict,
        }
    }

    /// Where one print's camera view is kept.
    fn view_path_of(&self, print_id: PrintId) -> PathBuf {
        self.view_directory.join(format!("{print_id}.json"))
    }

    /// One print's camera view as it stands: nothing kept before its first
    /// judgement, or when the record cannot be read back.
    fn view_state(&self, print_id: PrintId) -> CameraViewState {
        std::fs::read(self.view_path_of(print_id))
            .ok()
            .and_then(|bytes| printobserver_types::serde_json::from_slice(&bytes).ok())
            .unwrap_or_default()
    }

    /// Keep one print's camera view. A record the filesystem refuses is said
    /// on the log and leaves the one before standing, as
    /// [`record_check`](Self::record_check) does.
    fn keep_view_state(&self, print_id: PrintId, state: &CameraViewState) {
        let Ok(bytes) = printobserver_types::serde_json::to_vec(state) else {
            return;
        };
        if let Err(error) = replace_file(&self.view_path_of(print_id), &bytes) {
            eprintln!(
                "printobserver: print {print_id}: its camera view could not be recorded, so a later check reads the one before: {error}"
            );
        }
    }

    /// Keep a verdict, and say it. A verdict the filesystem goes on refusing
    /// ([`replace_file`]) is said on the log and leaves the one before
    /// standing, so the next check judges again from it.
    fn decide(&self, print_id: PrintId, verdict: &PrintAlignment) {
        let Ok(bytes) = printobserver_types::serde_json::to_vec(verdict) else {
            return;
        };
        match replace_file(&self.path_of(print_id), &bytes) {
            Ok(()) => eprintln!(
                "printobserver: print {print_id}: first-layer alignment {}",
                said(verdict)
            ),
            Err(error) => eprintln!(
                "printobserver: print {print_id}: first-layer alignment {} could not be recorded: {error}",
                said(verdict)
            ),
        }
    }

    /// The correction in force for one print: the one its last pass took, or,
    /// for a pass an earlier release recorded with no reference, its
    /// correction held against the calibration's own landmarks where it
    /// placed them. Nothing unless the print's alignment passed.
    #[must_use]
    pub fn correction_in_force(&self, print_id: PrintId) -> Option<ViewCorrection> {
        let calibration = self.calibration.as_deref()?;
        let PrintAlignment::Passed { correction_px, .. } = self.print_alignment(print_id) else {
            return None;
        };
        Some(self.view_state(print_id).correction.unwrap_or_else(|| {
            ViewCorrection::translation(
                correction_px,
                calibration
                    .landmarks
                    .iter()
                    .map(|landmark| ReferenceLandmark {
                        name: landmark.name.clone(),
                        offset_px: [0.0, 0.0],
                        score: 1.0,
                    })
                    .collect(),
            )
        }))
    }

    /// Why the print's alignment was last left pending, when the frame judged
    /// last was inconclusive.
    #[must_use]
    pub fn inconclusive(&self, print_id: PrintId) -> Option<InconclusiveReason> {
        self.view_state(print_id).inconclusive
    }

    /// The image the print's last check was shown, and when its frame was
    /// captured; nothing before its first check.
    #[must_use]
    pub fn last_check_image(&self, print_id: PrintId) -> Option<LastCheckImage> {
        self.view_state(print_id).last_check_image
    }

    /// Keep the image one check of a print was shown, and answer the change
    /// notice owed for it: on the print's first check image, and whenever its
    /// kind or its fallback's code differs from the last one kept.
    #[must_use]
    pub fn note_check_image(
        &self,
        print_id: PrintId,
        shown: LastCheckImage,
        frame_sha256: &str,
    ) -> Option<EventBody> {
        let mut state = self.view_state(print_id);
        let code = |image: &LastCheckImage| image.fallback.as_ref().map(|fallback| fallback.code);
        let changed = state
            .last_check_image
            .as_ref()
            .is_none_or(|last| last.kind != shown.kind || code(last) != code(&shown));
        let notice = changed.then(|| {
            body_of(&crate::judgements::CheckImageChangedPayload {
                kind: shown.kind,
                fallback: shown.fallback.clone(),
                frame_sha256: frame_sha256.to_owned(),
            })
        });
        state.last_check_image = Some(shown);
        self.keep_view_state(print_id, &state);
        notice.flatten()
    }

    /// Judge one frame of a print — its view always, and its first-layer
    /// alignment when `judging` asks and the alignment is due — keeping what
    /// the judgement decided and answering it with the records to append.
    ///
    /// The alignment is due on a frame of the configured camera once the
    /// plan has passed the print's first object layer: while it is pending,
    /// for [`AlignmentJudging::WhilePending`], and whatever it stands at for
    /// [`AlignmentJudging::Afresh`], a latched failure included. A frame whose
    /// view shows the camera moved again since the correction in force was
    /// taken sets the alignment back to pending, and is not judged for it: a
    /// later check's frame takes the fresh correction.
    ///
    /// Nothing is judged without a calibration, and a frame that is no JPEG is
    /// answered as such, with nothing recorded.
    #[must_use]
    pub fn judge(&self, print_id: PrintId, judging: &Judging<'_>) -> Judged {
        let Some(calibration) = self.calibration.as_deref() else {
            return Judged::unjudged(PrintAlignment::Unavailable);
        };
        let mut judged = Judged::unjudged(self.print_alignment(print_id));
        let mut state = self.view_state(print_id);
        let in_force = self.correction_in_force(print_id);
        let sha = judging.frame_sha256;
        let landmarks = match landmarks_in_frame(
            calibration,
            judging.frame_jpeg,
            judging.camera_snapshot_url,
        ) {
            Ok(landmarks) => landmarks,
            Err(error @ ComposeError::FrameMismatch { .. }) => {
                let reason = error.to_string();
                judged
                    .records
                    .extend(body_of(&crate::judgements::CameraViewJudgedPayload {
                        frame_sha256: sha.to_owned(),
                        outcome: ViewOutcome::FrameMismatch,
                        view_correction: in_force.clone(),
                        reason: Some(reason.clone()),
                    }));
                if Self::alignment_due(&judged.alignment, judging).is_some() {
                    let failure = failure_of(&error, calibration);
                    judged.records.extend(body_of(
                        &crate::judgements::FirstLayerAlignmentJudgedPayload {
                            verdict: crate::judgements::AlignmentJudgedVerdict::Failed,
                            offset_px: None,
                            correction_px: None,
                            match_strength: None,
                            reason: failure_sentence(&failure),
                            frame_sha256: sha.to_owned(),
                            view_correction: in_force.clone(),
                        },
                    ));
                    judged.alignment = PrintAlignment::Failed { reason: failure };
                    self.decide(print_id, &judged.alignment);
                    state.correction = None;
                    state.inconclusive = None;
                    self.keep_view_state(print_id, &state);
                }
                judged.correction = in_force.filter(|_| judged.alignment.passed());
                judged.mismatch = Some(reason);
                return judged;
            }
            Err(error) => {
                judged.unreadable = Some(error.to_string());
                return judged;
            }
        };
        // A plan laying no object leaves nothing a frame could be judged
        // against: the verdict file says so, and records nothing.
        if judging.alignment != AlignmentJudging::Never
            && judged.alignment == PrintAlignment::Pending
            && judging
                .plan
                .is_some_and(|plan| plan.index.first_object_layer().is_none())
        {
            judged.alignment = PrintAlignment::Unavailable;
            self.decide(print_id, &judged.alignment);
        }
        let due = Self::alignment_due(&judged.alignment, judging);
        let view = match (due, &judged.alignment) {
            (None, PrintAlignment::Passed { .. }) => {
                let view = judge_found(calibration, landmarks, in_force.as_ref());
                if view.outcome == ViewOutcome::ViewRefitRejected {
                    judged.alignment = PrintAlignment::Pending;
                    self.decide(print_id, &judged.alignment);
                    state.correction = None;
                    state.inconclusive = None;
                    self.keep_view_state(print_id, &state);
                    judged.rejected.clone_from(&view.reason);
                } else {
                    judged.correction.clone_from(&in_force);
                }
                judged
                    .records
                    .extend(view_record(sha, &view, in_force.clone()));
                view
            }
            (Some(first), _) => self.judge_alignment(
                print_id,
                calibration,
                landmarks,
                &first,
                judging,
                in_force,
                &mut state,
                &mut judged,
            ),
            (None, _) => {
                let view = judge_found(calibration, landmarks, None);
                judged.records.extend(view_record(sha, &view, None));
                view
            }
        };
        judged.inconclusive = state.inconclusive;
        judged.view = Some((view.outcome, view.reason));
        judged
    }

    /// Judge the first layer on a frame it is due on, and the frame's view
    /// against what that leaves in force.
    #[expect(
        clippy::too_many_arguments,
        reason = "the one judgement's inputs and the two records it updates"
    )]
    fn judge_alignment(
        &self,
        print_id: PrintId,
        calibration: &CameraCalibration,
        landmarks: Vec<printobserver_panels::FoundLandmark>,
        first: &DueAlignment,
        judging: &Judging<'_>,
        in_force: Option<ViewCorrection>,
        state: &mut CameraViewState,
        judged: &mut Judged,
    ) -> ViewJudgement {
        let sha = judging.frame_sha256;
        let uncorrected = judge_found(calibration, landmarks.clone(), None);
        let judgement = judge_first_layer(&FirstLayerInputs {
            alignment: AlignmentInputs {
                frame_jpeg: judging.frame_jpeg,
                camera_snapshot_url: judging.camera_snapshot_url,
                first_layer: &first.first_layer,
                print_z_mm: first.print_z_mm,
                calibration,
            },
            nozzle_xy_mm: first.nozzle_xy_mm,
        });
        let Ok(judgement) = judgement else {
            // The frame was decoded and held to the calibration above, so no
            // refusal is reached here; one that is leaves the alignment as it
            // stood, judged again on the next check's frame.
            judged.records.extend(view_record(sha, &uncorrected, None));
            return uncorrected;
        };
        let found = judgement.found;
        let (verdict, correction_px, reason) = match judgement.verdict {
            FirstLayerVerdict::Passed => (
                crate::judgements::AlignmentJudgedVerdict::Passed,
                found.map(|found| found.shift_px),
                "clearly_found".to_owned(),
            ),
            FirstLayerVerdict::Inconclusive(why) => (
                crate::judgements::AlignmentJudgedVerdict::Inconclusive,
                None,
                why.as_str().to_owned(),
            ),
        };
        judged.records.extend(body_of(
            &crate::judgements::FirstLayerAlignmentJudgedPayload {
                verdict,
                offset_px: found.map(|found| found.offset_px),
                correction_px,
                match_strength: found.map(|found| found.match_strength),
                reason,
                frame_sha256: sha.to_owned(),
                view_correction: in_force,
            },
        ));
        match (judgement.verdict, found) {
            (FirstLayerVerdict::Passed, Some(found)) => {
                let correction =
                    ViewCorrection::translation(found.shift_px, uncorrected.agreeing());
                judged.alignment = PrintAlignment::Passed {
                    offset_px: found.offset_px,
                    correction_px: found.shift_px,
                };
                self.decide(print_id, &judged.alignment);
                state.correction = Some(correction.clone());
                state.inconclusive = None;
                self.keep_view_state(print_id, state);
                if uncorrected.moved(calibration) {
                    judged
                        .records
                        .extend(body_of(&crate::judgements::CameraViewRefitPayload {
                            frame_sha256: sha.to_owned(),
                            view_correction: correction.clone(),
                        }));
                }
                let view = judge_found(calibration, landmarks, Some(&correction));
                judged
                    .records
                    .extend(view_record(sha, &view, Some(correction.clone())));
                judged.correction = Some(correction);
                view
            }
            (FirstLayerVerdict::Inconclusive(why), _) => {
                // Written even when it was pending already: a reader of the
                // verdict file learns a frame was judged.
                judged.alignment = PrintAlignment::Pending;
                self.decide(print_id, &judged.alignment);
                state.correction = None;
                state.inconclusive = Some(why);
                self.keep_view_state(print_id, state);
                judged.records.extend(view_record(sha, &uncorrected, None));
                uncorrected
            }
            (FirstLayerVerdict::Passed, None) => {
                judged.records.extend(view_record(sha, &uncorrected, None));
                uncorrected
            }
        }
    }

    /// The first layer a frame's judgement scores, when the alignment is due
    /// on it; nothing otherwise.
    fn alignment_due(alignment: &PrintAlignment, judging: &Judging<'_>) -> Option<DueAlignment> {
        let asked = match judging.alignment {
            AlignmentJudging::Never => false,
            AlignmentJudging::WhilePending => *alignment == PrintAlignment::Pending,
            AlignmentJudging::Afresh => true,
        };
        if !asked {
            return None;
        }
        let plan = judging.plan?;
        let index = &plan.index;
        let position = plan.file_position_bytes;
        let (first, ends) = index.first_object_layer()?;
        if position <= ends {
            return None;
        }
        Some(DueAlignment {
            first_layer: index.layer_toolpath(first).ok()?,
            print_z_mm: index
                .layer_at(position)
                .and_then(|layer| index.layer_z_mm(layer))?,
            nozzle_xy_mm: index.planned_head_at(position).ok().flatten(),
        })
    }

    /// Delete the verdict, the last check's record, the checked frames and the
    /// camera view of every print that is not open.
    ///
    /// An open print's record still being written — the `.json.part` beside it
    /// that [`replace_file`] renames over it — is its own, and kept: the watch
    /// sweeps after every read, so a sweep removing it between the write and the
    /// rename lost that record, and the print's next check showed no previous
    /// frame.
    ///
    /// A store that cannot list the open prints, a directory that cannot be
    /// listed and a file that cannot be removed each leave a verdict behind,
    /// which the next read's sweep tries again. No test makes either refuse,
    /// since that needs a store or filesystem made to fail beneath a running
    /// server; the `aligning` journeys prove the sweep removes a closed
    /// print's verdict and keeps an open one's, after a close and at start.
    pub async fn sweep(&self) {
        let Ok(open) = self.prints.open_prints().await else {
            return;
        };
        let open: Vec<String> = open
            .iter()
            .flat_map(|print| {
                [
                    format!("{}.json", print.id),
                    format!("{}.json.part", print.id),
                ]
            })
            .collect();
        for directory in [
            &self.directory,
            &self.previous_directory,
            &self.checked_directory,
            &self.view_directory,
        ] {
            let Ok(entries) = std::fs::read_dir(directory) else {
                continue;
            };
            for entry in entries.flatten() {
                let name = entry.file_name();
                if !open.iter().any(|kept| name.to_str() == Some(kept.as_str())) {
                    let _ = std::fs::remove_file(entry.path());
                }
            }
        }
    }
}

/// The newest of a print's checked frames, oldest first, at least `rise_mm`
/// below `print_z_mm` in the plan.
fn reference_in(kept: &[CheckedFrame], print_z_mm: f64, rise_mm: f64) -> Option<&CheckedFrame> {
    kept.iter()
        .rev()
        .find(|checked| checked.print_z_mm <= print_z_mm - rise_mm)
}

/// A print's checked frames once `checked` is kept among them: the newest
/// earlier frame at least `rise_mm` below it and every frame after that, then
/// it, at most [`MAX_CHECKED_FRAMES`] of them, the oldest dropped first; or it
/// alone when it is below the last kept, a plan gone back down.
fn kept_with(
    mut kept: Vec<CheckedFrame>,
    checked: CheckedFrame,
    rise_mm: f64,
) -> Vec<CheckedFrame> {
    if kept
        .last()
        .is_some_and(|last| checked.print_z_mm < last.print_z_mm)
    {
        kept.clear();
    }
    if let Some(reference) = kept
        .iter()
        .rposition(|earlier| earlier.print_z_mm <= checked.print_z_mm - rise_mm)
    {
        kept.drain(..reference);
    }
    kept.push(checked);
    if kept.len() > MAX_CHECKED_FRAMES {
        kept.drain(..kept.len() - MAX_CHECKED_FRAMES);
    }
    kept
}

/// How many times one file's replacement is attempted while the filesystem
/// refuses it for a moment, and the pause between two attempts: half a second
/// in all, on a check that is otherwise waiting on no one.
const REPLACE_ATTEMPTS: u32 = 20;
const REPLACE_PAUSE: Duration = Duration::from_millis(25);

/// Put `bytes` at `path` whole: written beside it, then renamed over it, so a
/// reader finds the old file or the new one and never part of either.
///
/// Windows refuses to replace a file another process holds open without
/// letting it be deleted — a scanner reading what was just written does — and
/// that refusal ends when the other process closes it. So a refusal of that
/// kind is attempted again, up to [`REPLACE_ATTEMPTS`] times; any other, or
/// one that outlasts them, is answered, with the file beside it removed.
///
/// # Errors
///
/// The filesystem's own error for the last attempt.
fn replace_file(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    let held = path.with_extension("json.part");
    attempt_while_refused(
        || {
            std::fs::write(&held, bytes)?;
            std::fs::rename(&held, path)
        },
        refused_for_a_moment,
        REPLACE_ATTEMPTS,
        REPLACE_PAUSE,
    )
    .inspect_err(|_| {
        let _ = std::fs::remove_file(&held);
    })
}

/// Run `attempt` until it succeeds, fails with an error `refused` does not
/// call momentary, or has been run `attempts` times, pausing between two.
fn attempt_while_refused(
    mut attempt: impl FnMut() -> std::io::Result<()>,
    refused: impl Fn(&std::io::Error) -> bool,
    attempts: u32,
    pause: Duration,
) -> std::io::Result<()> {
    let mut made = 1;
    loop {
        match attempt() {
            Err(error) if made < attempts && refused(&error) => {
                std::thread::sleep(pause);
                made += 1;
            }
            answered => return answered,
        }
    }
}

/// Whether Windows refused a replacement only because another handle has the
/// file open: `ERROR_ACCESS_DENIED` (5) or `ERROR_SHARING_VIOLATION` (32).
#[cfg(windows)]
fn refused_for_a_moment(error: &std::io::Error) -> bool {
    matches!(error.raw_os_error(), Some(5 | 32))
}

/// Elsewhere a rename replaces a file whoever has it open, so no refusal is
/// momentary.
#[cfg(not(windows))]
fn refused_for_a_moment(_error: &std::io::Error) -> bool {
    false
}

/// A frame size as the refusal spells it, `<w>x<h>`.
fn size_of(text: &str) -> [u32; 2] {
    let mut parts = text.split('x').map(|part| part.parse().unwrap_or(0));
    [parts.next().unwrap_or(0), parts.next().unwrap_or(0)]
}

/// A verdict as the log says it.
fn said(verdict: &PrintAlignment) -> String {
    match verdict {
        PrintAlignment::Unavailable => "unavailable: the G-code lays no object".to_owned(),
        PrintAlignment::Pending => "pending".to_owned(),
        PrintAlignment::Passed {
            offset_px,
            correction_px,
        } => format!(
            "passed, offset {offset_px:.1} px, corrected by [{:.0}, {:.0}] px",
            correction_px[0], correction_px[1]
        ),
        PrintAlignment::Failed {
            reason: AlignmentFailure::Offset { offset_px },
        } => format!("failed, offset {offset_px:.1} px"),
        PrintAlignment::Failed {
            reason: AlignmentFailure::Resolution { expected, got },
        } => format!(
            "failed: the camera's frames are {}x{}, the calibration's {}x{}",
            got[0], got[1], expected[0], expected[1]
        ),
        PrintAlignment::Failed {
            reason: AlignmentFailure::Stream,
        } => "failed: the camera is not the stream the calibration was fitted on".to_owned(),
    }
}

/// One print's camera view as it is kept.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(crate = "printobserver_types::serde")]
struct CameraViewState {
    /// The correction the print's last pass took, with its reference.
    #[serde(default)]
    correction: Option<ViewCorrection>,
    /// Why the frame judged last was inconclusive, while the alignment is
    /// pending.
    #[serde(default)]
    inconclusive: Option<InconclusiveReason>,
    /// The image the print's last check was shown.
    #[serde(default)]
    last_check_image: Option<LastCheckImage>,
}

/// The image a print's last check was shown, and when its frame was captured.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(crate = "printobserver_types::serde", deny_unknown_fields)]
#[schemars(crate = "printobserver_types::schemars")]
pub struct LastCheckImage {
    /// When the check's frame was captured.
    pub at: Timestamp,
    /// Which image the check was shown.
    pub kind: ImageKind,
    /// Why it was the whole frame; null for the three-panel image.
    pub fallback: Option<ImageFallback>,
}

impl Sample for LastCheckImage {
    fn sample_full() -> Self {
        Self {
            at: Timestamp::sample_full(),
            kind: ImageKind::WholeFrame,
            fallback: Some(ImageFallback::sample_full()),
        }
    }

    fn sample_minimal() -> Self {
        Self {
            at: Timestamp::sample_full(),
            kind: ImageKind::ThreePanel,
            fallback: None,
        }
    }
}

/// Whether a judgement of a frame judges the print's first-layer alignment.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AlignmentJudging {
    /// Never: a look, or a frame not the configured camera's own.
    Never,
    /// While the alignment is pending: every scheduled check's frame.
    WhilePending,
    /// Whatever it stands at, a latched failure included: the operator's
    /// recheck of the camera.
    Afresh,
}

/// One frame to judge.
#[derive(Debug, Clone, Copy)]
pub struct Judging<'a> {
    /// The frame's bytes.
    pub frame_jpeg: &'a [u8],
    /// The stored frame's SHA-256, which every record names.
    pub frame_sha256: &'a str,
    /// The stream it is of.
    pub camera_snapshot_url: &'a str,
    /// Where the print stood in its plan as it was taken.
    pub plan: Option<&'a PlanPosition>,
    /// Whether the first-layer alignment is judged.
    pub alignment: AlignmentJudging,
}

/// What one judgement of a frame decided, and the records it owes.
#[derive(Debug, Clone, PartialEq)]
pub struct Judged {
    /// The records to append, in order, each naming the frame.
    pub records: Vec<EventBody>,
    /// The print's alignment once judged.
    pub alignment: PrintAlignment,
    /// The correction the frame may be drawn through, when the alignment
    /// passed and the view allows it.
    pub correction: Option<ViewCorrection>,
    /// The view's outcome and its refusal's reason; nothing when the frame was
    /// not judged for its view.
    pub view: Option<(ViewOutcome, Option<String>)>,
    /// Why the correction in force was given up on this frame: the camera
    /// moved again.
    pub rejected: Option<String>,
    /// Why the frame is not the calibration's size or stream.
    pub mismatch: Option<String>,
    /// Why the frame could not be read at all.
    pub unreadable: Option<String>,
    /// Why the alignment was left pending, when the frame judged last was
    /// inconclusive.
    pub inconclusive: Option<InconclusiveReason>,
}

impl Judged {
    /// Nothing that could be judged of a frame — no calibration, or no stream
    /// it was fitted on — the alignment standing as it is, and the one record
    /// the frame is kept with.
    #[must_use]
    pub fn nothing_to_judge(alignment: PrintAlignment, kept_with: Option<EventBody>) -> Self {
        Self {
            records: kept_with.into_iter().collect(),
            ..Self::unjudged(alignment)
        }
    }

    /// Nothing judged, the alignment standing as it is.
    const fn unjudged(alignment: PrintAlignment) -> Self {
        Self {
            records: Vec::new(),
            alignment,
            correction: None,
            view: None,
            rejected: None,
            mismatch: None,
            unreadable: None,
            inconclusive: None,
        }
    }
}

impl PrintAlignment {
    /// Whether it passed.
    #[must_use]
    pub const fn passed(&self) -> bool {
        matches!(self, Self::Passed { .. })
    }
}

/// What a first-layer judgement due on a frame scores.
struct DueAlignment {
    /// The first object layer's toolpath.
    first_layer: printobserver_core::LayerToolpath,
    /// The print's height in its plan as the frame was taken.
    print_z_mm: f64,
    /// Where the plan has the nozzle then.
    nozzle_xy_mm: Option<[f64; 2]>,
}

/// One payload as the body appended for it; nothing for one that will not
/// render, which no declared payload is.
fn body_of<P: EventPayload>(payload: &P) -> Option<EventBody> {
    EventBody::of(payload).ok()
}

/// The record of one view judgement.
fn view_record(
    frame_sha256: &str,
    view: &ViewJudgement,
    view_correction: Option<ViewCorrection>,
) -> Option<EventBody> {
    body_of(&crate::judgements::CameraViewJudgedPayload {
        frame_sha256: frame_sha256.to_owned(),
        outcome: view.outcome,
        view_correction,
        reason: view.reason.clone(),
    })
}

/// The failure a frame refused for its size or stream latches.
fn failure_of(error: &ComposeError, calibration: &CameraCalibration) -> AlignmentFailure {
    match error {
        ComposeError::FrameMismatch {
            field: FrameField::Resolution,
            got,
            ..
        } => AlignmentFailure::Resolution {
            expected: calibration.image_size_px,
            got: size_of(got),
        },
        _ => AlignmentFailure::Stream,
    }
}

/// A failure in the sentence its record and its fallback give it in.
#[must_use]
pub fn failure_sentence(failure: &AlignmentFailure) -> String {
    match failure {
        AlignmentFailure::Offset { offset_px } => format!(
            "the printed first layer lies {offset_px:.1} px from where the calibration projects it"
        ),
        AlignmentFailure::Resolution { expected, got } => format!(
            "the camera's frames are {}x{} and the calibration's {}x{}",
            got[0], got[1], expected[0], expected[1]
        ),
        AlignmentFailure::Stream => {
            "the camera is not the stream the calibration was fitted on".to_owned()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{CheckedFrame, MAX_CHECKED_FRAMES, attempt_while_refused, kept_with, reference_in};
    use std::cell::Cell;
    use std::io::{Error, ErrorKind};
    use std::time::Duration;

    /// An attempt that is refused `refusals` times and then succeeds, counting
    /// how often it was made.
    fn refused_then_done(refusals: u32, made: &Cell<u32>) -> impl FnMut() -> std::io::Result<()> {
        move || {
            made.set(made.get() + 1);
            if made.get() <= refusals {
                Err(Error::from(ErrorKind::PermissionDenied))
            } else {
                Ok(())
            }
        }
    }

    #[test]
    fn a_momentary_refusal_is_attempted_again_until_it_passes() {
        let made = Cell::new(0);
        let answered =
            attempt_while_refused(refused_then_done(2, &made), |_| true, 5, Duration::ZERO);
        assert!(answered.is_ok(), "{answered:?}");
        assert_eq!(made.get(), 3);
    }

    #[test]
    fn a_refusal_that_outlasts_every_attempt_is_answered() {
        let made = Cell::new(0);
        let answered =
            attempt_while_refused(refused_then_done(9, &made), |_| true, 4, Duration::ZERO);
        assert_eq!(
            answered.map_err(|error| error.kind()),
            Err(ErrorKind::PermissionDenied)
        );
        assert_eq!(made.get(), 4);
    }

    #[test]
    fn a_refusal_that_is_not_momentary_is_answered_at_once() {
        let made = Cell::new(0);
        let answered =
            attempt_while_refused(refused_then_done(9, &made), |_| false, 4, Duration::ZERO);
        assert!(answered.is_err());
        assert_eq!(made.get(), 1);
    }

    /// The refusal the retry answers, made the way Windows makes it: the
    /// record held open by another handle that does not allow deleting it,
    /// and let go a moment later.
    #[cfg(windows)]
    #[test]
    fn a_record_held_open_by_another_handle_is_replaced_once_it_is_let_go() {
        use std::os::windows::fs::OpenOptionsExt;
        let directory = tempfile::TempDir::new().expect("a directory");
        let path = directory.path().join("record.json");
        std::fs::write(&path, b"before").expect("a record");
        let held = std::fs::OpenOptions::new()
            .read(true)
            .share_mode(0)
            .open(&path)
            .expect("the record held");
        let release = std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(150));
            drop(held);
        });
        super::replace_file(&path, b"after").expect("the record replaced");
        release.join().expect("the handle let go");
        assert_eq!(std::fs::read(&path).expect("the record"), b"after");
        assert!(!path.with_extension("json.part").exists());
    }

    #[cfg(windows)]
    #[test]
    fn only_a_held_file_is_a_momentary_refusal_on_windows() {
        assert!(super::refused_for_a_moment(&Error::from_raw_os_error(5)));
        assert!(super::refused_for_a_moment(&Error::from_raw_os_error(32)));
        assert!(!super::refused_for_a_moment(&Error::from_raw_os_error(2)));
    }

    /// A checked frame at `z` mm, captured `second` seconds into a print.
    fn at(second: i64, z: f64) -> CheckedFrame {
        CheckedFrame {
            frame: format!("frame-{second}.jpg").into(),
            captured_at: printobserver_types::Timestamp::from_unix_seconds(1_791_489_600 + second)
                .expect("a time"),
            print_z_mm: z,
        }
    }

    /// The heights of checked frames, in their order.
    fn heights(kept: &[CheckedFrame]) -> Vec<f64> {
        kept.iter().map(|checked| checked.print_z_mm).collect()
    }

    /// Once a print carries on from a planned change, its growth is measured
    /// only from frames checked after: one from before the wait, however far
    /// below, is no longer a reference, and the first checked after it is.
    #[test]
    fn after_a_planned_change_growth_is_measured_from_frames_after_it() {
        let state = tempfile::TempDir::new().expect("a state directory");
        let store =
            printobserver_store_sqlite::MemoryStore::new(state.path()).expect("an in-memory store");
        let alignments = super::Alignments::new(state.path(), None, std::sync::Arc::new(store))
            .expect("the verdicts' directories");
        let print_id = printobserver_types::PrintId::new();
        for (second, z) in [(0, 0.2), (60, 0.6)] {
            alignments.record_checked_frame(print_id, at(second, z), 0.4);
        }
        let before = alignments.growth_reference(print_id, 1.0, 0.4);
        assert_eq!(before.map(|checked| checked.print_z_mm), Some(0.6));

        alignments.restart_growth(print_id);
        assert_eq!(alignments.growth_reference(print_id, 1.0, 0.4), None);
        alignments.record_checked_frame(print_id, at(3_000, 0.8), 0.4);
        alignments.record_checked_frame(print_id, at(3_120, 1.4), 0.4);
        let after = alignments.growth_reference(print_id, 1.4, 0.4);
        assert_eq!(
            after.map(|checked| checked.captured_at),
            Some(at(3_000, 0.8).captured_at)
        );
        // A second change with nothing checked since forgets nothing more.
        alignments.restart_growth(print_id);
        alignments.restart_growth(print_id);
        assert!(alignments.checked_frames(print_id).is_empty());
    }

    #[test]
    fn a_check_measures_from_the_newest_frame_far_enough_below_it() {
        let kept = [at(0, 0.2), at(60, 0.8), at(120, 2.0), at(180, 2.6)];
        assert_eq!(
            reference_in(&kept, 5.8, 3.0).map(|checked| checked.print_z_mm),
            Some(2.6)
        );
        assert_eq!(
            reference_in(&kept, 3.5, 3.0).map(|checked| checked.print_z_mm),
            Some(0.2)
        );
        assert_eq!(reference_in(&kept, 3.1, 3.0), None, "none is 3 mm below");
    }

    #[test]
    fn only_what_a_later_check_can_measure_from_is_kept() {
        let mut kept = Vec::new();
        for (second, z) in [(0, 0.2), (60, 0.8), (120, 2.0), (180, 3.4), (240, 4.0)] {
            kept = kept_with(kept, at(second, z), 3.0);
        }
        assert_eq!(
            heights(&kept),
            [0.8, 2.0, 3.4, 4.0],
            "0.2 is below 0.8, which is itself 3 mm below 4.0, so no later check needs it"
        );
        assert_eq!(
            heights(&kept_with(kept, at(300, 1.0), 3.0)),
            [1.0],
            "a plan gone back down starts the record again"
        );
    }

    #[test]
    fn the_oldest_checked_frames_go_past_the_most_kept() {
        let mut kept = Vec::new();
        for second in 0..=i64::try_from(MAX_CHECKED_FRAMES).expect("a count") {
            kept = kept_with(kept, at(second, 5.0), 3.0);
        }
        assert_eq!(kept.len(), MAX_CHECKED_FRAMES);
        assert_eq!(kept[0].captured_at, at(1, 5.0).captured_at);
    }
}
