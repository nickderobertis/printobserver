//! `printobserver server` as a Windows service, when the service control manager
//! started it.
//!
//! Windows has one kind of process that survives a reboot and comes back after a
//! crash without anybody at the keyboard: a service the service control manager
//! runs. A service process is an ordinary program that, on starting, connects
//! back to the manager that started it and reports its state through the
//! connection; the manager owns the start, the stop and the restart, and reads
//! nothing off the process but those reports.
//!
//! So `printobserver server` connects first. When the manager started this
//! process, the connection succeeds, the manager calls back into
//! [`service_main`] on a thread of its own, and [`crate::service::run`] reports
//! the sequence over the real server there — with the manager's stop control
//! handed in as the future that ends it, so that the stop is the one graceful
//! shutdown [`printobserver_server::Running::stop`] is. When something else
//! started this process — a console, the gate, a client journey — the manager
//! refuses the connection with one specific error, and the program runs exactly
//! as it does on any other platform: [`run_if_service`] answers `None` and the
//! caller takes the console path.
//!
//! Only this module knows the manager's own vocabulary, and only this module is
//! compiled for Windows: the sequence it reports is [`crate::service`]'s, which
//! every platform's tier drives.

use std::ffi::OsString;
use std::path::PathBuf;
use std::sync::{Mutex, OnceLock};

use tokio::sync::oneshot;
use windows_service::define_windows_service;
use windows_service::service::{
    ServiceControl, ServiceControlAccept, ServiceExitCode, ServiceState, ServiceStatus, ServiceType,
};
use windows_service::service_control_handler::{self, ServiceControlHandlerResult};
use windows_service::service_dispatcher;
use windows_sys::Win32::Foundation::ERROR_FAILED_SERVICE_CONTROLLER_CONNECT;

use crate::failure::Exit;
use crate::service::{self, StatusReport, StatusReporter};

/// The name this process registers with the manager under.
///
/// The manager identifies an own-process service by the process it started and
/// ignores this name, so an installer may register the service under another
/// — a tier does, to keep several apart on one machine — and this program still
/// answers as that service. It cannot be empty, and it is the name the install
/// path states: `just check-repo`'s `service-install` holds it to that name.
pub const SERVICE_NAME: &str = "printobserver";

/// The configuration path the `server` invocation was given, for
/// [`service_main`] to read: the manager calls it back with the service's own
/// start arguments rather than this process's, and this process's are the ones
/// the registration's own command line carries.
static CONFIG: OnceLock<PathBuf> = OnceLock::new();

/// What the service exited with, for the caller of [`run_if_service`] to exit
/// with once the manager's dispatcher returns.
static EXIT: Mutex<Option<Exit>> = Mutex::new(None);

/// Run as a service if the manager started this process, and answer what the
/// service exited with; answer `None` when something else did, in which case
/// nothing has happened and the caller runs the console form.
///
/// Answers `Some(Exit::Refused)` when the manager could be connected to and
/// then would not run the service, which is a host whose manager is not
/// behaving as one.
pub fn run_if_service(config: PathBuf) -> Option<Exit> {
    // Set before dispatching, because `service_main` is called back on another
    // thread as soon as the dispatcher connects. A second call in one process
    // cannot happen — the dispatcher runs once — so a value already there is
    // left alone rather than treated as an error.
    let _ = CONFIG.set(config);
    match service_dispatcher::start(SERVICE_NAME, ffi_service_main) {
        Ok(()) => Some(
            EXIT.lock()
                .map_or(Exit::Refused, |exit| exit.unwrap_or(Exit::Refused)),
        ),
        Err(windows_service::Error::Winapi(error))
            if error.raw_os_error() == Some(not_a_service_code()) =>
        {
            None
        }
        Err(error) => {
            eprintln!("printobserver could not run as a Windows service: {error}");
            Some(Exit::Refused)
        }
    }
}

/// The error the manager answers a process it did not start with, as the
/// operating system reports it to a program.
fn not_a_service_code() -> i32 {
    i32::try_from(ERROR_FAILED_SERVICE_CONTROLLER_CONNECT)
        .expect("a Win32 error code fits the operating-system error a program reads")
}

define_windows_service!(ffi_service_main, service_main);

/// What the manager calls back into once it has started this process as a
/// service: register for its controls, then report the sequence over the real
/// server until the stop control arrives.
fn service_main(_arguments: Vec<OsString>) {
    let exit = serve_as_a_service();
    if let Ok(mut recorded) = EXIT.lock() {
        *recorded = Some(exit);
    }
}

/// Register with the manager and run the sequence; answer what it exited with.
fn serve_as_a_service() -> Exit {
    let Some(config) = CONFIG.get().cloned() else {
        eprintln!("printobserver was called back as a service with no configuration path");
        return Exit::Refused;
    };
    let (stop, stopped) = oneshot::channel::<()>();
    let stop = Mutex::new(Some(stop));
    let handler = move |control: ServiceControl| match control {
        // Stop and shutdown are the manager asking this service to stop: the
        // second is the machine going down, and it is answered the same way.
        // llmlint: ignore[changed_behavior_has_e2e] The shutdown control is one only the service control manager sends, and only while the machine is going down: no tool sends it to one service, so no journey can drive it without restarting the runner. It shares this one arm with the stop control, whose whole path — the channel, the stop-pending report, `Running::stop`, the stopped report — `test_service_manager_journey.py` drives through the real manager on the Windows cells.
        ServiceControl::Stop | ServiceControl::Shutdown => {
            if let Ok(mut sender) = stop.lock()
                && let Some(sender) = sender.take()
            {
                let _ = sender.send(());
            }
            ServiceControlHandlerResult::NoError
        }
        // The manager asking what state this service is in; it reads the last
        // status reported, so there is nothing to do but acknowledge.
        ServiceControl::Interrogate => ServiceControlHandlerResult::NoError,
        _ => ServiceControlHandlerResult::NotImplemented,
    };
    let handle = match service_control_handler::register(SERVICE_NAME, handler) {
        Ok(handle) => handle,
        // llmlint: ignore[changed_behavior_has_e2e] A manager that called this process back and then refuses it a handler is a host whose manager is not behaving as one; nothing can make the real manager do that to prove the branch. What it does is the smallest true thing: say so in the console form's words and exit `Refused`, with no status to report because no handle was granted.
        Err(error) => {
            eprintln!("printobserver could not register with the service control manager: {error}");
            return Exit::Refused;
        }
    };
    let runtime = match tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
    {
        Ok(runtime) => runtime,
        // llmlint: ignore[changed_behavior_has_e2e] tokio refusing to build a runtime is a host out of threads or descriptors, which no journey can arrange without taking the runner down with it. The branch mirrors `main.rs`'s console form for the same refusal — the same sentence, `Refused` — and reports the stop to the manager it registered with so the service does not hang in start pending.
        Err(error) => {
            eprintln!("printobserver could not start a runtime: {error}");
            let mut reporter = ManagerReporter { handle };
            let _ = reporter.report(StatusReport::Stopped {
                exit: Exit::Refused,
            });
            return Exit::Refused;
        }
    };
    let mut reporter = ManagerReporter { handle };
    runtime.block_on(service::run(&config, &mut reporter, async move {
        let _ = stopped.await;
    }))
}

/// The manager, as somewhere reports go.
struct ManagerReporter {
    /// The handle the manager handed this service when it registered.
    handle: service_control_handler::ServiceStatusHandle,
}

impl StatusReporter for ManagerReporter {
    fn report(&mut self, report: StatusReport) -> Result<(), String> {
        self.handle
            .set_service_status(status_of(report))
            .map_err(|error| error.to_string())
    }
}

/// One report, in the manager's own vocabulary.
///
/// A stop is accepted while the service is running and at no other time: a
/// manager may not ask a service that is still starting to stop, and one that
/// is already stopping has nothing further to accept. An exit of this program's
/// own is reported as a service-specific exit code, which is how a manager
/// records a status that is not one of the operating system's.
fn status_of(report: StatusReport) -> ServiceStatus {
    let current_state = match report.state() {
        service::ServiceState::StartPending => ServiceState::StartPending,
        service::ServiceState::Running => ServiceState::Running,
        service::ServiceState::StopPending => ServiceState::StopPending,
        service::ServiceState::Stopped => ServiceState::Stopped,
    };
    let controls_accepted = if report == StatusReport::Running {
        ServiceControlAccept::STOP | ServiceControlAccept::SHUTDOWN
    } else {
        ServiceControlAccept::empty()
    };
    let exit_code = match report.exit_code() {
        0 => ServiceExitCode::Win32(0),
        status => ServiceExitCode::ServiceSpecific(u32::from(status)),
    };
    ServiceStatus {
        service_type: ServiceType::OWN_PROCESS,
        current_state,
        controls_accepted,
        exit_code,
        checkpoint: 0,
        wait_hint: report.wait_hint(),
        process_id: None,
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use windows_service::service::{
        ServiceControlAccept, ServiceExitCode, ServiceState, ServiceType,
    };

    use super::{SERVICE_NAME, status_of};
    use crate::failure::Exit;
    use crate::service::{STOP_WAIT_HINT, StatusReport};

    /// The name is the one the install path states.
    #[test]
    fn the_service_name_is_the_programs() {
        assert_eq!(SERVICE_NAME, "printobserver");
    }

    /// A running service accepts a stop; a service in any other state does not.
    #[test]
    fn a_stop_is_accepted_while_running_and_at_no_other_time() {
        let running = status_of(StatusReport::Running);
        assert_eq!(running.current_state, ServiceState::Running);
        assert!(
            running
                .controls_accepted
                .contains(ServiceControlAccept::STOP)
        );
        assert!(
            running
                .controls_accepted
                .contains(ServiceControlAccept::SHUTDOWN)
        );

        let starting = status_of(StatusReport::StartPending {
            wait_hint: Duration::from_secs(30),
        });
        assert_eq!(starting.current_state, ServiceState::StartPending);
        assert!(starting.controls_accepted.is_empty());
        assert_eq!(starting.wait_hint, Duration::from_secs(30));
    }

    /// A clean stop is the operating system's zero; the program's own exit is a
    /// service-specific code carrying that exit.
    #[test]
    fn an_exit_of_this_programs_own_is_reported_as_service_specific() {
        let clean = status_of(StatusReport::Stopped {
            exit: Exit::Success,
        });
        assert_eq!(clean.current_state, ServiceState::Stopped);
        assert_eq!(clean.exit_code, ServiceExitCode::Win32(0));
        assert_eq!(clean.service_type, ServiceType::OWN_PROCESS);

        let refused = status_of(StatusReport::Stopped {
            exit: Exit::Unconfigured,
        });
        assert_eq!(
            refused.exit_code,
            ServiceExitCode::ServiceSpecific(u32::from(Exit::Unconfigured.status()))
        );

        let stopping = status_of(StatusReport::StopPending {
            wait_hint: STOP_WAIT_HINT,
        });
        assert_eq!(stopping.current_state, ServiceState::StopPending);
        assert_eq!(stopping.wait_hint, STOP_WAIT_HINT);
    }
}
