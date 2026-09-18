//! The supervisor run as a service, with a stand-in for the service manager.
//!
//! The sequence a service reports to the manager controlling it is
//! `printobserver::service`'s, and the one thing about it that is Windows's own
//! is where the reports go. This program runs that sequence over the real
//! server with the manager replaced by two streams: every report is printed
//! on standard output as one line the tier reads back, and the manager's stop
//! control is the first line — any line — that arrives on standard input. So a
//! tier on any platform drives a real server process through exactly the
//! transitions the Windows service reports, asks it a question while it is
//! running, sends the stop control, and reads that it stopped the way a service
//! is required to. Naming a state in `PRINTOBSERVER_FIXTURE_REFUSES_REPORT`
//! makes the stand-in refuse that one report, as a manager whose handle has
//! gone bad would, so a tier can see the sequence go on past a refusal.
//!
//! Built only under the `test-fixtures` feature, so nothing an ordinary
//! consumer installs carries it.

use std::io::{BufRead as _, Write as _};
use std::path::PathBuf;
use std::process::ExitCode;

use printobserver::service::{StatusReport, StatusReporter, run};
use tokio::sync::oneshot;

/// What every report line begins with.
const REPORTED: &str = "reported ";

/// The variable naming the one state whose report the stand-in refuses.
const REFUSES_REPORT: &str = "PRINTOBSERVER_FIXTURE_REFUSES_REPORT";

/// The manager, as a line on standard output per report, refusing the report
/// of the state it was told to refuse.
struct PrintingReporter {
    refuses: Option<String>,
}

impl StatusReporter for PrintingReporter {
    fn report(&mut self, report: StatusReport) -> Result<(), String> {
        let state = report.state().name();
        if self.refuses.as_deref() == Some(state) {
            return Err(format!("the stand-in manager refused the {state} report"));
        }
        let mut out = std::io::stdout().lock();
        writeln!(
            out,
            "{REPORTED}{state} wait-hint-ms={} exit={}",
            report.wait_hint().as_millis(),
            report.exit_code()
        )
        .and_then(|()| out.flush())
        .map_err(|error| error.to_string())
    }
}

/// `--config <path>`, and nothing else.
fn configuration() -> Result<PathBuf, String> {
    let arguments: Vec<String> = std::env::args().skip(1).collect();
    match arguments.as_slice() {
        [flag, path] if flag == "--config" => Ok(PathBuf::from(path)),
        _ => Err("usage: printobserver-service-fixture --config <path>".to_owned()),
    }
}

fn main() -> ExitCode {
    let config = match configuration() {
        Ok(config) => config,
        Err(usage) => {
            eprintln!("{usage}");
            return ExitCode::from(2);
        }
    };
    // The stop control: the stand-in manager sends it by writing a line, or by
    // closing the stream, which is what a manager that went away would amount
    // to.
    let (stop, stopped) = oneshot::channel::<()>();
    std::thread::spawn(move || {
        let mut line = String::new();
        let _ = std::io::stdin().lock().read_line(&mut line);
        let _ = stop.send(());
    });
    let runtime = match tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
    {
        Ok(runtime) => runtime,
        Err(error) => {
            eprintln!("the fixture could not start a runtime: {error}");
            return ExitCode::from(7);
        }
    };
    let mut reporter = PrintingReporter {
        refuses: std::env::var(REFUSES_REPORT).ok(),
    };
    let exit = runtime.block_on(run(&config, &mut reporter, async move {
        let _ = stopped.await;
    }));
    ExitCode::from(exit.status())
}
