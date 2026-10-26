//! The signals a store raises once a write that stored an event has committed.

use std::cell::Cell;
use std::sync::{Mutex, PoisonError};

use printobserver_core::store::StoredSignal;

/// Every signal handed to one store, raised in the order they were handed.
#[derive(Default)]
pub struct Signals {
    /// The signals.
    held: Mutex<Vec<StoredSignal>>,
}

impl core::fmt::Debug for Signals {
    fn fmt(&self, formatter: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        formatter.debug_struct("Signals").finish_non_exhaustive()
    }
}

impl Signals {
    /// Raise one more signal from now on.
    pub fn add(&self, signal: StoredSignal) {
        self.held
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .push(signal);
    }

    /// Raise every signal.
    ///
    /// The list is copied out first, so a signal that reads the store does not
    /// do so while this lock is held.
    pub fn raise(&self) {
        let held: Vec<StoredSignal> = self
            .held
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone();
        for signal in held {
            signal();
        }
    }
}

thread_local! {
    /// Whether this thread appended an event in the unit of work it is in.
    ///
    /// A unit of work runs on one thread from start to commit, and the append
    /// is made deep inside it — in a transaction any aggregate's method may
    /// open — so the unit's end asks this rather than every method saying so.
    static APPENDED: Cell<bool> = const { Cell::new(false) };
}

/// Note that this thread's unit of work appended an event.
pub fn note_appended() {
    APPENDED.with(|appended| appended.set(true));
}

/// Whether this thread's unit of work appended an event, forgetting it.
pub fn take_appended() -> bool {
    APPENDED.with(|appended| appended.replace(false))
}
