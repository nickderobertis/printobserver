//! What reaches a turn while it is running.
//!
//! One agent per print at a time. An event that arrives for a print whose turn
//! is still running does not start a second turn: it is written down like any
//! other and then handed to the turn already running, which takes it the next
//! time it looks at the print. Whatever that turn never took is handed, when it
//! returns, to one more turn in the same session — so nothing that arrived is
//! lost and no two agents ever disagree about one machine.
//!
//! This is a blocking structure on purpose. A look waits on it for up to a
//! minute and a half, and the caller is expected to wait on a thread of its
//! own rather than on an asynchronous worker, exactly as the harness itself is
//! driven.

use std::collections::BTreeMap;
use std::sync::{Condvar, Mutex};
use std::time::{Duration, Instant};

use printobserver_types::{EventRecord, PrintId};
use printobserver_vision_api::Detection;

/// One event waiting for a running turn, with what the detector did about it.
#[derive(Debug, Clone, PartialEq)]
pub struct Arrival {
    /// The event, as it was written down, carrying its image.
    pub event: EventRecord,
    /// What the detector did, when the event is one of its detections.
    pub detection: Option<Detection>,
}

/// One print's inbox.
#[derive(Debug, Default)]
struct Inbox {
    /// Whether a turn is running for this print.
    busy: bool,
    /// What arrived while it was running and it has not taken, oldest first.
    pending: Vec<Arrival>,
}

/// Every print's inbox, and the signal an arrival raises.
#[derive(Debug, Default)]
pub struct Inboxes {
    /// One inbox per print that has had a turn.
    state: Mutex<BTreeMap<PrintId, Inbox>>,
    /// Raised whenever something is handed to a running turn.
    arrived: Condvar,
}

impl Inboxes {
    /// Claim the print's turn for this arrival, or hand it to the turn already
    /// running.
    ///
    /// Answers `true` when the caller now holds the print's turn and must run
    /// it, and `false` when the arrival was handed to a turn already running.
    pub fn claim_or_hand_over(&self, print_id: PrintId, arrival: Arrival) -> bool {
        let mut state = self.state.lock().expect("the inboxes are not poisoned");
        let inbox = state.entry(print_id).or_default();
        if inbox.busy {
            inbox.pending.push(arrival);
            drop(state);
            self.arrived.notify_all();
            return false;
        }
        inbox.busy = true;
        true
    }

    /// What the turn that just returned never took, or — when it took
    /// everything — the print's turn released.
    pub fn next_or_release(&self, print_id: PrintId) -> Option<Vec<Arrival>> {
        let mut state = self.state.lock().expect("the inboxes are not poisoned");
        let inbox = state.entry(print_id).or_default();
        if inbox.pending.is_empty() {
            inbox.busy = false;
            return None;
        }
        Some(core::mem::take(&mut inbox.pending))
    }

    /// Release the print's turn, dropping anything still waiting for it.
    ///
    /// For a print that has ended: nothing waiting for it can be acted on.
    pub fn release(&self, print_id: PrintId) {
        let mut state = self.state.lock().expect("the inboxes are not poisoned");
        state.remove(&print_id);
    }

    /// Wait up to `wait` for anything to be handed to the print's running
    /// turn, and take it.
    ///
    /// Answers at once when something is already waiting, and answers nothing
    /// after the whole wait when nothing arrives or no turn is running.
    pub fn wait_and_take(&self, print_id: PrintId, wait: Duration) -> Vec<Arrival> {
        let deadline = Instant::now() + wait;
        let mut state = self.state.lock().expect("the inboxes are not poisoned");
        loop {
            if let Some(inbox) = state.get_mut(&print_id)
                && !inbox.pending.is_empty()
            {
                return core::mem::take(&mut inbox.pending);
            }
            let now = Instant::now();
            if now >= deadline {
                return Vec::new();
            }
            state = self
                .arrived
                .wait_timeout(state, deadline - now)
                .expect("the inboxes are not poisoned")
                .0;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{Arrival, Inboxes};
    use printobserver_types::{
        EventBody, EventId, EventKind, EventRecord, EventSource, PrintId, Timestamp,
    };
    use std::time::Duration;

    /// One arrival, of no kind in particular.
    fn arrival() -> Arrival {
        Arrival {
            event: EventRecord {
                id: EventId::new(),
                print_id: None,
                source: EventSource::new("obico"),
                received_at: Timestamp::now(),
                image: None,
                body: EventBody {
                    kind: EventKind::new("obico_failure_alert").expect("a kind"),
                    payload: printobserver_types::serde_json::Value::Null,
                },
                raw: None,
            },
            detection: None,
        }
    }

    /// A second arrival while a turn runs is handed over rather than claimed,
    /// and is what the turn's next look takes.
    #[test]
    fn an_arrival_during_a_turn_is_handed_to_it() {
        let inboxes = Inboxes::default();
        let print = PrintId::new();
        assert!(inboxes.claim_or_hand_over(print, arrival()));
        assert!(!inboxes.claim_or_hand_over(print, arrival()));
        assert_eq!(inboxes.wait_and_take(print, Duration::ZERO).len(), 1);
        assert_eq!(inboxes.next_or_release(print), None);
        assert!(inboxes.claim_or_hand_over(print, arrival()));
    }

    /// What a turn never took is handed to the next turn, not dropped.
    #[test]
    fn what_a_turn_never_took_is_handed_to_the_next() {
        let inboxes = Inboxes::default();
        let print = PrintId::new();
        assert!(inboxes.claim_or_hand_over(print, arrival()));
        assert!(!inboxes.claim_or_hand_over(print, arrival()));
        assert_eq!(inboxes.next_or_release(print).map(|left| left.len()), Some(1));
        assert!(!inboxes.claim_or_hand_over(print, arrival()));
    }
}
