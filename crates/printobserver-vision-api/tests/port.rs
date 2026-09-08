//! The vision port is implementable, dyn-compatible and shareable.
//!
//! A test-only implementation, held behind the same shared trait object the
//! supervision core will hold it behind, with every method called and awaited
//! and each asserted to answer that method's declared success type. It behaves
//! trivially rather than erroring, because a not-yet-implemented error would be
//! a variant no real implementation can ever produce.

#[path = "support/block_on.rs"]
mod block_on;

use std::sync::Arc;
use std::thread;

use block_on::block_on;
use printobserver_types::contract::Sample;
use printobserver_types::{EventKind, EventPayload, EventSource, RawBytes, Timestamp};
use printobserver_vision_api::{
    BoxFuture, FetchedImage, NormalizedAlert, VisionError, VisionPort,
};

/// The alert the trivial implementation answers with.
fn trivial_alert() -> NormalizedAlert {
    NormalizedAlert {
        source: EventSource::Obico,
        received_at: Timestamp::sample_full(),
        payload: EventPayload::sample_minimal(),
        raw: RawBytes::default(),
        image_url: None,
    }
}

/// A vision port that answers every method with the success type it declares.
struct TrivialVision;

impl VisionPort for TrivialVision {
    fn normalize(
        &self,
        body: RawBytes,
        content_type: Option<String>,
    ) -> BoxFuture<'_, Result<NormalizedAlert, VisionError>> {
        let _ = (body, content_type);
        Box::pin(async { Ok(trivial_alert()) })
    }

    fn fetch_image(
        &self,
        source_url: String,
    ) -> BoxFuture<'_, Result<FetchedImage, VisionError>> {
        let _ = source_url;
        Box::pin(async {
            Ok(FetchedImage { bytes: RawBytes::default(), content_type: String::new() })
        })
    }
}

/// Every method answers its declared success type, behind a shared trait object.
#[test]
fn every_method_answers_its_declared_success_type() {
    let port: Arc<dyn VisionPort> = Arc::new(TrivialVision);
    assert_eq!(block_on(port.normalize(RawBytes::default(), None)), Ok(trivial_alert()));
    assert_eq!(
        block_on(port.fetch_image(String::new())),
        Ok(FetchedImage { bytes: RawBytes::default(), content_type: String::new() })
    );
}

/// A normalized alert reads its kind off the closed pair it carries.
#[test]
fn a_normalized_alert_reads_its_kind_off_its_payload() {
    assert_eq!(trivial_alert().kind(), EventKind::ObicoFailureAlert);
}

/// The same trait object is shareable across threads, which is what core needs.
#[test]
fn the_trait_object_is_shareable_across_threads() {
    let port: Arc<dyn VisionPort> = Arc::new(TrivialVision);
    let handles: Vec<_> = (0..4)
        .map(|_| {
            let shared = Arc::clone(&port);
            thread::spawn(move || block_on(shared.normalize(RawBytes::default(), None)))
        })
        .collect();
    for handle in handles {
        assert_eq!(handle.join().expect("the thread completes"), Ok(trivial_alert()));
    }
}

/// Every variant of this port's error vocabulary says what it is.
#[test]
fn every_error_variant_says_what_it_is() {
    let variants = [
        VisionError::Malformed { detail: "not JSON".to_owned(), raw: RawBytes::new(b"x".to_vec()) },
        VisionError::TimedOut,
        VisionError::TooLarge { limit: 1_048_576 },
        VisionError::UnacceptableContentType { content_type: "text/html".to_owned() },
        VisionError::Unreachable { detail: "no route".to_owned() },
    ];
    for variant in variants {
        assert!(!variant.to_string().is_empty(), "{variant:?} says nothing");
    }
}
