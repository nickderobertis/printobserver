"""A program that stands in for `printobserver` where a journey is about the artifact around it.

Every route's proof ends the same way: the program the route put on a path is
run, and what it answers `--version` with is read back. The journeys that
prove the taking rather than the program carry a small program of their own
in the artifact instead of the real one, and this is where that program comes
from — one place, because the suite's fixtures and the stand-in registries
both need one and two shapes would prove two different things.

It comes in two forms. The shell form is what a POSIX host runs by its
interpreter line, and it is what the stand-in registries serve on one. The
compiled form is a real program, built from a few lines of Rust by the
toolchain every host of this repository already carries, once per distinct
answer and then copied. It is the only form a Windows host can run — a wheel's
script is copied to `Scripts` under the name it was given, a release artifact
is unpacked and run by its own name, and a file called `printobserver.exe`
that is not a program Windows can load fails as `not a valid Win32
application` — and it is the form the artifact suite's fixtures carry on every
host, because a route's artifact states what the program inside it was built
against, and on macOS that floor is read out of the program's own Mach-O load
commands, which a shell script does not have.
"""

from __future__ import annotations

import contextlib
import hashlib
import shutil
import sys
import tempfile
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
    the tree pins rather than whatever a caller's directory would; the program
    goes under `target`, which is build products already.

    Every process that wants one — each xdist worker of each suite that runs
    beside another — finds it under one name, made of the source it is built
    from and the toolchain building it: a change to either names a new program
    rather than reusing a stale one. A process that finds none compiles into a
    directory of its own and then publishes the whole program under that name,
    so no process ever reads one another is still writing, and none overwrites
    one another is running.

    Raises:
        StandInError: If `rustc` is not on PATH, or refused the program.
    """
    if shutil.which("rustc") is None:
        msg = (
            "a stand-in program on this host is a compiled one, and `rustc` is not on PATH: "
            "install the toolchain `rust-toolchain.toml` pins, with `rustup`"
        )
        raise StandInError(msg)
    text = RUST_BROKEN if broken else RUST_STAND_IN.format(answer=answer)
    toolchain = run(["rustc", "-vV"], cwd=repo.root, timeout=COMPILE_TIMEOUT_SECONDS)
    if toolchain.returncode != 0:
        msg = f"`rustc -vV` could not say which toolchain it is:\n{toolchain.stderr}"
        raise StandInError(msg)
    digest = hashlib.sha256(f"{text}\0{toolchain.stdout}".encode()).hexdigest()[:16]
    name = f"stand-in-{digest}{'.exe' if sys.platform == 'win32' else ''}"
    shared = repo.root / "target" / "stand-in"
    program = shared / name
    if program.is_file():
        return program
    shared.mkdir(parents=True, exist_ok=True)
    with tempfile.TemporaryDirectory(dir=shared, prefix="building-") as private:
        source = Path(private) / "stand_in.rs"
        built = Path(private) / name
        source.write_text(text, "utf-8")
        # Small: the artifacts carrying it are built, uploaded and unpacked by
        # every route proof of the suite.
        compiled = run(
            [
                "rustc",
                "--edition",
                "2021",
                "-C",
                "opt-level=s",
                "-C",
                "strip=symbols",
                "-C",
                "panic=abort",
                "-o",
                str(built),
                str(source),
            ],
            cwd=repo.root,
            timeout=COMPILE_TIMEOUT_SECONDS,
        )
        if compiled.returncode != 0 or not built.is_file():
            msg = f"`rustc` refused the stand-in program:\n{compiled.stdout}{compiled.stderr}"
            raise StandInError(msg)
        published(built, program)
    return program


def published(built: Path, program: Path) -> Path:
    """Put the whole program `built` at `program`, unless another process already has.

    A hard link either makes the name or finds it taken, in one step on every
    host this repository supports: a process can neither see the name before
    the program behind it is whole, nor replace one another process holds. The
    program a process finds there was built from the same source by the same
    toolchain, so it is the one this process would have published.
    """
    with contextlib.suppress(FileExistsError):
        program.hardlink_to(built)
    return program
