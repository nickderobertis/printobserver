//! The same walk, against a real `OctoPrint` rather than a socket.
//!
//! This binary is the printer integration tier and is deliberately not part of
//! `just check`: it drives the instance `just octoprint-up` started, with its
//! virtual printer and its hold print, which is why it is a continuous-
//! integration job of its own.
//!
//! Nothing here is a second walk. It is the cross-product `tests/journeys.rs`
//! runs, over a world whose printer port reaches a real machine — so the
//! request the server received, the caller\'s own values in it, the effect read
//! back through this same surface and the endpoints the process tree reached
//! are all asserted against the thing this program is actually for. Where the
//! fast world is told what to report, this one is driven there by the actions
//! that get it there, and then waited for.
//!
//! # It puts the hold print back
//!
//! `just test-integration` runs this beside `octoprint-env`\'s own suite, which
//! asserts the hold print is there to be acted on. This walk cancels it and
//! starts it again, so a second run against one environment finds it where the
//! first left it.

#[path = "support/announced.rs"]
mod announced;
#[path = "support/host.rs"]
mod host;
#[path = "support/machine.rs"]
mod machine;
#[path = "support/proxy.rs"]
mod proxy;
#[path = "support/scripted.rs"]
mod scripted;
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
#[path = "journeys/closing.rs"]
mod closing;
#[path = "journeys/confirming.rs"]
mod confirming;
#[path = "journeys/documented.rs"]
mod documented;
#[path = "journeys/durations.rs"]
mod durations;
#[path = "journeys/failures.rs"]
mod failures;
#[path = "journeys/filament.rs"]
mod filament;
#[path = "journeys/formats.rs"]
mod formats;
#[path = "journeys/materializing.rs"]
mod materializing;
#[path = "journeys/recovering.rs"]
mod recovering;
#[path = "journeys/redaction.rs"]
mod redaction;
#[path = "journeys/running.rs"]
mod running;
#[path = "journeys/tainting.rs"]
mod tainting;
#[path = "journeys/tier.rs"]
mod tier;
#[path = "support/unreachable.rs"]
mod unreachable;
#[path = "journeys/upgrading.rs"]
mod upgrading;

use world::World;

/// Every client command, against the machine beside the printer.
#[test]
fn every_client_command_is_proven_against_a_real_octoprint() {
    let world = World::open(world::SCRIPTED);

    tier::run(&world, tier::AGAINST_A_REAL_MACHINE);
}

/// A spool too short for a file is refused with its figures, overridden, and
/// charged what the cancelled print fed, against a real `OctoPrint`.
#[test]
fn a_short_spool_is_refused_overridden_and_charged_against_a_real_octoprint() {
    let world = World::open(world::SCRIPTED);

    filament::a_short_spool_is_refused_overridden_and_charged(&world);
}

/// A supervisor started over the store a published release wrote migrates it,
/// says what it changed, and reads every event that release wrote, with a real
/// `OctoPrint` on the far side of its printer port.
#[test]
fn a_store_a_published_release_wrote_is_migrated_and_still_reads_beside_a_real_octoprint() {
    upgrading::a_store_0_4_3_wrote_is_migrated_and_every_event_still_reads(world::SCRIPTED);
}

/// The documented operator workflow, carried out from the documentation alone,
/// against a real supervisor backed by a real `OctoPrint`.
///
/// The same walk the fast tier runs, over the machine beside the printer: a
/// reader who starts at the skill and follows only what it and the documents it
/// links to provide reads the context, opens the image, decides inside the
/// bounds that read reported, composes a second request out of a rejection's own
/// fields, records what it saw, and escalates.
#[test]
fn the_documented_operator_workflow_is_carried_out_from_the_documentation_alone() {
    let world = World::open(world::SCRIPTED);

    documented::walk(&world);
}

/// After a print interrupted part-way, every recovery command against a real
/// `OctoPrint`, each carried out or refused as Z trust and the part's height
/// rule, the machine following what was carried out and nothing else.
#[test]
fn every_recovery_command_is_proven_after_an_interrupted_print() {
    recovering::after_an_interrupted_print();
}

/// A print printobserver started and ran to its end leaves Z trusted and the
/// moves over the part allowed by the clearance measured.
#[test]
fn a_finished_print_printobserver_started_leaves_z_trusted() {
    recovering::after_a_print_printobserver_started_and_finished();
}

/// A recovery under a paused print makes it not resumable, and a paused print
/// no recovery moved resumes.
#[test]
fn a_recovery_under_a_paused_print_makes_its_resume_refused() {
    recovering::a_recovery_under_a_paused_print_makes_it_not_resumable();
}

/// Where away from the bed is down the axis, a lift moves Z down.
#[test]
fn a_lift_moves_away_from_the_bed_down_the_axis() {
    recovering::a_lift_moves_away_from_the_bed_whichever_way_that_is();
}
