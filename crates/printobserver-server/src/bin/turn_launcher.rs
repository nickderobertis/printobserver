//! A pass-through in front of a supervision turn's harness, for the real-print replay.
//!
//! Installed under the harness's own name ahead of it on the server's search
//! path, it starts the program named in the `launch.json` beside it with exactly
//! the arguments, input and environment it was handed, and copies that program's
//! output through byte for byte. Nothing the agent sees changes. Beside that it
//! does the two things the replay can learn nowhere else:
//!
//! - it keeps a copy of the output, the one place a run's usage exists, in the
//!   file the replay's control answer names;
//! - it tells the replay when the harness has exited and waits for the answer,
//!   so the replay's stand-ins stand at the next event before the server can
//!   release the print and let its scheduled check fire.
//!
//! A program rather than a script because the wiring tests that run it are in the
//! gate on every platform, and a Windows harness has to be an executable.

use std::io::{Read as _, Write as _};
use std::net::{SocketAddr, TcpStream};
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode, Stdio};

use printobserver_types::serde::Deserialize;

/// The file beside this program saying what to start and whom to tell.
const LAUNCH_FILE: &str = "launch.json";

/// The status an answer the replay took a report under carries.
const ANSWERED: &str = "200";

/// The two things the replay is told about a harness run.
#[derive(Clone, Copy)]
enum Report {
    /// The harness is about to start; the answer is where to keep its output.
    Started,
    /// The harness has exited; the answer is that the replay has moved on.
    Exited,
}

impl Report {
    /// The control path this report is posted to.
    const fn path(self) -> &'static str {
        match self {
            Self::Started => "/started",
            Self::Exited => "/exited",
        }
    }
}

/// One report to the replay's control address, and the body of its answer.
fn tell(control: SocketAddr, report: Report) -> Result<String, String> {
    let failed = |error: std::io::Error| format!("{control} did not answer: {error}");
    let mut stream = TcpStream::connect(control).map_err(failed)?;
    let path = report.path();
    stream
        .write_all(
            format!(
                "POST {path} HTTP/1.1\r\nHost: {control}\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
            )
            .as_bytes(),
        )
        .map_err(failed)?;
    let mut answer = String::new();
    stream.read_to_string(&mut answer).map_err(failed)?;
    let (head, body) = answer
        .split_once("\r\n\r\n")
        .ok_or_else(|| format!("{control} answered {path} with no whole response"))?;
    if head.split_whitespace().nth(1) != Some(ANSWERED) {
        return Err(format!("{control} refused {path}: {head}"));
    }
    Ok(body.to_owned())
}

/// What this launch was told to do, read from the file beside the program.
#[derive(Deserialize)]
#[serde(crate = "printobserver_types::serde", deny_unknown_fields)]
struct Launch {
    /// Where the replay's control endpoint answers.
    control: SocketAddr,
    /// The program to start.
    program: PathBuf,
    /// The arguments that precede this launch's own.
    #[serde(default)]
    arguments: Vec<String>,
}

/// The launch file, or why it could not be read.
fn launch() -> Result<Launch, String> {
    let beside = std::env::current_exe()
        .map_err(|error| format!("this program cannot find itself: {error}"))?
        .with_file_name(LAUNCH_FILE);
    let text = std::fs::read_to_string(&beside)
        .map_err(|error| format!("{} is unreadable: {error}", beside.display()))?;
    serde_json::from_str(&text)
        .map_err(|error| format!("{} is no launch file: {error}", beside.display()))
}

/// Start the target, copy its output through and keep it, and answer its status.
fn run(launch: &Launch, record: &Path) -> Result<ExitCode, String> {
    let program = launch.program.display();
    let mut kept = std::fs::File::create(record)
        .map_err(|error| format!("{} could not be created: {error}", record.display()))?;
    let mut child = Command::new(&launch.program)
        .args(&launch.arguments)
        .args(std::env::args_os().skip(1))
        .stdin(Stdio::inherit())
        .stderr(Stdio::inherit())
        .stdout(Stdio::piped())
        .spawn()
        .map_err(|error| format!("{program} could not be started: {error}"))?;
    let mut from_child = child
        .stdout
        .take()
        .ok_or_else(|| format!("{program} was started with no output to read"))?;
    let mut to_caller = std::io::stdout();
    let mut piece = [0_u8; 8192];
    loop {
        let count = from_child
            .read(&mut piece)
            .map_err(|error| format!("{program}'s output could not be read: {error}"))?;
        let Some(read) = piece.get(..count).filter(|read| !read.is_empty()) else {
            break;
        };
        to_caller
            .write_all(read)
            .and_then(|()| to_caller.flush())
            .map_err(|error| format!("the output could not be passed on: {error}"))?;
        kept.write_all(read)
            .map_err(|error| format!("{} could not be written: {error}", record.display()))?;
    }
    let status = child
        .wait()
        .map_err(|error| format!("{program} could not be waited for: {error}"))?;
    Ok(status
        .code()
        .and_then(|code| u8::try_from(code).ok())
        .map_or(ExitCode::FAILURE, ExitCode::from))
}

/// One launch from start to end.
fn launched() -> Result<ExitCode, String> {
    let launch = launch()?;
    let record = tell(launch.control, Report::Started)?;
    let status = run(&launch, Path::new(record.trim()))?;
    tell(launch.control, Report::Exited)?;
    Ok(status)
}

fn main() -> ExitCode {
    launched().unwrap_or_else(|why| {
        eprintln!("printobserver-server-turn-launcher: {why}");
        ExitCode::FAILURE
    })
}
