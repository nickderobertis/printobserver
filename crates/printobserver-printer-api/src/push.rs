//! What a printer pushes on its own, and the closed set of commands it may be
//! sent.
//!
//! A poll reads the printer's state; some of what matters never shows in one.
//! A firmware reset that lands the machine idle reads exactly like a cancel,
//! and a reset while idle changes nothing a read reports at all. The printer's
//! own push channel says so, and [`PrinterPush`] is that channel's vocabulary
//! here: what [`PrinterPort::push_events`](crate::PrinterPort::push_events)
//! answers.
//!
//! [`PrinterCommand`] is the other direction: the only commands
//! [`PrinterPort::send_command`](crate::PrinterPort::send_command) takes. It is
//! a closed vocabulary rather than a line, so nothing a caller writes reaches a
//! machine through it; each adapter renders a variant to its own line.

use core::pin::Pin;
use core::task::{Context, Poll};

/// A stream of values that arrive one at a time, asynchronously.
///
/// The one shape [`PrinterPort::push_events`](crate::PrinterPort::push_events)
/// answers in, declared here so that this port depends on nothing but the type
/// crate. It is the same contract as the ecosystem's own `Stream`: `Ready(None)`
/// is the end, after which nothing more arrives.
pub trait Stream {
    /// What the stream yields.
    type Item;

    /// Take the next value when one has arrived, `Ready(None)` at the end, or
    /// `Pending` — having arranged for the context's waker to be woken when
    /// either is ready.
    fn poll_next(self: Pin<&mut Self>, context: &mut Context<'_>) -> Poll<Option<Self::Item>>;
}

/// A stream this port answers, in the one shape a trait object can carry.
pub type BoxStream<'a, T> = Pin<Box<dyn Stream<Item = T> + Send + 'a>>;

/// One thing a printer pushed on its own.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PrinterPush {
    /// The printer reset: its firmware restarted, and every state it held —
    /// the job, the position, the heaters — is gone.
    Reset,
    /// The printer reported an error.
    Error {
        /// What kind of error the source reported.
        reason: PrinterErrorReason,
    },
    /// One line of the printer's terminal, as the source shows it.
    Log {
        /// The line.
        line: String,
    },
}

/// What kind of error a printer pushed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PrinterErrorReason {
    /// The printer's own firmware reported it: a thermal runaway, a kill, a
    /// halt. What ends a print `failed`.
    Firmware,
    /// Anything else the source reported, in its own word for it: a resend it
    /// could not satisfy, the host losing the line.
    Other(String),
}

/// A command a printer may be sent: a closed vocabulary, each variant rendered
/// by the adapter to its own line.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PrinterCommand {
    /// Report where the print head is. The answer arrives on the push channel.
    ReportPosition,
}
