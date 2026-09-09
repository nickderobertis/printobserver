"""The one place this repository starts a subprocess.

Every check, command and test helper runs programs through `run`, so its
contract is worth driving directly: it resolves the executable against PATH
before running it, it reports a program that is not there rather than raising
out of a caller that was going to report the failure itself, and it runs that
program on the directory it was given rather than on the repository the
caller's own environment names.
"""

# `assert` is how pytest states an assertion and how it produces the failure
# message a reader acts on; suppressions.toml carries the reason.

from __future__ import annotations

from pathlib import Path

import pytest
from repo_checks.expect import contains, equal, passing, truth
from repo_checks.shell import PROGRAM_NOT_FOUND, run

ABSENT = "a-program-this-repository-does-not-install"


def test_a_program_is_run_by_absolute_path(tmp_path: Path) -> None:
    """The executable is resolved against PATH, which is what fixes S607."""
    result = run(["git", "rev-parse", "--is-inside-work-tree"], cwd=tmp_path)

    truth(
        result.args[0].startswith("/") and result.args[0].endswith("/git"),
        describing=f"git to be run by absolute path, not as {result.args[0]!r}",
    )


def test_output_comes_back_captured() -> None:
    """Callers read what a program said, so the default captures both streams."""
    result = run(["git", "--version"])

    passing(result)
    contains(result.stdout, "git version")


def test_a_missing_program_reports_rather_than_raising() -> None:
    """A caller that reports its own failure gets a status it can act on."""
    result = run([ABSENT, "--version"])

    equal(result.returncode, PROGRAM_NOT_FOUND)
    contains(result.stderr, ABSENT)


def test_a_missing_program_raises_where_the_caller_asked_it_to() -> None:
    """`check` means the caller has no failure path of its own."""
    with pytest.raises(FileNotFoundError, match=ABSENT):
        run([ABSENT], check=True)


def test_output_can_be_left_to_the_terminal() -> None:
    """A long install's own progress is what a reader needs, so it is not captured."""
    result = run(["git", "--version"], capture=False)

    passing(result)
    equal(result.stdout, None)


def _repository(root: Path) -> Path:
    """A real git repository, initialized the way the suites here make one."""
    root.mkdir(parents=True, exist_ok=True)
    run(["git", "init", "-q", "-b", "main"], cwd=root, check=True)
    return root


def test_an_ambient_git_directory_does_not_move_a_subprocess(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    """Every hook git runs inherits `GIT_DIR`, and passes it on to what it starts.

    Inherited, it would point every `git` a hook starts at the repository the
    hook was invoked for — so a suite that inits a repository in a temporary
    directory would commit into the developer's own tree instead.
    """
    pushed = _repository(tmp_path / "pushed")
    elsewhere = _repository(tmp_path / "elsewhere")
    monkeypatch.setenv("GIT_DIR", str(pushed / ".git"))

    where = run(["git", "rev-parse", "--absolute-git-dir"], cwd=elsewhere)

    passing(where)
    equal(
        Path(where.stdout.strip()).resolve(),
        (elsewhere / ".git").resolve(),
        describing="the repository the subprocess worked on",
    )


def test_a_commit_lands_where_the_subprocess_was_pointed(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    """The failure this repairs is a commit landing in the wrong repository."""
    pushed = _repository(tmp_path / "pushed")
    elsewhere = _repository(tmp_path / "elsewhere")
    (elsewhere / "a-file.txt").write_text("a line\n", encoding="utf-8")
    monkeypatch.setenv("GIT_DIR", str(pushed / ".git"))

    run(["git", "add", "-A"], cwd=elsewhere, check=True)
    run(
        [
            "git",
            "-c",
            "user.email=t@example.com",
            "-c",
            "user.name=t",
            "commit",
            "-q",
            "-m",
            "chore: the base",
        ],
        cwd=elsewhere,
        check=True,
    )

    contains(run(["git", "log", "--oneline"], cwd=elsewhere).stdout, "chore: the base")
    equal(
        run(["git", "log", "--oneline"], cwd=pushed).returncode,
        128,
        describing="an empty repository",
    )
