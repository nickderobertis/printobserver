//! The one instant representation this vocabulary has.
//!
//! Every timestamp is an instant in UTC, serialized as an RFC 3339 string with
//! a zero offset. There is no local-time representation anywhere in this crate:
//! a string carrying a non-zero offset parses to the instant it denotes and
//! re-emits at a zero offset, and a string carrying no offset at all is refused
//! rather than guessed at.

use core::fmt;
use core::str::FromStr;
use std::borrow::Cow;

use chrono::{DateTime, SecondsFormat, TimeDelta, Utc};
use schemars::{JsonSchema, Schema, SchemaGenerator, json_schema};
use serde::{Deserialize, Deserializer, Serialize, Serializer, de::Error as _};

/// Why a string is not an instant this vocabulary can hold.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TimestampError {
    /// What is wrong with the string.
    detail: String,
}

impl TimestampError {
    /// What is wrong with the string.
    #[must_use]
    pub fn detail(&self) -> &str {
        &self.detail
    }
}

impl fmt::Display for TimestampError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "not an RFC 3339 instant: {}", self.detail)
    }
}

impl core::error::Error for TimestampError {}

/// An instant in UTC, serialized as an RFC 3339 string with a zero offset.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Timestamp(DateTime<Utc>);

impl Timestamp {
    /// The instant now.
    #[must_use]
    pub fn now() -> Self {
        #[cfg(feature = "replay-clock")]
        return Self(replay_clock::now(Utc::now()));
        #[cfg(not(feature = "replay-clock"))]
        Self(Utc::now())
    }

    /// Let `wait` pass on the real-print replay's clock rather than in real
    /// time, and answer whether it did.
    ///
    /// Only a build of the suite's own, under the `replay-clock` feature, with
    /// its clock file beside the program, answers `true`: every other build
    /// answers `false` and the caller waits as it always does. A caller asks
    /// only when nothing could end its wait early, so passing the wait at once
    /// moves every reading of the clock exactly as waiting it out would.
    #[must_use]
    // llmlint: ignore[code_lands_in_the_domain_that_owns_it] suppressions.toml has the reason.
    pub fn pass_replay_time(wait: core::time::Duration) -> bool {
        #[cfg(feature = "replay-clock")]
        return replay_clock::advance(wait);
        #[cfg(not(feature = "replay-clock"))]
        {
            let _ = wait;
            false
        }
    }

    /// A span measured on the host's steady clock, as the real-print replay's
    /// clock would measure it.
    ///
    /// The replay jumps its clock from one recorded event to the next, minutes or
    /// hours apart, in a few seconds of real time, so the agent's minimum interval,
    /// measured across a jump, has to count the jump too, or the agent's change at
    /// the second of two recorded events an hour apart reads as seconds after its
    /// change at the first. Only a build of the suite's own, under
    /// the `replay-clock` feature, with its clock file beside the program, adds
    /// anything: every other build answers `steady` unchanged. The answer never
    /// goes back, because a steady clock never does.
    #[must_use]
    // llmlint: ignore[code_lands_in_the_domain_that_owns_it] suppressions.toml has the reason.
    pub fn on_replay_clock(steady: core::time::Duration) -> core::time::Duration {
        #[cfg(feature = "replay-clock")]
        return replay_clock::steady(steady);
        #[cfg(not(feature = "replay-clock"))]
        steady
    }

    /// The instant this many seconds after the Unix epoch.
    ///
    /// # Errors
    ///
    /// Returns an error when the count of seconds denotes no representable
    /// instant.
    pub fn from_unix_seconds(seconds: i64) -> Result<Self, TimestampError> {
        DateTime::from_timestamp(seconds, 0)
            .map(Self)
            .ok_or_else(|| TimestampError {
                detail: format!(
                    "{seconds} seconds from the Unix epoch is not a representable instant"
                ),
            })
    }

    /// The instant this many seconds after this one.
    ///
    /// Sub-second precision is kept rather than dropped: an adjustment asked to
    /// stand for a minute stands for a minute, rather than for a minute less
    /// however far into a second it happened to be made.
    ///
    /// # Errors
    ///
    /// Returns an error when the result is not a representable instant.
    pub fn plus_seconds(self, seconds: i64) -> Result<Self, TimestampError> {
        // `try_seconds` rather than `seconds`: a count of seconds that is not a
        // representable span is answered here rather than raised, because the
        // caller is a duration somebody asked for.
        TimeDelta::try_seconds(seconds)
            .and_then(|span| self.0.checked_add_signed(span))
            .map(Self)
            .ok_or_else(|| TimestampError {
                detail: format!("{seconds} seconds after {self} is not a representable instant"),
            })
    }

    /// The latest instant this type writes as a string it reads back.
    ///
    /// The last moment of the year 9999: RFC 3339 spells a year in four
    /// digits, so a later instant is written with a sign and more digits that
    /// no RFC 3339 reader accepts — and that a store ordering instants by their
    /// text would order before every earlier one. Reading what is due by this
    /// instant reads everything due at any instant this type can record.
    ///
    /// # Panics
    ///
    /// Never: the instant is a fixed one every `chrono` represents.
    #[must_use]
    pub fn latest() -> Self {
        let last_day = chrono::NaiveDate::from_ymd_opt(9999, 12, 31)
            .and_then(|day| day.and_hms_nano_opt(23, 59, 59, 999_999_999))
            .expect("the last moment of the year 9999 is a date-time");
        Self(last_day.and_utc())
    }

    /// The instant as the UTC date-time it is.
    #[must_use]
    pub const fn as_utc(&self) -> &DateTime<Utc> {
        &self.0
    }
}

impl From<DateTime<Utc>> for Timestamp {
    fn from(instant: DateTime<Utc>) -> Self {
        Self(instant)
    }
}

impl fmt::Display for Timestamp {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        // `use_z` is what makes the offset a literal zero rather than `+00:00`,
        // and `AutoSi` keeps whatever sub-second precision the instant carries
        // so that an instant survives a round trip unchanged.
        write!(
            formatter,
            "{}",
            self.0.to_rfc3339_opts(SecondsFormat::AutoSi, true)
        )
    }
}

impl FromStr for Timestamp {
    type Err = TimestampError;

    fn from_str(text: &str) -> Result<Self, Self::Err> {
        DateTime::parse_from_rfc3339(text)
            .map(|offset| Self(offset.with_timezone(&Utc)))
            .map_err(|error| TimestampError {
                detail: format!("{text:?}: {error}"),
            })
    }
}

impl Serialize for Timestamp {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.collect_str(self)
    }
}

impl<'de> Deserialize<'de> for Timestamp {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let text = String::deserialize(deserializer)?;
        text.parse().map_err(D::Error::custom)
    }
}

impl JsonSchema for Timestamp {
    fn schema_name() -> Cow<'static, str> {
        Cow::Borrowed("Timestamp")
    }

    fn json_schema(_generator: &mut SchemaGenerator) -> Schema {
        json_schema!({
            "type": "string",
            "format": "date-time",
            "title": "Timestamp",
            "description": "An instant in UTC, as an RFC 3339 string with a zero offset."
        })
    }
}

/// The clock of the real-print replay: this program's wall clock moved by a file
/// beside it. Compiled only under the `replay-clock` feature, which the suite's own
/// build alone enables: a print replayed in minutes has to read as the hours it
/// took, or every time the agent reads contradicts every position it reads.
// llmlint: ignore[code_lands_in_the_domain_that_owns_it, changed_behavior_has_e2e] See suppressions.toml.
#[cfg(feature = "replay-clock")]
mod replay_clock {
    use chrono::{DateTime, TimeDelta, Utc};
    use core::time::Duration;
    use std::path::PathBuf;
    use std::sync::{Mutex, OnceLock};

    /// The file beside the program: the milliseconds to add, and the latest instant
    /// to answer in milliseconds since the epoch, or nothing past that for none.
    const CLOCK_FILE: &str = "clock";

    /// Set in the environment to wait in real time all the same: what the suite
    /// compares a jumped replay against.
    const REAL_WAITS: &str = "PRINTOBSERVER_REPLAY_REAL_WAITS";

    fn path() -> Option<&'static PathBuf> {
        static PATH: OnceLock<Option<PathBuf>> = OnceLock::new();
        PATH.get_or_init(|| {
            std::env::current_exe()
                .ok()
                .map(|program| program.with_file_name(CLOCK_FILE))
        })
        .as_ref()
    }

    /// Move the clock on by `wait`: the file's offset grows by it and its
    /// ceiling stands, written whole and then renamed into place so a reader
    /// never sees half of it. Nothing is moved, and `false` answered, when
    /// there is no file to move or real waits were asked for.
    pub(super) fn advance(wait: core::time::Duration) -> bool {
        if std::env::var_os(REAL_WAITS).is_some() {
            return false;
        }
        let Some(file) = path() else {
            return false;
        };
        let Ok(text) = std::fs::read_to_string(file) else {
            return false;
        };
        let mut words = text.split_whitespace();
        let Some(Ok(offset)) = words.next().map(str::parse::<i64>) else {
            return false;
        };
        let Ok(by) = i64::try_from(wait.as_millis()) else {
            return false;
        };
        let ceiling = words
            .next()
            .map(|ceiling| format!(" {ceiling}"))
            .unwrap_or_default();
        let written = file.with_extension(format!("advance-{}", std::process::id()));
        std::fs::write(
            &written,
            format!("{}{ceiling}\n", offset.saturating_add(by)),
        )
        .is_ok()
            && std::fs::rename(&written, file).is_ok()
    }

    /// The offset the clock file holds, in milliseconds, when there is one to read.
    fn offset_ms() -> Option<i64> {
        let text = std::fs::read_to_string(path()?).ok()?;
        text.split_whitespace().next()?.parse().ok()
    }

    /// What the steady clock has answered: the offset its first reading found, and
    /// its latest answer.
    struct Steady {
        first_ms: Option<i64>,
        latest: Duration,
    }

    /// `real` moved on by how far the clock file's offset has grown since the first
    /// reading of it, and never less than the last answer. A file that cannot be read
    /// moves nothing.
    pub(super) fn steady(real: Duration) -> Duration {
        static STEADY: Mutex<Steady> = Mutex::new(Steady {
            first_ms: None,
            latest: Duration::ZERO,
        });
        let mut held = STEADY
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let moved_ms = match (offset_ms(), held.first_ms) {
            (Some(offset), Some(first)) => offset.saturating_sub(first),
            (Some(offset), None) => {
                held.first_ms = Some(offset);
                0
            }
            (None, _) => 0,
        };
        let by = Duration::from_millis(moved_ms.unsigned_abs());
        let answer = if moved_ms >= 0 {
            real.saturating_add(by)
        } else {
            real.saturating_sub(by)
        }
        .max(held.latest);
        held.latest = answer;
        answer
    }

    pub(super) fn now(real: DateTime<Utc>) -> DateTime<Utc> {
        let Some(text) = path().and_then(|file| std::fs::read_to_string(file).ok()) else {
            return real;
        };
        let mut numbers = text.split_whitespace().map(str::parse::<i64>);
        let Some(Ok(offset)) = numbers.next() else {
            return real;
        };
        let Some(moved) =
            TimeDelta::try_milliseconds(offset).and_then(|by| real.checked_add_signed(by))
        else {
            return real;
        };
        match numbers.next() {
            Some(Ok(ceiling)) => {
                DateTime::from_timestamp_millis(ceiling).map_or(moved, |held| moved.min(held))
            }
            _ => moved,
        }
    }
}
