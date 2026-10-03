//! Which program a harness is started as, under a given search path.
//!
//! On Windows npm installs Claude Code as a `claude.cmd` launcher in front of
//! a native program in its package, and a program started by name is found as
//! an `.exe` alone. The lookup that answers that is plain path arithmetic, so
//! it is driven here on every host over a directory laid out exactly as npm
//! lays one out — the launcher, and the package beside it — rather than only
//! on a Windows runner.

use std::ffi::OsString;
use std::fs;
use std::path::{Path, PathBuf};

use printobserver_oneharness::{HarnessSignIn, SIGN_INS};

use crate::support::Fixture;

/// Where npm's package for Claude Code keeps its native program, relative to
/// the directory npm writes the launcher into.
const NPM_PROGRAM: &str = "node_modules/@anthropic-ai/claude-code/bin/claude.exe";

/// One identity's entry in the adapter's table.
fn entry(identity: &str) -> &'static HarnessSignIn {
    SIGN_INS
        .iter()
        .find(|entry| entry.identity() == identity)
        .unwrap_or_else(|| panic!("`{identity}` is not in the adapter's table"))
}

/// Write an empty file, and the directories above it.
fn touch(path: &Path) -> PathBuf {
    fs::create_dir_all(path.parent().expect("a file has a directory")).expect("its directory");
    fs::write(path, "").expect("the file is writable");
    path.to_path_buf()
}

/// A directory npm installed Claude Code into globally: the launcher, and the
/// package holding the native program it runs.
fn npm_prefix(root: &Path) -> PathBuf {
    touch(&root.join("claude.cmd"));
    touch(&root.join(NPM_PROGRAM));
    root.to_path_buf()
}

/// A directory holding Claude Code as a native program of its own name.
fn native(root: &Path) -> PathBuf {
    touch(&root.join(format!("claude{}", std::env::consts::EXE_SUFFIX)));
    root.to_path_buf()
}

/// A search path over these directories, in order.
fn search(directories: &[&Path]) -> OsString {
    std::env::join_paths(directories).expect("a search path")
}

/// A path holding only npm's launcher and its package starts the native
/// program behind the launcher.
#[test]
fn only_npms_launcher_on_the_path_starts_the_program_behind_it() {
    let fixture = Fixture::new("launching-npm");
    let empty = fixture.path("empty");
    fs::create_dir_all(&empty).expect("an empty directory");
    let prefix = npm_prefix(&fixture.path("npm"));
    let claude = entry("claude-code");

    let path = search(&[&empty, &prefix]);
    assert_eq!(
        claude.program_behind_launcher(&path),
        Some(prefix.join(NPM_PROGRAM))
    );
    assert_eq!(claude.program_on(&path), prefix.join(NPM_PROGRAM));
}

/// A path on which the program is found by its own name starts that, wherever
/// on the path the launcher is.
#[test]
fn a_native_program_on_the_path_is_started_by_name() {
    let fixture = Fixture::new("launching-native");
    let prefix = npm_prefix(&fixture.path("npm"));
    let installed = native(&fixture.path("native"));
    let claude = entry("claude-code");

    for path in [
        search(&[&installed]),
        search(&[&installed, &prefix]),
        search(&[&prefix, &installed]),
    ] {
        assert_eq!(claude.program_behind_launcher(&path), None, "{path:?}");
        assert_eq!(
            claude.program_on(&path),
            PathBuf::from("claude"),
            "{path:?}"
        );
    }
}

/// A launcher whose package is not beside it is no program to start, and a
/// harness npm does not install behind a launcher has none: both are started
/// by name, and fail the way a missing program does.
#[test]
fn a_launcher_with_no_package_or_a_harness_without_one_is_started_by_name() {
    let fixture = Fixture::new("launching-bare");
    let bare = fixture.path("bare");
    touch(&bare.join("claude.cmd"));
    touch(&bare.join("codex.cmd"));
    touch(&bare.join(NPM_PROGRAM));

    let claude_alone = fixture.path("claude-alone");
    touch(&claude_alone.join("claude.cmd"));
    assert_eq!(
        entry("claude-code").program_behind_launcher(&search(&[&claude_alone])),
        None
    );
    assert_eq!(
        entry("codex").program_behind_launcher(&search(&[&bare])),
        None
    );
    assert_eq!(
        entry("codex").program_on(&search(&[&bare])),
        PathBuf::from("codex")
    );
}
