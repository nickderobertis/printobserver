//! What this crate says about what it did, and the small total functions.
//!
//! A record nobody can read is not an audit trail, so every error this crate
//! answers and every rejection it writes down says what happened in words a
//! caller can act on — and each says something different from the others.

use std::collections::BTreeMap;

use printobserver_printer_api::PrinterError;
use printobserver_store_api::StoreError;
use printobserver_supervisor_api::SupervisorError;
use printobserver_types::{
    ActionKind, Actor, ActorClass, Adjustable, PrintAction, PrintId, PrinterState, Range,
    RejectionReason, SafetyEnvelope,
};
use printobserver_vision_api::VisionError;

use printobserver_core::{
    Clock, CoreError, SystemClock, effective_bounds, plus_seconds, rejection_detail,
    restoring_action, seconds_between, unix_seconds,
};

use crate::world::{World, permissive_envelope};

/// Every rejection reason, one of each.
fn every_rejection() -> Vec<RejectionReason> {
    vec![
        RejectionReason::OutOfBounds {
            adjustable: Adjustable::Feedrate,
            requested: 5.0,
            allowed: Range::new(0.5, 2.0),
        },
        RejectionReason::ActorMayNotRequest {
            actor_class: ActorClass::Agent,
            action: ActionKind::Cancel,
        },
        RejectionReason::NoActivePrint,
        RejectionReason::InvalidFromState {
            state: PrinterState::Operational,
        },
        RejectionReason::MinIntervalNotElapsed {
            interval_s: 300,
            since_last_s: 4,
        },
        RejectionReason::UnsupportedAdjustable {
            adjustable: Adjustable::ToolTarget { tool: 1 },
        },
    ]
}

/// Every error this crate answers says which port failed, and says it distinctly.
#[test]
fn every_core_error_says_what_failed_and_says_it_distinctly() {
    let errors = vec![
        CoreError::Store(StoreError::Database {
            detail: "the database is locked".to_owned(),
        }),
        CoreError::Printer(PrinterError::Unreachable {
            detail: "no route".to_owned(),
        }),
        CoreError::Vision(VisionError::TimedOut),
        CoreError::Supervisor(SupervisorError::TimedOut),
        CoreError::NoSuchPrint {
            print_id: PrintId::new(),
        },
        CoreError::Unrepresentable {
            detail: "too far from the epoch".to_owned(),
        },
    ];
    let mut said: Vec<String> = errors.iter().map(ToString::to_string).collect();
    for message in &said {
        assert!(!message.is_empty());
    }
    said.sort();
    said.dedup();
    assert_eq!(said.len(), errors.len(), "two errors read the same");
}

/// Every rejection reads back in words a caller can act on, and distinctly.
#[test]
fn every_rejection_reads_back_in_words_a_caller_can_act_on() {
    let mut said: Vec<String> = every_rejection().iter().map(rejection_detail).collect();
    assert!(
        said[0].contains("0.5") && said[0].contains('2'),
        "{}",
        said[0]
    );
    for message in &said {
        assert!(!message.is_empty());
    }
    said.sort();
    said.dedup();
    assert_eq!(said.len(), 6, "two rejections read the same");
}

/// A restoring request is built for every adjustable, bounded by no duration.
#[test]
fn a_restoring_request_is_built_for_every_adjustable() {
    let cases = [
        (Adjustable::Feedrate, 1.0, ActionKind::SetFeedrateFactor),
        (Adjustable::Flowrate, 1.0, ActionKind::SetFlowrateFactor),
        (
            Adjustable::ToolTarget { tool: 0 },
            215.0,
            ActionKind::SetToolTargetC,
        ),
        (Adjustable::BedTarget, 60.0, ActionKind::SetBedTargetC),
        (Adjustable::Fan, 40.0, ActionKind::SetFanPercent),
    ];
    for (adjustable, prior, kind) in cases {
        let action = restoring_action(adjustable, prior);
        assert_eq!(action.kind(), kind);
        assert_eq!(action.actor(), &Actor::System);
        assert!(!action.reason().is_empty());
        assert_eq!(
            printobserver_core::adjustment(&action),
            Some((adjustable, prior))
        );
    }
}

/// The bounds answer the range one adjustable may take, and nothing for one the
/// envelope does not name.
#[test]
fn the_bounds_answer_the_range_one_adjustable_may_take() {
    let bounds = effective_bounds(&permissive_envelope(), None);
    assert_eq!(bounds.range(Adjustable::Fan), Some(Range::new(0.0, 100.0)));
    assert_eq!(bounds.range(Adjustable::ToolTarget { tool: 1 }), None);
    assert_eq!(bounds.narrowings, Vec::new());
}

/// An envelope naming nothing bounds nothing, which is a printer with no
/// adjustable at all rather than one with every adjustable free.
#[test]
fn an_envelope_naming_nothing_bounds_nothing() {
    let empty = SafetyEnvelope {
        allowed: BTreeMap::new(),
        actions: BTreeMap::new(),
        agent_min_interval_s: 0,
    };
    let bounds = effective_bounds(&empty, None);
    assert_eq!(bounds.effective.allowed, BTreeMap::new());
}

/// The system clock is what a running supervisor is given, and it moves forward.
#[test]
fn the_system_clock_reads_the_system() {
    let clock = SystemClock;
    let first = clock.now();
    let second = SystemClock.now();
    assert!(second >= first);
    assert!(unix_seconds(first) > 0);
    assert_eq!(seconds_between(first, first), 0);
    assert_eq!(
        seconds_between(
            first,
            plus_seconds(first, 90).expect("a representable instant")
        ),
        90
    );
    assert!(plus_seconds(first, i64::MAX).is_err());
}

/// A supervisor says what it is without saying what it holds.
#[test]
fn a_supervisor_says_what_it_is() {
    let world = World::new();
    let said = format!("{:?}", world.core);
    assert!(said.contains("Supervisor"), "{said}");
    assert!(said.contains("config"), "{said}");
}

/// A bounded change nobody could time is refused rather than opened.
#[test]
fn a_duration_that_names_no_representable_instant_is_answered() {
    let world = World::new();
    let print = world.open_print(7);
    world.printer.reports_state(PrinterState::Printing);

    let answered = world.request(
        print.id,
        PrintAction::SetFanPercent {
            percent: 80.0,
            duration_s: Some(i64::MAX),
            reason: "for as long as time allows".to_owned(),
            actor: Actor::Operator,
        },
    );

    assert!(
        matches!(answered, Err(CoreError::Unrepresentable { .. })),
        "{answered:?}"
    );
}

/// Acknowledging a failure without asking for a stop reaches no printer method.
#[test]
fn acknowledging_a_failure_without_a_stop_reaches_no_printer_method() {
    let world = World::new();
    let print = world.open_print(7);
    world.printer.reports_state(PrinterState::Printing);
    world.journal.clear();

    let outcome = world
        .request(
            print.id,
            PrintAction::AcknowledgeFailure {
                event_id: printobserver_types::EventId::new(),
                disposition: printobserver_types::AcknowledgementDisposition::Watch,
                reason: "watching it more closely".to_owned(),
                actor: Actor::Operator,
            },
        )
        .expect("the request is recorded");

    assert_eq!(
        outcome.record.decision,
        printobserver_types::PolicyDecision::Accepted
    );
    assert_eq!(
        outcome.record.outcome,
        Some(printobserver_types::ExecutionOutcome::Succeeded)
    );
    assert_eq!(world.journal.printer_actions(), Vec::new());
    world.journal.assert_no_violations();
}
