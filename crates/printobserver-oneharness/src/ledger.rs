//! One print's supervision sessions and turns, on disk.
//!
//! Nothing about a live session is held in memory. A print's whole supervision
//! history — every session it has opened, in order, and every turn that ran —
//! is one file under the state directory, so a port rebuilt from that directory
//! alone continues the conversation the previous one was in.

use core::fmt;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use printobserver_supervisor_api::{SessionPhase, SupervisionSession, SupervisorError};
use printobserver_types::{PrintId, Timestamp};
use serde::{Deserialize, Serialize};

/// The directory under the state directory this port keeps its ledgers in.
pub const SESSIONS_DIRECTORY: &str = "supervisor-sessions";

/// The ledger's own on-disk shape, as a closed set rather than free text.
///
/// A record is written under exactly one of these and read back under exactly
/// one of these, so a ledger a later build wrote is refused by the reader
/// rather than read as though this build had written it. Widening this type is
/// what adding a shape looks like, and a reader that has not been widened
/// cannot silently accept the new one.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum LedgerFormat {
    /// One print's sessions and turns, in order, with no turn's model calls.
    /// Still read: it is [`LedgerFormat::V2`] with every turn's calls unknown.
    #[serde(rename = "1")]
    V1,
    /// [`LedgerFormat::V1`], each turn carrying the usage of every model call
    /// it made. Still read: it is [`LedgerFormat::V3`] with no compaction and
    /// no turn's start known.
    #[serde(rename = "2")]
    V2,
    /// The shape this build writes: [`LedgerFormat::V2`], each turn carrying
    /// when it started, beside every idle compaction of the print's sessions.
    #[serde(rename = "3")]
    V3,
}

/// The shape this build writes.
const LEDGER_FORMAT: LedgerFormat = LedgerFormat::V3;

/// The name of the session watching one print, at one point in its sequence.
///
/// Every one of these is derived from a print id rather than taken from a
/// caller, and a persisted one is read back under the same rule: an empty name
/// is what a turn recorded against no conversation would carry, and the ledger
/// exists to say which conversation each turn belongs to.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
#[serde(transparent)]
pub struct SessionName(String);

impl SessionName {
    /// The session watching one print at one point in its sequence.
    ///
    /// The first session of a print is named for the print alone. A print whose
    /// session has been closed and which then receives another event opens the
    /// next in the sequence, whose name carries that sequence beside the print
    /// id rather than resuming the closed one.
    #[must_use]
    pub fn of(print_id: &PrintId, sequence: usize) -> Self {
        Self(if sequence <= 1 {
            format!("print-{print_id}")
        } else {
            format!("print-{print_id}-{sequence}")
        })
    }

    /// The session a probe of the turn configuration runs in: the caller's name
    /// under a prefix no print's session carries, so a probe never continues a
    /// print's conversation.
    ///
    /// # Errors
    ///
    /// Returns [`SupervisorError::Unavailable`] when `name` is empty.
    pub fn probe(name: &str) -> Result<Self, SupervisorError> {
        if name.trim().is_empty() {
            return Err(SupervisorError::Unavailable {
                detail: "a probe's session name that is empty names no conversation".to_owned(),
            });
        }
        Ok(Self(format!("probe-{name}")))
    }

    /// The name as `OneHarness` and the ledger spell it.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for SessionName {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl<'de> Deserialize<'de> for SessionName {
    /// Read a persisted name, refusing one that names no conversation.
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let text = String::deserialize(deserializer)?;
        if text.trim().is_empty() {
            return Err(serde::de::Error::custom(
                "a session name that is empty names no conversation",
            ));
        }
        Ok(Self(text))
    }
}

/// The tokens one model call of a turn read and wrote, as the harness reported
/// them: each count absent when it reported none, never a guess of zero.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CallUsage {
    /// Input tokens billed outside the prompt cache.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub input_tokens: Option<u64>,
    /// Output tokens.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub output_tokens: Option<u64>,
    /// Input tokens read from the prompt cache.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cache_read_tokens: Option<u64>,
    /// Input tokens written to the prompt cache.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cache_write_tokens: Option<u64>,
}

impl CallUsage {
    /// The size of the conversation this call leaves: its whole input, cache
    /// read and written included, and its output. `None` when the harness
    /// reported none of the four.
    #[must_use]
    pub fn conversation_tokens(&self) -> Option<u64> {
        let counts = [
            self.input_tokens,
            self.cache_read_tokens,
            self.cache_write_tokens,
            self.output_tokens,
        ];
        counts.iter().any(Option::is_some).then(|| {
            counts
                .iter()
                .flatten()
                .fold(0_u64, |sum, count| sum.saturating_add(*count))
        })
    }

    /// One call's usage, as `OneHarness` reports it.
    #[must_use]
    pub const fn of(usage: &oneharness_core::domain::signals::Usage) -> Self {
        Self {
            input_tokens: usage.input_tokens,
            output_tokens: usage.output_tokens,
            cache_read_tokens: usage.cache_read_tokens,
            cache_write_tokens: usage.cache_write_tokens,
        }
    }
}

/// One supervision turn as this port recorded it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RecordedTurn {
    /// The session the turn ran in.
    pub session_name: SessionName,
    /// When it ended.
    pub ran_at: Timestamp,
    /// When it started: after any compaction of its session it cancelled had
    /// ended, and before its first run. Absent from a turn an earlier shape
    /// recorded.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub started_at: Option<Timestamp>,
    /// Why the turn produced no assessment, when it produced none. A turn that
    /// failed is written down and the loop carries on.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub failure: Option<String>,
    /// The usage of each model call the turn made, in order: empty when the
    /// harness reported no per-call usage.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub calls: Vec<CallUsage>,
}

/// How an idle compaction ended, in `OneHarness`'s words.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CompactionEnd {
    /// The session continues from the compacted conversation.
    Completed,
    /// An event for the print cancelled it; the session is as it was.
    Cancelled,
    /// The harness has no compaction `OneHarness` can drive.
    Unsupported,
    /// The harness did not complete it; the session is as it was.
    Failed,
}

impl CompactionEnd {
    /// The end `OneHarness` reported.
    #[must_use]
    pub const fn of(status: oneharness_core::domain::compact::CompactStatus) -> Self {
        use oneharness_core::domain::compact::CompactStatus;
        match status {
            CompactStatus::Completed => Self::Completed,
            CompactStatus::Cancelled => Self::Cancelled,
            CompactStatus::Unsupported => Self::Unsupported,
            CompactStatus::Failed => Self::Failed,
        }
    }
}

/// One compaction of a print's session, taken while no turn ran.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RecordedCompaction {
    /// The session compacted.
    pub session_name: SessionName,
    /// When it started.
    pub started_at: Timestamp,
    /// When its harness process had exited and its end was known.
    pub ended_at: Timestamp,
    /// How it ended.
    pub ended: CompactionEnd,
    /// The conversation's size when it was started: the last turn's last
    /// call's input, cache included, and its output.
    pub size_before: u64,
    /// The context size before, as the harness reported it, when it did.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reported_before: Option<u64>,
    /// The context size after, as the harness reported it, when it did.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reported_after: Option<u64>,
    /// The compaction's own model usage, each count absent where the harness
    /// reported none.
    #[serde(default)]
    pub usage: CallUsage,
    /// Why it did not complete, when the harness said.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    /// How long a scheduled check waited for it to end, in milliseconds, when
    /// one did: the one event that lets an idle compaction end rather than
    /// cancelling it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scheduled_check_waited_ms: Option<u64>,
}

/// One print's sessions and turns.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct PrintLedger {
    /// The on-disk shape this file was written under.
    schema_version: LedgerFormat,
    /// The print this ledger is about.
    print_id: PrintId,
    /// Every session opened for the print, in order; the last is the current
    /// one, which is closed exactly when the print's supervision is.
    sessions: Vec<SupervisionSession>,
    /// Every turn that ran, in order.
    turns: Vec<RecordedTurn>,
    /// Every idle compaction of the print's sessions, in order. Absent from
    /// a ledger an earlier shape wrote, which is a print never compacted.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    compactions: Vec<RecordedCompaction>,
}

impl PrintLedger {
    /// Where one print's ledger lives under a state directory.
    pub(crate) fn path(state_dir: &Path, print_id: &PrintId) -> PathBuf {
        state_dir
            .join(SESSIONS_DIRECTORY)
            .join(format!("{print_id}.json"))
    }

    /// Read a print's ledger, answering an empty one for a print with none.
    ///
    /// A record is refused unless it is the shape this build writes and is the
    /// ledger of the print it was selected by. Both are refusals rather than
    /// repairs: a ledger written under a shape this build does not know is one
    /// whose fields it would be guessing at, and a ledger holding another
    /// print's sessions under this print's name is a state directory that has
    /// been rearranged underneath the supervisor. Reading either as this
    /// print's own history is how one print's conversation ends up continuing
    /// another's.
    pub(crate) fn read(state_dir: &Path, print_id: &PrintId) -> io::Result<Self> {
        let path = Self::path(state_dir, print_id);
        match fs::read_to_string(&path) {
            Ok(text) => {
                let ledger: Self = serde_json::from_str(&text).map_err(|error| {
                    io::Error::other(format!(
                        "{} is not a ledger this build reads (it writes {LEDGER_FORMAT:?}): {error}",
                        path.display()
                    ))
                })?;
                if ledger.print_id != *print_id {
                    return Err(io::Error::other(format!(
                        "{} holds the sessions of print {}, not of print {print_id}",
                        path.display(),
                        ledger.print_id
                    )));
                }
                ledger
                    .agrees_with_itself()
                    .map_err(|detail| io::Error::other(format!("{}: {detail}", path.display())))?;
                // An earlier shape is read as this one, and written back as it.
                Ok(Self {
                    schema_version: LEDGER_FORMAT,
                    ..ledger
                })
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                // A print with no ledger has no history only where the ledger
                // directory is one, or is not there yet. Windows answers "not
                // found" for a file under something that is not a directory,
                // where Unix answers "not a directory", so that is asked here
                // rather than read as a print that has never been supervised.
                let directory = state_dir.join(SESSIONS_DIRECTORY);
                if fs::metadata(&directory).is_ok_and(|found| !found.is_dir()) {
                    return Err(io::Error::other(format!(
                        "{} is something other than a directory, so no ledger can be read from it",
                        directory.display()
                    )));
                }
                Ok(Self {
                    schema_version: LEDGER_FORMAT,
                    print_id: *print_id,
                    sessions: Vec::new(),
                    turns: Vec::new(),
                    compactions: Vec::new(),
                })
            }
            Err(error) => Err(error),
        }
    }

    /// Whether the sessions and turns read back are this print's own history.
    ///
    /// Three things a ledger this build wrote is always true of, and each of
    /// them is what makes the file answer the question it exists for. A session
    /// of another print, a session named outside the sequence, or a turn
    /// recorded against a session that is not here — each would be read as this
    /// print's history and none of it would be. They are refused rather than
    /// dropped: a history missing the turn that mattered reads exactly like one
    /// where nothing happened.
    fn agrees_with_itself(&self) -> Result<(), String> {
        for (index, session) in self.sessions.iter().enumerate() {
            if session.print_id != self.print_id {
                return Err(format!(
                    "session `{}` is of print {}, and this is the ledger of print {}",
                    session.session_name, session.print_id, self.print_id
                ));
            }
            let expected = SessionName::of(&self.print_id, index + 1);
            if session.session_name != expected.as_str() {
                return Err(format!(
                    "session {} of this print is named `{}`, and this build names it `{expected}`",
                    index + 1,
                    session.session_name
                ));
            }
        }
        let opened = |name: &SessionName| {
            self.sessions
                .iter()
                .any(|session| session.session_name == name.as_str())
        };
        for turn in &self.turns {
            if !opened(&turn.session_name) {
                return Err(format!(
                    "a turn is recorded against session `{}`, which this print has never opened",
                    turn.session_name
                ));
            }
        }
        for compaction in &self.compactions {
            if !opened(&compaction.session_name) {
                return Err(format!(
                    "a compaction is recorded against session `{}`, which this print has never opened",
                    compaction.session_name
                ));
            }
        }
        Ok(())
    }

    /// Write the ledger back, creating the directory it lives in on the way.
    ///
    /// Whole-file, because a print's sessions and its turns have to agree: a
    /// turn appended without the session it ran in is a history that reads
    /// wrong rather than one that is missing an entry.
    pub(crate) fn write(&self, state_dir: &Path) -> io::Result<()> {
        let path = Self::path(state_dir, &self.print_id);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        let text = serde_json::to_string_pretty(self).map_err(io::Error::other)?;
        // Written beside it and moved over it, so a reader never sees a ledger
        // half-written: an idle compaction writes its end on a thread of its
        // own while the print's history is read.
        let mut written = path.clone().into_os_string();
        written.push(".writing");
        let written = PathBuf::from(written);
        fs::write(&written, text)?;
        fs::rename(&written, &path)
    }

    /// Every session opened for this print, in order.
    pub(crate) fn sessions(&self) -> &[SupervisionSession] {
        &self.sessions
    }

    /// Every turn recorded for this print, in order.
    pub(crate) fn turns(&self) -> &[RecordedTurn] {
        &self.turns
    }

    /// Every idle compaction recorded for this print, in order.
    pub(crate) fn compactions(&self) -> &[RecordedCompaction] {
        &self.compactions
    }

    /// Write down one idle compaction.
    pub(crate) fn record_compaction(&mut self, compaction: RecordedCompaction) {
        self.compactions.push(compaction);
    }

    /// The session a turn arriving now runs in: the current one while it is
    /// open, and the next of the sequence once it has been closed.
    pub(crate) fn session_for_next_turn(&self) -> SessionName {
        match self.sessions.last() {
            Some(session) if session.closed_at.is_none() => {
                SessionName(session.session_name.clone())
            }
            _ => SessionName::of(&self.print_id, self.sessions.len() + 1),
        }
    }

    /// The name the session after the current one takes.
    pub(crate) fn name_after_current(&self) -> SessionName {
        SessionName::of(&self.print_id, self.sessions.len() + 1)
    }

    /// Close the current session, if one is open, with this reason.
    ///
    /// Answers the session it closed, so a caller can write down what it did.
    pub(crate) fn close_current(
        &mut self,
        reason: &str,
        at: Timestamp,
    ) -> Option<SupervisionSession> {
        let session = self
            .sessions
            .last_mut()
            .filter(|session| session.closed_at.is_none())?;
        session.closed_at = Some(at);
        session.close_reason = Some(reason.to_owned());
        Some(session.clone())
    }

    /// Write down the session a turn ran in, opening it when the turn opened it.
    ///
    /// The phase comes from the run's own report rather than from what this
    /// ledger expected, which is why it is passed in.
    pub(crate) fn record_session(
        &mut self,
        name: &SessionName,
        harness_identity: &str,
        phase: SessionPhase,
        at: Timestamp,
    ) -> SupervisionSession {
        let known = self.sessions.iter().rposition(|session| {
            session.session_name == name.as_str() && session.closed_at.is_none()
        });
        if let Some(index) = known {
            let session = &mut self.sessions[index];
            if phase == SessionPhase::Created {
                session.created_at = at;
            }
            session.last_turn_at = at;
            harness_identity.clone_into(&mut session.harness_identity);
            return session.clone();
        }
        let session = SupervisionSession {
            print_id: self.print_id,
            session_name: name.to_string(),
            harness_identity: harness_identity.to_owned(),
            created_at: at,
            last_turn_at: at,
            closed_at: None,
            close_reason: None,
        };
        self.sessions.push(session.clone());
        session
    }

    pub(crate) fn record_turn(
        &mut self,
        session_name: &SessionName,
        (started_at, at): (Timestamp, Timestamp),
        failure: Option<&str>,
        calls: Vec<CallUsage>,
    ) {
        self.turns.push(RecordedTurn {
            session_name: session_name.clone(),
            ran_at: at,
            started_at: Some(started_at),
            failure: failure.map(ToOwned::to_owned),
            calls,
        });
    }
}
