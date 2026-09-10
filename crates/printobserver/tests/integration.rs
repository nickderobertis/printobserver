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

use world::World;

// journey: every-command-against-a-real-printer
/// Every client command, against the machine beside the printer.
#[test]
fn every_client_command_is_proven_against_a_real_octoprint() {
    let world = World::open(world::SCRIPTED);

    tier::run(&world, tier::AGAINST_A_REAL_MACHINE);
}

// journey: a-supervision-turn-from-the-documentation-alone
/// One supervision turn, carried out from the committed documentation alone,
/// against a real supervisor backed by a real `OctoPrint`.
///
/// The same walk the fast tier runs, over the machine beside the printer: a
/// reader who starts at the skill and follows only what it and the documents it
/// links to provide reads the context, opens the image, decides inside the
/// bounds that read reported, composes a second request out of a rejection's own
/// fields, records what it saw, and escalates — and six copies of that
/// documentation, each missing one of those, cannot.
#[test]
fn one_supervision_turn_is_carried_out_from_the_documentation_alone() {
    let world = World::open(world::SCRIPTED);

    documented::walk(&world);
}
