//! Waiting for a started supervisor to say where it is serving, behind a
//! deadline.
//!
//! A supervisor announces itself with one line on standard error, and every
//! journey that starts one waits for that line before it asks it anything. A
//! blocking read of that stream has no verdict when the line never comes: a
//! supervisor that starts slowly or never announces holds the whole tier with
//! nothing reported. So the stream is read on a thread of its own, each line
//! handed over as it arrives, and the wait for the announcement is bounded. The
//! deadline is a backstop and never the signal: the wait returns the moment the
//! line arrives. Where it runs out, the child is stopped and the failure
//! carries every line read up to then, which is what says why.
//!
//! The thread goes on reading once nobody wants the lines, so a supervisor
//! nobody is listening to any more never blocks on, or fails writing to, a
//! stream whose reader has gone.

use std::fmt;
use std::io::{BufRead as _, BufReader};
use std::process::Child;
use std::sync::mpsc::{Receiver, RecvTimeoutError, channel};
use std::time::{Duration, Instant};

/// What the announcement begins with; the address follows it.
pub const SERVING_ON: &str = "printobserver is serving on ";

/// How long a started supervisor is given to announce itself.
///
/// Far longer than any healthy start: a loaded runner has taken tens of
/// seconds to bring a coverage-instrumented program to its first line, and a
/// bound that trips on a slow start reports a failure nobody caused.
pub const DEADLINE: Duration = Duration::from_secs(180);

/// How long the rest of a stream is read for once its child has been stopped.
///
/// A stopped child's stream closes at once. This bounds the one case where it
/// does not — a process the child started still holding it open — so reading
/// the rest can never be the unbounded wait this module exists to remove.
const DRAIN: Duration = Duration::from_secs(10);

/// A child's standard error, as far as it has been read.
pub struct Stream {
    /// Every line read so far, each with its line ending.
    printed: String,
    /// The lines the reading thread has handed over since.
    lines: Receiver<String>,
}

impl Stream {
    /// Read `child`'s standard error on a thread of its own.
    fn of(child: &mut Child) -> Self {
        let stderr = child
            .stderr
            .take()
            .expect("the supervisor was started with its standard error piped");
        let (sender, lines) = channel();
        std::thread::spawn(move || {
            let mut reader = BufReader::new(stderr);
            let mut wanted = true;
            loop {
                let mut line = Vec::new();
                match reader.read_until(b'\n', &mut line) {
                    Ok(0) | Err(_) => break,
                    Ok(_) => {
                        if wanted {
                            wanted = sender
                                .send(String::from_utf8_lossy(&line).into_owned())
                                .is_ok();
                        }
                    }
                }
            }
        });
        Self {
            printed: String::new(),
            lines,
        }
    }

    /// Everything the child printed, once it has been stopped: what was read
    /// before, and the rest until the stream closes or [`DRAIN`] runs out.
    pub fn through_the_end(mut self) -> String {
        let until = Instant::now() + DRAIN;
        while let Some(remaining) = until.checked_duration_since(Instant::now()) {
            match self.lines.recv_timeout(remaining) {
                Ok(line) => self.printed.push_str(&line),
                Err(_) => break,
            }
        }
        self.printed
    }
}

/// A supervisor that did not announce itself, and everything it printed.
pub struct Unannounced {
    /// How long the wait lasted.
    waited: Duration,
    /// Whether its stream closed before the deadline, rather than the deadline
    /// running out with the stream still open.
    closed: bool,
    /// Every line it printed, the rest read after it was stopped included.
    printed: String,
}

impl fmt::Display for Unannounced {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let why = if self.closed {
            "its output ended"
        } else {
            "the deadline ran out"
        };
        write!(
            formatter,
            "it did not say where it is serving: {why} after {:?}, and it was stopped. \
             Everything it printed:\n{}",
            self.waited, self.printed
        )
    }
}

/// Wait up to `deadline` for `child` to say where it is serving; answer the
/// address and its stream as read so far.
///
/// Where the line does not come — the deadline runs out, or the stream closes
/// first — the child is stopped and reaped before this answers, and what is
/// answered carries everything it printed.
pub fn within(child: &mut Child, deadline: Duration) -> Result<(String, Stream), Unannounced> {
    let started = Instant::now();
    let mut stream = Stream::of(child);
    let closed = loop {
        let remaining = deadline.saturating_sub(started.elapsed());
        match stream.lines.recv_timeout(remaining) {
            Ok(line) => {
                stream.printed.push_str(&line);
                if let Some(address) = line.strip_prefix(SERVING_ON) {
                    return Ok((address.trim().to_owned(), stream));
                }
            }
            Err(RecvTimeoutError::Timeout) => break false,
            Err(RecvTimeoutError::Disconnected) => break true,
        }
    };
    let waited = started.elapsed();
    let _ = child.kill();
    let _ = child.wait();
    Err(Unannounced {
        waited,
        closed,
        printed: stream.through_the_end(),
    })
}

/// Wait for `child` — `what` names it — to say where it is serving, within
/// [`DEADLINE`]; fail with everything it printed where it does not.
pub fn serving(child: &mut Child, what: &str) -> (String, Stream) {
    within(child, DEADLINE).unwrap_or_else(|unannounced| panic!("{what}: {unannounced}"))
}
