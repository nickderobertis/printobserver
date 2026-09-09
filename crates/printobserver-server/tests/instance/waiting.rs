//! Waiting for a real machine to get where it is going.
//!
//! The virtual printer dwells ten seconds at a time, so an action reaches it
//! when the dwell it was issued during ends rather than at once. Every wait here
//! says what it was waiting for, so a tier that times out reports the state the
//! machine was actually in rather than a bare deadline.

use core::time::Duration;
use std::time::Instant;

/// How long one state change is given on a machine that dwells ten seconds.
pub const REACHED: Duration = Duration::from_secs(120);

/// Wait until `condition` holds, or panic saying what never happened.
pub async fn until<F, Fut>(described: &str, mut condition: F)
where
    F: FnMut() -> Fut,
    Fut: core::future::Future<Output = Option<String>>,
{
    let deadline = Instant::now() + REACHED;
    let mut last = String::from("(nothing was read yet)");
    while Instant::now() < deadline {
        match condition().await {
            None => return,
            Some(seen) => last = seen,
        }
        tokio::time::sleep(Duration::from_millis(500)).await;
    }
    panic!("waited {REACHED:?} for {described}; the last thing read was {last}");
}
