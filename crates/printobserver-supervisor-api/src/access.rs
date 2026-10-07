//! How one supervision run reaches the server it runs under.
//!
//! # A run is handed a credential of its own, and nothing else
//!
//! Every run the port starts authenticates to the server's API with a
//! credential minted for that run alone, bound to the agent class, the session
//! the run is in and the print the turn is about. The core mints it at the turn
//! boundary and revokes it when the turn returns; the port asks for it through
//! [`TurnAccess::issue`] immediately before each run, naming the session that
//! run is in — which is the port's own to know, because a harness that refuses
//! to continue a session moves the turn into the next one.
//!
//! The run is handed it through its environment alone, as [`SERVER_ENV`] and
//! [`CREDENTIAL_ENV`]: no file is written holding it, and the context command a
//! turn is given names no configuration file, so the command-line program the
//! agent runs reads no file either.

use crate::SupervisorError;

/// The variable a run is told where the server is in.
pub const SERVER_ENV: &str = "PRINTOBSERVER_SERVER";

/// The variable a run is handed its own credential in.
pub const CREDENTIAL_ENV: &str = "PRINTOBSERVER_CREDENTIAL";

/// What a pass renders as, wherever one is rendered.
const REDACTED: &str = "<redacted>";

/// What one run is handed to reach the server with: where the server is, and
/// the credential minted for that run.
///
/// The credential is readable in exactly one place — [`Self::environment`],
/// which is what a run's environment is built from — and neither rendering of
/// this type shows it.
pub struct TurnPass {
    /// The address the server answers on, spelled the way a client reads it.
    server: Option<String>,
    /// The credential minted for this run.
    credential: String,
}

impl TurnPass {
    /// One pass: the server's address, when it is known, and a credential.
    #[must_use]
    pub const fn new(server: Option<String>, credential: String) -> Self {
        Self { server, credential }
    }

    /// The variables a run's environment carries, as `(name, value)` pairs:
    /// the server's address when it is known, and the credential.
    #[must_use]
    pub fn environment(&self) -> Vec<(&'static str, &str)> {
        let mut assigned = Vec::with_capacity(2);
        if let Some(server) = &self.server {
            assigned.push((SERVER_ENV, server.as_str()));
        }
        assigned.push((CREDENTIAL_ENV, self.credential.as_str()));
        assigned
    }
}

impl core::fmt::Debug for TurnPass {
    fn fmt(&self, formatter: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        formatter
            .debug_struct("TurnPass")
            .field("server", &self.server)
            .field("credential", &REDACTED)
            .finish()
    }
}

/// Issues the runs of one supervision turn their credentials.
///
/// The core hands one of these to [`crate::SupervisorPort::run_turn`] for the
/// turn it starts, already bound to the agent class and that turn's print, and
/// revokes everything it issued when the turn returns.
pub trait TurnAccess: Send + Sync {
    /// The pass the run in `session_name` presents.
    ///
    /// Issuing again within the same turn — a harness that refused to continue
    /// one session moves the turn into the next — revokes the pass issued
    /// before, so a turn holds one live credential at a time.
    ///
    /// # Errors
    ///
    /// Returns [`SupervisorError::Unavailable`] when no credential could be
    /// minted, which is a run that could reach nothing and is not started.
    fn issue(&self, session_name: &str) -> Result<TurnPass, SupervisorError>;
}

#[cfg(test)]
mod tests {
    use super::{CREDENTIAL_ENV, SERVER_ENV, TurnPass};

    /// A pass's environment carries the address and the credential, and
    /// neither rendering of it shows the credential.
    #[test]
    fn a_pass_carries_both_variables_and_renders_no_credential() {
        let pass = TurnPass::new(
            Some("http://127.0.0.1:8420".to_owned()),
            "qz7vk3xhw9mrbt2ycf5jdlgnps46auei".to_owned(),
        );

        assert_eq!(
            pass.environment(),
            vec![
                (SERVER_ENV, "http://127.0.0.1:8420"),
                (CREDENTIAL_ENV, "qz7vk3xhw9mrbt2ycf5jdlgnps46auei"),
            ]
        );
        assert!(!format!("{pass:?}").contains("qz7vk3xhw9mrbt2ycf5jdlgnps46auei"));
        assert_eq!(
            TurnPass::new(None, "c".to_owned()).environment(),
            vec![(CREDENTIAL_ENV, "c")]
        );
    }
}
