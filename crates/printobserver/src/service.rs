//! Running the supervisor under a service manager that is told how it is doing.
//!
//! A Unix service manager learns what the program is doing from the process
//! itself: it is running while the process is, and it stopped cleanly when the
//! process exited zero. The Windows service control manager is told instead. A
//! service reports each state it enters — start pending, running, stop pending,
//! stopped — and, with each pending state, how long the manager should wait
//! before treating the next report as overdue; a service that reports late, or
//! reports a state the manager did not expect, is one the manager kills.
//!
//! This module is that sequence, written once and independent of where the
//! reports go. [`StatusReporter`] is where they go — the service control
//! manager, or a fixture recording them — and [`run`] is the sequence itself,
//! over the real server: the stop control a manager sends is handed in as a
//! future, and what follows it is [`Running::stop`], which is the one graceful
//! shutdown the Unix termination signal takes too. There is no second shutdown
//! for a service, and a tier on any platform can drive this sequence over a
//! real server without a service control manager to hand.

use std::path::Path;
use std::time::Duration;

use printobserver_server::{Running, Server};

use crate::failure::Exit;

/// The states a service reports to the manager controlling it, in the order it
/// reports them.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ServiceState {
    /// Registered with the manager and starting the server: reading the
    /// configuration, asking the printer whether it answers, taking the port.
    StartPending,
    /// Serving, and answering the API.
    Running,
    /// Asked to stop, and finishing the last request.
    StopPending,
    /// Stopped, carrying the exit code the manager records.
    Stopped,
}

impl ServiceState {
    /// Every state, in the order a service reports them.
    pub const ALL: [Self; 4] = [
        Self::StartPending,
        Self::Running,
        Self::StopPending,
        Self::Stopped,
    ];

    /// The state's name as a report spells it.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::StartPending => "start-pending",
            Self::Running => "running",
            Self::StopPending => "stop-pending",
            Self::Stopped => "stopped",
        }
    }
}

// A state added to the enum does not compile here until this match counts it,
// and the count is held to `ServiceState::ALL` as the build runs.
const _: () = {
    const fn counted(state: ServiceState) -> usize {
        match state {
            ServiceState::StartPending
            | ServiceState::Running
            | ServiceState::StopPending
            | ServiceState::Stopped => 4,
        }
    }
    assert!(counted(ServiceState::Running) == ServiceState::ALL.len());
};

/// One report to the manager: the state entered, and what that state carries.
///
/// A pending state carries how long the manager should wait before treating the
/// next report as overdue, and nothing else; a stopped state carries the exit
/// the service stopped with, and nothing else; running carries nothing. Each
/// state's payload is its own variant, so a report of a running service with a
/// wait hint, or a starting one with an exit, cannot be written.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StatusReport {
    /// Starting, and how long that may take.
    StartPending {
        /// How long the manager should wait for the next report.
        wait_hint: Duration,
    },
    /// Serving.
    Running,
    /// Stopping, and how long that may take.
    StopPending {
        /// How long the manager should wait for the next report.
        wait_hint: Duration,
    },
    /// Stopped, and how.
    Stopped {
        /// What this program exited with, as the console form exits with it.
        exit: Exit,
    },
}

impl StatusReport {
    /// The state this report enters.
    #[must_use]
    pub const fn state(self) -> ServiceState {
        match self {
            Self::StartPending { .. } => ServiceState::StartPending,
            Self::Running => ServiceState::Running,
            Self::StopPending { .. } => ServiceState::StopPending,
            Self::Stopped { .. } => ServiceState::Stopped,
        }
    }

    /// How long the manager should wait for the next report: the pending
    /// states' own hint, and zero for a state that is not pending, which is
    /// what a manager expects of one.
    #[must_use]
    pub const fn wait_hint(self) -> Duration {
        match self {
            Self::StartPending { wait_hint } | Self::StopPending { wait_hint } => wait_hint,
            Self::Running | Self::Stopped { .. } => Duration::ZERO,
        }
    }

    /// The status the manager records: the stopped state's exit, and zero for
    /// a service that has not exited.
    #[must_use]
    pub const fn exit_code(self) -> u8 {
        match self {
            Self::Stopped { exit } => exit.status(),
            Self::StartPending { .. } | Self::Running | Self::StopPending { .. } => 0,
        }
    }
}

/// Where a service's status reports go.
///
/// On Windows that is the service control manager, through the handle it
/// handed the service when it registered. In a tier it is whatever records the
/// sequence, so that the sequence is proven on a platform with no manager.
pub trait StatusReporter {
    /// Deliver one report.
    ///
    /// # Errors
    ///
    /// Returns the manager's own refusal, rendered, when the report could not be
    /// delivered. The sequence goes on regardless — a manager that cannot be
    /// told the service is stopping is not one the service should keep running
    /// for — and the refusal is said on standard error.
    fn report(&mut self, report: StatusReport) -> Result<(), String>;
}

/// How long the manager is told starting may take.
///
/// Starting reads the configuration, asks the configured printer whether it is
/// the one answering there, opens the store and takes the port. The printer is
/// the slow one: an `OctoPrint` on a small board answers in well under a second
/// when it answers, and the adapter gives up on one that does not long before
/// this hint runs out.
pub const START_WAIT_HINT: Duration = Duration::from_secs(30);

/// How long the manager is told stopping may take.
///
/// Stopping finishes the request in flight and no more; the hint is generous so
/// that a manager on a loaded board waits for the answer rather than killing a
/// process that was about to exit cleanly.
pub const STOP_WAIT_HINT: Duration = Duration::from_secs(30);

/// Run the supervisor as a service, reporting each state to `reporter`, until
/// `stop` resolves; answer the exit the service stopped with.
///
/// The sequence: start pending, then either running or — where the server will
/// not start — stopped carrying [`Exit::Unconfigured`], which is what the
/// console form exits with for the same refusal; then, once `stop` resolves,
/// stop pending, the one graceful shutdown, and stopped carrying
/// [`Exit::Success`]. What went wrong is said on standard error in the console
/// form's own words, so a manager that keeps a service's output has the same
/// sentence an operator at a console would read.
pub async fn run<R: StatusReporter>(
    config: &Path,
    reporter: &mut R,
    stop: impl Future<Output = ()>,
) -> Exit {
    deliver(
        reporter,
        StatusReport::StartPending {
            wait_hint: START_WAIT_HINT,
        },
    );
    let running: Running = match Server::start(config).await {
        Ok(running) => running,
        Err(error) => {
            eprintln!("printobserver will not start: {error}");
            deliver(
                reporter,
                StatusReport::Stopped {
                    exit: Exit::Unconfigured,
                },
            );
            return Exit::Unconfigured;
        }
    };
    eprintln!("printobserver is serving on {}", running.address());
    deliver(reporter, StatusReport::Running);

    stop.await;
    deliver(
        reporter,
        StatusReport::StopPending {
            wait_hint: STOP_WAIT_HINT,
        },
    );
    // The one shutdown: what `serve_until_signalled` takes once the Unix
    // termination signal arrives, and nothing a service does instead.
    running.stop().await;
    deliver(
        reporter,
        StatusReport::Stopped {
            exit: Exit::Success,
        },
    );
    Exit::Success
}

/// Deliver one report, saying so when the manager refuses it.
fn deliver<R: StatusReporter>(reporter: &mut R, report: StatusReport) {
    if let Err(refusal) = reporter.report(report) {
        eprintln!(
            "printobserver could not report that it is {}: {refusal}",
            report.state().name()
        );
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::{START_WAIT_HINT, STOP_WAIT_HINT, ServiceState, StatusReport};
    use crate::failure::Exit;

    /// Every state has a name of its own, spelled the way a report prints it.
    #[test]
    fn every_state_has_a_name_of_its_own() {
        let names: Vec<&str> = ServiceState::ALL
            .into_iter()
            .map(ServiceState::name)
            .collect();
        assert_eq!(
            names,
            ["start-pending", "running", "stop-pending", "stopped"]
        );
    }

    /// A pending report carries its hint and no exit; a running report carries
    /// neither; a stopped report carries the exit and no hint.
    #[test]
    fn a_report_carries_what_its_state_means_and_nothing_else() {
        let pending = StatusReport::StartPending {
            wait_hint: START_WAIT_HINT,
        };
        assert_eq!(pending.state(), ServiceState::StartPending);
        assert_eq!(pending.wait_hint(), START_WAIT_HINT);
        assert_eq!(pending.exit_code(), 0);

        let running = StatusReport::Running;
        assert_eq!(running.state(), ServiceState::Running);
        assert_eq!(running.wait_hint(), Duration::ZERO);
        assert_eq!(running.exit_code(), 0);

        let stopping = StatusReport::StopPending {
            wait_hint: STOP_WAIT_HINT,
        };
        assert_eq!(stopping.state(), ServiceState::StopPending);
        assert_eq!(stopping.wait_hint(), STOP_WAIT_HINT);

        let stopped = StatusReport::Stopped {
            exit: Exit::Unconfigured,
        };
        assert_eq!(stopped.state(), ServiceState::Stopped);
        assert_eq!(stopped.wait_hint(), Duration::ZERO);
        assert_eq!(stopped.exit_code(), Exit::Unconfigured.status());
    }

    /// Both hints are generous rather than tight: a manager on a loaded board
    /// that runs out of patience kills a process that was about to answer.
    #[test]
    fn both_hints_are_measured_in_tens_of_seconds() {
        assert!(START_WAIT_HINT >= Duration::from_secs(10));
        assert!(STOP_WAIT_HINT >= Duration::from_secs(10));
    }
}
