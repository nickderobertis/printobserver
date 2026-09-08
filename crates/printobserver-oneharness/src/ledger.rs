//! One print's supervision sessions and turns, on disk.
//!
//! Nothing about a live session is held in memory. A print's whole supervision
//! history — every session it has opened, in order, and every turn that ran —
//! is one file under the state directory, so a port rebuilt from that directory
//! alone continues the conversation the previous one was in.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use printobserver_types::{PrintId, SupervisionSession, Timestamp};
use serde::{Deserialize, Serialize};

/// The directory under the state directory this port keeps its ledgers in.
pub const SESSIONS_DIRECTORY: &str = "supervisor-sessions";

/// The ledger's own on-disk shape version, independent of every other.
const LEDGER_SCHEMA_VERSION: &str = "1";

/// The name of the session watching one print, at one point in its sequence.
///
/// The first session of a print is named for the print alone. A print whose
/// session has been closed and which then receives another event opens the
/// next in the sequence, whose name carries that sequence beside the print id
/// rather than resuming the closed one.
#[must_use]
pub fn session_name(print_id: &PrintId, sequence: usize) -> String {
    if sequence <= 1 {
        format!("print-{print_id}")
    } else {
        format!("print-{print_id}-{sequence}")
    }
}

/// One supervision turn as this port recorded it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RecordedTurn {
    /// The session the turn ran in.
    pub session_name: String,
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
    schema_version: String,
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
    pub(crate) fn read(state_dir: &Path, print_id: &PrintId) -> io::Result<Self> {
        let path = Self::path(state_dir, print_id);
        match fs::read_to_string(&path) {
            Ok(text) => serde_json::from_str(&text).map_err(io::Error::other),
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(Self {
                schema_version: LEDGER_SCHEMA_VERSION.to_owned(),
                print_id: *print_id,
                sessions: Vec::new(),
                turns: Vec::new(),
            }),
            Err(error) => Err(error),
        }
    }

    /// Write the ledger back, creating the directory it lives in.
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
    pub(crate) fn session_for_next_turn(&self) -> String {
        match self.sessions.last() {
            Some(session) if session.closed_at.is_none() => session.session_name.clone(),
            _ => session_name(&self.print_id, self.sessions.len() + 1),
        }
    }

    /// The name the session after the current one takes.
    pub(crate) fn name_after_current(&self) -> String {
        session_name(&self.print_id, self.sessions.len() + 1)
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
        name: &str,
        harness_identity: &str,
        opened: bool,
        at: Timestamp,
    ) -> SupervisionSession {
        let known = self
            .sessions
            .iter()
            .rposition(|session| session.session_name == name && session.closed_at.is_none());
        if let Some(index) = known {
            let session = &mut self.sessions[index];
            if opened {
                session.created_at = at;
            }
            session.last_turn_at = at;
            harness_identity.clone_into(&mut session.harness_identity);
            return session.clone();
        }
        let session = SupervisionSession {
            print_id: self.print_id,
            session_name: name.to_owned(),
            harness_identity: harness_identity.to_owned(),
            created_at: at,
            last_turn_at: at,
            closed_at: None,
            close_reason: None,
        };
        self.sessions.push(session.clone());
        session
    }

    /// Write down one turn.
    pub(crate) fn record_turn(&mut self, session_name: &str, at: Timestamp, failure: Option<&str>) {
        self.turns.push(RecordedTurn {
            session_name: session_name.to_owned(),
            ran_at: at,
            failure: failure.map(ToOwned::to_owned),
        });
    }
}
