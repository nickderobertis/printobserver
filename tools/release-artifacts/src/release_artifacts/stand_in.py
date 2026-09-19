"""A program that stands in for `printobserver` where a journey is about the artifact around it.

Every route's proof ends the same way: the program the route put on a path is
run, and what it answers `--version` with is read back. The journeys that
prove the taking rather than the program carry a small program of their own
in the artifact instead of the real one, and this is where that program comes
from — one place, because the suite's fixtures and the stand-in registries
both need one and two shapes would prove two different things.

It is written the way the host runs a program. A POSIX host runs a shell
script by its interpreter line, and that is the whole of it there. A Windows
host runs no interpreter line: a wheel's script is copied to `Scripts` under
the name it was given, a release artifact is unpacked and run by its own
name, and a file called `printobserver.exe` that is not a program Windows can
load fails as `not a valid Win32 application`. So on Windows the stand-in is a
real program, compiled from a few lines of Rust by the toolchain every host of
this repository already carries, once per distinct answer and then copied.
"""

from __future__ import annotations

import hashlib
import shutil
import sys
from pathlib import Path

from repo_checks.model import Repo
from repo_checks.shell import run

#: The stand-in as a POSIX host runs it. `{answer}` is what it says to
#: `--version`; anything else is refused on standard error.
POSIX_STAND_IN = """#!/bin/sh
if [ "${{1:-}}" = "--version" ]; then
    echo "{answer}"
    exit 0
fi
echo "printobserver: a stand-in program, which does nothing else" >&2
exit 1
"""

#: The stand-in that installs and does not run, which is what a registry
#: serving a broken artifact looks like from the outside.
POSIX_BROKEN = """#!/bin/sh
echo "printobserver: this program cannot run on this host" >&2
exit 1
"""

#: The same two programs, as a Windows host runs one: compiled.
RUST_STAND_IN = """fn main() {{
    let first = std::env::args().nth(1);
    if first.as_deref() == Some("--version") {{
        println!("{{}}", r#"{answer}"#);
        std::process::exit(0);
    }}
    eprintln!("printobserver: a stand-in program, which does nothing else");
    std::process::exit(1);
}}
"""

RUST_BROKEN = """fn main() {
    eprintln!("printobserver: this program cannot run on this host");
    std::process::exit(1);
}
"""

#: How long one compilation is given.
COMPILE_TIMEOUT_SECONDS = 300


class StandInError(RuntimeError):
    """A stand-in program could not be made on this host."""


#: Every program compiled so far, by what it says: a suite asking for the same
#: answer twice compiles it once.
_compiled: dict[tuple[str, bool], Path] = {}


def compiled_here() -> bool:
    """Whether this host runs a stand-in only as a compiled program."""
    return sys.platform == "win32"


def stand_in_program(
    repo: Repo,
    path: Path,
    answer: str,
    *,
    broken: bool = False,
    compiled: bool | None = None,
) -> Path:
    """Write a program at `path` that answers `--version` with `answer`, and answer its path.

    Args:
        repo: The tree whose pinned toolchain compiles the Windows form.
        path: Where the program goes, under the name the caller wants it by.
        answer: The whole line it prints to `--version`.
        broken: Write the one that installs and does not run instead.
        compiled: Write the compiled form rather than the shell one. This
            host's own answer when omitted; a test on a POSIX host passes
            `True` to prove the compiled form on a host that can also read it.

    Raises:
        StandInError: If the compiled form is wanted and this host could not
            build it.
    """
    if '"#' in answer:
        msg = f"a stand-in cannot answer {answer!r}: it closes the literal that carries it"
        raise StandInError(msg)
    path.parent.mkdir(parents=True, exist_ok=True)
    if compiled is None:
        compiled = compiled_here()
    if not compiled:
        body = POSIX_BROKEN if broken else POSIX_STAND_IN.format(answer=answer)
        path.write_text(body, encoding="utf-8")
        path.chmod(0o755)
        return path
    built = _compiled.get((answer, broken))
    if built is None or not built.is_file():
        built = _compile(repo, answer, broken=broken)
        _compiled[answer, broken] = built
    shutil.copy2(built, path)
    path.chmod(0o755)
    return path


def _compile(repo: Repo, answer: str, *, broken: bool) -> Path:
    """Compile one stand-in with the tree's own toolchain, and answer where it is.

    Compiled from the repository root so that `rustup` resolves the toolchain
    the tree pins rather than whatever a caller's directory would; the sources
    and the program go under `target`, which is build products already.

    Raises:
        StandInError: If `rustc` is not on PATH, or refused the program.
    """
    into = repo.root / "target" / "stand-in"
    into.mkdir(parents=True, exist_ok=True)
    digest = hashlib.sha256(answer.encode("utf-8")).hexdigest()[:16]
    stem = f"stand-in-{'broken' if broken else digest}"
    source = into / f"{stem}.rs"
    program = into / (f"{stem}.exe" if sys.platform == "win32" else stem)
    source.write_text(RUST_BROKEN if broken else RUST_STAND_IN.format(answer=answer), "utf-8")
    if shutil.which("rustc") is None:
        msg = (
            "a stand-in program on this host is a compiled one, and `rustc` is not on PATH: "
            "install the toolchain `rust-toolchain.toml` pins, with `rustup`"
        )
        raise StandInError(msg)
    compiled = run(
        ["rustc", "--edition", "2021", "-O", "-o", str(program), str(source)],
        cwd=repo.root,
        timeout=COMPILE_TIMEOUT_SECONDS,
    )
    if compiled.returncode != 0 or not program.is_file():
        msg = f"`rustc` refused the stand-in program:\n{compiled.stdout}{compiled.stderr}"
        raise StandInError(msg)
    return program
