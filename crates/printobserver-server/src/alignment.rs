//! Each print's first-layer alignment verdict: whether the camera
//! calibration's projection lands on that print's printed first layer.
//!
//! A projection can be wrong with the camera unmoved, so each print verifies
//! it once. The first time the printer watch finds a print's file position
//! past the end of its first object layer, one frame is fetched from the
//! camera and [`verify_first_layer_alignment`] rules on it; a fetch that gives
//! no good frame is tried again at the next read. The verdict is written to
//! `<state_dir>/alignment/<print id>.json`, so it stands through a restart
//! while the print is open, and is deleted once the print closes. A print is
//! checked once, and a failed print stays failed.
//!
//! Which check draws the toolpath on its image reads the verdict through
//! [`Alignments::print_alignment`] and draws only for a print that `Passed`.
//!
//! Beside each verdict is kept the frame the print's last check was given,
//! which the next check's image shows as the previous one
//! ([`Alignments::previous_check`]): written under
//! `<state_dir>/previous-check/<print id>.json`, standing through a restart
//! while the print is open, and swept with the verdict once it closes.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use printobserver_core::store::PrintStore;
use printobserver_core::{ActiveJob, Supervisor};
use printobserver_panels::alignment::SEARCH_PX;
use printobserver_panels::{
    AlignmentInputs, CameraCalibration, ComposeError, FrameField, verify_first_layer_alignment,
};
use printobserver_types::PrintId;
use printobserver_types::contract::Sample;
use printobserver_types::schemars::JsonSchema;
use printobserver_types::serde::{Deserialize, Serialize};

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
    /// calibration allows.
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
    },
    /// It does not, or the frames cannot be judged against the calibration.
    Failed {
        /// Why.
        reason: AlignmentFailure,
    },
}

impl Sample for PrintAlignment {
    fn sample_full() -> Self {
        Self::Passed { offset_px: 14.5 }
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

/// The alignment verdicts of every print, what decides them, and the frame
/// each print's last check was given.
pub struct Alignments {
    /// Where each print's verdict is kept.
    directory: PathBuf,
    /// Where each print's last check's frame is named.
    previous_directory: PathBuf,
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
        Ok(Self {
            directory,
            previous_directory,
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
    /// as the previous. A record the filesystem refuses leaves the one before
    /// it standing, so the next check shows an older frame as the previous
    /// rather than none. No test makes this write fail: that needs the state
    /// directory made to refuse writes beneath a running server, which refuses
    /// the store's own writes first. What a check makes of a record it cannot
    /// use — an empty panel 3 — is proved by
    /// `a_lost_previous_frame_empties_panel_3_and_the_record_goes_with_the_print`.
    pub fn record_check(&self, print_id: PrintId, check: &PreviousCheck) {
        let Ok(bytes) = printobserver_types::serde_json::to_vec(check) else {
            return;
        };
        let path = self.previous_path_of(print_id);
        let held = path.with_extension("json.part");
        if std::fs::write(&held, bytes).is_ok() {
            let _ = std::fs::rename(&held, &path);
        }
    }

    /// One print's verdict as it stands.
    #[must_use]
    pub fn print_alignment(&self, print_id: PrintId) -> PrintAlignment {
        if self.calibration.is_none() {
            return PrintAlignment::Unavailable;
        }
        std::fs::read(self.path_of(print_id))
            .ok()
            .and_then(|bytes| printobserver_types::serde_json::from_slice(&bytes).ok())
            .unwrap_or(PrintAlignment::Pending)
    }

    /// Rule on the running print's alignment once its first object layer is
    /// down, when it has no verdict yet. Answers the verdict it wrote, and
    /// nothing when it wrote none — not yet due, or no good frame came.
    pub async fn check(
        &self,
        supervisor: &Supervisor,
        active: ActiveJob,
    ) -> Option<PrintAlignment> {
        let calibration = self.calibration.as_ref()?;
        // A verdict file that cannot be read back is no verdict, and is
        // decided again over it.
        if self.print_alignment(active.print_id) != PrintAlignment::Pending {
            return None;
        }
        let position = u64::try_from(active.file_position_bytes?).ok()?;
        let index = supervisor.print_gcode_index(active.print_id).await?;
        let Some((first, ends)) = index.first_object_layer() else {
            return self.decide(active.print_id, PrintAlignment::Unavailable);
        };
        if position <= ends {
            return None;
        }
        let first_layer = index.layer_toolpath(first).ok()?;
        let print_z_mm = index
            .layer_at(position)
            .and_then(|layer| index.layer_z_mm(layer))?;
        let (url, frame) = supervisor.fetch_camera_frame(active.print_id).await?;
        let verdict = match verify_first_layer_alignment(&AlignmentInputs {
            frame_jpeg: &frame,
            camera_snapshot_url: &url,
            first_layer: &first_layer,
            print_z_mm,
            calibration,
        }) {
            Ok(found) if found.passed => PrintAlignment::Passed {
                offset_px: found.offset_px,
            },
            Ok(found) => PrintAlignment::Failed {
                reason: AlignmentFailure::Offset {
                    offset_px: found.offset_px,
                },
            },
            Err(ComposeError::FrameMismatch {
                field: FrameField::Resolution,
                got,
                ..
            }) => PrintAlignment::Failed {
                reason: AlignmentFailure::Resolution {
                    expected: calibration.image_size_px,
                    got: size_of(&got),
                },
            },
            Err(ComposeError::FrameMismatch {
                field: FrameField::Stream,
                ..
            }) => PrintAlignment::Failed {
                reason: AlignmentFailure::Stream,
            },
            // A first layer projecting nowhere in the frame is as far from
            // it as the check looks.
            Err(ComposeError::OutlineOffFrame) => PrintAlignment::Failed {
                reason: AlignmentFailure::Offset {
                    offset_px: f64::from(SEARCH_PX),
                },
            },
            Err(_) => return None,
        };
        self.decide(active.print_id, verdict)
    }

    /// Keep a verdict, and say it. A verdict the filesystem refuses to keep is
    /// decided again at the next read rather than lost; no test makes the
    /// state directory refuse a write, which needs a filesystem made to fail
    /// beneath a running server, and the retry is the same path a fetch that
    /// gave no frame takes, which `aligning` proves.
    fn decide(&self, print_id: PrintId, verdict: PrintAlignment) -> Option<PrintAlignment> {
        let bytes = printobserver_types::serde_json::to_vec(&verdict).ok()?;
        let path = self.path_of(print_id);
        let held = path.with_extension("json.part");
        std::fs::write(&held, bytes).ok()?;
        std::fs::rename(&held, &path).ok()?;
        eprintln!(
            "printobserver: print {print_id}: first-layer alignment {}",
            said(&verdict)
        );
        Some(verdict)
    }

    /// Delete the verdict and the last check's record of every print that is
    /// not open.
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
            .map(|print| format!("{}.json", print.id))
            .collect();
        for directory in [&self.directory, &self.previous_directory] {
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
        PrintAlignment::Passed { offset_px } => format!("passed, offset {offset_px:.1} px"),
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
