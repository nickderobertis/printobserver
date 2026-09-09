//! The whole loop, against a real `OctoPrint` and a real `OneHarness`.
//!
//! This binary is the integration tier and it is deliberately not part of `just
//! check`: it drives the instance `just octoprint-up` started, with its virtual
//! printer and its hold print, which is why it is a continuous-integration job
//! of its own.
//!
//! Nothing here stands in for a layer. The printer is the `OctoPrint` adapter
//! against the scripted instance; the store is the durable one; the vision port
//! is the `Obico` adapter fetching from a host this tier serves; and the
//! supervising agent is the real `OneHarness` engine with only the paid
//! provider process replaced, by the deterministic responder `OneHarness`
//! itself publishes. What is exercised is the server: its ingress, its API, the
//! policy every action passes, its record, and its restart.
//!
//! # Where each operation's effect is confirmed
//!
//! Six of the ten actions have a read-back at the machine — start, pause,
//! resume, cancel, tool target and bed target — and are confirmed by a status
//! read afterwards. Three of them have none: `OctoPrint` reports neither an
//! applied feedrate factor nor an applied flowrate factor nor a fan setting.
//! For those the observable is that the **real instance accepted the request**,
//! which it answers only for a request it understood: the action record this
//! server answers carries the execution outcome the instance's own answer
//! produced, and a request `OctoPrint` refused reaches this tier as a failure
//! rather than as a success. What that request looked like in `OctoPrint`'s own
//! vocabulary is proven where that vocabulary is allowed to be named — the
//! adapter's own integration tier, which forwards every request through a
//! recording proxy — because no crate but that adapter may construct an
//! `OctoPrint` request, this one included.
//!
//! # It puts the hold print back
//!
//! `just test-integration` runs this tier and `octoprint-env`'s own suite
//! against the one instance `just octoprint-up` started, and that suite asserts
//! the hold print is there to be acted on. This walk cancels it, so this walk
//! starts it again — at both ends, so that a second run against one environment
//! finds it where the first left it.

#[path = "support/http_host.rs"]
mod http_host;

#[path = "instance/composition.rs"]
mod composition;
#[path = "instance/scripted.rs"]
mod scripted;
#[path = "instance/waiting.rs"]
mod waiting;

#[path = "live/loop.rs"]
mod whole_loop;

/// The whole loop, in the one order a shared machine admits.
#[test]
fn the_whole_loop_is_proven_against_a_real_octoprint_and_a_real_harness() {
    let instance = scripted::scripted();
    assert!(
        instance.printing,
        "the scripted environment did not start a print, and this walk needs one"
    );

    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .expect("a runtime");
    runtime.block_on(whole_loop::walk(&instance));
}
