//! A pass-through in front of a supervision turn's harness, for the real-print replay.
//!
//! Installed under the harness's own name ahead of it on the server's search
//! path, it starts the program named in the `launch.json` beside it with exactly
//! the arguments, input and environment it was handed, and copies that program's
//! output through byte for byte. Nothing the agent sees changes. Beside that it
//! does the two things the replay can learn nowhere else:
//!
//! - it keeps a copy of the output, the only place a turn's usage, models and
//!   compaction boundaries exist, in the file the replay's control answer names;
//! - it tells the replay when the harness has exited and waits for the answer,
//!   so the replay's stand-ins stand at the next event before the server can
//!   release the print and let its scheduled check fire.
//!
//! A program rather than a script because the wiring tests that run it are in the
//! gate on every platform, and a Windows harness has to be an executable.

use std::io::{Read as _, Write as _};
use std::net::TcpStream;
use std::path::PathBuf;
use std::process::{Command, ExitCode, Stdio};

/// The file beside this program saying what to start and whom to tell.
const LAUNCH_FILE: &str = "launch.json";

/// One request to the replay's control address, and the body of its answer.
fn tell(control: &str, path: &str) -> std::io::Result<String> {
    let mut stream = TcpStream::connect(control)?;
    stream.write_all(
        format!(
            "POST {path} HTTP/1.1\r\nHost: {control}\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
        )
        .as_bytes(),
    )?;
    let mut answer = String::new();
    stream.read_to_string(&mut answer)?;
    Ok(answer
        .split_once("\r\n\r\n")
        .map_or_else(String::new, |(_, body)| body.to_owned()))
}

/// What this launch was told to do, read from the file beside the program.
struct Launch {
    /// Where the replay's control endpoint answers.
    control: String,
    /// The program to start, and the arguments that precede this launch's own.
    target: Vec<String>,
}

/// The launch file, or why it could not be read.
fn launch() -> Result<Launch, String> {
    let beside = std::env::current_exe()
        .map_err(|error| format!("this program cannot find itself: {error}"))?
        .with_file_name(LAUNCH_FILE);
    let text = std::fs::read_to_string(&beside)
        .map_err(|error| format!("{} is unreadable: {error}", beside.display()))?;
    let document: serde_json::Value = serde_json::from_str(&text)
        .map_err(|error| format!("{} is not JSON: {error}", beside.display()))?;
    let control = document["control"]
        .as_str()
        .ok_or_else(|| format!("{} names no control address", beside.display()))?
        .to_owned();
    let target: Vec<String> = document["target"]
        .as_array()
        .map(|words| {
            words
                .iter()
                .filter_map(|word| word.as_str().map(str::to_owned))
                .collect()
        })
        .unwrap_or_default();
    if target.is_empty() {
        return Err(format!("{} names no program to start", beside.display()));
    }
    Ok(Launch { control, target })
}

/// Start the target, copy its output through and keep it, and answer its status.
fn run(launch: &Launch, record: &PathBuf) -> Result<ExitCode, String> {
    let (program, before) = launch
        .target
        .split_first()
        .ok_or_else(|| "no program to start".to_owned())?;
    let mut child = Command::new(program)
        .args(before)
        .args(std::env::args_os().skip(1))
        .stdin(Stdio::inherit())
        .stderr(Stdio::inherit())
        .stdout(Stdio::piped())
        .spawn()
        .map_err(|error| format!("{program} could not be started: {error}"))?;
    let mut kept = std::fs::File::create(record)
        .map_err(|error| format!("{} could not be created: {error}", record.display()))?;
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
    let record = tell(&launch.control, "/started")
        .map_err(|error| format!("{} did not answer: {error}", launch.control))?;
    let status = run(&launch, &PathBuf::from(record.trim()))?;
    tell(&launch.control, "/exited")
        .map_err(|error| format!("{} did not answer: {error}", launch.control))?;
    Ok(status)
}

fn main() -> ExitCode {
    launched().unwrap_or_else(|why| {
        eprintln!("printobserver-server-turn-launcher: {why}");
        ExitCode::FAILURE
    })
}
