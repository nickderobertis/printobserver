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

/// One report to the manager: the state entered, how long the manager should
/// wait for the next report, and the exit code the state carries.
///
/// The wait hint is meaningful for the two pending states alone and zero for
/// the other two; the exit code is meaningful for [`ServiceState::Stopped`]
/// alone and zero for the other three, which is what a manager expects of a
/// service that has not exited.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StatusReport {
    /// The state the service has entered.
    pub state: ServiceState,
    /// How long the manager should wait before treating the next report as
    /// overdue, for a pending state.
    pub wait_hint: Duration,
    /// The status this program exited with, for a stopped state: one of
    /// [`Exit`]'s, as the console form of the program exits with it.
    pub exit_code: u8,
}

impl StatusReport {
    /// A report of a pending state, with how long it may take.
    #[must_use]
    pub const fn pending(state: ServiceState, wait_hint: Duration) -> Self {
        Self {
            state,
            wait_hint,
            exit_code: 0,
        }
    }

    /// The report that the service is running.
    #[must_use]
    pub const fn running() -> Self {
        Self {
            state: ServiceState::Running,
            wait_hint: Duration::ZERO,
            exit_code: 0,
        }
    }

    /// The report that the service has stopped, and how.
    #[must_use]
    pub const fn stopped(exit: Exit) -> Self {
        Self {
            state: ServiceState::Stopped,
            wait_hint: Duration::ZERO,
            exit_code: exit.status(),
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
        StatusReport::pending(ServiceState::StartPending, START_WAIT_HINT),
    );
    let running: Running = match Server::start(config).await {
        Ok(running) => running,
        Err(error) => {
            eprintln!("printobserver will not start: {error}");
            deliver(reporter, StatusReport::stopped(Exit::Unconfigured));
            return Exit::Unconfigured;
        }
    };
    eprintln!("printobserver is serving on {}", running.address());
    deliver(reporter, StatusReport::running());

    stop.await;
    deliver(
        reporter,
        StatusReport::pending(ServiceState::StopPending, STOP_WAIT_HINT),
    );
    // The one shutdown: what `serve_until_signalled` takes once the Unix
    // termination signal arrives, and nothing a service does instead.
    running.stop().await;
    deliver(reporter, StatusReport::stopped(Exit::Success));
    Exit::Success
}

/// Deliver one report, saying so when the manager refuses it.
fn deliver<R: StatusReporter>(reporter: &mut R, report: StatusReport) {
    if let Err(refusal) = reporter.report(report) {
        eprintln!(
            "printobserver could not report that it is {}: {refusal}",
            report.state.name()
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
        let names: Vec<&str> = [
            ServiceState::StartPending,
            ServiceState::Running,
            ServiceState::StopPending,
            ServiceState::Stopped,
        ]
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
        let pending = StatusReport::pending(ServiceState::StartPending, START_WAIT_HINT);
        assert_eq!(pending.wait_hint, START_WAIT_HINT);
        assert_eq!(pending.exit_code, 0);

        let running = StatusReport::running();
        assert_eq!(running.state, ServiceState::Running);
        assert_eq!(running.wait_hint, Duration::ZERO);
        assert_eq!(running.exit_code, 0);

        let stopped = StatusReport::stopped(Exit::Unconfigured);
        assert_eq!(stopped.state, ServiceState::Stopped);
        assert_eq!(stopped.wait_hint, Duration::ZERO);
        assert_eq!(stopped.exit_code, Exit::Unconfigured.status());
    }

    /// Both hints are generous rather than tight: a manager on a loaded board
    /// that runs out of patience kills a process that was about to answer.
    #[test]
    fn both_hints_are_measured_in_tens_of_seconds() {
        assert!(START_WAIT_HINT >= Duration::from_secs(10));
        assert!(STOP_WAIT_HINT >= Duration::from_secs(10));
    }
}
