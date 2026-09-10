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
//!
//! What is on the far side of the printer port here is a socket rather than a
//! printer, which is the one thing this tier does not have for real and is what
//! lets it run in every gate. `tests/integration.rs` runs the same walk over
//! the `OctoPrint` `just octoprint-up` provisioned.

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
#[path = "journeys/confirming.rs"]
mod confirming;
#[path = "journeys/documented.rs"]
mod documented;
#[path = "journeys/documenting.rs"]
mod documenting;
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
#[path = "journeys/tier.rs"]
mod tier;

use printobserver_server as server_assets;

use world::World;

// journey: every-command-against-a-real-supervisor
/// The whole tier, over one supervisor.
///
/// One world rather than one per journey: the supervisor is a process and the
/// store is a file, and starting sixteen of each to walk sixteen commands would
/// spend the tier's time on process startup rather than on what it proves.
#[test]
fn every_client_command_is_proven_against_a_real_supervisor() {
    let world = World::open(world::STOOD_IN);

    tier::run(&world, tier::WHOLE);
}

// journey: documented-examples-print-what-they-show
/// Every command example the reference documents show, run against a real
/// supervisor and compared with what the document shows beside it.
///
/// The falsifying half runs beside it: the same walk over documentation
/// carrying an altered output and an example this check has no way to run, both
/// refused. A documentation check that accepted whatever it was shown would be
/// worse than none, because it would read as proof.
#[test]
fn every_documented_example_prints_what_the_document_shows() {
    let world = World::open(world::STOOD_IN);

    documenting::accepts_the_committed_documentation(&world);
    documenting::refuses_documentation_that_has_drifted();
}

/// One supervision turn, carried out from the committed documentation alone,
/// against a real supervisor over a stood-in machine.
///
/// The printer tier runs the same walk against the `OctoPrint` `just
/// octoprint-up` provisioned, which is where the journey inventory's entry for
/// it points; this is the same walk in every gate, so a change that made the
/// documentation unfollowable is refused before the printer tier runs.
#[test]
fn one_supervision_turn_is_carried_out_from_the_documentation_alone() {
    let world = World::open(world::STOOD_IN);

    documented::walk(&world);
}

// journey: a-supervision-turn-from-the-installed-assets-alone
/// One supervision turn from what the running server materialized, and nothing
/// else.
///
/// The skill an installed program writes links out for everything it does not
/// say itself, and the agent that reads it stands in a state directory rather
/// than in a checkout. This copies what the server wrote into a directory
/// carrying nothing else, opens every link the skill carries there, and carries
/// the whole turn out of that copy.
#[test]
fn the_installed_assets_carry_one_supervision_turn() {
    let world = World::open(world::STOOD_IN);

    documenting::the_installed_assets_carry_the_turn(&world);
}
