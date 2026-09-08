"""The commands the recipes and hooks run that do something rather than check it."""

from __future__ import annotations

import re
import shutil
import sys
from pathlib import Path

from repo_checks.model import Repo
from repo_checks.shell import run

CONVENTIONAL = re.compile(r"^(?P<type>[a-z]+)(?:\([^)]+\))?!?: .+")
# The subject git writes for itself when it makes a merge commit, which is
# `fmt-merge-msg`'s grammar: the kind of thing merged, one or more quoted refs,
# and the optional `of <remote>` and `into <branch>` tails. Publishing a branch
# of this repository merges the base into it first, and git — not a person —
# writes `Merge remote-tracking branch 'origin/main' into <branch>` for that
# commit. Holding a subject nobody typed to Conventional Commits refused the
# merge, so no branch could be published once `main` had moved under it.
#
# The quoted ref is what keeps this narrow: a subject a person authors, prose
# beginning `Merge` included, carries none and is ruled on exactly as before.
GENERATED_MERGE = re.compile(
    r"^Merge (?:remote-tracking )?(?:branch|branches|commit|commits|tag|tags) "
    r"'[^']+'(?:(?:,| and) '[^']+')*(?: of \S+)?(?: into \S+)?$"
)


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

    A subject git generates for a merge is not one anybody authored, so it is
    admitted as written; everything else is held to the declared type list.
    """
    subject = message_file.read_text(encoding="utf-8").splitlines()[0].strip()
    if subject.startswith("#") or not subject:
        return 0
    if GENERATED_MERGE.match(subject):
        return 0
    return _rule_on_subject(repo, subject, "commit subject")


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
