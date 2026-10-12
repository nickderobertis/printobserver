//! Spike-only linker wrapper: runs the real linker and records how long it took.
//!
//! `measure.py` builds this with `rustc` and names it as cargo's linker for the
//! host target. `SPIKE_REAL_LINKER` names the linker it stands in front of
//! (`link.exe` on Windows, `cc` elsewhere) and `SPIKE_LINK_TIMINGS` the file one
//! JSON line per link is appended to. Its exit status is the real linker's.

use std::io::Write as _;
use std::process::{Command, ExitCode};
use std::time::Instant;

fn main() -> ExitCode {
    let real = std::env::var("SPIKE_REAL_LINKER")
        .unwrap_or_else(|_| if cfg!(windows) { "link.exe" } else { "cc" }.to_owned());
    let arguments: Vec<String> = std::env::args().skip(1).collect();
    let started = Instant::now();
    let status = Command::new(&real).args(&arguments).status();
    let elapsed = started.elapsed();
    let output = arguments
        .iter()
        .find_map(|argument| argument.strip_prefix("/OUT:").map(str::to_owned))
        .or_else(|| {
            arguments
                .iter()
                .position(|argument| argument == "-o")
                .and_then(|at| arguments.get(at + 1).cloned())
        })
        .or_else(|| arguments.iter().find(|a| a.starts_with('@')).cloned())
        .unwrap_or_default();
    if let Ok(sink) = std::env::var("SPIKE_LINK_TIMINGS") {
        if let Ok(mut file) = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(sink)
        {
            let output = output.replace('\\', "/").replace('"', "'");
            let _ = writeln!(
                file,
                "{{\"linker\":\"{}\",\"output\":\"{output}\",\"ms\":{:.3}}}",
                real.replace('\\', "/"),
                elapsed.as_secs_f64() * 1000.0
            );
        }
    }
    match status {
        Ok(status) => ExitCode::from(u8::try_from(status.code().unwrap_or(1)).unwrap_or(1)),
        Err(error) => {
            eprintln!("timed-linker: `{real}` could not run: {error}");
            ExitCode::from(1)
        }
    }
}
