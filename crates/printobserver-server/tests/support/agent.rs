//! A supervising agent these journeys stand in for.
//!
//! The agent is a separate process on the far side of the supervisor port, and
//! what it answers is not what these journeys are about — they are about what
//! the server does with the answer. So this stands in for one, opening a
//! session on the first turn of a print and continuing it afterwards, which is
//! the behaviour the server's session handling depends on.
//!
//! It is issued its turn's credential the way the adapter is, before it does
//! anything else, and a journey about that credential can hold the turn open
//! while it acts with it: [`StandInAgent::hold_turns`].
//!
//! The real `OneHarness` adapter, driving `OneHarness`'s own published
//! deterministic responder, is what this crate's integration tier runs.

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};

use printobserver_supervisor_api::{
    AgentAssessment, BoxFuture, CREDENTIAL_ENV, Confidence, SERVER_ENV, SupervisorError,
    SupervisorPort, TurnAccess, TurnOutcome, TurnRequest,
};
use printobserver_supervisor_api::{SessionPhase, SupervisionSession};
use printobserver_types::{PrintId, Timestamp};

/// The harness identity this stands in for.
pub const IDENTITY: &str = "claude-code";

/// The longest a held turn waits to be released.
const HELD_AT_MOST: core::time::Duration = core::time::Duration::from_secs(60);

/// A supervising agent that opens one session per print and continues it.
#[derive(Debug, Default)]
pub struct StandInAgent {
    /// One session per print, and how long it has been running.
    sessions: Mutex<BTreeMap<PrintId, SupervisionSession>>,
    /// Every turn it has been asked to take, in order.
    turns: Mutex<Vec<TurnRequest>>,
    /// How long each turn takes, which is what a journey about the ingress
    /// answering before its handling completes makes long.
    dwell: Mutex<core::time::Duration>,
    /// Every pass a turn was issued, in order.
    passes: Mutex<Vec<Pass>>,
    /// Whether turns wait to be released before they answer.
    holding: Mutex<bool>,
    /// What every turn fails with once it has run, when turns fail.
    failing: Mutex<Option<SupervisorError>>,
    /// Whether each turn is moved into a second session after its first, as
    /// the adapter moves a turn whose session the harness refused to continue.
    moving: Mutex<bool>,
    /// Wakes a held turn.
    released: tokio::sync::Notify,
}

/// What one turn was issued to reach the server with.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Pass {
    /// The session the turn was issued it for.
    pub session_name: String,
    /// The address it was told the server is at.
    pub server: Option<String>,
    /// The credential minted for it.
    pub credential: String,
}

/// The session name the stand-in runs one print's turns in: the one the
/// adapter names a print's first session.
#[must_use]
pub fn session_of(print_id: PrintId) -> String {
    format!("print-{print_id}")
}

impl StandInAgent {
    /// An agent that answers at once.
    #[must_use]
    pub fn new() -> Arc<Self> {
        Arc::new(Self::default())
    }

    /// Make every turn take this long, so a journey can watch the handling run
    /// on after the answer has gone back.
    pub fn taking(&self, dwell: core::time::Duration) {
        *self.dwell.lock().expect("the agent is not poisoned") = dwell;
    }

    /// Every pass a turn was issued, in order.
    #[must_use]
    pub fn passes(&self) -> Vec<Pass> {
        self.passes
            .lock()
            .expect("the agent is not poisoned")
            .clone()
    }

    /// Make every turn fail with `error` once it has run, as a harness that
    /// timed out or could not be reached fails one.
    pub fn fail_turns_with(&self, error: SupervisorError) {
        *self.failing.lock().expect("the agent is not poisoned") = Some(error);
    }

    /// Move every turn into a second session after it was issued its first
    /// pass, and issue it the second session's: what the adapter does when the
    /// harness refuses to continue a session.
    pub fn move_turns_to_a_second_session(&self) {
        *self.moving.lock().expect("the agent is not poisoned") = true;
    }

    /// Make every turn wait, once it has been issued its pass, until
    /// [`StandInAgent::release_turns`].
    pub fn hold_turns(&self) {
        *self.holding.lock().expect("the agent is not poisoned") = true;
    }

    /// Let every held turn answer, and stop holding the ones after it.
    pub fn release_turns(&self) {
        *self.holding.lock().expect("the agent is not poisoned") = false;
        self.released.notify_waiters();
    }

    /// Wait until `count` passes have been issued.
    ///
    /// # Panics
    ///
    /// Panics when they have not been inside half a minute.
    pub async fn issued(&self, count: usize) -> Vec<Pass> {
        let deadline = tokio::time::Instant::now() + core::time::Duration::from_secs(30);
        loop {
            let passes = self.passes();
            if passes.len() >= count {
                return passes;
            }
            assert!(
                tokio::time::Instant::now() < deadline,
                "only {} of {count} turns were issued a pass",
                passes.len()
            );
            tokio::time::sleep(core::time::Duration::from_millis(20)).await;
        }
    }

    /// Every turn it has been asked to take, in order.
    #[must_use]
    pub fn turns(&self) -> Vec<TurnRequest> {
        self.turns
            .lock()
            .expect("the agent is not poisoned")
            .clone()
    }
}

/// One assessment, which is the same every turn: what a journey reads is what
/// the server did with it.
fn assessment() -> AgentAssessment {
    AgentAssessment {
        summary: "the first layer is down and the walls are clean".to_owned(),
        confidence: Confidence::High,
        should_continue: true,
        did: "read the event, the picture and the print's context".to_owned(),
        why: "nothing in the picture is coming away from the bed".to_owned(),
        escalating: false,
    }
}

impl SupervisorPort for StandInAgent {
    fn run_turn(
        &self,
        request: TurnRequest,
        access: std::sync::Arc<dyn TurnAccess>,
    ) -> BoxFuture<'_, Result<TurnOutcome, SupervisorError>> {
        let dwell = *self.dwell.lock().expect("the agent is not poisoned");
        let print_id = request.print_id;
        let mut sessions_run = vec![session_of(print_id)];
        if *self.moving.lock().expect("the agent is not poisoned") {
            sessions_run.push(format!("{}-2", session_of(print_id)));
        }
        for session_name in sessions_run {
            let issued = match printobserver_supervisor_api::TurnSession::new(session_name.as_str())
                .and_then(|session| access.issue(&session))
            {
                Ok(issued) => issued,
                Err(error) => return Box::pin(async move { Err(error) }),
            };
            let environment = issued.environment();
            let value = |name: &str| {
                environment
                    .iter()
                    .find(|(named, _)| *named == name)
                    .map(|(_, value)| value.clone())
            };
            self.passes
                .lock()
                .expect("the agent is not poisoned")
                .push(Pass {
                    session_name,
                    server: value(SERVER_ENV),
                    credential: value(CREDENTIAL_ENV).unwrap_or_default(),
                });
        }
        let failing = self
            .failing
            .lock()
            .expect("the agent is not poisoned")
            .clone();
        let holding = *self.holding.lock().expect("the agent is not poisoned");
        self.turns
            .lock()
            .expect("the agent is not poisoned")
            .push(request);
        let mut sessions = self.sessions.lock().expect("the agent is not poisoned");
        let now = Timestamp::now();
        let (phase, session) = match sessions.get(&print_id).cloned() {
            Some(mut held) => {
                held.last_turn_at = now;
                (SessionPhase::Continued, held)
            }
            None => (
                SessionPhase::Created,
                SupervisionSession {
                    print_id,
                    session_name: session_of(print_id),
                    harness_identity: IDENTITY.to_owned(),
                    created_at: now,
                    last_turn_at: now,
                    closed_at: None,
                    close_reason: None,
                },
            ),
        };
        sessions.insert(print_id, session.clone());
        drop(sessions);
        Box::pin(async move {
            if holding {
                let released = self.released.notified();
                tokio::pin!(released);
                released.as_mut().enable();
                // Bounded, so a journey that failed before releasing it does
                // not hold the runtime open behind it.
                if *self.holding.lock().expect("the agent is not poisoned") {
                    let _ = tokio::time::timeout(HELD_AT_MOST, released).await;
                }
            }
            if !dwell.is_zero() {
                tokio::time::sleep(dwell).await;
            }
            if let Some(error) = failing {
                return Err(error);
            }
            Ok(TurnOutcome {
                session,
                phase,
                assessment: assessment(),
            })
        })
    }

    fn close_session(
        &self,
        print_id: PrintId,
        close_reason: String,
    ) -> BoxFuture<'_, Result<(), SupervisorError>> {
        let mut sessions = self.sessions.lock().expect("the agent is not poisoned");
        if let Some(session) = sessions.get_mut(&print_id) {
            session.closed_at = Some(Timestamp::now());
            session.close_reason = Some(close_reason);
        }
        drop(sessions);
        Box::pin(async move { Ok(()) })
    }
}
