//! The supervisor port is implementable, dyn-compatible and shareable.
//!
//! A test-only implementation, held behind the same shared trait object the
//! supervision core will hold it behind, with every method called and awaited
//! and each asserted to answer that method's declared success type. It behaves
//! trivially rather than erroring, because a not-yet-implemented error would be
//! a variant no real implementation can ever produce.

#[path = "support/block_on.rs"]
mod block_on;

use std::sync::Arc;
use std::thread;

use block_on::block_on;
use printobserver_supervisor_api::{
    BoxFuture, SupervisorError, SupervisorPort, TurnOutcome, TurnRequest,
};
use printobserver_types::contract::Sample;
use printobserver_types::{AgentAssessment, PrintId, SessionPhase, SupervisionSession};

/// The outcome the trivial implementation answers with.
fn trivial_outcome() -> TurnOutcome {
    TurnOutcome {
        session: SupervisionSession::sample_minimal(),
        phase: SessionPhase::Created,
        assessment: AgentAssessment::sample_full(),
    }
}

/// The request the trivial implementation is driven with.
fn trivial_request() -> TurnRequest {
    TurnRequest {
        print_id: PrintId::sample_full(),
        event: printobserver_types::EventRecord::sample_minimal(),
        image_path: None,
        context_command: String::new(),
    }
}

/// A supervisor that answers every method with the success type it declares.
struct TrivialSupervisor;

impl SupervisorPort for TrivialSupervisor {
    fn run_turn(
        &self,
        request: TurnRequest,
    ) -> BoxFuture<'_, Result<TurnOutcome, SupervisorError>> {
        let _ = request;
        Box::pin(async { Ok(trivial_outcome()) })
    }

    fn close_session(
        &self,
        print_id: PrintId,
        close_reason: String,
    ) -> BoxFuture<'_, Result<(), SupervisorError>> {
        let _ = (print_id, close_reason);
        Box::pin(async { Ok(()) })
    }
}

/// Every method answers its declared success type, behind a shared trait object.
#[test]
fn every_method_answers_its_declared_success_type() {
    let port: Arc<dyn SupervisorPort> = Arc::new(TrivialSupervisor);
    assert_eq!(block_on(port.run_turn(trivial_request())), Ok(trivial_outcome()));
    assert_eq!(block_on(port.close_session(PrintId::sample_full(), String::new())), Ok(()));
}

/// The same trait object is shareable across threads, which is what core needs.
#[test]
fn the_trait_object_is_shareable_across_threads() {
    let port: Arc<dyn SupervisorPort> = Arc::new(TrivialSupervisor);
    let handles: Vec<_> = (0..4)
        .map(|_| {
            let shared = Arc::clone(&port);
            thread::spawn(move || block_on(shared.run_turn(trivial_request())))
        })
        .collect();
    for handle in handles {
        assert_eq!(handle.join().expect("the thread completes"), Ok(trivial_outcome()));
    }
}

/// Every variant of this port's error vocabulary says what it is.
#[test]
fn every_error_variant_says_what_it_is() {
    let variants = [
        SupervisorError::InvalidAnswer { detail: "no summary".to_owned() },
        SupervisorError::IdentityRefused { detail: "unknown identity".to_owned() },
        SupervisorError::Unavailable { detail: "no harness".to_owned() },
        SupervisorError::TimedOut,
    ];
    for variant in variants {
        assert!(!variant.to_string().is_empty(), "{variant:?} says nothing");
    }
}
