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
use std::net::SocketAddr;
use std::process::Child;
use std::sync::mpsc::{Receiver, RecvTimeoutError, channel};
use std::time::{Duration, Instant};

/// What the announcement begins with; the address follows it.
///
/// Spelled as the program prints it rather than imported, because the program
/// prints it from its binary rather than from anything a test can link. It is
/// not left to drift: every journey here that starts the real program waits
/// for this line, and fails naming everything the program printed when the
/// two differ.
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
pub const DRAIN: Duration = Duration::from_secs(10);

/// A child's standard error: what the wait has taken off the reading thread,
/// and the handle it takes the rest through.
pub struct Stream {
    printed: String,
    lines: Receiver<String>,
}

impl Stream {
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

    /// What the child printed, once it has been stopped: what was read before,
    /// and the rest until the stream closes — or, where something still holds
    /// it open, until [`DRAIN`] runs out.
    pub fn collected(mut self) -> String {
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

enum Why {
    Deadline,
    /// The stream closed first: the child exited, or closed its standard
    /// error, before it announced.
    Closed,
    NoAddress(String),
}

/// A supervisor that did not say where it is serving, and everything it
/// printed — the rest read after it was stopped included.
pub struct Unannounced {
    why: Why,
    waited: Duration,
    printed: String,
}

impl fmt::Display for Unannounced {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let why = match &self.why {
            Why::Deadline => "the deadline ran out".to_owned(),
            Why::Closed => "its output ended".to_owned(),
            Why::NoAddress(named) => format!("it announced `{named}`, which is no address"),
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
/// Where no address comes — the deadline runs out, the stream closes first, or
/// the announcement names no socket address — the child is stopped and reaped
/// before this answers, and what is answered carries everything it printed.
pub fn within(child: &mut Child, deadline: Duration) -> Result<(SocketAddr, Stream), Unannounced> {
    let started = Instant::now();
    let mut stream = Stream::of(child);
    let why = loop {
        let remaining = deadline.saturating_sub(started.elapsed());
        match stream.lines.recv_timeout(remaining) {
            Ok(line) => {
                stream.printed.push_str(&line);
                if let Some(named) = line.strip_prefix(SERVING_ON) {
                    let named = named.trim();
                    match named.parse() {
                        Ok(address) => return Ok((address, stream)),
                        Err(_) => break Why::NoAddress(named.to_owned()),
                    }
                }
            }
            Err(RecvTimeoutError::Timeout) => break Why::Deadline,
            Err(RecvTimeoutError::Disconnected) => break Why::Closed,
        }
    };
    let waited = started.elapsed();
    let _ = child.kill();
    let _ = child.wait();
    Err(Unannounced {
        why,
        waited,
        printed: stream.collected(),
    })
}

/// Wait for `child` — `what` names it — to say where it is serving, within
/// [`DEADLINE`]; fail with everything it printed where it does not.
pub fn serving(child: &mut Child, what: &str) -> (SocketAddr, Stream) {
    within(child, DEADLINE).unwrap_or_else(|unannounced| panic!("{what}: {unannounced}"))
}
