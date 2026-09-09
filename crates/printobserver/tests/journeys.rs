//! Every client command, against a real supervisor the server command started.
//!
//! # What each run is held to
//!
//! Every invocation of the cross-product carries all of it: the request the
//! server received is the one that command is supposed to make **carrying the
//! caller's own values**, the effect is confirmed by reading it back through
//! this same surface with those same values in the record, and the process
//! tree connected to no endpoint but the configured one. Three assertions on
//! the same run rather than one chosen invocation per command, because an
//! alternate input form that sent the wrong request would otherwise pass for
//! having been driven only against the destination.
//!
//! # Every assertion here is proven to bite
//!
//! `tests/journeys/tainting.rs` drives this program with one defect in it —
//! substituting a value, substituting a reason, connecting to the machine
//! directly, connecting only under one combination of options, and putting
//! image bytes in the output — through these same assertions, and each is
//! refused. A tier whose assertions cannot catch a violation is a tier nobody
//! has proven catches one.

#[path = "support/machine.rs"]
mod machine;
#[path = "support/proxy.rs"]
mod proxy;
#[path = "support/traced.rs"]
mod traced;
#[path = "support/walk.rs"]
mod walk;
#[path = "support/world.rs"]
mod world;

#[path = "journeys/answers.rs"]
mod answers;
#[path = "journeys/carrying.rs"]
mod carrying;
#[path = "journeys/confirming.rs"]
mod confirming;
#[path = "journeys/durations.rs"]
mod durations;
#[path = "journeys/failures.rs"]
mod failures;
#[path = "journeys/formats.rs"]
mod formats;
#[path = "journeys/materializing.rs"]
mod materializing;
#[path = "journeys/redaction.rs"]
mod redaction;
#[path = "journeys/running.rs"]
mod running;
#[path = "journeys/tainting.rs"]
mod tainting;

use world::World;

/// The whole tier, over one supervisor.
///
/// One world rather than one per journey: the supervisor is a process and the
/// store is a file, and starting sixteen of each to walk sixteen commands would
/// spend the tier's time on process startup rather than on what it proves. The
/// journeys are ordered because they share one print — pausing is valid from
/// printing and resuming from paused — and because the ones that take a print
/// somewhere else run last.
#[test]
fn every_client_command_is_proven_against_a_real_supervisor() {
    let world = World::open();

    confirming::the_cross_product_walk(&world);
    formats::both_renderings_carry_the_same_fields(&world);
    carrying::every_output_carries_only_what_the_answer_carried(&world);
    materializing::an_image_is_a_path_that_opens(&world);
    materializing::a_path_that_names_no_file_here_is_its_own_failure(&world);
    failures::every_command_owes_its_failures(&world);
    failures::no_mutating_command_runs_without_a_reason(&world);
    tainting::every_assertion_here_refuses_the_defect_it_is_about(&world);
    durations::every_adjustment_is_a_bounded_intervention(&world);
    redaction::no_run_of_the_walk_prints_the_credential(&world);
}
