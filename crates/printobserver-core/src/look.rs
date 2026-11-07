//! A fresh look at a print: what arrived, what the camera shows, and what the
//! printer says, now — or at the printer, with no print named.
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
//! # The frame is fetched first, and kept with its event in one write
//!
//! The camera is asked before anything is written, and the event is then
//! written together with its frame
//! ([`EventStore::append_event_with_image`](crate::store::EventStore::append_event_with_image)),
//! so the first read anybody following the log makes of a look or a scheduled
//! check already finds its image. A store that will not keep the frame leaves
//! the event written without it, with the store's refusal recorded against it.
//!
//! # A look at the printer
//!
//! [`Supervisor::take_printer_look`] is a look with no print named: before a
//! print, between prints, after one — the bed check before a start among
//! them. Its event and its frame belong to no print. It waits on nothing that
//! arrives for a print, so it can never take an event meant for a print's
//! agent, and it delivers nothing.
//!
//! # A camera answering a server error is asked again
//!
//! A fetch the camera answered `500` to `599` is made again after
//! [`CameraRetry::backoff`], up to [`CameraRetry::retries`] more times, each
//! refusal written down as a [`CameraRetryPayload`] as it happens. Only a
//! fetch whose attempts all ran out is the port failure it always was. Any
//! other refusal — a `404`, a timeout, a host refusing the connection — is one
//! a second attempt would not change, and fails the fetch at once.
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
use printobserver_types::{EventBody, EventRecord, ImageRef, PrintId, Timestamp};

use crate::error::CoreError;
use crate::inbox::Arrival;
use crate::kinds::{
    CameraDegradedPayload, CameraLookPayload, CameraRetryPayload, FrameCause, PortFailureSite,
};
use crate::store::{EventDraft, ImageDraft};
use crate::supervisor::Supervisor;
use printobserver_vision_api::{FetchedImage, FrameFault, VisionError};

/// One fresh look at a print.
///
/// An answer rather than a record, so it admits fields beside its own the way
/// every answer of the server does: a client that could not open the frame's
/// path says so in a field of its own beside the rest of the answer.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(crate = "printobserver_types::serde")]
#[schemars(crate = "printobserver_types::schemars")]
pub struct Look {
    /// The look itself, as it was written into the print's history — or the
    /// printer's, for a look at the printer — carrying the frame as its image
    /// when there is one.
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

    /// Take a frame, write one look down with it, and read the printer.
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
        let written = match self.stores().prints.print(print_id).await {
            Ok(Some(print)) => {
                let fetched = self.fetch_configured_frame(Some(print.id)).await;
                let payload = CameraLookPayload {
                    waited_s: u32::try_from(waited.as_secs()).unwrap_or(u32::MAX),
                    delivered: arrived.iter().map(|each| each.event.id).collect(),
                    frame_captured_at: self.frame_captured_at(fetched.as_ref()),
                };
                self.write_with_frame(Some(print.id), &payload, fetched)
                    .await
                    .map(|event| (print, event))
            }
            Ok(None) => Err(CoreError::NoSuchPrint { print_id }),
            Err(error) => Err(error.into()),
        };
        let (print, event) = match written {
            Ok(written) => written,
            Err(error) => {
                self.inboxes().put_back(print_id, arrived);
                return Err(error);
            }
        };
        let arrived: Vec<EventRecord> = arrived.into_iter().map(|each| each.event).collect();
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
        Ok(self
            .answered(event, arrived, printer, job, detector_paused)
            .await)
    }

    /// Take a frame of the printer with no print named, write the look down
    /// with it, and read the printer.
    ///
    /// The look and its frame belong to no print. Nothing is waited on here
    /// and nothing is delivered: a look at the printer takes no event any
    /// print's agent is owed.
    ///
    /// # Errors
    ///
    /// Returns the store's own error when the look could not be written down.
    /// A camera or printer that will not answer is recorded and answered as an
    /// absence rather than as an error.
    pub async fn take_printer_look(&self, waited: Duration) -> Result<Look, CoreError> {
        let fetched = self.fetch_configured_frame(None).await;
        let payload = CameraLookPayload {
            waited_s: u32::try_from(waited.as_secs()).unwrap_or(u32::MAX),
            delivered: Vec::new(),
            frame_captured_at: self.frame_captured_at(fetched.as_ref()),
        };
        let event = self.write_with_frame(None, &payload, fetched).await?;
        let printer = self.read_snapshot().await.ok();
        let job = self.read_job().await.ok();
        Ok(self.answered(event, Vec::new(), printer, job, false).await)
    }

    /// The answer to one look, its frame's path read off the store.
    async fn answered(
        &self,
        event: EventRecord,
        arrived: Vec<EventRecord>,
        printer: Option<PrinterSnapshot>,
        job: Option<JobSnapshot>,
        detector_paused: bool,
    ) -> Look {
        let frame = event.image.clone();
        let image_path = match &frame {
            Some(reference) => self.image_path(reference).await,
            None => None,
        };
        Look {
            event,
            arrived,
            frame,
            image_path,
            printer,
            job,
            detector_paused,
        }
    }

    /// Fetch a frame from the configured camera, when one is configured: the
    /// URL it was fetched from, and what the fetch came to.
    pub(crate) async fn fetch_configured_frame(
        &self,
        print_id: Option<PrintId>,
    ) -> Option<(String, Frame)> {
        let url = self
            .config()
            .camera_snapshot_url
            .as_ref()?
            .as_str()
            .to_owned();
        let fetched = self.fetch_frame(&url, print_id).await;
        Some((url, fetched))
    }

    /// When a fetched frame was captured: what its source says, or now.
    pub(crate) fn frame_captured_at(&self, fetched: Option<&(String, Frame)>) -> Option<Timestamp> {
        match fetched {
            Some((_, Frame::Good(image))) => {
                Some(image.captured_at.unwrap_or_else(|| self.clock().now()))
            }
            _ => None,
        }
    }

    /// Write one event down with what a fetch gave as its image.
    ///
    /// A good frame is kept with the event in one write. A camera whose
    /// frames were all bad is written down as a [`CameraDegradedPayload`] in
    /// the event's place, carrying no frame. A camera that would not answer
    /// leaves the event written without one, and the refusal recorded against
    /// it; so does a store that could not write the frame's bytes
    /// ([`StoreError::Io`](crate::store::StoreError::Io)), which stored
    /// nothing of the write.
    ///
    /// # Errors
    ///
    /// Returns the store's own error when the event could not be written down
    /// at all.
    pub(crate) async fn write_with_frame<P: printobserver_types::EventPayload>(
        &self,
        print_id: Option<PrintId>,
        payload: &P,
        fetched: Option<(String, Frame)>,
    ) -> Result<EventRecord, CoreError> {
        let draft = |body: EventBody| EventDraft {
            print_id,
            source: crate::kinds::system_source(),
            received_at: self.clock().now(),
            body,
            raw: None,
        };
        let body = EventBody::of(payload)?;
        let Some((url, fetched)) = fetched else {
            return Ok(self.stores().events.append_event(draft(body)).await?);
        };
        let refused = match fetched {
            Frame::Good(image) => {
                let image = ImageDraft {
                    source_url: Some(url),
                    content_type: image.content_type,
                    bytes: image.bytes,
                };
                match self
                    .stores()
                    .events
                    .append_event_with_image(draft(body.clone()), image)
                    .await
                {
                    Ok(event) => return Ok(event),
                    // The frame's bytes could not be written, and nothing was
                    // stored: the event is written without its frame. Any
                    // other refusal is the event's own, and answered as one.
                    Err(error @ crate::store::StoreError::Io { .. }) => error.to_string(),
                    Err(error) => return Err(error.into()),
                }
            }
            Frame::Failed(detail) => detail,
            Frame::Degraded(degraded) => {
                return self
                    .append_system_event_to(print_id, EventBody::of(&degraded)?)
                    .await;
            }
        };
        let event = self.stores().events.append_event(draft(body)).await?;
        self.record_port_failure_to(print_id, event.id, PortFailureSite::CameraLook, refused)
            .await;
        Ok(event)
    }

    /// Fetch frames from the camera until one is good, up to
    /// [`FRAME_ATTEMPTS`] of them, each asked again while the camera answers a
    /// server error.
    pub(crate) async fn fetch_frame(&self, url: &str, print_id: Option<PrintId>) -> Frame {
        let mut faults: Vec<FrameFault> = Vec::new();
        while faults.len() < FRAME_ATTEMPTS as usize {
            let fetched = match self.fetch_retrying(url, print_id).await {
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

    /// Fetch one frame, asking again after [`CameraRetry::backoff`] each time
    /// the camera answers a server error, up to [`CameraRetry::retries`] more
    /// times — and writing each such answer down, as it happens, before the
    /// wait.
    async fn fetch_retrying(
        &self,
        url: &str,
        print_id: Option<PrintId>,
    ) -> Result<FetchedImage, VisionError> {
        let retry = self.config().camera_retry;
        let attempts = retry.retries.saturating_add(1);
        let mut attempt: u32 = 1;
        loop {
            let error = match self.vision().fetch_image(url.to_owned()).await {
                Ok(fetched) => return Ok(fetched),
                Err(error) => error,
            };
            let VisionError::Status {
                status, excerpt, ..
            } = &error
            else {
                return Err(error);
            };
            if !error.is_server_error() || attempt >= attempts {
                return Err(error);
            }
            let retried = CameraRetryPayload {
                attempt,
                attempts,
                status: *status,
                excerpt: excerpt.clone(),
                backoff_ms: u64::try_from(retry.backoff.as_millis()).unwrap_or(u64::MAX),
            };
            // A record that will not render, or that the store refuses, costs
            // the fetch nothing: the retry is made whether or not it was
            // written down.
            if let Ok(body) = EventBody::of(&retried) {
                let _ = self.append_system_event_to(print_id, body).await;
            }
            // A wait the system could not start is no reason to give up the
            // frame: the next attempt is made at once instead.
            let _ = crate::delay::Delay::new(retry.backoff).await;
            attempt += 1;
        }
    }
}

/// How often, and how far apart, a fetch the camera answered with a server
/// error is made again.
///
/// The defaults bound what a camera failing every attempt costs a scheduled
/// check: with the vision adapter's own per-fetch timeout of five seconds,
/// four attempts and three waits are at most about 21.5 s.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CameraRetry {
    /// How many more times a fetch is made after its first is answered with a
    /// server error.
    pub retries: u32,
    /// How long after a server error the next attempt is made.
    pub backoff: Duration,
}

impl Default for CameraRetry {
    fn default() -> Self {
        Self {
            retries: DEFAULT_CAMERA_RETRIES,
            backoff: DEFAULT_CAMERA_RETRY_BACKOFF,
        }
    }
}

/// How many more times a fetch the camera answered with a server error is
/// made: three, so four attempts in all.
pub const DEFAULT_CAMERA_RETRIES: u32 = 3;

/// How long after a server error the next attempt at a frame is made.
pub const DEFAULT_CAMERA_RETRY_BACKOFF: Duration = Duration::from_millis(500);

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
