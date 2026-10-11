//! A wait that holds no thread of its caller's, for a future that must pause.
//!
//! Core declares no runtime, so it has no timer to borrow. A start that waits
//! for `OctoPrint`'s analysis of its file re-reads the file at an interval, and
//! sleeping the calling thread between reads would hold whatever executor is
//! driving the request for the whole of the wait. [`Delay`] instead parks
//! nothing: it hands its waker to a thread of its own that sleeps and wakes it,
//! so it completes on any executor, [`block_on`](crate::block_on) included. A
//! system that will not start that thread answers the operating system's own
//! error, and the caller gives up waiting rather than spinning.
//!
//! How long the wait as a whole lasts is never read from here: that is the
//! injected [`Clock`](crate::Clock)'s, so a test drives it by moving the clock.

use core::future::Future;
use core::pin::Pin;
use core::task::{Context, Poll, Waker};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

/// What the sleeping thread and the future share: whether the wait is over,
/// and the waker of the latest poll, which is the one to wake.
#[derive(Debug, Default)]
struct Shared {
    /// Whether the duration has passed.
    elapsed: bool,
    /// The waker the future was last polled with.
    waker: Option<Waker>,
}

/// A future that is ready once a duration has passed.
#[derive(Debug)]
pub(crate) struct Delay {
    /// How long to wait.
    duration: Duration,
    /// What it shares with its sleeping thread; absent until the first poll
    /// starts that thread.
    shared: Option<Arc<Mutex<Shared>>>,
}

impl Delay {
    /// A wait of this long, started when it is first polled.
    pub(crate) const fn new(duration: Duration) -> Self {
        Self {
            duration,
            shared: None,
        }
    }
}

impl Future for Delay {
    type Output = std::io::Result<()>;

    fn poll(mut self: Pin<&mut Self>, context: &mut Context<'_>) -> Poll<Self::Output> {
        if let Some(shared) = &self.shared {
            let mut held = shared.lock().unwrap_or_else(PoisonError::into_inner);
            if held.elapsed {
                return Poll::Ready(Ok(()));
            }
            // An executor may poll again with a different waker; the one to
            // wake is always the latest.
            held.waker = Some(context.waker().clone());
            return Poll::Pending;
        }
        let shared = Arc::new(Mutex::new(Shared {
            elapsed: false,
            waker: Some(context.waker().clone()),
        }));
        let sleeper = Arc::clone(&shared);
        let duration = self.duration;
        let started = std::thread::Builder::new()
            .name("printobserver-delay".to_owned())
            .spawn(move || {
                std::thread::sleep(duration);
                let waker = {
                    let mut held = sleeper.lock().unwrap_or_else(PoisonError::into_inner);
                    held.elapsed = true;
                    held.waker.take()
                };
                if let Some(waker) = waker {
                    waker.wake();
                }
            });
        if let Err(error) = started {
            return Poll::Ready(Err(error));
        }
        self.shared = Some(shared);
        Poll::Pending
    }
}

#[cfg(test)]
mod tests {
    use super::Delay;
    use core::future::Future;
    use core::pin::Pin;
    use core::task::{Context, Poll, Waker};
    use std::sync::Arc;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::task::Wake;
    use std::time::{Duration, Instant};

    /// A waker that remembers whether it was woken.
    #[derive(Default)]
    struct Flag(AtomicBool);

    impl Wake for Flag {
        fn wake(self: Arc<Self>) {
            self.0.store(true, Ordering::SeqCst);
        }
    }

    /// A delay waits at least as long as it was asked to, and then completes.
    #[test]
    fn a_delay_completes_after_its_duration() {
        let started = Instant::now();
        crate::block_on(Delay::new(Duration::from_millis(20))).expect("the wait is taken");
        assert!(started.elapsed() >= Duration::from_millis(20));
    }

    /// Polled again with another waker before it elapses, a delay wakes that
    /// later waker and not the first.
    #[test]
    fn a_delay_wakes_the_waker_it_was_last_polled_with() {
        let first = Arc::new(Flag::default());
        let latest = Arc::new(Flag::default());
        let mut delay = Delay::new(Duration::from_millis(30));
        let mut pinned = Pin::new(&mut delay);
        let first_waker = Waker::from(Arc::clone(&first));
        let latest_waker = Waker::from(Arc::clone(&latest));
        assert!(matches!(
            pinned.as_mut().poll(&mut Context::from_waker(&first_waker)),
            Poll::Pending
        ));
        assert!(matches!(
            pinned
                .as_mut()
                .poll(&mut Context::from_waker(&latest_waker)),
            Poll::Pending
        ));
        let deadline = Instant::now() + Duration::from_secs(10);
        while !latest.0.load(Ordering::SeqCst) && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(2));
        }
        assert!(
            latest.0.load(Ordering::SeqCst),
            "the latest waker was not woken"
        );
        assert!(!first.0.load(Ordering::SeqCst), "a stale waker was woken");
        assert!(matches!(
            pinned
                .as_mut()
                .poll(&mut Context::from_waker(&latest_waker)),
            Poll::Ready(Ok(()))
        ));
    }
}
