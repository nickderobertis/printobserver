"""`python -m repo_checks` — this repository's checks and hook commands.

Silent on success. On failure every finding is printed as one line naming the
check and what disagreed.
"""

from __future__ import annotations

import argparse
import sys
from pathlib import Path

from repo_checks import commands
from repo_checks.checks_integration import integration_tier
from repo_checks.checks_suppressions import suppressions
from repo_checks.model import Repo
from repo_checks.registry import ALL, CHECKS, WORKFLOW_CHECKS

COMMANDS = (
    "install-tools",
    "install-hooks",
    "commit-msg",
    "pr-title",
    "coverage",
    "docs-schemas-write",
)

# The checks that read a base revision as well as the tree: what a change adds,
# and what it takes away.
BASE_AWARE = {"suppressions": suppressions, "integration-tier": integration_tier}


def main(argv: list[str] | None = None) -> int:
    """Run one check, one group of checks, or one command."""
    parser = argparse.ArgumentParser(prog="repo-check", description=__doc__)
    parser.add_argument(
        "name",
        help=f"one of: all, workflows, {', '.join(sorted(ALL))}, {', '.join(COMMANDS)}",
    )
    parser.add_argument("argument", nargs="?", help="the commit-message file, for commit-msg")
    parser.add_argument("--root", default=".", help="the tree to read (default: the cwd)")
    parser.add_argument("--base", default=None, help="a base revision to compare a change against")
    parsed = parser.parse_args(argv)

    repo = Repo(Path(parsed.root))

    if parsed.name == "install-tools":
        return commands.install_tools(repo)
    if parsed.name == "install-hooks":
        return commands.install_hooks(repo)
    if parsed.name == "docs-schemas-write":
        return commands.docs_schemas_write(repo)
    if parsed.name == "coverage":
        return commands.coverage(repo)
    if parsed.name == "pr-title":
        return commands.pr_title(repo)
    if parsed.name == "commit-msg":
        if parsed.argument is None:
            parser.error("commit-msg needs the path to the commit message file")
        return commands.commit_msg(repo, Path(parsed.argument))

    if parsed.name == "all":
        selected = CHECKS
    elif parsed.name == "workflows":
        selected = WORKFLOW_CHECKS
    elif parsed.name in ALL:
        selected = {parsed.name: ALL[parsed.name]}
    else:
        parser.error(f"unknown check {parsed.name!r}")

    findings: list[str] = []
    for name, check in selected.items():
        results = BASE_AWARE[name](repo, parsed.base) if name in BASE_AWARE else check(repo)
        findings.extend(f"{name}: {finding}" for finding in results)

    for finding in findings:
        print(finding, file=sys.stderr)
    return 1 if findings else 0


if __name__ == "__main__":
    raise SystemExit(main())
