//! The ingress: one received body written down, correlated and illustrated.
//!
//! This is the path a body the producer posted takes, and every one of its
//! steps is a write the store keeps:
//!
//! 1. The body is read into this system's own vocabulary. One that cannot be
//!    read is recorded under the malformed-external-event kind carrying its
//!    bytes, and *then* refused to the caller.
//! 2. The alert is correlated to a print. Obico's own print id continues the
//!    print already recorded for it — open or ended — and an id no print record
//!    holds opens one. A print that has ended does not reopen, and no second
//!    record is ever opened for an id the store already holds one for, because
//!    a replacement record is a reopening under another name.
//! 3. The event is appended.
//! 4. The snapshot the body names is fetched **now**, because the URL Obico
//!    sends is short-lived. A fetch that fails leaves the event recorded with
//!    no image and the failure recorded beside it, as an event of its own
//!    against the same print, rather than losing the alert.
//!
//! # Which kind a failed fetch is recorded under
//!
//! The supervision domain declares a kind for exactly this: a
//! [`PortFailurePayload`] at the [`ImageWrite`](PortFailureSite::ImageWrite)
//! site, naming the event whose handling reached the failing call. That is what a failed fetch is — the
//! alert itself was read perfectly, and only getting the image it pointed at
//! failed — and it is the same kind and the same site the supervision core
//! records its own image failures under.
//!
//! It is deliberately **not** the malformed-external-event kind. That kind
//! means the body that arrived could not be read, and recording a failed fetch
//! under it would claim the alert was unreadable while carrying the alert's own
//! bytes as the thing that could not be read — a false audit record about a
//! body this system read and stored. The alert's bytes stay where they belong,
//! on the alert's own event; the failure carries that event's identifier
//! instead, so a reader holding the alert reaches the failure and a reader
//! holding the failure reaches the alert.
//!
//! Its `detail` is the refusal in [`VisionError`]'s own words, which is what
//! names the variant: timed out, too large naming the limit, an unacceptable
//! content type naming what arrived, or unreachable carrying its detail. A
//! variant the port gains is carried by the same line with nothing here to
//! change.

use std::sync::Arc;

use printobserver_core::store::{EventDraft, EventStore, ImageStore, PrintStore, StoreError};
use printobserver_core::{ImageRecord, PrintRecord};
use printobserver_core::{PortFailurePayload, PortFailureSite, system_source};
use printobserver_types::{EventBody, EventId, EventRecord, PrintId, RawBytes, Timestamp};
use printobserver_vision_api::{
    MalformedExternalEventPayload, ProviderPrint, VisionError, VisionPort,
};

use crate::events::obico_source;
use crate::vision::{ObicoVision, ObicoVisionConfig, ObicoVisionError};

/// What one received body left behind it.
#[derive(Debug, Clone, PartialEq)]
pub struct Receipt {
    /// The event as the store recorded it.
    pub event: EventRecord,
    /// The print it was correlated to, when it was about one.
    pub print: Option<PrintRecord>,
    /// The snapshot stored beside it, when one was.
    pub image: Option<ImageRecord>,
    /// Why no snapshot was stored, when one was named and the fetch failed.
    pub image_failure: Option<VisionError>,
}

/// Why the ingress did not accept a body.
#[derive(Debug, Clone, PartialEq)]
pub enum IngressError {
    /// The body could not be read. It was recorded first, and here it is.
    ///
    /// The record is boxed because it is far the larger of the two variants and
    /// a refusal is the rarer of the two answers.
    Refused {
        /// Why it could not be read.
        refusal: VisionError,
        /// The malformed-external-event record it was written down as.
        recorded: Box<EventRecord>,
    },
    /// The store refused a write, so nothing was written down.
    Store {
        /// What the store said.
        error: StoreError,
    },
    /// The record this system would have written down would not render as
    /// JSON, so nothing was written down.
    Unrepresentable {
        /// What would not render.
        detail: String,
    },
}

impl core::fmt::Display for IngressError {
    fn fmt(&self, formatter: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::Refused { refusal, recorded } => {
                write!(
                    formatter,
                    "the body was recorded as event {} and refused: {refusal}",
                    recorded.id
                )
            }
            Self::Store { error } => write!(formatter, "nothing was written down: {error}"),
            Self::Unrepresentable { detail } => write!(
                formatter,
                "nothing was written down: the record would not render: {detail}"
            ),
        }
    }
}

impl core::error::Error for IngressError {}

impl From<StoreError> for IngressError {
    fn from(error: StoreError) -> Self {
        Self::Store { error }
    }
}

impl From<printobserver_types::serde_json::Error> for IngressError {
    fn from(error: printobserver_types::serde_json::Error) -> Self {
        Self::Unrepresentable {
            detail: error.to_string(),
        }
    }
}

/// What one refusal says, in the one line the malformed kind carries.
///
/// A refusal to *read a body* carries its own line and that line is the whole
/// answer; every other refusal is one this path does not produce today, and it
/// says so in its own words rather than being flattened to a placeholder.
fn detail_of(refusal: &VisionError) -> String {
    match refusal {
        VisionError::Malformed { detail, .. } => detail.clone(),
        other => other.to_string(),
    }
}

/// The Obico ingress: the adapter and the three stores it writes through.
///
/// It names the three aggregates its steps touch and no other: the print the
/// alert is correlated to, the event log it is appended to, and the image
/// stored beside it. Actions, interventions and sessions are the supervision
/// domain's own to write, and nothing here can reach them.
#[derive(Clone)]
pub struct ObicoIngress {
    /// The adapter that reads a body and fetches a snapshot.
    vision: ObicoVision,
    /// The print records an alert is correlated to.
    prints: Arc<dyn PrintStore>,
    /// The log every step of the handling is written down in.
    events: Arc<dyn EventStore>,
    /// The images stored beside the events.
    images: Arc<dyn ImageStore>,
}

impl ObicoIngress {
    /// The ingress, over the three stores it writes through, under the bounds
    /// given.
    ///
    /// # Errors
    ///
    /// Returns [`ObicoVisionError`] when the adapter's HTTP client cannot be
    /// built.
    pub fn new(
        prints: Arc<dyn PrintStore>,
        events: Arc<dyn EventStore>,
        images: Arc<dyn ImageStore>,
        config: ObicoVisionConfig,
    ) -> Result<Self, ObicoVisionError> {
        Ok(Self {
            vision: ObicoVision::new(config)?,
            prints,
            events,
            images,
        })
    }

    /// The adapter this ingress reads and fetches through.
    #[must_use]
    pub const fn vision(&self) -> &ObicoVision {
        &self.vision
    }

    /// Record one body this system could not read, and refuse it.
    async fn refuse(&self, body: RawBytes, refusal: VisionError) -> IngressError {
        let rendered = match EventBody::of(&MalformedExternalEventPayload {
            detail: detail_of(&refusal),
        }) {
            Ok(rendered) => rendered,
            Err(error) => return IngressError::from(error),
        };
        let draft = EventDraft {
            print_id: None,
            source: obico_source(),
            received_at: Timestamp::now(),
            body: rendered,
            raw: Some(body),
        };
        match self.events.append_event(draft).await {
            Ok(recorded) => IngressError::Refused {
                refusal,
                recorded: Box::new(recorded),
            },
            Err(error) => IngressError::Store { error },
        }
    }

    /// The print one alert belongs to, opening a record only for an unknown id.
    ///
    /// An id the store already holds a print for is not an unknown id, whether
    /// that print is open or ended, so it is answered rather than replaced. The
    /// correlation is the one the adapter hands over beside the body, which is
    /// the same one the supervision core reads.
    async fn print_for(
        &self,
        print: Option<&ProviderPrint>,
    ) -> Result<Option<PrintRecord>, IngressError> {
        let Some(print) = print else {
            return Ok(None);
        };
        if let Some(held) = self.prints.print_by_provider_id(print.id).await? {
            return Ok(Some(held));
        }
        Ok(Some(
            self.prints
                .open_print(Some(print.id), print.file_name.clone())
                .await?,
        ))
    }

    /// Record why no snapshot was stored, beside the event that named it.
    ///
    /// A port failure at the image-write site, naming the alert's own event, so
    /// that a reader holding either one reaches the other. It carries no `raw`
    /// of its own: the bytes that arrived are on the alert's event, and no
    /// bytes arrived for the response that never came.
    async fn record_image_failure(
        &self,
        print_id: PrintId,
        event_id: EventId,
        failure: &VisionError,
    ) -> Result<(), IngressError> {
        let draft = EventDraft {
            print_id: Some(print_id),
            source: system_source(),
            received_at: Timestamp::now(),
            body: EventBody::of(&PortFailurePayload {
                event_id,
                site: PortFailureSite::ImageWrite,
                detail: failure.to_string(),
            })?,
            raw: None,
        };
        self.events.append_event(draft).await?;
        Ok(())
    }

    /// Receive one body the producer posted.
    ///
    /// # Errors
    ///
    /// Returns [`IngressError::Refused`], carrying the record the body was
    /// written down as, when the body cannot be read; and
    /// [`IngressError::Store`] when a write fails.
    pub async fn receive(
        &self,
        body: RawBytes,
        content_type: Option<String>,
    ) -> Result<Receipt, IngressError> {
        let alert = match self.vision.normalize(body.clone(), content_type).await {
            Ok(alert) => alert,
            Err(refusal) => return Err(self.refuse(body, refusal).await),
        };
        let print = self.print_for(alert.print.as_ref()).await?;
        let event = self
            .events
            .append_event(EventDraft {
                print_id: print.as_ref().map(|record| record.id),
                source: alert.source,
                received_at: alert.received_at,
                body: alert.body,
                raw: Some(alert.raw),
            })
            .await?;

        let mut image = None;
        let mut image_failure = None;
        if let (Some(source_url), Some(record)) = (alert.image_url.as_ref(), print.as_ref()) {
            match self.vision.fetch_image(source_url.clone()).await {
                Ok(fetched) => {
                    image = Some(
                        self.images
                            .put_image(
                                record.id,
                                event.id,
                                Some(source_url.clone()),
                                fetched.content_type,
                                fetched.bytes,
                            )
                            .await?,
                    );
                }
                Err(failure) => {
                    self.record_image_failure(record.id, event.id, &failure)
                        .await?;
                    image_failure = Some(failure);
                }
            }
        }
        Ok(Receipt {
            event,
            print,
            image,
            image_failure,
        })
    }
}

#[cfg(test)]
mod tests {
    use printobserver_core::store::StoreError;
    use printobserver_types::contract::Sample as _;
    use printobserver_types::{EventRecord, RawBytes};
    use printobserver_vision_api::VisionError;

    use super::{IngressError, detail_of};

    /// A refusal that is not about reading a body says so in its own words.
    #[test]
    fn a_refusal_that_is_not_about_a_body_says_so_in_its_own_words() {
        assert_eq!(
            detail_of(&VisionError::Malformed {
                detail: "not JSON".to_owned(),
                raw: RawBytes::default(),
            }),
            "not JSON"
        );
        assert_eq!(
            detail_of(&VisionError::TimedOut),
            VisionError::TimedOut.to_string()
        );
    }

    /// Every refusal says which it is, and the recorded one names its record.
    #[test]
    fn every_refusal_says_which_it_is() {
        let recorded = EventRecord::sample_full();
        let refused = IngressError::Refused {
            refusal: VisionError::TimedOut,
            recorded: Box::new(recorded.clone()),
        };
        assert!(refused.to_string().contains(&recorded.id.to_string()));
        let refused_by_store = IngressError::from(StoreError::Database {
            detail: "the disk is full".to_owned(),
        });
        assert!(refused_by_store.to_string().contains("the disk is full"));
        let unrepresentable = IngressError::from(
            printobserver_types::serde_json::from_str::<u8>("not a number")
                .expect_err("a string is no number"),
        );
        assert!(matches!(
            unrepresentable,
            IngressError::Unrepresentable { .. }
        ));
        assert!(
            unrepresentable
                .to_string()
                .contains("the record would not render")
        );
    }
}
