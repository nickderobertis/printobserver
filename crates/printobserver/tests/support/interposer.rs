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
//! # When the recording is complete
//!
//! A descendant may outlive the program it was started by, close the output
//! streams it inherited and only then connect somewhere, so neither the root's
//! exit nor the end of its output says the tree has finished. The library holds
//! one connection to a Unix socket this module listens on — made by the first
//! observed process, inherited by every one after it, and never closed by any
//! of them — and the recording is read only once that connection has reached
//! its end, which is the last process of the tree exiting. A tree that does not
//! finish within [`LIFECYCLE_BOUND`], or that never connected at all, is
//! refused rather than read as a tree that did nothing more.
//!
//! # One library, both platforms
//!
//! The same source builds on Linux, loaded with `LD_PRELOAD`, so the recording
//! and the reading of it are proven on every host this tier runs on — not only
//! on the runners where they are the only observation there is.

use std::io::{ErrorKind, Read as _};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

/// The interposer's source, compiled into each scratch directory that needs it.
const SOURCE: &str = include_str!("interposer.c");

/// The variable naming the file the interposer appends what it saw to.
const LOG_ENV: &str = "PRINTOBSERVER_INTERPOSE_LOG";

/// The variable naming the paths it refuses, one per line.
const WATCH_ENV: &str = "PRINTOBSERVER_INTERPOSE_WATCH";

/// The variable naming the socket the tree's lifecycle connection is made to.
const LIFECYCLE_ENV: &str = "PRINTOBSERVER_INTERPOSE_LIFECYCLE";

/// The variable naming the descriptor an ancestor holds that connection on,
/// which no process this module starts may arrive already carrying.
const LIFECYCLE_FD_ENV: &str = "PRINTOBSERVER_INTERPOSE_LIFECYCLE_FD";

/// How long a tree is given, after its root has exited, for its last process
/// to finish: a bound on a descendant that never does, rather than a pause.
pub const LIFECYCLE_BOUND: Duration = Duration::from_secs(20);

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
pub fn interposed(command: Command, scratch: &Path, watching: &[PathBuf]) -> Interposed {
    interposed_within(command, scratch, watching, LIFECYCLE_BOUND)
}

/// [`interposed`], giving the tree `bound` after its root exits to finish.
///
/// # Panics
///
/// As [`interposed`] does, and when the tree has not finished within `bound`
/// or never made its lifecycle connection — each a recording that cannot be
/// read as complete.
pub fn interposed_within(
    mut command: Command,
    scratch: &Path,
    watching: &[PathBuf],
    bound: Duration,
) -> Interposed {
    let library = built(scratch);
    let log = scratch.join(unique("interposed", "log"));
    std::fs::write(&log, "").expect("the interposer's log is writable");
    let lifecycle = Lifecycle::listening();
    command
        .env(LOADER_ENV, &library)
        .env(LOG_ENV, &log)
        .env(WATCH_ENV, watched(watching))
        .env(LIFECYCLE_ENV, &lifecycle.path)
        .env_remove(LIFECYCLE_FD_ENV)
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
    let loaded = std::fs::read_to_string(&log)
        .unwrap_or_else(|error| panic!("the interposer's log {} reads: {error}", log.display()));
    // Read before the tree's end is waited for, so a library that was never
    // loaded is said at once rather than after the wait runs out.
    assert!(
        activity(&loaded, root).is_some(),
        "the interposer at {} was not loaded into process {root}, so nothing it recorded \
         says what that process did. {LOADER_ENV} is ignored for {IGNORED_FOR}; this tier's \
         claims about what the program reaches rest on the observation. It said:\n{}{}",
        library.display(),
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    );
    if let Err(incomplete) = lifecycle.finished(bound) {
        panic!(
            "the observation of process {root} is not complete, so what the interposer \
             recorded cannot be read as everything its tree did: {incomplete}"
        );
    }
    let recorded = std::fs::read_to_string(&log)
        .unwrap_or_else(|error| panic!("the interposer's log {} reads: {error}", log.display()));
    let activity = activity(&recorded, root)
        .expect("a recording that said the library was loaded still says so");
    Interposed { output, activity }
}

/// The socket an observed tree holds its one lifecycle connection to.
pub struct Lifecycle {
    listener: UnixListener,
    /// Where it listens, which the tree is told through its environment.
    pub path: PathBuf,
}

impl Lifecycle {
    /// Listen at a path of this run's own, short enough for any platform's
    /// socket address: the system's temporary directory rather than a scratch
    /// directory nested beneath it.
    pub fn listening() -> Self {
        let path = std::env::temp_dir().join(unique("po-lifecycle", "sock"));
        let listener = UnixListener::bind(&path).unwrap_or_else(|error| {
            panic!(
                "the interposer's lifecycle socket {} could not be bound: {error}",
                path.display()
            )
        });
        listener
            .set_nonblocking(true)
            .expect("the lifecycle socket can be polled");
        Self { listener, path }
    }

    /// Wait until every connection the tree made has reached its end, for at
    /// most `bound`.
    ///
    /// # Errors
    ///
    /// Says why the tree cannot be taken as finished: no process ever made the
    /// connection, or some process still held it when `bound` ran out.
    pub fn finished(&self, bound: Duration) -> Result<(), String> {
        let deadline = Instant::now() + bound;
        let mut held: Vec<UnixStream> = Vec::new();
        let mut connected = 0_usize;
        loop {
            loop {
                match self.listener.accept() {
                    Ok((stream, _)) => {
                        stream
                            .set_nonblocking(true)
                            .expect("a lifecycle connection can be polled");
                        held.push(stream);
                        connected += 1;
                    }
                    Err(error) if error.kind() == ErrorKind::WouldBlock => break,
                    Err(error) => return Err(format!("the lifecycle socket refused: {error}")),
                }
            }
            held.retain_mut(still_held);
            if connected > 0 && held.is_empty() {
                return Ok(());
            }
            if Instant::now() >= deadline {
                return Err(if connected == 0 {
                    format!(
                        "no process of the tree made its lifecycle connection to {} within \
                         {bound:?}",
                        self.path.display()
                    )
                } else {
                    format!(
                        "{} of the {connected} lifecycle connections the tree made were still \
                         held {bound:?} after its root exited: a process it started is still \
                         running",
                        held.len()
                    )
                });
            }
            std::thread::sleep(Duration::from_millis(10));
        }
    }
}

/// Whether a lifecycle connection is still held by some process of the tree.
///
/// Nothing is ever written on it, so what a read answers is whether it has
/// reached its end: no bytes and no error once the last holder has exited.
fn still_held(stream: &mut UnixStream) -> bool {
    let mut ignored = [0_u8; 64];
    loop {
        match stream.read(&mut ignored) {
            Ok(0) => return false,
            Err(error) if error.kind() == ErrorKind::WouldBlock => return true,
            Ok(_) => {}
            Err(error) if error.kind() == ErrorKind::Interrupted => {}
            // A connection reset by its last holder has ended as surely.
            Err(_) => return false,
        }
    }
}

impl Drop for Lifecycle {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.path);
    }
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
///
/// The variables this module hands it are the ones the C source reads, held
/// to it before the build: a rename on either side is refused here rather
/// than read as a program that touched nothing, or as a descriptor nothing
/// clears any more.
fn built(scratch: &Path) -> PathBuf {
    for (name, value) in [
        ("PO_LOG_ENV", LOG_ENV),
        ("PO_WATCH_ENV", WATCH_ENV),
        ("PO_LIFECYCLE_ENV", LIFECYCLE_ENV),
        ("PO_LIFECYCLE_FD_ENV", LIFECYCLE_FD_ENV),
    ] {
        assert!(
            SOURCE.contains(&format!("#define {name} \"{value}\"")),
            "the interposer's source does not read {value} as {name}"
        );
    }
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

/// A lifecycle descriptor named but not inherited — malformed, carrying more
/// than a number, closed, or reused for a file — is not taken: the process
/// makes a connection of its own, so the tree is finished once it exits
/// rather than read as one nobody connected to.
#[cfg(test)]
mod a_named_descriptor_that_is_not_one {
    use std::process::{Command, Stdio};
    use std::time::Duration;

    use super::{LIFECYCLE_ENV, LIFECYCLE_FD_ENV, LOADER_ENV, Lifecycle, built};

    #[test]
    fn is_replaced_by_a_connection_of_its_own() {
        let scratch = tempfile::TempDir::new().expect("a scratch directory");
        let library = built(scratch.path());
        let file = scratch.path().join("standard-input");
        std::fs::write(&file, "").expect("a file for standard input");
        for named in ["not-a-descriptor", "0x", "7abc", "-1", "4096", "0"] {
            let lifecycle = Lifecycle::listening();
            // Standard input, descriptor 0, is a file here: a descriptor an
            // ancestor's connection could once have had, since reused.
            let input = std::fs::File::open(&file).expect("the file opens");
            // This test binary again, selecting no test: a program the
            // interposer loads into that does nothing and exits.
            let status = Command::new(std::env::current_exe().expect("test executable"))
                .args(["--exact", "no::such::test"])
                .env(LOADER_ENV, &library)
                .env(LIFECYCLE_ENV, &lifecycle.path)
                .env(LIFECYCLE_FD_ENV, named)
                .stdin(input)
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .status()
                .expect("the program runs");
            assert!(status.success(), "the program failed: {status}");
            assert_eq!(
                lifecycle.finished(Duration::from_secs(5)),
                Ok(()),
                "a process told its lifecycle is on {named:?} made no connection of its own"
            );
        }
    }
}
