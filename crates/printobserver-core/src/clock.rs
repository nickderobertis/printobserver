//! The one time source this crate reads.
//!
//! Core never calls the system clock. Every instant it stamps, and every
//! comparison it makes against one, is read through a [`Clock`], so that a
//! test drives time by advancing this one dependency alone.
//!
//! A clock has two readings, and they answer different questions. [`Clock::now`]
//! is the wall clock: every instant this crate **records** — an action's
//! request, an event's arrival, an intervention's persisted deadline — is
//! stamped from it, so that the history reads in the absolute time it was
//! written. [`Clock::elapsed`] is a clock nothing steps, and every **duration
//! measured while the supervisor lives** is measured on it: a bounded
//! intervention's expiry, the agent's minimum interval between changes to the
//! machine, and the detector's resume grace. A host's wall clock is moved by
//! time synchronisation, and a duration measured across one such step is cut
//! short or drawn out by the step.
//!
//! The two meet only where a recorded instant has to become a live wait: an
//! intervention's persisted deadline, or the instant the agent last acted, read
//! back after a restart. Each is translated once, against a [`ClockReading`]
//! that took both readings together, and is counted down on
//! [`Clock::elapsed`] from then on.

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

/// Both of a clock's readings, taken together.
///
/// The one translation between them: where an absolute instant falls on the
/// clock nothing steps, as seen from the moment this was read. It is taken
/// once per instant translated, and never again, so that a later step of the
/// wall clock cannot move a wait already being counted down.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ClockReading {
    /// What the wall clock read.
    pub wall: Timestamp,
    /// What the clock nothing steps read at the same moment.
    pub elapsed: Duration,
}

impl ClockReading {
    /// Read both of one clock's readings now.
    #[must_use]
    pub fn of(clock: &dyn Clock) -> Self {
        Self {
            wall: clock.now(),
            elapsed: clock.elapsed(),
        }
    }

    /// Where `instant` falls on [`Clock::elapsed`], as this reading sees it:
    /// as far after this reading's own `elapsed` as `instant` is after its
    /// `wall`, and as far before it — down to the clock's origin — as `instant`
    /// is before.
    #[must_use]
    pub fn elapsed_at(&self, instant: Timestamp) -> Duration {
        let later = instant.as_utc() >= self.wall.as_utc();
        let offset = instant
            .as_utc()
            .signed_duration_since(*self.wall.as_utc())
            .num_microseconds()
            .unwrap_or(if later { i64::MAX } else { i64::MIN });
        let apart = Duration::from_micros(offset.unsigned_abs());
        if later {
            self.elapsed.saturating_add(apart)
        } else {
            self.elapsed.saturating_sub(apart)
        }
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
