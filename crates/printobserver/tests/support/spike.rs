//! Spike-only timing sink for `spike-windows-gate-breakdown`.
//!
//! When `PRINTOBSERVER_SPIKE_TIMINGS` names a file, every operation timed here
//! appends one JSON line to it: what kind of operation it was, how long it took,
//! and which test process and thread took it. Unset, nothing is written and
//! nothing about the tier changes: no assertion, deadline or order reads it.
//! The temporary workflow binds the sink and collects its records. The tests
//! below drive the real traced socket walk, including JSON escaping through
//! its actual thread name. All measurement instrumentation is removed after
//! the draft's paired before/after runs.

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

/// The measurement environment binding stays the same in the producer and in
/// the temporary workflow collecting its records.
#[test]
fn the_measurement_jobs_bind_the_producers_sink() {
    let workflow = include_str!("../../../../.github/workflows/ci.yml");
    let bound: Vec<&str> = workflow
        .lines()
        .filter_map(|line| line.trim().strip_prefix(&format!("{SINK}: ")))
        .collect();
    assert_eq!(bound.len(), 2);
    let uploaded: Vec<&str> = workflow
        .lines()
        .filter_map(|line| line.trim().strip_prefix("path: "))
        .collect();
    assert_eq!(
        uploaded, bound,
        "each artifact must collect its producer's sink"
    );
}

/// The temporary measurement wiring observes a real socket/process walk,
/// including a disabled sink and a sink the filesystem refuses.
#[test]
fn a_real_observation_writes_only_to_an_enabled_writable_sink() {
    use std::process::Command;

    let scratch = tempfile::TempDir::new().expect("measurement scratch");
    let sink = scratch.path().join("timings.jsonl");
    for destination in [None, Some(sink.as_path()), Some(scratch.path())] {
        let mut process = Command::new(std::env::current_exe().expect("test binary"));
        process.args([
            "--exact",
            "traced::regression::each_invocation_observes_its_descendants_and_refused_attempts_only",
            "--nocapture",
        ]);
        process.env_remove(SINK);
        if let Some(destination) = destination {
            process.env(SINK, destination);
        }
        let output = process.output().expect("real measured walk");
        assert!(
            output.status.success(),
            "{}{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        match destination {
            None => assert!(!sink.exists()),
            Some(destination) if destination.is_file() => {
                let records = std::fs::read_to_string(destination).expect("timing records");
                let records: Vec<printobserver_types::serde_json::Value> = records
                    .lines()
                    .map(|line| {
                        printobserver_types::serde_json::from_str(line).expect("JSON timing record")
                    })
                    .collect();
                let phases: std::collections::BTreeSet<&str> = records
                    .iter()
                    .map(|record| record["kind"].as_str().expect("phase"))
                    .collect();
                #[cfg(windows)]
                let expected = [
                    "traced:etw-lock-wait",
                    "traced:etw-logman-start",
                    "traced:etw-logman-stop",
                    "traced:etw-tracerpt",
                    "traced:etw-decode",
                    "traced:invocation-run",
                ];
                #[cfg(target_os = "macos")]
                let expected = ["traced:interposed-run"];
                #[cfg(not(any(windows, target_os = "macos")))]
                let expected = ["traced:strace-run"];
                assert_eq!(phases, expected.into_iter().collect());
                for record in records {
                    assert!(
                        record["kind"]
                            .as_str()
                            .expect("phase")
                            .starts_with("traced:")
                    );
                    assert!(record["ms"].as_f64().expect("duration") >= 0.0);
                    assert!(record["pid"].as_u64().expect("process") > 0);
                    assert_eq!(record["thread"], super::regression::OBSERVING_THREAD);
                }
            }
            Some(_) => {
                assert!(String::from_utf8_lossy(&output.stderr).contains("was not recorded"));
            }
        }
    }
}
