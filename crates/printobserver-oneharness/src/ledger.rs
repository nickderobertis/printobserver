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

use printobserver_types::{PrintId, SessionPhase, SupervisionSession, Timestamp};
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
    /// The shape this build writes: one print's sessions and turns, in order.
    #[serde(rename = "1")]
    V1,
}

/// The shape this build writes.
const LEDGER_FORMAT: LedgerFormat = LedgerFormat::V1;

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

/// One supervision turn as this port recorded it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RecordedTurn {
    /// The session the turn ran in.
    pub session_name: SessionName,
    /// When it ran.
    pub ran_at: Timestamp,
    /// Why the turn produced no assessment, when it produced none. A turn that
    /// failed is written down and the loop carries on.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub failure: Option<String>,
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
                Ok(ledger)
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(Self {
                schema_version: LEDGER_FORMAT,
                print_id: *print_id,
                sessions: Vec::new(),
                turns: Vec::new(),
            }),
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
        for turn in &self.turns {
            if !self
                .sessions
                .iter()
                .any(|session| session.session_name == turn.session_name.as_str())
            {
                return Err(format!(
                    "a turn is recorded against session `{}`, which this print has never opened",
                    turn.session_name
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
        fs::write(path, text)
    }

    /// Every session opened for this print, in order.
    pub(crate) fn sessions(&self) -> &[SupervisionSession] {
        &self.sessions
    }

    /// Every turn recorded for this print, in order.
    pub(crate) fn turns(&self) -> &[RecordedTurn] {
        &self.turns
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
        at: Timestamp,
        failure: Option<&str>,
    ) {
        self.turns.push(RecordedTurn {
            session_name: session_name.clone(),
            ran_at: at,
            failure: failure.map(ToOwned::to_owned),
        });
    }
}
