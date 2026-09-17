//! A recording stand-in for a harness program, and nothing else.
//!
//! It replaces the paid provider process and only that: whatever runs it —
//! `printobserver sign-in`, or `OneHarness` on a supervision turn — runs it
//! exactly as it would run the real program, found by name on the caller's
//! path. What it does is write down what it was handed.
//!
//! **Its sign-in** records its arguments, the user it ran as, the directory the
//! adapter's table variable named, its `HOME`, whether its standard input is its
//! parent's own, and how many sockets its parent holds; then writes a simulated
//! sign-in state file into that directory, prompts on standard error, and echoes
//! what it was answered on standard output.
//!
//! **Its supervision turn** refuses to answer unless that state file is there,
//! records beside it what it read, and answers the assessment it was given.
//!
//! Nothing here spells a variable's name: each stand-in is written for one entry
//! of the adapter's own table, and reads the variable that entry declares.

use std::path::{Path, PathBuf};
use std::sync::{Mutex, MutexGuard};

use printobserver_server::HarnessSignIn;

/// Held while a stand-in is written and while a journey starts a process.
///
/// A process forked on one test thread while another is still writing a
/// stand-in inherits that write for the moment before it runs its own program,
/// and the stand-in cannot be run while anything holds it open for writing —
/// the run fails as "text file busy", about a file nothing is still writing.
static FORKING: Mutex<()> = Mutex::new(());

/// Take the lock every stand-in write and every process start holds.
pub fn forking() -> MutexGuard<'static, ()> {
    FORKING
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

/// The simulated sign-in state file a stand-in's sign-in writes.
pub const SIGNED_IN: &str = "signed-in";

/// What a stand-in's sign-in writes into [`SIGNED_IN`].
pub const SIGN_IN_STATE: &str = "a-simulated-sign-in";

/// The file a stand-in's sign-in records into, beside the state file.
pub const SIGN_IN_SEEN: &str = "sign-in-seen";

/// The file a stand-in's supervision turn records into, beside the state file.
pub const TURN_SEEN: &str = "turn-seen";

/// What a stand-in's sign-in prints on standard output for the line it was
/// answered, before the line itself.
pub const ANSWERED: &str = "was answered: ";

/// Put a stand-in for one harness program in a directory, and answer its path.
///
/// `invocations` is a file every invocation of it appends one line to, whatever
/// it was asked — which is what lets a journey assert it was never run at all.
/// `status` is what its sign-in exits with, and `answer` is the document its
/// supervision turn prints.
///
/// Each record it writes appears whole: it is written beside its name and
/// renamed into place, because a journey polls for the record and a read that
/// landed between two of its lines would find a record with no `state`.
pub fn stand_in(
    directory: &Path,
    harness: &HarnessSignIn,
    invocations: &Path,
    status: u8,
    answer: &str,
) -> PathBuf {
    let program = harness.program();
    let variable = harness.config_env();
    // The answer is printed single-quoted, which is a quoting a document
    // carrying a single quote would end early.
    assert!(
        !answer.contains('\''),
        "a stand-in's answer carries a single quote"
    );
    let script = format!(
        r#"#!/bin/sh
umask 077
echo "{program} $*" >> "{invocations}" 2>/dev/null
dir="${{{variable}}}"
case "$1" in
    -p)
        if [ ! -f "$dir/{SIGNED_IN}" ]; then
            echo "{program}: not signed in, no state in '$dir'" >&2
            exit 1
        fi
        {{
            echo "user=$(id -un)"
            echo "directory=$dir"
            echo "home=${{HOME:-}}"
            echo "state=$(cat "$dir/{SIGNED_IN}")"
        }} > "$dir/{TURN_SEEN}.part"
        mv "$dir/{TURN_SEEN}.part" "$dir/{TURN_SEEN}"
        printf '%s\n' '{answer}'
        exit 0
        ;;
    --version)
        echo "0.0.0 (a recording stand-in for {program})"
        exit 0
        ;;
esac
{{
    echo "argv=$*"
    echo "user=$(id -un)"
    echo "directory=$dir"
    echo "cwd=$(pwd)"
    echo "home=${{HOME:-}}"
    echo "stdin=$(readlink /proc/self/fd/0)"
    echo "parent_stdin=$(readlink /proc/$PPID/fd/0)"
    echo "parent_sockets=$(ls -l /proc/$PPID/fd | grep -c 'socket:')"
}} > "$dir/{SIGN_IN_SEEN}.part"
mv "$dir/{SIGN_IN_SEEN}.part" "$dir/{SIGN_IN_SEEN}"
echo "{SIGN_IN_STATE}" > "$dir/{SIGNED_IN}"
echo "{program} is asking for the code it printed a link to" >&2
if IFS= read -r answered; then
    echo "{program} {ANSWERED}$answered"
fi
exit {status}
"#,
        invocations = invocations.display(),
    );
    let _held = forking();
    std::fs::create_dir_all(directory).expect("a directory for the stand-in");
    let path = directory.join(program);
    std::fs::write(&path, script).expect("the stand-in is writable");
    let mut mode = std::fs::metadata(&path)
        .expect("the stand-in is there")
        .permissions();
    std::os::unix::fs::PermissionsExt::set_mode(&mut mode, 0o755);
    std::fs::set_permissions(&path, mode).expect("the stand-in is executable");
    path
}

/// One `key=value` line a stand-in recorded.
pub fn recorded(recording: &str, key: &str) -> String {
    recording
        .lines()
        .find_map(|line| line.strip_prefix(&format!("{key}=")))
        .unwrap_or_else(|| panic!("the stand-in recorded no {key}: {recording}"))
        .to_owned()
}

/// The document a stand-in's supervision turn answers with: an assessment the
/// generated schema accepts, in the result document `OneHarness` reads a
/// Claude Code run's answer out of.
pub fn assessment_answer() -> String {
    let assessment = printobserver_types::serde_json::json!({
        "summary": "the first layer is down and the walls are clean",
        "confidence": "high",
        "should_continue": true,
        "did": "read the context and changed nothing",
        "why": "nothing about the print needs an intervention",
        "escalating": false,
    })
    .to_string();
    printobserver_types::serde_json::json!({
        "type": "result",
        "result": assessment,
        "is_error": false,
        "session_id": "0198a000-0000-7000-8000-000000000042",
    })
    .to_string()
}
