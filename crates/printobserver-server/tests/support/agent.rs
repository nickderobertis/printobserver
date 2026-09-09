//! A supervising agent these journeys stand in for.
//!
//! The agent is a separate process on the far side of the supervisor port, and
//! what it answers is not what these journeys are about — they are about what
//! the server does with the answer. So this stands in for one, opening a
//! session on the first turn of a print and continuing it afterwards, which is
//! the behaviour the server's session handling depends on.
//!
//! The real `OneHarness` adapter, driving `OneHarness`'s own published
//! deterministic responder, is what this crate's integration tier runs.

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};

use printobserver_supervisor_api::{
    BoxFuture, SupervisorError, SupervisorPort, TurnOutcome, TurnRequest,
};
use printobserver_types::{
    AgentAssessment, Confidence, PrintId, SessionPhase, SupervisionSession, Timestamp,
};

/// The harness identity this stands in for.
pub const IDENTITY: &str = "claude-code";

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
    ) -> BoxFuture<'_, Result<TurnOutcome, SupervisorError>> {
        let dwell = *self.dwell.lock().expect("the agent is not poisoned");
        let print_id = request.print_id;
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
                    session_name: format!("watch-{print_id}"),
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
            if !dwell.is_zero() {
                tokio::time::sleep(dwell).await;
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
