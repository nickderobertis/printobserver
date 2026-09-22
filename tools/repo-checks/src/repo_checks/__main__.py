"""`python -m repo_checks` — this repository's checks and hook commands.

Silent on success. On failure every finding is printed as one line naming the
check and what disagreed.
"""

from __future__ import annotations

import argparse
import sys
from pathlib import Path

from repo_checks import (
    commands,
    gh_release,
    powershell_release,
    release_plz_release,
    windows_lint,
)
from repo_checks.checks_integration import integration_tier
from repo_checks.checks_suppressions import suppressions
from repo_checks.model import Repo
from repo_checks.registry import ALL, CHECKS, WORKFLOW_CHECKS

COMMANDS = (
    "install-tools",
    "install-gh",
    "install-release-plz",
    "install-powershell",
    "tool-version",
    "install-hooks",
    "commit-msg",
    "pr-title",
    "coverage",
    "lint-windows-target",
    "docs-schemas-write",
)

# The checks that read a base revision as well as the tree: what a change adds,
# and what it takes away.
BASE_AWARE = {"suppressions": suppressions, "integration-tier": integration_tier}


def _into(named: str | None) -> Path | None:
    """The directory an install verb was pointed at, or none for its own default."""
    return Path(named) if named else None


def main(argv: list[str] | None = None) -> int:
    """Run one check, one group of checks, or one command."""
    parser = argparse.ArgumentParser(prog="repo-check", description=__doc__)
    parser.add_argument(
        "name",
        help=f"one of: all, workflows, {', '.join(sorted(ALL))}, {', '.join(COMMANDS)}",
    )
    parser.add_argument(
        "argument",
        nargs="?",
        help=(
            "the commit-message file, for commit-msg; the tool, for tool-version and "
            "install-tools; the release, for install-gh, install-release-plz and "
            "install-powershell"
        ),
    )
    parser.add_argument("--root", default=".", help="the tree to read (default: the cwd)")
    parser.add_argument("--base", default=None, help="a base revision to compare a change against")
    parser.add_argument(
        "--releases",
        default=None,
        help=(
            "where an install verb downloads a release's artifacts from, its producer's own "
            "forge by default. Only an https address or a loopback one is fetched, which is "
            "what lets a suite serve an installer a stand-in release over real HTTP"
        ),
    )
    parser.add_argument(
        "--into",
        default=None,
        help="the directory an install verb puts the program in, its own default otherwise",
    )
    parsed = parser.parse_args(argv)

    repo = Repo(Path(parsed.root))

    match parsed.name:
        case "install-tools":
            return commands.install_tools(repo, parsed.argument)
        case "install-gh":
            if parsed.argument is None:
                parser.error("install-gh needs the release to install")
            return gh_release.install_gh(
                parsed.argument,
                _into(parsed.into),
                releases=parsed.releases or gh_release.RELEASES,
            )
        case "install-release-plz":
            if parsed.argument is None:
                parser.error("install-release-plz needs the release to install")
            return release_plz_release.install_release_plz(
                parsed.argument,
                _into(parsed.into),
                releases=parsed.releases or release_plz_release.RELEASES,
            )
        case "install-powershell":
            if parsed.argument is None:
                parser.error("install-powershell needs the release to install")
            return powershell_release.install_powershell(
                parsed.argument,
                _into(parsed.into),
                releases=parsed.releases or powershell_release.RELEASES,
            )
        case "tool-version":
            if parsed.argument is None:
                parser.error("tool-version needs the command of a toolchain tool")
            return commands.tool_version(repo, parsed.argument)
        case "install-hooks":
            return commands.install_hooks(repo)
        case "docs-schemas-write":
            return commands.docs_schemas_write(repo)
        case "coverage":
            return commands.coverage(repo)
        case "lint-windows-target":
            return windows_lint.lint_windows_target(repo)
        case "pr-title":
            return commands.pr_title(repo)
        case "commit-msg":
            if parsed.argument is None:
                parser.error("commit-msg needs the path to the commit message file")
            return commands.commit_msg(repo, Path(parsed.argument))
        case "all":
            selected = CHECKS
        case "workflows":
            selected = WORKFLOW_CHECKS
        case name if name in ALL:
            selected = {name: ALL[name]}
        case _:
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
