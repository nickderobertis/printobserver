//! A fresh look at a print: what arrived, what the camera shows, and what the
//! printer says, now.
//!
//! The context a turn reads is the one collected when its event arrived; a
//! look is how the agent sees the print as it is *now*, as often as it wants,
//! and how it watches for a while before deciding. A look waits up to
//! [`MAX_LOOK_WAIT_S`](crate::MAX_LOOK_WAIT_S) for something to arrive for the
//! print and returns the moment something does — that is how an event that
//! arrives during a turn reaches the agent already running rather than a
//! second one.
//!
//! # Two halves, because one of them blocks
//!
//! [`Supervisor::await_arrivals`] waits on a thread of its own and takes what
//! arrived; [`Supervisor::take_look`] writes the look down, takes the frame and
//! reads the printer. The server calls the first on a blocking thread, exactly
//! as it drives a turn, so that a minute and a half of waiting holds no
//! asynchronous worker.
//!
//! # Every frame is kept
//!
//! A look is written down as a [`CameraLookPayload`] event, and the frame is
//! that event's image, so a print's whole record of what it looked like is in
//! its history rather than in whatever scratch directory asked.
//!
//! # A frame that shows nothing is not handed over as one
//!
//! Each frame is judged before it is kept
//! ([`VisionPort::judge_frame`](printobserver_vision_api::VisionPort::judge_frame)):
//! one that does not decode whole, or decodes near-uniform grey, is fetched
//! again, up to [`FRAME_ATTEMPTS`] in all. The first good one is the look's
//! frame. When every one is bad the look is written down as a
//! [`CameraDegradedPayload`] in its place, carrying no frame, so nothing the
//! camera could not show is handed to the agent as a picture of the print. A
//! camera that will not answer at all is a port failure recorded against the
//! look, as it always was.

use std::path::PathBuf;
use std::time::Duration;

use printobserver_printer_api::{JobSnapshot, PrinterSnapshot, PrinterState};
use printobserver_types::contract::Sample;
use printobserver_types::schemars::JsonSchema;
use printobserver_types::serde::{Deserialize, Serialize};
use printobserver_types::{EventBody, EventRecord, ImageRef, PrintId};

use crate::error::CoreError;
use crate::inbox::Arrival;
use crate::kinds::{CameraDegradedPayload, CameraLookPayload, FrameCause, PortFailureSite};
use crate::supervisor::Supervisor;
use printobserver_vision_api::{FetchedImage, FrameFault};

/// One fresh look at a print.
///
/// An answer rather than a record, so it admits fields beside its own the way
/// every answer of the server does: a client that could not open the frame's
/// path says so in a field of its own beside the rest of the answer.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(crate = "printobserver_types::serde")]
#[schemars(crate = "printobserver_types::schemars")]
pub struct Look {
    /// The look itself, as it was written into the print's history, carrying
    /// the frame as its image when there is one.
    pub event: EventRecord,
    /// The events for this print that arrived while the look waited, oldest
    /// first; the look returned early when there were any.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub arrived: Vec<EventRecord>,
    /// The frame the camera gave, absent when no camera is configured or it
    /// gave none — which the history records as a port failure.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub frame: Option<ImageRef>,
    /// The absolute path the frame's bytes are at, on the supervisor's host.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub image_path: Option<PathBuf>,
    /// The printer's state now, absent when it could not be read.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub printer: Option<PrinterSnapshot>,
    /// The job the printer reports now, absent when it could not be read.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub job: Option<JobSnapshot>,
    /// Whether the print is paused and the pause is the detector's, which is
    /// what an adjustment asked for now would be applied under.
    pub detector_paused: bool,
}

impl Sample for Look {
    fn sample_full() -> Self {
        Self {
            event: EventRecord::sample_full(),
            arrived: vec![EventRecord::sample_full()],
            frame: Some(ImageRef::sample_full()),
            image_path: Some(PathBuf::from("/var/lib/printobserver/images/look.jpg")),
            printer: Some(PrinterSnapshot::sample_full()),
            job: Some(JobSnapshot::sample_full()),
            detector_paused: true,
        }
    }

    fn sample_minimal() -> Self {
        Self {
            event: EventRecord::sample_minimal(),
            arrived: Vec::new(),
            frame: None,
            image_path: None,
            printer: None,
            job: None,
            detector_paused: false,
        }
    }
}

impl Supervisor {
    /// Wait up to `wait` for anything to arrive for one print's running turn,
    /// and take it.
    ///
    /// **This blocks the calling thread** for as long as it waits; call it
    /// where a blocking wait is expected.
    #[must_use]
    pub fn await_arrivals(&self, print_id: PrintId, wait: Duration) -> Vec<Arrival> {
        self.inboxes().wait_and_take(print_id, wait)
    }

    /// Write one look down, take a frame, and read the printer.
    ///
    /// # Errors
    ///
    /// Returns [`CoreError::NoSuchPrint`] when nothing is held under that
    /// identifier, and the store's own error when the look could not be
    /// written down. A camera or printer that will not answer is recorded and
    /// answered as an absence rather than as an error.
    ///
    /// What arrived is delivered by the look written down, so a look that
    /// could not be written down delivers nothing: what it was handed goes
    /// back to the print's running turn, ahead of anything that arrived since,
    /// for its next look or the turn after it.
    pub async fn take_look(
        &self,
        print_id: PrintId,
        waited: Duration,
        arrived: Vec<Arrival>,
    ) -> Result<Look, CoreError> {
        let url = self
            .config()
            .camera_snapshot_url
            .as_ref()
            .map(|url| url.as_str().to_owned());
        let fetched = match &url {
            Some(url) => Some(self.fetch_frame(url).await),
            None => None,
        };
        let written = match &fetched {
            Some(Frame::Degraded(degraded)) => self.write_degraded_down(print_id, degraded).await,
            _ => self.write_look_down(print_id, waited, &arrived).await,
        };
        let (print, mut event) = match written {
            Ok(written) => written,
            Err(error) => {
                self.inboxes().put_back(print_id, arrived);
                return Err(error);
            }
        };
        let arrived: Vec<EventRecord> = arrived.into_iter().map(|each| each.event).collect();
        let frame = match (fetched, url) {
            (Some(fetched), Some(url)) => self.keep_frame(print.id, &event, url, fetched).await,
            _ => None,
        };
        let image_path = match &frame {
            Some(reference) => self.image_path(reference).await,
            None => None,
        };
        event.image.clone_from(&frame);
        let printer = self.read_snapshot().await.ok();
        let job = self.read_job().await.ok();
        // `OctoPrint` clears a job's position when it is cancelled or fails,
        // so every read that finds the print running is where it is next known
        // to have stopped. The look is already written down and answered
        // either way: a position the store would not keep costs this look
        // nothing, and the next read records one.
        if let Some(job) = &job
            && print.ended_at.is_none()
        {
            let _ = self
                .record_position_from(&print, job, self.clock().now())
                .await;
        }
        let detector_paused = self.detector_paused(print.id)
            && printer
                .as_ref()
                .is_some_and(|taken| taken.connection == PrinterState::Paused);
        Ok(Look {
            event,
            arrived,
            frame,
            image_path,
            printer,
            job,
            detector_paused,
        })
    }

    /// Write one degraded look down in its print's history, in place of the
    /// look.
    async fn write_degraded_down(
        &self,
        print_id: PrintId,
        degraded: &CameraDegradedPayload,
    ) -> Result<(crate::records::PrintRecord, EventRecord), CoreError> {
        let print = self
            .stores()
            .prints
            .print(print_id)
            .await?
            .ok_or(CoreError::NoSuchPrint { print_id })?;
        let event = self
            .append_system_event(print.id, EventBody::of(degraded)?)
            .await?;
        Ok((print, event))
    }

    /// Write one look down in its print's history, naming what it delivered.
    async fn write_look_down(
        &self,
        print_id: PrintId,
        waited: Duration,
        arrived: &[Arrival],
    ) -> Result<(crate::records::PrintRecord, EventRecord), CoreError> {
        let print = self
            .stores()
            .prints
            .print(print_id)
            .await?
            .ok_or(CoreError::NoSuchPrint { print_id })?;
        let payload = CameraLookPayload {
            waited_s: u32::try_from(waited.as_secs()).unwrap_or(u32::MAX),
            delivered: arrived.iter().map(|each| each.event.id).collect(),
        };
        let event = self
            .append_system_event(print.id, EventBody::of(&payload)?)
            .await?;
        Ok((print, event))
    }

    /// Fetch frames from the camera until one is good, up to
    /// [`FRAME_ATTEMPTS`] of them.
    pub(crate) async fn fetch_frame(&self, url: &str) -> Frame {
        let mut faults: Vec<FrameFault> = Vec::new();
        while faults.len() < FRAME_ATTEMPTS as usize {
            let fetched = match self.vision().fetch_image(url.to_owned()).await {
                Ok(fetched) => fetched,
                Err(error) => return Frame::Failed(error.to_string()),
            };
            match self.vision().judge_frame(fetched).await {
                Ok(good) => return Frame::Good(good),
                Err(fault) => faults.push(fault),
            }
        }
        let cause = match faults.last() {
            Some(FrameFault::Undecodable { .. }) => FrameCause::Undecodable,
            _ => FrameCause::Blank,
        };
        let detail = faults
            .iter()
            .enumerate()
            .map(|(index, fault)| format!("frame {}: {fault}", index + 1))
            .collect::<Vec<_>>()
            .join("; ");
        Frame::Degraded(CameraDegradedPayload {
            attempts: FRAME_ATTEMPTS,
            cause,
            detail,
        })
    }

    /// Keep what one fetch gave as an event's image: a good frame stored,
    /// a camera that would not answer recorded against the event, and a
    /// degraded one kept as nothing.
    pub(crate) async fn keep_frame(
        &self,
        print_id: PrintId,
        event: &EventRecord,
        url: String,
        fetched: Frame,
    ) -> Option<ImageRef> {
        let image = match fetched {
            Frame::Good(image) => image,
            Frame::Failed(detail) => {
                self.record_port_failure(print_id, event.id, PortFailureSite::CameraLook, detail)
                    .await;
                return None;
            }
            Frame::Degraded(_) => return None,
        };
        match self
            .stores()
            .images
            .put_image(
                print_id,
                event.id,
                Some(url),
                image.content_type,
                image.bytes,
            )
            .await
        {
            Ok(record) => Some(ImageRef {
                id: record.id,
                sha256: record.sha256,
            }),
            Err(error) => {
                self.record_port_failure(
                    print_id,
                    event.id,
                    PortFailureSite::CameraLook,
                    error.to_string(),
                )
                .await;
                None
            }
        }
    }
}

/// How many frames one look or observation fetches before it records the
/// camera degraded: the first, and two more.
pub const FRAME_ATTEMPTS: u32 = 3;

/// What fetching a frame came to.
#[derive(Debug)]
pub(crate) enum Frame {
    /// A good frame.
    Good(FetchedImage),
    /// The camera would not answer, in its own words.
    Failed(String),
    /// Every frame fetched was bad.
    Degraded(CameraDegradedPayload),
}
