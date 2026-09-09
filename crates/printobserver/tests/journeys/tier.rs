//! The tier itself, over whichever world it was given.
//!
//! One entry rather than one per journey, because the journeys share a print:
//! pausing is valid from printing and resuming from paused, so the order they
//! run in is what a print actually goes through, and the ones that take a print
//! somewhere else run last.
//!
//! # Two depths, one walk
//!
//! [`WHOLE`] is what the fast tier runs, over a world whose printer port
//! reaches a socket. [`AGAINST_A_REAL_MACHINE`] is what the printer
//! integration tier runs: the same cross-product, over the `OctoPrint` `just
//! octoprint-up` provisioned. What the second leaves out is everything that is
//! about **this program** rather than about the machine — the renderings, the
//! failure classes, the credential — because those are proven once against a
//! world that can be driven deterministically rather than twice against one
//! that takes its own time.

use crate::machine::Reports;
use crate::world::World;

use super::{
    carrying, confirming, durations, failures, formats, materializing, redaction, tainting,
};

/// Everything.
pub const WHOLE: &str = "whole";

/// The cross-product walk alone, which is what a real machine adds.
pub const AGAINST_A_REAL_MACHINE: &str = "against-a-real-machine";

/// Run the tier over one world.
///
/// # Panics
///
/// Panics when the depth is not one of the two this tier runs at.
pub fn run(world: &World, depth: &str) {
    assert!(
        depth == WHOLE || depth == AGAINST_A_REAL_MACHINE,
        "there is no `{depth}` depth to run this tier at"
    );
    confirming::the_cross_product_walk(world);
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
    tainting::every_assertion_here_refuses_the_defect_it_is_about(world);
    durations::every_adjustment_is_a_bounded_intervention(world);
    redaction::no_run_of_the_walk_prints_the_credential(world);
}
