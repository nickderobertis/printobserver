//! Spike-only timing sink for `spike-windows-gate-breakdown`.
//!
//! When `PRINTOBSERVER_SPIKE_TIMINGS` names a file, every operation timed here
//! appends one JSON line to it: what kind of operation it was, how long it took,
//! and which test process and thread took it. Unset, nothing is written and
//! nothing about the tier changes: no assertion, deadline or order reads it.
//! `tools/gate-spike/measure.py` sets the variable and folds the lines into its
//! report; the line is the shape `tools/gate-spike/spike_names.py`'s
//! `JourneyTiming` reads, and `tools/gate-spike/tests/test_records.py` holds the
//! two together by running `the_spike_timing_sink_records_a_timed_operation`.

use std::fmt::Write as _;
use std::io::Write as _;
use std::time::Instant;

/// Read on every call, so a process started without it writes nothing.
const SINK: &str = "PRINTOBSERVER_SPIKE_TIMINGS";

/// Record one operation of `kind` that began at `started`.
pub fn note(kind: &str, started: Instant) {
    let Some(sink) = std::env::var_os(SINK) else {
        return;
    };
    let elapsed = started.elapsed();
    let thread = std::thread::current();
    let line = format!(
        "{{\"kind\":\"{}\",\"ms\":{:.3},\"pid\":{},\"thread\":\"{}\"}}\n",
        escaped(kind),
        elapsed.as_secs_f64() * 1000.0,
        std::process::id(),
        escaped(thread.name().unwrap_or("unnamed")),
    );
    // One write of the whole line, so lines from parallel test processes do not
    // interleave inside each other. A sink that cannot take it is said rather
    // than failing the test it is timing: the record is the spike's, not the
    // test's.
    let written = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&sink)
        .and_then(|mut file| file.write_all(line.as_bytes()));
    if let Err(error) = written {
        eprintln!(
            "spike timing sink: `{kind}` was not recorded in {}: {error}",
            std::path::Path::new(&sink).display()
        );
    }
}

/// Run `operation`, recording how long it took as one `kind`.
pub fn timed<T>(kind: &str, operation: impl FnOnce() -> T) -> T {
    let started = Instant::now();
    let answer = operation();
    note(kind, started);
    answer
}

/// A string as the inside of a JSON string literal.
fn escaped(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for character in text.chars() {
        match character {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            control if control.is_control() => {
                let _ = write!(out, "\\u{:04x}", u32::from(control));
            }
            other => out.push(other),
        }
    }
    out
}
