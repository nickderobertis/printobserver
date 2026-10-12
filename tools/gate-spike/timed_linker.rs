//! Spike-only linker wrapper: runs the real linker and records how long it took.
//!
//! `measure.py` builds this with `rustc` and names it as cargo's linker for the
//! host target. `SPIKE_REAL_LINKER` names the linker it stands in front of
//! (`link.exe` on Windows, `cc` elsewhere) and `SPIKE_LINK_TIMINGS` the file one
//! JSON line per link is appended to, in the shape `spike_names.LinkTiming`
//! reads. It exits as the real linker did: with its code, or on Unix with 128
//! plus the signal that ended it, as a shell reports one.

use std::io::Write as _;
use std::process::{Command, ExitStatus};
use std::time::Instant;

/// The exit code a shell would report for how the real linker ended.
fn code_of(status: ExitStatus) -> i32 {
    #[cfg(unix)]
    {
        use std::os::unix::process::ExitStatusExt as _;
        if let Some(signal) = status.signal() {
            return 128 + signal;
        }
    }
    status.code().unwrap_or(1)
}

/// What the link wrote: `/OUT:` on Windows, `-o` elsewhere, or the response file.
fn output_of(arguments: &[String]) -> String {
    arguments
        .iter()
        .find_map(|argument| argument.strip_prefix("/OUT:").map(str::to_owned))
        .or_else(|| {
            arguments
                .iter()
                .position(|argument| argument == "-o")
                .and_then(|at| arguments.get(at + 1).cloned())
        })
        .or_else(|| arguments.iter().find(|a| a.starts_with('@')).cloned())
        .unwrap_or_default()
}

fn main() {
    let real = std::env::var("SPIKE_REAL_LINKER")
        .unwrap_or_else(|_| if cfg!(windows) { "link.exe" } else { "cc" }.to_owned());
    let arguments: Vec<String> = std::env::args().skip(1).collect();
    let started = Instant::now();
    let status = Command::new(&real).args(&arguments).status();
    let elapsed = started.elapsed();
    if let Ok(sink) = std::env::var("SPIKE_LINK_TIMINGS") {
        if let Ok(mut file) = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&sink)
        {
            let output = output_of(&arguments).replace('\\', "/").replace('"', "'");
            let written = writeln!(
                file,
                "{{\"linker\":\"{}\",\"output\":\"{output}\",\"ms\":{:.3}}}",
                real.replace('\\', "/").replace('"', "'"),
                elapsed.as_secs_f64() * 1000.0
            );
            if let Err(error) = written {
                eprintln!("timed-linker: could not record this link in {sink}: {error}");
            }
        }
    }
    match status {
        Ok(status) => std::process::exit(code_of(status)),
        Err(error) => {
            eprintln!("timed-linker: `{real}` could not run: {error}");
            std::process::exit(127);
        }
    }
}
