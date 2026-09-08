//! The two places this crate holds a call at, so that its own tests can make
//! two callers contend rather than leaving the contention to the scheduler.
//!
//! A race a test provokes by starting two threads and hoping is a race the
//! scheduler may serialise by chance, and an implementation serialised by
//! chance passes a journey it should fail. So the store reaches a hold point at
//! the exact step a race would be decided at — where a settle call reads the
//! intervention's outcome, and where an image's bytes are about to be put in
//! place — and a test that has armed the point decides when each call proceeds.
//!
//! A point nothing armed costs one uncontended lock and holds nothing.

use std::sync::{Condvar, Mutex, MutexGuard, PoisonError};

use printobserver_types::InterventionOutcome;

/// What a point is holding, and what a test has let through.
#[derive(Debug, Default)]
struct Held {
    /// Whether calls reaching this point are held at all.
    armed: bool,
    /// The labels of the calls held here now.
    holding: Vec<String>,
    /// The labels a test has let through and which have not yet proceeded.
    released: Vec<String>,
}

/// A place a call is held at while a test decides when it proceeds.
#[derive(Debug, Default)]
pub struct HoldPoint {
    /// What is held here.
    held: Mutex<Held>,
    /// Signalled whenever a call arrives, is released, or proceeds.
    changed: Condvar,
}

/// The lock, taken whether or not a panicking test poisoned it.
fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

impl HoldPoint {
    /// Hold every call that reaches this point until it is released by label.
    pub fn arm(&self) {
        lock(&self.held).armed = true;
    }

    /// Stop holding calls, and let every call held here now proceed.
    pub fn disarm(&self) {
        let mut held = lock(&self.held);
        held.armed = false;
        let mut holding = held.holding.clone();
        held.released.append(&mut holding);
        self.changed.notify_all();
    }

    /// Block until a call carrying each of these labels is held here.
    pub fn await_holding(&self, labels: &[&str]) {
        let mut held = lock(&self.held);
        while !labels
            .iter()
            .all(|label| held.holding.iter().any(|found| found == label))
        {
            held = self
                .changed
                .wait(held)
                .unwrap_or_else(PoisonError::into_inner);
        }
    }

    /// Let the call carrying this label proceed.
    pub fn release(&self, label: &str) {
        lock(&self.held).released.push(label.to_owned());
        self.changed.notify_all();
    }

    /// Reached by the store when a call gets to this point.
    pub(crate) fn reach(&self, label: &str) {
        let mut held = lock(&self.held);
        if !held.armed {
            return;
        }
        held.holding.push(label.to_owned());
        self.changed.notify_all();
        loop {
            if let Some(index) = held.released.iter().position(|found| found == label) {
                held.released.remove(index);
                if let Some(index) = held.holding.iter().position(|found| found == label) {
                    held.holding.remove(index);
                }
                self.changed.notify_all();
                return;
            }
            held = self
                .changed
                .wait(held)
                .unwrap_or_else(PoisonError::into_inner);
        }
    }
}

/// The points a store of this crate holds calls at.
#[derive(Debug, Default)]
pub struct HoldPoints {
    /// Where a settle call has read the intervention's outcome and not yet
    /// written its own.
    settle: HoldPoint,
    /// Where an image's bytes are written and not yet in place.
    image_write: HoldPoint,
}

impl HoldPoints {
    /// Where a settle call has read the intervention's outcome.
    ///
    /// Calls are held here under the label of the outcome each is settling,
    /// which is what lets a test release two contending calls in a chosen
    /// order rather than in whichever order they happened to arrive.
    #[must_use]
    pub const fn settle(&self) -> &HoldPoint {
        &self.settle
    }

    /// Where an image's bytes are written and not yet in place.
    ///
    /// Calls are held here under the digest of the bytes being stored.
    #[must_use]
    pub const fn image_write(&self) -> &HoldPoint {
        &self.image_write
    }
}

/// The label a settle call settling this outcome is held under.
///
/// A test releasing two contending calls names them by this rather than by the
/// order they arrived in, which no test controls.
#[must_use]
pub fn settle_label(outcome: &InterventionOutcome) -> &'static str {
    match outcome {
        InterventionOutcome::StillActive => "still_active",
        InterventionOutcome::Restored => "restored",
        InterventionOutcome::RestoreUnavailable => "restore_unavailable",
        InterventionOutcome::RestoreFailed { .. } => "restore_failed",
        InterventionOutcome::Superseded { .. } => "superseded",
    }
}
