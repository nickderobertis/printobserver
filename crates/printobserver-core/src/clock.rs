//! The one time source this crate reads.
//!
//! Core never calls the system clock. Every instant it stamps, and every
//! comparison it makes against one — a bounded intervention's expiry, the
//! agent's minimum interval — is read through a [`Clock`], so that expiry is
//! driven by time passing and by nothing else and a test drives it by
//! advancing this one dependency alone.

use printobserver_types::Timestamp;

/// The instant core reads the current time from.
///
/// Shareable across threads, because the expiry driver reads it from a thread
/// of its own while callers read it from theirs.
pub trait Clock: Send + Sync {
    /// The instant now.
    fn now(&self) -> Timestamp;
}

/// The system's own clock, which is what a running supervisor is given.
#[derive(Debug, Clone, Copy, Default)]
pub struct SystemClock;

impl Clock for SystemClock {
    fn now(&self) -> Timestamp {
        Timestamp::now()
    }
}

/// The instant `seconds` after `instant`, saturating rather than wrapping.
///
/// # Errors
///
/// Returns the timestamp crate's own error when the sum denotes no
/// representable instant.
pub fn plus_seconds(
    instant: Timestamp,
    seconds: i64,
) -> Result<Timestamp, printobserver_types::TimestampError> {
    Timestamp::from_unix_seconds(unix_seconds(instant).saturating_add(seconds))
}

/// How many whole seconds `later` is after `earlier`, saturating.
#[must_use]
pub fn seconds_between(earlier: Timestamp, later: Timestamp) -> i64 {
    unix_seconds(later).saturating_sub(unix_seconds(earlier))
}

/// One instant as whole seconds after the Unix epoch.
#[must_use]
pub fn unix_seconds(instant: Timestamp) -> i64 {
    instant.as_utc().timestamp()
}
