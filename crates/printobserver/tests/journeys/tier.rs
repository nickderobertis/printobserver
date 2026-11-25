//! The tier itself, over whichever world it was given.
//!
//! One entry rather than one per journey, because the journeys share a print:
//! pausing is valid from printing and resuming from paused, so the order they
//! run in is what a print actually goes through, and the ones that take a print
//! somewhere else run last.
//!
//! # The depths
//!
//! [`WALK`] is the fast tier's cross-product at every spelling and the
//! journeys that act on what it leaves, over a world whose printer port
//! reaches a socket. [`AGAINST_A_REAL_MACHINE`] is what the printer
//! integration tier runs: the same cross-product, over the `OctoPrint` `just
//! octoprint-up` provisioned. What the second leaves out is everything that is
//! about **this program** rather than about the machine — the renderings, the
//! failure classes, the credential — because those are proven once against a
//! world that can be driven deterministically rather than twice against one
//! that takes its own time. For the same reason it drives each command's
//! covering invocations rather than its whole cross-product of spellings
//! ([`crate::walk::covering`]): every spelling still reaches the real machine
//! once, and every combination of them reaches the socket.
//!
//! [`DEFECTS`], [`INTERVENTIONS`] and [`CREDENTIAL`] are the rest of what the
//! fast tier runs: the tainted variant's defects, the bounded interventions and
//! the credential's redaction. Between them they are most of the tier's time,
//! and none of them needs what the walk leaves behind beyond a print this
//! program started, so each is a test of its own over a world of its own that
//! runs beside [`WALK`] rather than after it. [`CLOSE_PAST_A_POLL`] is one
//! more beside it, over a world with no print started at all: an operator's
//! close of a record left open past the watch's default poll.
//!
//! [`CLOSING`] is one more, an operator's close of a record left open, over a
//! world of its own because it ends the print every journey of the walk acts
//! on.

use crate::machine::Reports;
use crate::world::World;

use super::{
    carrying, closing, confirming, durations, failures, formats, materializing, redaction, tainting,
};

/// The walk and every journey about this program that needs what it left,
/// over one print.
pub const WALK: &str = "walk";

/// Every assertion here, over the defect it is about.
pub const DEFECTS: &str = "defects";

/// Every adjustment, as a bounded intervention.
pub const INTERVENTIONS: &str = "interventions";

/// No run of the walk printing the credential.
pub const CREDENTIAL: &str = "credential";

/// A record left open past a default poll, ended by its own close: a world of
/// its own, with no print started first.
pub const CLOSE_PAST_A_POLL: &str = "close-past-a-poll";

/// An operator's close of a record left open, and every close refused while
/// the record might still be live: a world of its own.
pub const CLOSING: &str = "closing";

/// The cross-product walk alone, which is what a real machine adds.
pub const AGAINST_A_REAL_MACHINE: &str = "against-a-real-machine";

/// Run the tier over one world.
///
/// # Panics
///
/// Panics when the depth is not one of the seven this tier runs at.
pub fn run(world: &World, depth: &str) {
    if depth == CLOSE_PAST_A_POLL {
        closing::a_record_left_open_past_a_default_poll_is_ended_by_its_close(world);
        return;
    }
    if depth == CLOSING {
        closing::an_operator_closes_a_record_left_open(world);
        return;
    }
    let beside: Option<fn(&World)> = match depth {
        DEFECTS => Some(tainting::every_assertion_here_refuses_the_defect_it_is_about),
        INTERVENTIONS => Some(durations::every_adjustment_is_a_bounded_intervention),
        CREDENTIAL => Some(redaction::no_run_of_the_walk_prints_the_credential),
        WALK | AGAINST_A_REAL_MACHINE => None,
        _ => panic!("there is no `{depth}` depth to run this tier at"),
    };
    if let Some(journey) = beside {
        confirming::on_a_print_this_program_started(world);
        journey(world);
        return;
    }
    confirming::the_cross_product_walk(
        world,
        if depth == WALK {
            confirming::Reach::Every
        } else {
            confirming::Reach::Covering
        },
    );
    if depth == AGAINST_A_REAL_MACHINE {
        // The environment's own suite asserts a print is running, and this walk
        // took the machine through cancelling and starting to drive the
        // vocabulary. It is put back where the bring-up left it.
        world.wants(Reports::Printing);
        return;
    }
    formats::both_renderings_carry_the_same_fields(world);
    carrying::every_output_carries_only_what_the_answer_carried(world);
    materializing::an_image_is_a_path_that_opens(world);
    materializing::a_path_that_names_no_file_here_is_its_own_failure(world);
    failures::every_command_owes_its_failures(world);
    failures::no_mutating_command_runs_without_a_reason(world);
    failures::an_action_the_machine_refuses_is_answered_as_that(world);
    // Last, because it asks the machine to sit idle.
    failures::a_recovery_the_machine_refuses_is_answered_as_that(world);
}
