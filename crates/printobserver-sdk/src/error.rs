//! Every way a call can end other than with the answer it asked for.
//!
//! Five of them, and each is a different thing for a caller to do next: the
//! supervisor was not there, this client would not send the request, the
//! supervisor said no in words, the policy refused the action, or the machine
//! itself refused it. The fourth is the one with a shape rather than a
//! sentence, because a caller that can read the range allowed can ask again
//! for something inside it.

use crate::contract::{ActionAnswer, PolicyDecision, Range, RejectionReason};

/// The policy's own refusal of one action, as a caller reads it.
///
/// [`Rejection::reason`] is the refusal itself, matched rather than read;
/// [`Rejection::requested`] and [`Rejection::allowed`] are the value that was
/// asked for and the range that is allowed, which the policy states when it
/// ruled on a value and which are absent when it ruled on something else.
#[derive(Debug, Clone, PartialEq)]
pub struct Rejection {
    /// Why the action was refused.
    pub reason: RejectionReason,
    /// The value that was asked for, where the policy ruled on one.
    pub requested: Option<f64>,
    /// The range that is allowed, where the policy ruled on a value.
    pub allowed: Option<Range>,
    /// The whole answer, whose record carries the request and the decision.
    pub answer: Box<ActionAnswer>,
}

impl Rejection {
    /// The refusal one answer carries, when the decision on it was a refusal.
    #[must_use]
    pub fn of(answer: ActionAnswer) -> Option<Self> {
        let PolicyDecision::Rejected(reason) = &answer.record.decision else {
            return None;
        };
        let (requested, allowed) = match reason {
            RejectionReason::OutOfBounds {
                requested, allowed, ..
            } => (Some(*requested), Some(allowed.clone())),
            _ => (None, None),
        };
        Some(Self {
            reason: reason.clone(),
            requested,
            allowed,
            answer: Box::new(answer),
        })
    }
}

impl core::fmt::Display for Rejection {
    fn fmt(&self, formatter: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(
            formatter,
            "the supervisor's policy refused this action: {:?}",
            self.reason
        )?;
        if let (Some(requested), Some(allowed)) = (self.requested, self.allowed.as_ref()) {
            write!(
                formatter,
                ". It was asked for {requested}, and what is allowed is {} to {}",
                allowed.min, allowed.max
            )?;
        }
        Ok(())
    }
}

/// Why a call did not answer what it asked for.
#[derive(Debug, Clone, PartialEq)]
pub enum ClientError {
    /// Nothing answered at the configured address.
    Unreachable {
        /// Where this client looked.
        address: String,
        /// What happened there, in the words of whatever failed.
        detail: String,
    },
    /// This client would not send the request, because it carried no reason.
    ///
    /// Every mutating operation carries a reason, and a call whose reason is
    /// empty is refused here rather than at the server: nothing reaches the
    /// policy, the printer or the record.
    NoReason,
    /// A value of the request could not be rendered as JSON.
    Unsendable {
        /// What could not be rendered, in the words of whatever refused it.
        detail: String,
    },
    /// The supervisor answered something this client cannot read.
    Unreadable {
        /// The status it answered under.
        status: u16,
        /// What could not be read, in the words of whatever refused it.
        detail: String,
    },
    /// The supervisor will not do what it was asked, and said why.
    Refused {
        /// The status it answered under.
        status: u16,
        /// What it said, in its own words.
        detail: String,
    },
    /// The policy accepted the action and the machine refused it.
    PrinterRefused {
        /// What the printer said.
        detail: String,
        /// The record of the request and the decision taken on it.
        answer: Box<ActionAnswer>,
    },
    /// The policy refused the action, and the refusal is the answer.
    Rejected(Box<Rejection>),
}

impl core::fmt::Display for ClientError {
    fn fmt(&self, formatter: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::Unreachable { address, detail } => write!(
                formatter,
                "nothing answered at {address}: {detail}. Start the supervisor there, or \
                 point this client at the address it is answering on"
            ),
            Self::NoReason => formatter
                .write_str("every mutating call carries a reason, and this one carries none"),
            Self::Unsendable { detail } => {
                write!(
                    formatter,
                    "this request could not be rendered as JSON: {detail}"
                )
            }
            Self::Unreadable { status, detail } => write!(
                formatter,
                "the supervisor answered {status} with something this client cannot read: \
                 {detail}"
            ),
            Self::Refused { status, detail } => {
                write!(formatter, "the supervisor answered {status}: {detail}")
            }
            Self::PrinterRefused { detail, .. } => write!(
                formatter,
                "the policy accepted this action and the machine refused it: {detail}. Look \
                 at the printer, then ask again"
            ),
            Self::Rejected(rejection) => write!(formatter, "{rejection}"),
        }
    }
}

impl core::error::Error for ClientError {}
