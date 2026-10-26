"""The one way a suite here makes a git repository of its own.

Every commit git makes ends by asking whether the repository wants
maintenance, and the answer can start a repack that runs on, detached, after
the command that started it has returned. Since git 2.54 a repository holding a
hundred loose objects is enough — and a fixture that commits a copy of this
tree holds several hundred. That repack packs the loose objects and deletes them
and their emptied `objects/XX` directories while the test goes on reading the
same `.git`: a tool walking it meets a directory that was there a moment ago.

So a fixture repository runs no automatic maintenance at all. The settings are
the repository's own, written before its first commit, so they assume nothing
of the host's or the runner's git configuration, hold on every git version, and
reach every command a test later runs in it.
"""

from __future__ import annotations

from pathlib import Path

from repo_checks.shell import run

#: What turns automatic maintenance off: `maintenance.auto` for the git that
#: asks `git maintenance run --auto` after a command, and `gc.auto` for the
#: older one that asks `git gc --auto` instead, or whose maintenance runs it.
QUIESCENT = (("maintenance.auto", "false"), ("gc.auto", "0"))


def init_repository(root: Path, *, branch: str = "main", env: dict[str, str] | None = None) -> Path:
    """Make `root` a git repository that never maintains itself, and answer it.

    Args:
        root: The directory to initialize, made if it is not there yet.
        branch: The branch its first commit lands on.
        env: The environment git runs under, or the caller's when omitted.

    Raises:
        subprocess.CalledProcessError: If git refuses either step.
    """
    root.mkdir(parents=True, exist_ok=True)
    run(["git", "init", "-q", "-b", branch], cwd=root, env=env, check=True)
    for name, value in QUIESCENT:
        run(["git", "config", "--local", name, value], cwd=root, env=env, check=True)
    return root
