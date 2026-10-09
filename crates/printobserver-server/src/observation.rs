//! The driver that gives every active print its periodic observation.
//!
//! Core decides which prints are owed an observation and writes each one down
//! ([`Supervisor::observe_active_prints`]); this is what asks it to, once per
//! [`CoreConfig::observation_interval`](printobserver_core::CoreConfig), and
//! runs each turn an observation claimed. Each round waits one interval after
//! the last one finished, as the expiry driver sleeps between its sweeps: the
//! first is one interval after the server starts rather than at the start, so a
//! restart is not itself a reason to look, and a slow round is followed by an
//! interval rather than by a burst of rounds making up for it.
//!
//! # Why it lives in the server rather than beside the expiry driver
//!
//! Core's expiry driver is a thread of its own that polls with no reactor, and
//! the printer port allows that. An observation takes the camera's frame
//! through the vision port, whose HTTP client needs Tokio's reactor, so the
//! observation is driven from this server's runtime — written down and run on
//! blocking threads of it, exactly as the ingress worker writes down an alert
//! and runs the turn it claimed.
//!
//! # It holds the supervisor weakly
//!
//! Like the expiry driver, it holds a weak reference, so it stops of its own
//! accord once the last handle to the supervisor is dropped; a server that is
//! stopped also aborts it.

use std::sync::{Arc, Weak};

use printobserver_core::Supervisor;
use tokio::task::JoinHandle;

/// Start observing every active print once per the supervisor's interval.
pub fn start(supervisor: &Arc<Supervisor>) -> JoinHandle<()> {
    let interval = supervisor.config().observation_interval;
    let weak: Weak<Supervisor> = Arc::downgrade(supervisor);
    tokio::spawn(async move {
        loop {
            tokio::time::sleep(interval).await;
            let Some(supervisor) = weak.upgrade() else {
                return;
            };
            let observing = Arc::clone(&supervisor);
            let observed = tokio::task::spawn_blocking(move || {
                printobserver_core::block_on(observing.observe_active_prints())
            })
            .await;
            debug_assert!(observed.is_ok(), "one periodic observation panicked");
            // A round the store refused claimed nothing, and a print whose
            // observation it refused claimed no turn: the next round tries
            // again, and there is nobody to answer either to here.
            let Ok(Ok(observed)) = observed else {
                continue;
            };
            for turn in observed.turns {
                let runner = Arc::clone(&supervisor);
                tokio::spawn(async move {
                    let supervised = tokio::task::spawn_blocking(move || {
                        let _ = printobserver_core::block_on(runner.run_supervision(turn));
                    })
                    .await;
                    debug_assert!(
                        supervised.is_ok(),
                        "the supervision of one observation panicked"
                    );
                });
            }
        }
    })
}
