//! What one invocation's own process tree did, recorded from inside it.
//!
//! # Why this exists beside `strace`
//!
//! On Linux this tier observes a program with `strace`. macOS has no `strace`,
//! and its own tracer, `dtruss`, needs System Integrity Protection turned off,
//! which no hosted runner has. So there the observation is made from inside the
//! program instead: `interposer.c`, built here with the system's own `cc` into
//! the run's scratch directory, is loaded into the program through
//! `DYLD_INSERT_LIBRARIES` and replaces `connect` and the file-access calls
//! through dyld's interposing table. The variable is inherited, so every child
//! the program starts is observed too, exactly as `strace -f` follows one.
//!
//! # What SIP strips, and why that does not weaken the claim
//!
//! dyld ignores every `DYLD_*` variable for a binary SIP protects — anything
//! under `/usr/bin` or `/System`, say — and for a program signed with the
//! hardened runtime. A process of that kind that the program started would run
//! unobserved. What this tier holds is a property of **this program** and of the
//! processes it starts itself, and the program under test is a cargo build that
//! is not hardened-runtime signed; the load is not assumed either, because the
//! library records `loaded <pid>` from its constructor and a run whose own
//! process did not record one panics rather than reading as a run that did
//! nothing. The client commands start no system binary to reach anything: the
//! one place a connection is opened is this program's own transport.
//!
//! # One library, both platforms
//!
//! The same source builds on Linux, loaded with `LD_PRELOAD`, so the recording
//! and the reading of it are proven on every host this tier runs on — not only
//! on the runners where they are the only observation there is.

use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};

/// The interposer's source, compiled into each scratch directory that needs it.
const SOURCE: &str = include_str!("interposer.c");

/// The variable naming the file the interposer appends what it saw to.
const LOG_ENV: &str = "PRINTOBSERVER_INTERPOSE_LOG";

/// The variable naming the paths it refuses, one per line.
const WATCH_ENV: &str = "PRINTOBSERVER_INTERPOSE_WATCH";

/// The variable the dynamic loader reads a library to insert from.
#[cfg(target_os = "macos")]
const LOADER_ENV: &str = "DYLD_INSERT_LIBRARIES";
/// The variable the dynamic loader reads a library to insert from.
#[cfg(not(target_os = "macos"))]
const LOADER_ENV: &str = "LD_PRELOAD";

/// The programs the loader will not insert a library into.
#[cfg(target_os = "macos")]
const IGNORED_FOR: &str = "a SIP-protected program and a hardened-runtime one";
/// The programs the loader will not insert a library into.
#[cfg(not(target_os = "macos"))]
const IGNORED_FOR: &str = "a set-user-ID program and a statically linked one";

/// The file the built library is kept under.
#[cfg(target_os = "macos")]
const LIBRARY: &str = "interposer.dylib";
/// The file the built library is kept under.
#[cfg(not(target_os = "macos"))]
const LIBRARY: &str = "interposer.so";

/// What `cc` is told before the source: build a library to be inserted.
#[cfg(target_os = "macos")]
const LINKED_AS: &[&str] = &["-dynamiclib"];
/// What `cc` is told before the source: build a library to be inserted.
#[cfg(not(target_os = "macos"))]
const LINKED_AS: &[&str] = &["-shared", "-fPIC"];

/// What `cc` is told after the source: the libraries it calls into.
#[cfg(target_os = "macos")]
const LINKED_WITH: &[&str] = &[];
/// What `cc` is told after the source: the libraries it calls into.
#[cfg(not(target_os = "macos"))]
const LINKED_WITH: &[&str] = &["-ldl"];

/// How one run's own files are named apart from every other's.
static RUNS: AtomicU64 = AtomicU64::new(0);

/// One run under the interposer.
pub struct Interposed {
    /// What the program exited with and printed.
    pub output: Output,
    /// Every line the interposer recorded for the process tree, with the lines
    /// saying where it was loaded taken out: empty when the tree connected to
    /// nothing and touched no watched path.
    pub activity: String,
}

impl Interposed {
    /// Every access to a watched path the interposer refused, one per line and
    /// with every connection taken out: empty when the tree touched no watched
    /// path, and naming the path and `EACCES` for each one it did.
    pub fn touched(&self) -> String {
        let mut touched = String::new();
        for line in self.activity.lines() {
            if !line.starts_with("connected ") {
                touched.push_str(line);
                touched.push('\n');
            }
        }
        touched
    }
}

/// Run one command with the interposer loaded into it, refusing every access
/// to a watched path.
///
/// # Panics
///
/// Panics when the interposer cannot be built, when the command cannot be
/// started, or when the interposer was not loaded into the command's own
/// process — each of which is a run whose recording says nothing about it.
pub fn interposed(mut command: Command, scratch: &Path, watching: &[PathBuf]) -> Interposed {
    let library = built(scratch);
    let log = scratch.join(unique("interposed", "log"));
    std::fs::write(&log, "").expect("the interposer's log is writable");
    command
        .env(LOADER_ENV, &library)
        .env(LOG_ENV, &log)
        .env(WATCH_ENV, watched(watching))
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let child = command
        .spawn()
        .unwrap_or_else(|error| panic!("the program could not be started to be observed: {error}"));
    let root = child.id();
    let output = child
        .wait_with_output()
        .unwrap_or_else(|error| panic!("the observed program could not be waited for: {error}"));
    let recorded = std::fs::read_to_string(&log)
        .unwrap_or_else(|error| panic!("the interposer's log {} reads: {error}", log.display()));
    let activity = activity(&recorded, root).unwrap_or_else(|| {
        panic!(
            "the interposer at {} was not loaded into process {root}, so nothing it recorded \
             says what that process did. {LOADER_ENV} is ignored for {IGNORED_FOR}; this \
             tier's claims about what the program reaches rest on the observation. It \
             said:\n{}{}",
            library.display(),
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr),
        )
    });
    Interposed { output, activity }
}

/// What a recording says the tree did, when the recording is of `root`.
///
/// `None` when the interposer never said it was loaded into that process.
pub fn activity(recorded: &str, root: u32) -> Option<String> {
    let root = root.to_string();
    let mut loaded = false;
    let mut kept = String::new();
    for line in recorded.lines() {
        if let Some(process) = line.strip_prefix("loaded ") {
            loaded |= process == root;
        } else {
            kept.push_str(line);
            kept.push('\n');
        }
    }
    loaded.then_some(kept)
}

/// Every watched path as the interposer reads them: each as given and, where it
/// differs, as the filesystem resolves it, one per line.
fn watched(watching: &[PathBuf]) -> String {
    let mut named: Vec<String> = Vec::new();
    for path in watching {
        named.push(path.display().to_string());
        if let Ok(resolved) = std::fs::canonicalize(path)
            && resolved != *path
        {
            named.push(resolved.display().to_string());
        }
    }
    named.join("\n")
}

/// The interposer, built into one scratch directory once.
fn built(scratch: &Path) -> PathBuf {
    let library = scratch.join(LIBRARY);
    if library.is_file() {
        return library;
    }
    let source = scratch.join(unique("interposer", "c"));
    std::fs::write(&source, SOURCE).expect("the interposer's source is writable");
    let staged = scratch.join(unique("interposer", "staged"));
    let compiled = Command::new("cc")
        .args(LINKED_AS)
        .arg("-o")
        .arg(&staged)
        .arg(&source)
        .args(LINKED_WITH)
        .output()
        .unwrap_or_else(|error| {
            panic!(
                "`cc` could not run to build the interposer, and this tier's observation of \
                 what a run reaches rests on it: {error}"
            )
        });
    assert!(
        compiled.status.success(),
        "`cc` did not build the interposer, and this tier's observation of what a run reaches \
         rests on it:\n{}",
        String::from_utf8_lossy(&compiled.stderr)
    );
    // Renamed into place whole, so a run beside this one never loads half a file.
    std::fs::rename(&staged, &library).expect("the built interposer moves into place");
    library
}

/// A file name no other run in this process or any other shares.
fn unique(stem: &str, extension: &str) -> String {
    format!(
        "{stem}-{}-{}.{extension}",
        std::process::id(),
        RUNS.fetch_add(1, Ordering::Relaxed)
    )
}
