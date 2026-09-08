//! One supervision turn per print at a time.
//!
//! An event arriving while a turn for the same print is still running is queued
//! behind it rather than opening a second concurrent conversation: two turns
//! talking to one session at once is two agents disagreeing about one machine.
//!
//! The queue is per print rather than global, because two prints are two
//! conversations and nothing is served by serializing them against each other.
//! It is an asynchronous lock rather than a `std` one because a guard is held
//! across an await and has to be sendable; the wakers are what make a queued
//! caller resume the moment the turn ahead of it returns rather than on a poll.

use core::future::Future;
use core::pin::Pin;
use core::task::{Context, Poll, Waker};
use std::collections::BTreeMap;
use std::sync::Mutex;

use printobserver_types::PrintId;

/// One print's queue: whether a turn holds it, and who is waiting.
#[derive(Debug, Default)]
struct Queue {
    /// Whether a turn is running for this print.
    held: bool,
    /// The callers waiting for it, in the order they arrived.
    waiting: Vec<Waker>,
}

/// The per-print turn locks.
#[derive(Debug, Default)]
pub struct TurnLocks {
    /// One queue per print that has ever taken a turn.
    queues: Mutex<BTreeMap<PrintId, Queue>>,
}

impl TurnLocks {
    /// Wait until no turn is running for this print, then hold it.
    pub const fn acquire(&self, print_id: PrintId) -> Acquire<'_> {
        Acquire {
            locks: self,
            print_id,
        }
    }

    /// Release one print's turn and wake whoever is queued behind it.
    fn release(&self, print_id: PrintId) {
        let woken = {
            let mut queues = self.queues.lock().expect("the turn locks are not poisoned");
            let queue = queues.entry(print_id).or_default();
            queue.held = false;
            core::mem::take(&mut queue.waiting)
        };
        for waker in woken {
            waker.wake();
        }
    }
}

/// The future that waits for one print's turn.
#[derive(Debug)]
pub struct Acquire<'a> {
    /// The locks being waited on.
    locks: &'a TurnLocks,
    /// The print whose turn is being waited for.
    print_id: PrintId,
}

impl<'a> Future for Acquire<'a> {
    type Output = TurnGuard<'a>;

    fn poll(self: Pin<&mut Self>, context: &mut Context<'_>) -> Poll<Self::Output> {
        let mut queues = self
            .locks
            .queues
            .lock()
            .expect("the turn locks are not poisoned");
        let queue = queues.entry(self.print_id).or_default();
        if queue.held {
            queue.waiting.push(context.waker().clone());
            return Poll::Pending;
        }
        queue.held = true;
        drop(queues);
        Poll::Ready(TurnGuard {
            locks: self.locks,
            print_id: self.print_id,
        })
    }
}

/// The turn one print is running, released when this is dropped.
#[derive(Debug)]
pub struct TurnGuard<'a> {
    /// The locks this guard came from.
    locks: &'a TurnLocks,
    /// The print whose turn is being held.
    print_id: PrintId,
}

impl Drop for TurnGuard<'_> {
    fn drop(&mut self) {
        self.locks.release(self.print_id);
    }
}
