"""`just skilltest-classify BASE HEAD`: whether a change owes the real-print suite.

Owed when the diff touches a path `repo-policy.toml`'s `[skilltest] owed_paths`
names, the `[supervisor]` table an install template writes, the release of an
`oneharness` package in `Cargo.lock`, or the Claude Code release the job installs.
Unknown is never omission: a diff it cannot read owes the suite.

It prints the `name=value` lines a workflow step appends to `GITHUB_OUTPUT`, and
says why in the step summary a run names.
"""

from __future__ import annotations

import fnmatch
import os
import re
import sys
import tomllib
from dataclasses import dataclass
from pathlib import Path

from repo_checks.shell import run

POLICY = "repo-policy.toml"
LOCK = "Cargo.lock"
#: A package of the program the supervisor's port runs on.
HARNESS_PACKAGE = re.compile(r"^oneharness")
#: The table of an install template that says what a supervision turn runs on.
SUPERVISOR_TABLE = re.compile(r"^\[supervisor\]\n(?P<body>.*?)(?=^\[)", re.MULTILINE | re.DOTALL)


class UndecidableError(Exception):
    """A diff or a file this could not read, which owes the suite."""


@dataclass(frozen=True)
class Classification:
    """Whether the suite is owed, and why."""

    owed: bool
    reason: str

    def outputs(self) -> str:
        """The lines a workflow step appends to `GITHUB_OUTPUT`."""
        return f"owed={'true' if self.owed else 'false'}\nreason={self.reason}\n"


def _git(root: Path, *arguments: str) -> str:
    # llmlint: ignore[async_typed_clients_at_boundaries] See suppressions.toml.
    done = run(["git", *arguments], cwd=root)
    if done.returncode != 0:
        msg = f"`git {arguments[0]}` failed: {done.stderr.strip()}"
        raise UndecidableError(msg)
    return done.stdout


def _shown(root: Path, ref: str, path: str) -> str | None:
    try:
        return _git(root, "show", f"{ref}:{path}")
    except UndecidableError:
        return None


def _matches(path: str, pattern: str) -> bool:
    """Whether a path is under a `dir/**` pattern, or is the file a pattern names."""
    if pattern.endswith("/**"):
        return path.startswith(pattern[: -len("**")])
    return fnmatch.fnmatchcase(path, pattern)


def _harness_versions(lock: str | None) -> dict[str, str]:
    packages = tomllib.loads(lock or "").get("package", [])
    return {
        package["name"]: package["version"]
        for package in packages
        if HARNESS_PACKAGE.match(package["name"])
    }


def _supervisor_table(template: str | None) -> str | None:
    found = SUPERVISOR_TABLE.search(template or "")
    return found["body"] if found else None


def classify(root: Path, base: str, head: str) -> Classification:
    """Whether the diff from the merge base of `base` and `head` to `head` owes the suite."""
    try:
        return _decided(root, base, head)
    except (UndecidableError, tomllib.TOMLDecodeError, KeyError, TypeError) as error:
        return Classification(owed=True, reason=f"the diff could not be read: {error}")


def _decided(root: Path, base: str, head: str) -> Classification:
    merge_base = _git(root, "merge-base", base, head).strip()
    changed = _git(root, "diff", "--name-only", merge_base, head).splitlines()
    head_policy = tomllib.loads(_shown(root, head, POLICY) or "")["skilltest"]
    for listed in ("owed_paths", "install_templates"):
        held = head_policy[listed]
        if not isinstance(held, list) or not all(isinstance(entry, str) for entry in held):
            msg = f"{POLICY}'s [skilltest] {listed} is no list of paths"
            raise UndecidableError(msg)
    for path in changed:
        for pattern in head_policy["owed_paths"]:
            if _matches(path, pattern):
                return Classification(owed=True, reason=f"{path} is under {pattern}")
    if LOCK in changed:
        before, after = (_harness_versions(_shown(root, ref, LOCK)) for ref in (merge_base, head))
        if before != after:
            return Classification(owed=True, reason=f"{LOCK} moves an oneharness package")
    for template in head_policy["install_templates"]:
        if template in changed and _supervisor_table(
            _shown(root, merge_base, template)
        ) != _supervisor_table(_shown(root, head, template)):
            return Classification(owed=True, reason=f"{template} changes [supervisor]")
    if POLICY in changed:
        before_policy = tomllib.loads(_shown(root, merge_base, POLICY) or "").get("skilltest", {})
        if before_policy.get("claude_code") != head_policy["claude_code"]:
            return Classification(
                owed=True, reason="the Claude Code release the job installs moves"
            )
    return Classification(
        owed=False,
        reason=f"none of the {len(changed)} changed paths can move the agent's judgement",
    )


def main(arguments: list[str]) -> int:
    """Print the classification of one diff, or install the release the job runs on."""
    if arguments == ["--install-harness"]:
        policy = tomllib.loads(Path(POLICY).read_text(encoding="utf-8"))
        package = f"@anthropic-ai/claude-code@{policy['skilltest']['claude_code']}"
        # llmlint: ignore[async_typed_clients_at_boundaries] See suppressions.toml.
        return run(["npm", "install", "-g", package], capture=False).returncode
    if len(arguments) != 2:
        print("usage: classify.py BASE HEAD", file=sys.stderr)
        return 2
    answer = classify(Path.cwd(), arguments[0], arguments[1])
    sys.stdout.write(answer.outputs())
    summary = os.environ.get("GITHUB_STEP_SUMMARY")
    if summary:
        with Path(summary).open("a", encoding="utf-8") as written:
            owed = "owed" if answer.owed else "not owed"
            written.write(f"The real-print suite is {owed}: {answer.reason}\n")
    return 0


if __name__ == "__main__":
    raise SystemExit(main(sys.argv[1:]))
