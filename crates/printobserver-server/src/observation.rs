//! The driver that gives every active print its periodic observation.
//!
//! Core decides which prints are owed an observation and writes each one down
//! ([`Supervisor::observe_active_prints`]); this is what asks it to, once per
//! [`CoreConfig::observation_interval`](printobserver_core::CoreConfig), and
//! runs each turn an observation claimed. The first observation is one
//! interval after the server starts rather than at the start, so a restart is
//! not itself a reason to look.
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
use tokio::time::{Instant, MissedTickBehavior};

/// Start observing every active print once per the supervisor's interval.
pub fn start(supervisor: &Arc<Supervisor>) -> JoinHandle<()> {
    let interval = supervisor.config().observation_interval;
    let weak: Weak<Supervisor> = Arc::downgrade(supervisor);
    tokio::spawn(async move {
        let mut ticks = tokio::time::interval_at(Instant::now() + interval, interval);
        // An observation that took longer than the interval is followed by the
        // next one an interval later, not by a burst making up the ones missed.
        ticks.set_missed_tick_behavior(MissedTickBehavior::Delay);
        loop {
            ticks.tick().await;
            let Some(supervisor) = weak.upgrade() else {
                return;
            };
            let observing = Arc::clone(&supervisor);
            let observed = tokio::task::spawn_blocking(move || {
                printobserver_core::block_on(observing.observe_active_prints())
            })
            .await;
            debug_assert!(observed.is_ok(), "one periodic observation panicked");
            let Ok(Ok(turns)) = observed else {
                continue;
            };
            for turn in turns {
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
