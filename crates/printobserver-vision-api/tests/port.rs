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
use printobserver_types::{EventBody, EventSource, RawBytes, Timestamp};
use printobserver_vision_api::{
    BoxFuture, Detection, FetchedImage, MalformedExternalEventPayload, NormalizedAlert,
    ProviderPrint, STATUS_EXCERPT_MAX_BYTES, VisionError, VisionPort, body_excerpt,
};

/// The alert the trivial implementation answers with.
fn trivial_alert() -> NormalizedAlert {
    NormalizedAlert {
        source: EventSource::new("trivial"),
        received_at: Timestamp::sample_full(),
        body: EventBody::of(&MalformedExternalEventPayload::sample_full())
            .expect("a payload renders"),
        raw: RawBytes::default(),
        image_url: None,
        print: Some(ProviderPrint {
            id: 4211,
            file_name: Some("benchy.gcode".to_owned()),
            started_at: None,
        }),
        detection: Some(trivial_detection()),
        signal: None,
    }
}

/// The detection the trivial alert carries and the trivial port clears.
const fn trivial_detection() -> Detection {
    Detection {
        warning: false,
        paused_the_print: true,
        provider_printer_id: 41,
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

    fn fetch_image(&self, source_url: String) -> BoxFuture<'_, Result<FetchedImage, VisionError>> {
        let _ = source_url;
        Box::pin(async {
            Ok(FetchedImage {
                captured_at: None,
                bytes: RawBytes::default(),
                content_type: String::new(),
            })
        })
    }

    fn clear_detection(&self, detection: Detection) -> BoxFuture<'_, Result<(), VisionError>> {
        let _ = detection;
        Box::pin(async { Ok(()) })
    }
}

/// Every method answers its declared success type, behind a shared trait object.
#[test]
fn every_method_answers_its_declared_success_type() {
    let port: Arc<dyn VisionPort> = Arc::new(TrivialVision);
    assert_eq!(
        block_on(port.normalize(RawBytes::default(), None)),
        Ok(trivial_alert())
    );
    assert_eq!(
        block_on(port.fetch_image(String::new())),
        Ok(FetchedImage {
                captured_at: None,
            bytes: RawBytes::default(),
            content_type: String::new()
        })
    );
    assert_eq!(block_on(port.clear_detection(trivial_detection())), Ok(()));
}

/// A normalized alert reads its kind off the body it carries.
#[test]
fn a_normalized_alert_reads_its_kind_off_its_body() {
    assert_eq!(trivial_alert().kind().as_str(), "malformed_external_event");
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
        assert_eq!(
            handle.join().expect("the thread completes"),
            Ok(trivial_alert())
        );
    }
}

/// Every variant of this port's error vocabulary says what it is.
#[test]
fn every_error_variant_says_what_it_is() {
    let variants = [
        VisionError::Malformed {
            detail: "not JSON".to_owned(),
            raw: RawBytes::new(b"x".to_vec()),
        },
        VisionError::TimedOut,
        VisionError::TooLarge { limit: 1_048_576 },
        VisionError::UnacceptableContentType {
            content_type: "text/html".to_owned(),
        },
        VisionError::Unreachable {
            detail: "no route".to_owned(),
        },
        VisionError::Status {
            detail: "http://camera/frame.jpeg answered 500 Internal Server Error".to_owned(),
            status: 500,
            excerpt: "exit status 69".to_owned(),
        },
        VisionError::Status {
            detail: "http://camera/frame.jpeg answered 404 Not Found".to_owned(),
            status: 404,
            excerpt: String::new(),
        },
        VisionError::NotConfigured {
            detail: "no provider API is configured".to_owned(),
        },
    ];
    for variant in variants {
        assert!(!variant.to_string().is_empty(), "{variant:?} says nothing");
    }
}

/// A refusing answer is a server error only from `500` to `599`, and only
/// a refusing answer is one: an unreachable host or a timeout is not.
#[test]
fn only_a_five_hundred_answer_is_a_server_error() {
    let answered = |status: u16| VisionError::Status {
        detail: format!("answered {status}"),
        status,
        excerpt: String::new(),
    };
    for status in [500, 502, 503, 599] {
        assert!(answered(status).is_server_error(), "{status}");
    }
    for status in [301, 400, 404, 499, 600] {
        assert!(!answered(status).is_server_error(), "{status}");
    }
    assert!(!VisionError::TimedOut.is_server_error());
    assert!(
        !VisionError::Unreachable {
            detail: "connection refused".to_owned()
        }
        .is_server_error()
    );
}

/// A body's excerpt is its start, as trimmed text on one line, bounded, and
/// never cut through a character.
#[test]
fn a_body_excerpt_is_bounded_text_on_one_line() {
    assert_eq!(body_excerpt(b"exit status 69\n"), "exit status 69");
    assert_eq!(body_excerpt(b""), "");
    assert_eq!(body_excerpt(b"two\r\nlines"), "two  lines");
    assert_eq!(body_excerpt(&[0x66, 0xff, 0x6f]), "f\u{fffd}o");
    let long = "é".repeat(STATUS_EXCERPT_MAX_BYTES);
    let excerpt = body_excerpt(long.as_bytes());
    assert!(excerpt.len() <= STATUS_EXCERPT_MAX_BYTES);
    assert!(excerpt.chars().all(|letter| letter == 'é'), "{excerpt}");
    assert_eq!(excerpt.chars().count(), STATUS_EXCERPT_MAX_BYTES / 2);
    let replaced = body_excerpt(&[0xff; 4 * STATUS_EXCERPT_MAX_BYTES]);
    assert!(
        replaced.len() <= STATUS_EXCERPT_MAX_BYTES,
        "{} bytes of text from bytes that are not UTF-8",
        replaced.len()
    );
    assert!(replaced.chars().all(|letter| letter == '\u{fffd}'));
}

/// An address is a URL naming a host over HTTP, and nothing that only begins
/// like one.
#[test]
fn a_web_address_is_an_http_url_naming_a_host() {
    for accepted in [
        "http://127.0.0.1:3334",
        " https://obico.example/ ",
        "http://127.0.0.1:1984/api/frame.jpeg?src=camera",
        "HTTP://Printer.local",
        "http://[::1]:8080/frame.jpg",
        "http://[fe80::1]",
        "http://192.168.1.20:1984/api/frame.jpeg",
        "http://printer-1.local",
    ] {
        let address = printobserver_vision_api::WebAddress::new(accepted)
            .unwrap_or_else(|why| panic!("{accepted:?} was refused: {why}"));
        assert_eq!(address.as_str(), accepted.trim());
        assert_eq!(address.to_string(), accepted.trim());
    }
    for refused in [
        "http://?x",
        "http://[invalid",
        "http://[]",
        "http://[abc]",
        "http://[:::]",
        "http://[::1]x",
        "http://host..example",
        "http://-host",
        "http://host-:80",
        "http://999.999.999.999",
        "http://1.2.3",
        "http://printer.3",
        "http://",
        "http://host:99999",
        "http://host:port",
        "http://user@host",
        "http://ho st/",
        "ftp://127.0.0.1/frame.jpg",
        "file:///var/lib/frame.jpg",
        "127.0.0.1:3334",
        "",
    ] {
        assert!(
            printobserver_vision_api::WebAddress::new(refused).is_err(),
            "{refused:?} was accepted"
        );
    }
}
