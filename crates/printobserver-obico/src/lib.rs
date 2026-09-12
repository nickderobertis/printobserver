//! `printobserver-obico`.
//!
//! Owns: the `Obico` adapter — the one implementation of
//! `printobserver-vision-api`, which reads the body the self-hosted `Obico`
//! webhook notification plugin posts into an event under this adapter's own
//! kind and fetches the snapshot that body names; the two event kinds and the
//! source name it writes ([`events`]); and the ingress that writes both down.
//!
//! May depend on: `printobserver-types`, `printobserver-vision-api`,
//! `printobserver-store-api` and `printobserver-core` — the type crate, two
//! ports, and the supervision domain whose port-failure kind a failed fetch is
//! recorded under — plus whatever it needs to reach `Obico`. Never another
//! implementation crate.
//!
//! # The three shapes, and the fourth thing that arrives
//!
//! The producer sends three bodies — a failure alert, a printer notification
//! about a print, and a printer notification about no print — and
//! [`normalize`](printobserver_vision_api::VisionPort::normalize) reads each into the kind this adapter
//! declares for it. The fourth thing that arrives is a body this system cannot
//! read, and it is **recorded** rather than dropped: [`ObicoIngress::receive`]
//! writes it down under the malformed-external-event kind carrying its bytes
//! and *then* refuses it to the caller, because an alert this system cannot
//! read is the one thing it must not lose.
//!
//! Cannot read covers three cases and not one: a body that is not well-formed
//! at all, a well-formed body omitting a field the contracts declare required,
//! and a well-formed body whose required field carries a value of the wrong
//! kind. The last two are what an implementation defaults its way past, so
//! nothing here supplies a default for a field the producer is declared to
//! send.
//!
//! # Why the image is fetched during the handling
//!
//! The URL `Obico` sends is short-lived, so the fetch happens while the alert
//! is being handled rather than when somebody first reads the event: a
//! supervision decision taken an hour later still has to be able to look at
//! what the camera saw. The fetch is bounded by
//! [`DEFAULT_FETCH_TIMEOUT`] and [`DEFAULT_MAX_IMAGE_BYTES`], and a fetch that
//! fails leaves the event recorded with no image and the failure recorded
//! beside it rather than losing the event.

pub mod events;
mod fetch;
mod ingress;
mod normalize;
mod vision;

pub use events::{
    OBICO_SOURCE, ObicoFailureAlertPayload, ObicoNotificationType, ObicoPrinterNotificationPayload,
    obico_source,
};
pub use ingress::{IngressError, ObicoIngress, Receipt};
pub use vision::{
    DEFAULT_FETCH_TIMEOUT, DEFAULT_MAX_IMAGE_BYTES, ObicoVision, ObicoVisionConfig,
    ObicoVisionError,
};
