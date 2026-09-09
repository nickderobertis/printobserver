//! What this program exits with, and what it says when it does.
//!
//! # Five exits, each a different thing to do next
//!
//! A caller — and the supervising agent is one — decides what to do next from
//! the status before it reads a word, so every class this program declares has
//! a status of its own. A server that is not there, a program nobody
//! configured, an action the policy refused and an image path that names no
//! file here are four different problems with four different answers, and
//! collapsing any two of them would send somebody to fix the wrong thing.
//!
//! [`Exit::Rejected`] is the one whose message is not this program's own: the
//! policy's reason, the value asked for and the range allowed come back in the
//! answer, and printing them is what lets the next request be one that is
//! accepted.

/// What this program exited with.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Exit {
    /// It did what it was asked.
    Success,
    /// The arguments do not name anything this program does.
    Usage,
    /// Nothing answered at the configured address.
    Unreachable,
    /// Nothing configured this program with a server to talk to.
    Unconfigured,
    /// The policy refused the action.
    Rejected,
    /// The answer carried a path to an image, and no file is there.
    ImageElsewhere,
    /// The supervisor answered something this program will not act on.
    Refused,
}

impl Exit {
    /// Every exit this program declares, and there is no other.
    pub const ALL: [Self; 7] = [
        Self::Success,
        Self::Usage,
        Self::Unreachable,
        Self::Unconfigured,
        Self::Rejected,
        Self::ImageElsewhere,
        Self::Refused,
    ];

    /// The status the operating system sees.
    #[must_use]
    pub const fn status(self) -> u8 {
        match self {
            Self::Success => 0,
            Self::Usage => 2,
            Self::Unreachable => 3,
            Self::Unconfigured => 4,
            Self::Rejected => 5,
            Self::ImageElsewhere => 6,
            Self::Refused => 7,
        }
    }

    /// This exit's own name, for a report that walks them.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Success => "success",
            Self::Usage => "usage",
            Self::Unreachable => "unreachable",
            Self::Unconfigured => "unconfigured",
            Self::Rejected => "rejected",
            Self::ImageElsewhere => "image-elsewhere",
            Self::Refused => "refused",
        }
    }
}

impl core::fmt::Display for Exit {
    fn fmt(&self, formatter: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        formatter.write_str(self.as_str())
    }
}

/// One thing that went wrong, and what to do about it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Failure {
    /// Which class it is.
    pub exit: Exit,
    /// What to tell the caller, naming a concrete next action.
    pub detail: String,
}

impl Failure {
    /// One failure of one class.
    pub fn of(exit: Exit, detail: impl core::fmt::Display) -> Self {
        Self {
            exit,
            detail: detail.to_string(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::Exit;

    /// Every exit this program declares is distinguishable from every other.
    ///
    /// A caller — and the supervising agent is one — decides what to do next
    /// from the status before it reads a word, so two classes sharing a status
    /// would be two different problems with one answer.
    #[test]
    fn every_exit_is_distinguishable_from_every_other_and_from_success() {
        let mut statuses: Vec<u8> = Exit::ALL.iter().map(|exit| exit.status()).collect();
        let mut names: Vec<&str> = Exit::ALL.iter().map(|exit| exit.as_str()).collect();
        let held = statuses.len();
        statuses.sort_unstable();
        statuses.dedup();
        names.sort_unstable();
        names.dedup();

        assert_eq!(statuses.len(), held, "two exits share a status");
        assert_eq!(names.len(), held, "two exits share a name");
        assert_eq!(Exit::Success.status(), 0);
        assert!(
            Exit::ALL
                .iter()
                .all(|exit| *exit == Exit::Success || exit.status() != 0),
            "an exit that is not success exits zero"
        );
        assert_eq!(Exit::Rejected.to_string(), "rejected");
    }
}
