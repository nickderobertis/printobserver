//! The one time source this crate reads.
//!
//! Core never calls the system clock. Every instant it stamps, and every
//! comparison it makes against one — a bounded intervention's expiry, the
//! agent's minimum interval — is read through a [`Clock`], so that expiry is
//! driven by time passing and by nothing else and a test drives it by
//! advancing this one dependency alone.
//!
//! A wait that is held only in memory — the detector's resume grace — is
//! timed on [`Clock::elapsed`] instead, which nothing steps: a host's wall
//! clock is moved by time synchronisation, and a wait measured across one
//! such step is cut short or drawn out by the step.

use std::sync::OnceLock;
use std::time::{Duration, Instant};

use printobserver_types::Timestamp;

/// The instant core reads the current time from.
///
/// Shareable across threads, because the expiry driver reads it from a thread
/// of its own while callers read it from theirs.
pub trait Clock: Send + Sync {
    /// The instant now.
    fn now(&self) -> Timestamp;

    /// How much time has passed since an origin of this clock's own, on a
    /// clock nothing steps: two readings differ by the time that passed
    /// between them, whatever the wall clock did meanwhile.
    ///
    /// The provided reading is [`Clock::now`]'s, as time since the Unix epoch,
    /// which a step of the wall clock moves; a clock over a host's own time
    /// answers its monotonic clock instead, as [`SystemClock`] does.
    fn elapsed(&self) -> Duration {
        let micros = self.now().as_utc().timestamp_micros();
        Duration::from_micros(u64::try_from(micros).unwrap_or_default())
    }
}

/// The system's own clock, which is what a running supervisor is given.
#[derive(Debug, Clone, Copy, Default)]
pub struct SystemClock;

impl Clock for SystemClock {
    fn now(&self) -> Timestamp {
        Timestamp::now()
    }

    fn elapsed(&self) -> Duration {
        static ORIGIN: OnceLock<Instant> = OnceLock::new();
        ORIGIN.get_or_init(Instant::now).elapsed()
    }
}

/// The instant `seconds` after `instant`.
///
/// Sub-second precision is kept: a bounded change asked to stand for a minute
/// stands for a minute, rather than for a minute less however far into a second
/// it happened to be made.
///
/// # Errors
///
/// Returns the timestamp crate's own error when the sum denotes no
/// representable instant.
pub fn plus_seconds(
    instant: Timestamp,
    seconds: i64,
) -> Result<Timestamp, printobserver_types::TimestampError> {
    instant.plus_seconds(seconds)
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
