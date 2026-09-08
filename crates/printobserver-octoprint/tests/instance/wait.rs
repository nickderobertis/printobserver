//! Waiting for a real machine to get where it was asked to go.
//!
//! Every action this port takes reaches the printer through a queue of G-code,
//! and the hold print dwells for ten seconds at a time, so a state the instance
//! will reach is not a state it has reached. Nothing here sleeps a fixed amount:
//! each wait names what it is waiting for, and says so when it gives up.

use std::time::{Duration, Instant};

/// How often a wait asks again.
const INTERVAL: Duration = Duration::from_millis(500);

/// Wait until an attempt answers, or give up saying what was being waited for.
///
/// # Panics
///
/// Panics when the limit passes with no answer, naming what did not happen and
/// what was seen instead.
pub fn until<T>(
    describing: &str,
    limit: Duration,
    mut attempt: impl FnMut() -> Result<T, String>,
) -> T {
    let deadline = Instant::now() + limit;
    loop {
        let last = match attempt() {
            Ok(value) => return value,
            Err(seen) => seen,
        };
        assert!(
            Instant::now() < deadline,
            "waited {limit:?} for {describing}, and saw {last}"
        );
        std::thread::sleep(INTERVAL);
    }
}
