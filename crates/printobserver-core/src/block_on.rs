//! A dependency-free executor, so that the expiry driver can run its own work.
//!
//! Core declares the type crate and the four port crates and nothing else, so
//! there is no runtime to borrow. The expiry driver is a thread of core's own —
//! a bounded intervention expires with nothing asking it to — and this is what
//! that thread runs its futures on: it parks itself until the future it was
//! given is ready.
//!
//! **A port implementation core drives has to be pollable off any runtime.**
//! Nothing here provides a reactor, so a port whose futures only make progress
//! inside some other executor would never complete on this thread. That is a
//! requirement on the adapters rather than a hidden assumption: core's own
//! asynchronous methods are ordinary futures its caller drives, and this
//! executor is reached only by the expiry driver.

use core::future::Future;
use core::task::{Context, Poll};
use std::sync::Arc;
use std::task::{Wake, Waker};

/// Unparks the thread that is blocking on a future.
struct Unpark(std::thread::Thread);

impl Wake for Unpark {
    fn wake(self: Arc<Self>) {
        self.0.unpark();
    }
}

/// Run one future to completion on the calling thread.
pub fn block_on<F: Future>(future: F) -> F::Output {
    let waker = Waker::from(Arc::new(Unpark(std::thread::current())));
    let mut context = Context::from_waker(&waker);
    let mut future = Box::pin(future);
    loop {
        match future.as_mut().poll(&mut context) {
            Poll::Ready(value) => return value,
            Poll::Pending => std::thread::park(),
        }
    }
}
