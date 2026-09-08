"""The commands the recipes and hooks run that do something rather than check it."""

from __future__ import annotations

import re
import shutil
import sys
from pathlib import Path

from repo_checks.model import Repo
from repo_checks.shell import run

CONVENTIONAL = re.compile(r"^(?P<type>[a-z]+)(?:\([^)]+\))?!?: .+")


def install_tools(repo: Repo) -> int:
    """Put every tool `repo-policy.toml` declares on PATH, skipping those present."""
    for tool in repo.policy["toolchain"]["tool"]:
        if shutil.which(tool["command"]):
            continue
        print(f"installing {tool['command']}", file=sys.stderr)
        result = run(tool["install"].split(), cwd=repo.root, capture=False)
        if result.returncode != 0:
            print(
                f"failed to install {tool['command']}. Run `{tool['install']}` by hand.",
                file=sys.stderr,
            )
            return 1
    return 0


def install_hooks(repo: Repo) -> int:
    """Point git at the committed hooks, where this tree is a git repository."""
    if not (repo.root / ".git").exists():
        return 0
    run(["git", "config", "core.hooksPath", ".githooks"], cwd=repo.root, check=True)
    return 0


def commit_msg(repo: Repo, message_file: Path) -> int:
    """Refuse an authored commit subject that is not a Conventional Commit.

    The subject git writes for a merge it is completing is not one anybody
    authored, so it is admitted as written; everything else is held to the
    declared type list.
    """
    subject = message_file.read_text(encoding="utf-8").splitlines()[0].strip()
    if subject.startswith("#") or not subject:
        return 0
    if _a_merge_is_in_progress(repo):
        return 0
    return _rule_on_subject(repo, subject, "commit subject")


def _a_merge_is_in_progress(repo: Repo) -> bool:
    """Whether git is part-way through a merge it is writing this commit for.

    Publishing a branch of this repository merges the base into it first, and
    git — not a person — writes `Merge remote-tracking branch 'origin/main'
    into <branch>` for that commit. Holding a subject nobody typed to
    Conventional Commits refused the merge, so no branch could be published
    once `main` had moved under it.

    What distinguishes that commit is its *state*, not its wording: git writes
    `MERGE_HEAD` into the git directory when a merge starts and removes it once
    the merge commit is made, so it is present exactly while git is completing
    one and absent for an ordinary commit whatever its subject says. Reading
    the subject instead would hand anybody a bypass of the whole convention by
    typing one word, because `Merge branch 'main'` typed by a person is
    textually identical to what git generates.

    `run` drops the variables naming a repository, so this asks about the tree
    the hook was pointed at rather than about whichever repository a `pre-push`
    hook further out happened to be pushing. A tree that is no git repository
    at all answers no, and its subject is ruled on as usual.
    """
    located = run(["git", "rev-parse", "--verify", "--quiet", "MERGE_HEAD"], cwd=repo.root)
    return located.returncode == 0


def pr_title(repo: Repo) -> int:
    """Refuse a pull-request title that is not a Conventional Commit subject."""
    import os

    title = os.environ.get("PR_TITLE", "").strip()
    if not title:
        print("PR_TITLE is empty. Pass the pull-request title in the environment.", file=sys.stderr)
        return 1
    return _rule_on_subject(repo, title, "pull-request title")


def _rule_on_subject(repo: Repo, subject: str, what: str) -> int:
    """Hold one subject to the type list `repo-policy.toml` declares."""
    admitted = [
        *repo.policy["commits"]["release_types"],
        *repo.policy["commits"]["non_release_types"],
    ]
    match = CONVENTIONAL.match(subject)
    if not match:
        print(
            f"{what} is not a Conventional Commit: {subject!r}\n"
            f"Use `<type>(<scope>): <subject>`, with type one of "
            f"{', '.join(sorted(admitted))}.",
            file=sys.stderr,
        )
        return 1
    if match["type"] not in admitted:
        print(
            f"{what} uses type `{match['type']}`, which `repo-policy.toml` does not "
            f"admit. Use one of {', '.join(sorted(admitted))}.",
            file=sys.stderr,
        )
        return 1
    return 0


def coverage(repo: Repo) -> int:
    """Fail the build below the line-coverage floors `repo-policy.toml` records."""
    floors = repo.policy["gate"]["coverage"]
    failed = False

    rust = run(
        ["cargo", "llvm-cov", "report", "--summary-only", f"--fail-under-lines={floors['rust']}"],
        cwd=repo.root,
    )
    if rust.returncode != 0:
        print(rust.stdout, file=sys.stderr)
        print(
            f"Rust line coverage is below the {floors['rust']}% floor. Add tests that "
            f"drive the uncovered lines, or explain the floor change in AGENTS.md.",
            file=sys.stderr,
        )
        failed = True

    python = run(["uv", "run", "-q", "coverage", "combine"], cwd=repo.root)
    if python.returncode not in (0, 1):
        print(python.stderr, file=sys.stderr)
        failed = True
    report = run(
        ["uv", "run", "-q", "coverage", "report", f"--fail-under={floors['python']}"],
        cwd=repo.root,
    )
    if report.returncode != 0:
        print(report.stdout, file=sys.stderr)
        print(
            f"Python line coverage is below the {floors['python']}% floor. Add tests "
            f"that drive the uncovered lines.",
            file=sys.stderr,
        )
        failed = True

    return 1 if failed else 0
