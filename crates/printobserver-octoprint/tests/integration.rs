//! The printer port, proven against a real `OctoPrint`.
//!
//! This binary is the integration tier and it is deliberately not part of `just
//! check`: it drives the instance `just octoprint-up` started, with its virtual
//! printer and its hold print, which is why it is a continuous-integration job
//! of its own.
//!
//! It begins and ends by putting the hold print back. Beginning that way makes
//! the tier re-runnable against one instance rather than only against a freshly
//! started one. Ending that way is what keeps the *other* tier on the same
//! machine true: `just test-integration` runs this binary and `octoprint-env`'s
//! own suite against the one instance `just octoprint-up` started, and that
//! suite asserts the hold print is there to be acted on. This walk cancels it,
//! so this walk puts it back. The two tiers never run at once — `nx.json`
//! declares `test-integration` unparallelisable, because there is one machine.
//!
//! It is one test rather than several, and that is the point: the journeys act
//! on one shared machine — one of them cancels the print another is reading —
//! so the order they run in is part of what is being proven, and a runner free
//! to interleave them would be proving something else. Reading comes first,
//! while the environment's own print is still running; the failure walk needs
//! that same running print for its state conflict; acting comes last, because it
//! cancels, starts and cancels again.

#[path = "support/block_on.rs"]
mod block_on;
#[path = "support/numbers.rs"]
mod numbers;
#[path = "support/received.rs"]
mod received;

#[path = "instance/env.rs"]
mod env;
#[path = "instance/proxy.rs"]
mod proxy;
#[path = "instance/raw.rs"]
mod raw;
#[path = "instance/wait.rs"]
mod wait;

#[path = "live/acting.rs"]
mod acting;
#[path = "live/failing.rs"]
mod failing;
#[path = "live/reading.rs"]
mod reading;

/// Reading, pinning, failing and acting, in the one order a shared machine
/// admits.
#[test]
fn the_printer_port_is_proven_against_a_real_octoprint() {
    let instance = env::scripted();
    assert!(
        instance.printing,
        "the scripted environment did not start a print, and every journey here needs one"
    );
    assert!(
        instance.hold_seconds > 0,
        "the scripted environment states no hold"
    );

    acting::hold_the_print_running(&instance);
    reading::walk(&instance);
    reading::pin_completion(&instance);
    failing::walk(&instance);
    acting::walk(&instance);
    acting::hold_the_print_running(&instance);
}
