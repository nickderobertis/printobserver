//! A wait that holds no thread of its caller's, for a future that must pause.
//!
//! Core declares no runtime, so it has no timer to borrow. A start that waits
//! for `OctoPrint`'s analysis of its file re-reads the file at an interval, and
//! sleeping the calling thread between reads would hold whatever executor is
//! driving the request for the whole of the wait. [`Delay`] instead parks
//! nothing: it hands its waker to a thread of its own that sleeps and wakes it,
//! so it completes on any executor, [`block_on`](crate::block_on) included.
//!
//! How long the wait as a whole lasts is never read from here: that is the
//! injected [`Clock`](crate::Clock)'s, so a test drives it by moving the clock.

use core::future::Future;
use core::pin::Pin;
use core::task::{Context, Poll};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

/// A future that is ready once a duration has passed.
#[derive(Debug)]
pub(crate) struct Delay {
    /// How long to wait.
    duration: Duration,
    /// Set by the sleeping thread once the duration has passed; absent until
    /// the first poll starts it.
    elapsed: Option<Arc<AtomicBool>>,
}

impl Delay {
    /// A wait of this long, started when it is first polled.
    pub(crate) const fn new(duration: Duration) -> Self {
        Self {
            duration,
            elapsed: None,
        }
    }
}

impl Future for Delay {
    type Output = ();

    fn poll(mut self: Pin<&mut Self>, context: &mut Context<'_>) -> Poll<()> {
        if let Some(elapsed) = &self.elapsed {
            return if elapsed.load(Ordering::Acquire) {
                Poll::Ready(())
            } else {
                Poll::Pending
            };
        }
        let elapsed = Arc::new(AtomicBool::new(false));
        let flag = Arc::clone(&elapsed);
        let waker = context.waker().clone();
        let duration = self.duration;
        let started = std::thread::Builder::new()
            .name("printobserver-delay".to_owned())
            .spawn(move || {
                std::thread::sleep(duration);
                flag.store(true, Ordering::Release);
                waker.wake();
            });
        if started.is_err() {
            // A thread the system will not start is a wait not taken: the
            // caller re-reads at once, which is slower to give up than a wait
            // but never wrong.
            return Poll::Ready(());
        }
        self.elapsed = Some(elapsed);
        Poll::Pending
    }
}

#[cfg(test)]
mod tests {
    use super::Delay;
    use std::time::{Duration, Instant};

    /// A delay waits at least as long as it was asked to, and then completes.
    #[test]
    fn a_delay_completes_after_its_duration() {
        let started = Instant::now();
        crate::block_on(Delay::new(Duration::from_millis(20)));
        assert!(started.elapsed() >= Duration::from_millis(20));
    }
}
