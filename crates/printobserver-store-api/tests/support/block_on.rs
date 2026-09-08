//! A dependency-free executor, so that a port crate's tests await its methods.
//!
//! This crate declares the type crate as its only dependency, tests included,
//! so there is no runtime to borrow. This is one: it parks the calling thread
//! until the future it was given is ready, which is all a contract test needs.

use std::future::Future;
use std::sync::Arc;
use std::task::{Context, Poll, Wake, Waker};

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
