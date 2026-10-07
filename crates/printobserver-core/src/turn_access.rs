//! Where a supervision turn's runs take their credentials from.
//!
//! # Opened at the turn boundary, revoked when the turn returns
//!
//! Every turn is handed an issuer of its own, opened under the print's turn
//! lock just before the turn runs and bound to the agent class and that print.
//! What it issues — one credential per run, bound to the session the run is in
//! — is admitted by the server's API while the turn runs, and revoked when the
//! turn returns: on success, failure and timeout alike, and when the turn is
//! abandoned, because dropping an [`OpenedTurn`] revokes too.
//!
//! The authority itself is the composition root's: minting and holding a
//! credential needs a random source and a digest, and this crate names no
//! implementation of either. A supervisor nothing installed one on hands its
//! turns an issuer that issues nothing, so a turn's runs reach no server rather
//! than reaching it as somebody else.

use std::sync::Arc;

use printobserver_supervisor_api::{SupervisorError, TurnAccess, TurnPass, TurnSession};
use printobserver_types::PrintId;

/// One turn's issuer, and what revokes everything it issued.
pub struct OpenedTurn {
    /// What the supervisor port is handed.
    access: Arc<dyn TurnAccess>,
    /// What revokes it, taken when it runs.
    revoke: Option<Box<dyn FnOnce() + Send>>,
}

impl OpenedTurn {
    /// One turn's issuer, and what revokes everything it issued.
    #[must_use]
    pub fn new(access: Arc<dyn TurnAccess>, revoke: Box<dyn FnOnce() + Send>) -> Self {
        Self {
            access,
            revoke: Some(revoke),
        }
    }

    /// The issuer, as the supervisor port is handed it.
    #[must_use]
    pub fn access(&self) -> Arc<dyn TurnAccess> {
        Arc::clone(&self.access)
    }

    /// Revoke every credential this turn was issued.
    pub fn revoke(mut self) {
        self.revoke_now();
    }

    /// Run the revocation, once.
    fn revoke_now(&mut self) {
        if let Some(revoke) = self.revoke.take() {
            revoke();
        }
    }
}

impl Drop for OpenedTurn {
    fn drop(&mut self) {
        self.revoke_now();
    }
}

impl core::fmt::Debug for OpenedTurn {
    fn fmt(&self, formatter: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        formatter
            .debug_struct("OpenedTurn")
            .field("revoked", &self.revoke.is_none())
            .finish_non_exhaustive()
    }
}

/// Opens one issuer per supervision turn.
pub trait TurnAuthority: Send + Sync {
    /// The issuer for one turn about one print.
    fn open(&self, print_id: PrintId) -> OpenedTurn;
}

/// The issuer a supervisor with no authority installed hands its turns: one
/// that issues nothing.
#[derive(Debug)]
pub(crate) struct NoAccess;

impl TurnAccess for NoAccess {
    fn issue(&self, _session: &TurnSession) -> Result<TurnPass, SupervisorError> {
        Err(SupervisorError::Unavailable {
            detail: "this supervisor issues its turns no credential".to_owned(),
        })
    }
}

impl NoAccess {
    /// An opened turn over this issuer, with nothing to revoke.
    pub(crate) fn opened() -> OpenedTurn {
        OpenedTurn::new(Arc::new(Self), Box::new(|| {}))
    }
}
