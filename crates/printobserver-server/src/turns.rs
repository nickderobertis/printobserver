//! The credentials supervision turns authenticate with, held in memory alone.
//!
//! # One credential per run, bound before it exists
//!
//! A supervision turn's runs reach the server's API with a credential minted
//! for that run: [`GENERATED_TURN_CREDENTIAL_BYTES`] bytes from the operating
//! system's cryptographically secure random source, bound to the agent class,
//! the session the run is in and the print the turn is about. The core opens
//! one turn's issuer of them at the turn boundary through
//! [`TurnAuthority::open`], hands it to the supervisor port, and revokes it
//! when the turn returns — on success, failure and timeout alike, and when the
//! turn is abandoned, because dropping what was opened revokes too.
//!
//! # Nothing here is persisted
//!
//! The registry is a map in this process's memory, keyed by the SHA-256 of each
//! credential rather than by the credential itself: nothing is written, so a
//! server that restarts admits none of the credentials the one before it
//! minted, and no turn survives a restart for one to be owed to. Looking a
//! presented credential up by its digest takes as long whatever was guessed:
//! what varies is the digest, which says nothing about the credential.

use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};

use printobserver_core::{OpenedTurn, TurnAuthority};
use printobserver_supervisor_api::{SupervisorError, TurnAccess, TurnPass};
use printobserver_types::PrintId;
use sha2::{Digest as _, Sha256};

/// How many random bytes a turn's credential is drawn from: the same as every
/// credential this program generates.
pub const GENERATED_TURN_CREDENTIAL_BYTES: usize = crate::config::GENERATED_CREDENTIAL_BYTES;

/// Who one turn credential authenticates as.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TurnBinding {
    /// The session the run that holds it is in: never empty, because the only
    /// constructor is `Scope::issue_drawn`, which refuses an empty one.
    session_name: String,
    /// The print the turn is about.
    print_id: PrintId,
}

impl TurnBinding {
    /// The session the run holding this credential is in, as a request's
    /// `Actor::Agent { session_name }` spells it.
    #[must_use]
    pub fn session_name(&self) -> &str {
        &self.session_name
    }

    /// The print the turn holding this credential is about.
    #[must_use]
    pub const fn print_id(&self) -> PrintId {
        self.print_id
    }
}

/// One live credential, by the scope that issued it.
#[derive(Debug)]
struct Held {
    /// The turn that issued it.
    scope: u64,
    /// Who it authenticates as.
    binding: TurnBinding,
}

/// The registry behind every handle onto it.
#[derive(Debug)]
struct Registry {
    /// The live credentials, keyed by the SHA-256 of each.
    held: Mutex<HashMap<[u8; 32], Held>>,
    /// The identifier the next scope takes.
    next_scope: AtomicU64,
    /// Where the server answers, as a turn's runs are told.
    server: Option<SocketAddr>,
}

/// Every live turn credential, by its digest. Cloning it is another handle
/// onto the same registry.
#[derive(Debug, Clone)]
pub struct TurnCredentials(Arc<Registry>);

/// One credential's digest.
fn digest(credential: &[u8]) -> [u8; 32] {
    Sha256::digest(credential).into()
}

impl TurnCredentials {
    /// An empty registry whose turns' runs are told the server is at `server`.
    #[must_use]
    pub fn new(server: Option<SocketAddr>) -> Self {
        Self(Arc::new(Registry {
            held: Mutex::new(HashMap::new()),
            next_scope: AtomicU64::new(0),
            server,
        }))
    }

    /// The registry, held.
    fn held(&self) -> MutexGuard<'_, HashMap<[u8; 32], Held>> {
        self.0
            .held
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    /// Who a presented credential authenticates as, when it is a live turn's.
    #[must_use]
    pub fn admit(&self, presented: &[u8]) -> Option<TurnBinding> {
        self.held()
            .get(&digest(presented))
            .map(|held| held.binding.clone())
    }

    /// How many turn credentials are live.
    #[must_use]
    pub fn live(&self) -> usize {
        self.held().len()
    }

    /// Forget every credential one scope issued.
    fn revoke(&self, scope: u64) {
        self.held().retain(|_, held| held.scope != scope);
    }
}

impl TurnAuthority for TurnCredentials {
    fn open(&self, print_id: PrintId) -> OpenedTurn {
        let scope = Arc::new(Scope {
            registry: self.clone(),
            id: self.0.next_scope.fetch_add(1, Ordering::Relaxed),
            print_id,
            closed: Mutex::new(false),
        });
        let closing = Arc::clone(&scope);
        OpenedTurn::new(scope, Box::new(move || closing.close()))
    }
}

/// One turn's issuer, as the port is handed it.
#[derive(Debug)]
struct Scope {
    /// Where the credentials it issues are admitted.
    registry: TurnCredentials,
    /// Which turn this is.
    id: u64,
    /// The print the turn is about.
    print_id: PrintId,
    /// Whether the turn has returned, after which nothing more is issued.
    closed: Mutex<bool>,
}

impl Scope {
    /// Revoke every credential this turn was issued, and issue no more.
    fn close(&self) {
        *self
            .closed
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = true;
        self.registry.revoke(self.id);
    }
}

impl TurnAccess for Scope {
    fn issue(&self, session_name: &str) -> Result<TurnPass, SupervisorError> {
        self.issue_drawn(session_name, getrandom::fill)
    }
}

impl Scope {
    /// Issue the run in `session_name` a credential, drawn by `draw` — the
    /// operating system's random source, for every run this server starts.
    /// A draw that is refused registers nothing.
    fn issue_drawn(
        &self,
        session_name: &str,
        draw: impl FnOnce(&mut [u8]) -> Result<(), getrandom::Error>,
    ) -> Result<TurnPass, SupervisorError> {
        use base64::Engine as _;

        // The session is what a turn's requests are held to claiming, so one
        // that names none is no session a request could claim.
        if session_name.trim().is_empty() {
            return Err(SupervisorError::Unavailable {
                detail: "a run in no session is issued no credential".to_owned(),
            });
        }

        let mut drawn = [0_u8; GENERATED_TURN_CREDENTIAL_BYTES];
        draw(&mut drawn).map_err(|error| SupervisorError::Unavailable {
            detail: format!(
                "no credential could be minted for the turn, because the operating system's \
                 random source refused: {error}"
            ),
        })?;
        let credential = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(drawn);
        let digested = digest(credential.as_bytes());
        let pass = TurnPass::new(self.registry.0.server, credential)?;
        // Held across the insertion, so a turn that returns while a run is
        // being issued its credential leaves nothing live behind it.
        let closed = self
            .closed
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if *closed {
            return Err(SupervisorError::Unavailable {
                detail: "the turn this run belongs to has already returned".to_owned(),
            });
        }
        let mut held = self.registry.held();
        held.retain(|_, live| live.scope != self.id);
        held.insert(
            digested,
            Held {
                scope: self.id,
                binding: TurnBinding {
                    session_name: session_name.to_owned(),
                    print_id: self.print_id,
                },
            },
        );
        drop(held);
        drop(closed);
        Ok(pass)
    }
}

#[cfg(test)]
mod tests {
    use printobserver_core::TurnAuthority as _;
    use printobserver_supervisor_api::{CREDENTIAL_ENV, SERVER_ENV, TurnPass};
    use printobserver_types::PrintId;

    use super::{GENERATED_TURN_CREDENTIAL_BYTES, TurnBinding, TurnCredentials};

    /// The value one variable of a pass carries.
    fn value_of(pass: &TurnPass, name: &str) -> String {
        pass.environment()
            .into_iter()
            .find(|(named, _)| *named == name)
            .map_or_else(|| panic!("the pass carries no {name}"), |(_, value)| value)
    }

    /// A credential is admitted as the session and print it was issued for
    /// while its turn runs, a second issue replaces the first, and revoking
    /// the turn admits none of them and issues no more.
    #[test]
    fn a_turns_credential_lives_exactly_as_long_as_its_turn() {
        let registry = TurnCredentials::new(Some("127.0.0.1:1".parse().expect("an address")));
        let print_id = PrintId::new();
        let opened = registry.open(print_id);
        let access = opened.access();
        let pass = access.issue("print-a").expect("one is minted");
        let first = value_of(&pass, CREDENTIAL_ENV);

        assert_eq!(value_of(&pass, SERVER_ENV), "http://127.0.0.1:1");
        // Unpadded URL-safe base64 of the drawn bytes.
        assert_eq!(
            first.len(),
            (GENERATED_TURN_CREDENTIAL_BYTES * 4).div_ceil(3)
        );
        assert_eq!(
            registry.admit(first.as_bytes()),
            Some(TurnBinding {
                session_name: "print-a".to_owned(),
                print_id,
            })
        );
        let second = value_of(&access.issue("print-a-2").expect("minted"), CREDENTIAL_ENV);
        assert_ne!(first, second);
        assert_eq!(registry.admit(first.as_bytes()), None);
        assert_eq!(registry.live(), 1);

        opened.revoke();
        assert_eq!(registry.admit(second.as_bytes()), None);
        assert!(access.issue("print-a-3").is_err());
        assert!(
            registry.open(print_id).access().issue("  ").is_err(),
            "a run in no session was issued a credential"
        );
        assert_eq!(registry.live(), 0);
    }

    /// A draw the random source refuses issues nothing and registers nothing,
    /// so the run it was for is not started.
    #[test]
    fn a_refused_draw_issues_and_registers_nothing() {
        let registry = TurnCredentials::new(None);
        let scope = super::Scope {
            registry: registry.clone(),
            id: 0,
            print_id: PrintId::new(),
            closed: std::sync::Mutex::new(false),
        };

        let Err(refused) = scope.issue_drawn("print-a", |_| Err(getrandom::Error::UNSUPPORTED))
        else {
            panic!("a refused draw issued a credential");
        };

        assert!(
            refused.to_string().contains("random source refused"),
            "{refused}"
        );
        assert_eq!(registry.live(), 0);
    }

    /// Dropping one turn's opened issuer revokes it, and another turn's
    /// credential is left alone.
    #[test]
    fn dropping_one_turn_revokes_its_own_credential_and_no_other() {
        let registry = TurnCredentials::new(None);
        let kept = registry.open(PrintId::new());
        let kept_credential = value_of(
            &kept.access().issue("kept").expect("minted"),
            CREDENTIAL_ENV,
        );
        let dropped = registry.open(PrintId::new());
        let dropped_credential = value_of(
            &dropped.access().issue("gone").expect("minted"),
            CREDENTIAL_ENV,
        );
        drop(dropped);

        assert_eq!(registry.admit(dropped_credential.as_bytes()), None);
        assert!(registry.admit(kept_credential.as_bytes()).is_some());
    }
}
