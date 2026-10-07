//! A supervision turn's credential: minted for it, admitted while it runs, and
//! revoked when it returns.
//!
//! What core owns is *when*: an issuer opened under the print's turn lock just
//! before the turn runs, handed to the supervisor port, and revoked when the
//! turn returns. These journeys drive that through the real event loop over an
//! authority that remembers which credentials are live — the composition
//! root's own registry, and how it draws a credential, are the server's tier
//! to hold. The stand-in agent is issued its credential the way the adapter
//! is, and records who the authority admitted it as while its turn ran.

use printobserver_printer_api::PrinterState;
use printobserver_supervisor_api::SupervisorError;

use crate::fakes::TurnBinding;
use crate::world::{World, failure_alert};

/// A turn's credential is admitted as that turn's agent session and print
/// while it runs, and not at all once the turn has returned.
#[test]
fn a_turns_credential_is_admitted_while_it_runs_and_revoked_when_it_returns() {
    let world = World::new();
    let print = world.open_print(7);
    world.printer.reports_state(PrinterState::Printing);

    world
        .handle(failure_alert(7))
        .expect("the event is handled");

    let issued = world.agent.issued();
    assert_eq!(issued.len(), 1, "the turn was issued no credential");
    let (credential, admitted) = &issued[0];
    assert_eq!(
        admitted.as_ref(),
        Some(&TurnBinding {
            session_name: format!("print-{}", print.id),
            print_id: print.id,
        }),
        "the turn's own credential was not admitted as its session and print"
    );
    assert_eq!(world.turns.admit(credential), None);
    assert_eq!(world.turns.live(), 0);
}

/// A turn that fails is revoked exactly as one that succeeds, and two turns
/// are issued two different credentials.
#[test]
fn a_failed_turn_is_revoked_too_and_each_turn_gets_its_own_credential() {
    let world = World::new();
    world.open_print(7);
    world.printer.reports_state(PrinterState::Printing);

    world
        .handle(failure_alert(7))
        .expect("the event is handled");
    world.agent.fails(SupervisorError::TimedOut);
    world
        .handle(failure_alert(7))
        .expect("the event is handled");

    let issued = world.agent.issued();
    assert_eq!(issued.len(), 2, "each turn is issued a credential");
    assert_ne!(issued[0].0, issued[1].0, "two turns shared a credential");
    assert!(
        issued[1].1.is_some(),
        "the failing turn's credential was never live"
    );
    for (credential, _) in &issued {
        assert_eq!(world.turns.admit(credential), None);
    }
}
