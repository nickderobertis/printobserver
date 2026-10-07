"""Which projects one run of a gate tier reaches, and the one place that decides it.

Every recipe of `just check` that fans out over the project graph takes a tier:

* `affected`, the default, runs `nx affected` against an explicitly derived
  base commit — `NX_BASE` where the environment names one, and otherwise the
  merge base of `HEAD` with the base branch `repo-policy.toml` declares. Nx's
  own implicit default is not used, because it is not deterministic across a
  shallow checkout.
* `all` runs `nx run-many` over every project: the full sweep.

`affected` fails closed to the whole graph rather than to nothing. Where no
merge base can be derived there is no change to scope by, and where a change
touches a file no project owns — the justfile, `AGENTS.md`, a workflow,
`repo-policy.toml`, a script — the graph cannot say which suites read it, so
both run every project and say why. An `NX_BASE` that is neither a plain ref
name nor a commit SHA, or that names no commit, is refused rather than guessed
around: it came from outside, and a base nobody meant is a narrower run nobody
asked for.

Two verbs:

* `run TIER TARGET...` runs the targets over the tier's projects, saying first
  which projects that is and why;
* `select` answers the tier a continuous-integration run of the gate is for,
  off the event the forge started it with: the release pull request
  `release-plz` opens takes the sweep, every other pull request and every push
  to the base branch the affected tier, and a run nothing here recognises the
  sweep, because the whole graph is the answer that cannot miss anything.
"""

from __future__ import annotations

import argparse
import json
import os
import re
import sys
from collections.abc import Mapping, Sequence
from dataclasses import dataclass
from enum import StrEnum
from pathlib import Path

from repo_checks.model import Repo
from repo_checks.shell import run


class Tier(StrEnum):
    """The two tiers a gate run is at: the projects a change reaches, or every one."""

    AFFECTED = "affected"
    ALL = "all"


AFFECTED = Tier.AFFECTED
ALL = Tier.ALL
TIERS = tuple(Tier)

#: The variables a caller names the base and head commits in. `nx-set-shas`
#: exports both, and Nx reads them itself too.
BASE_VARIABLE = "NX_BASE"
HEAD_VARIABLE = "NX_HEAD"

#: What a base may be: a commit SHA, abbreviated or whole, or a plain ref name —
#: letters, digits and `. _ / -`, starting with neither `-` nor `.` nor `/`, with
#: no `..` or `//` anywhere and no trailing `/` or `.lock`, which is the subset of
#: git's own ref-name rules a value interpolated nowhere but `git` and `nx` needs.
SHA = re.compile(r"^[0-9a-f]{7,64}$")
REF_NAME = re.compile(r"^(?![-./])(?!.*\.\.)(?!.*//)(?!.*/$)(?!.*\.lock$)[A-Za-z0-9._/-]+$")

#: The exit a refused tier or base answers with, apart from the exit of an Nx
#: run, which is passed back as it is.
REFUSED = 2

#: Where `release-plz.toml` names the head-branch prefix of the release pull
#: request — the one statement of it, which `release-plz` and the selector both
#: read, so neither restates the other's default.
RELEASE_BRANCH_KEY = "pr_branch_prefix"

#: How many of the paths no project owns a run names before counting the rest.
NAMED_PATHS = 5


class TierError(ValueError):
    """A tier or a base this module refuses to run against."""


@dataclass(frozen=True, slots=True)
class Scope:
    """What one run of a tier reaches: a base to diff against, or the whole graph."""

    #: The base commit, as a SHA, or none where the run is over every project.
    base: str | None
    #: The head commit, where the caller named one; the working tree otherwise.
    head: str | None
    #: Why the run reaches what it reaches, as a reader is told it.
    reason: str

    def nx(self, *targets: str) -> list[str]:
        """The `nx` invocation running `targets` over this scope."""
        if self.base is None:
            return ["bunx", "nx", "run-many", "-t", *targets]
        head = [f"--head={self.head}"] if self.head is not None else []
        return ["bunx", "nx", "affected", f"--base={self.base}", *head, "-t", *targets]


def _git(repo: Repo, *arguments: str) -> str | None:
    """One git query's answer, or none where git refused it."""
    answer = run(["git", *arguments], cwd=repo.root)
    return answer.stdout.strip() if answer.returncode == 0 else None


def _commit(repo: Repo, variable: str, value: str) -> str:
    """The commit a caller-named base or head resolves to, refused unless it is one."""
    if not (SHA.match(value) or REF_NAME.match(value)):
        msg = (
            f"{variable}={value!r} is neither a plain ref name nor a commit SHA: a ref is "
            f"letters, digits and `. _ / -`, and a SHA is 7 to 64 lowercase hex digits"
        )
        raise TierError(msg)
    resolved = _git(repo, "rev-parse", "--verify", "--quiet", f"{value}^{{commit}}")
    if not resolved:
        msg = f"{variable}={value!r} names no commit in this checkout"
        raise TierError(msg)
    return resolved


def _merge_base(repo: Repo) -> tuple[str, str] | None:
    """The merge base of `HEAD` with the base branch, and the ref it was taken against.

    Raises:
        TierError: If `repo-policy.toml`'s base branch is not a plain ref name.
    """
    branch = repo.policy["repository"]["base_branch"]
    if not isinstance(branch, str) or not REF_NAME.match(branch):
        msg = f"repo-policy.toml's `repository.base_branch` {branch!r} is not a plain ref name"
        raise TierError(msg)
    for candidate in (f"origin/{branch}", branch):
        found = _git(repo, "merge-base", candidate, "HEAD")
        if found:
            return found, candidate
    return None


def _separated(listing: str | None) -> list[str]:
    """The paths a `-z` listing names, none where git answered nothing."""
    return [path for path in listing.split("\0") if path] if listing else []


def _changed(repo: Repo, base: str, head: str | None) -> list[str]:
    """Every path a diff from `base` touches, as Nx's own affected run reads it.

    Against the working tree where no head is named, untracked files included,
    and with renames split into the path they left and the one they took.
    """
    diff = ["diff", "--name-only", "--no-renames", "-z", base, *([head] if head else [])]
    paths = _separated(_git(repo, *diff))
    if head is None:
        paths.extend(_separated(_git(repo, "ls-files", "--others", "--exclude-standard", "-z")))
    return sorted({path for path in paths if path})


def _project_roots(repo: Repo) -> list[str]:
    """Every project's root, repository-relative."""
    return [path.parent.relative_to(repo.root).as_posix() for path in repo.project_paths]


def _unowned(repo: Repo, paths: Sequence[str]) -> list[str]:
    """The paths no project's root holds."""
    roots = _project_roots(repo)
    return [path for path in paths if not any(path.startswith(f"{root}/") for root in roots)]


def tier_named(named: str) -> Tier:
    """The tier a caller named, narrowed at the boundary it came in through.

    Raises:
        TierError: If `named` is not one of `TIERS`.
    """
    try:
        return Tier(named)
    except ValueError:
        msg = f"unknown tier {named!r}: use {AFFECTED.value!r} (the default) or {ALL.value!r}"
        raise TierError(msg) from None


def resolve(repo: Repo, tier: str, environ: Mapping[str, str] | None = None) -> Scope:
    """What `tier` reaches in `repo` from here.

    Raises:
        TierError: If `tier` is not one of `TIERS`, `NX_BASE` or `NX_HEAD` is set
            to something that is not a plain ref name or SHA naming a commit, or
            the base branch `repo-policy.toml` declares is not a plain ref name.
    """
    environment = os.environ if environ is None else environ
    if tier_named(tier) is ALL:
        return Scope(None, None, "every project: the full sweep, as asked")

    named_head = environment.get(HEAD_VARIABLE, "")
    head = _commit(repo, HEAD_VARIABLE, named_head) if named_head else None
    named = environment.get(BASE_VARIABLE, "")
    if named:
        base = _commit(repo, BASE_VARIABLE, named)
        how = f"{BASE_VARIABLE}={named}"
    else:
        found = _merge_base(repo)
        if found is None:
            branch = repo.policy["repository"]["base_branch"]
            return Scope(
                None,
                None,
                f"every project: no merge base of HEAD with origin/{branch} or {branch} could "
                f"be derived, and the whole graph is what cannot miss a change",
            )
        base, against = found
        how = f"the merge base of HEAD with {against}"

    unowned = _unowned(repo, _changed(repo, base, head))
    if unowned:
        more = len(unowned) - NAMED_PATHS
        shown = ", ".join(unowned[:NAMED_PATHS]) + (f" and {more} more" if more > 0 else "")
        return Scope(
            None,
            None,
            f"every project: the change since {base[:12]} ({how}) touches {shown}, which no "
            f"project owns, so the graph cannot say which suites read it",
        )
    return Scope(base, head, f"the projects the change since {base[:12]} ({how}) reaches")


def affected_projects(repo: Repo, scope: Scope, target: str) -> set[str] | None:
    """The projects carrying `target` that `scope` reaches, or none for every one.

    Raises:
        TierError: If Nx could not answer.
    """
    if scope.base is None:
        return None
    head = [f"--head={scope.head}"] if scope.head is not None else []
    shown = run(
        [
            "bunx",
            "nx",
            "show",
            "projects",
            "--affected",
            f"--base={scope.base}",
            *head,
            f"--withTarget={target}",
            "--json",
        ],
        cwd=repo.root,
    )
    if shown.returncode != 0:
        msg = f"`nx show projects --affected` failed:\n{shown.stdout}{shown.stderr}"
        raise TierError(msg)
    listed = json.loads(shown.stdout)
    if not isinstance(listed, list) or not all(isinstance(name, str) for name in listed):
        msg = (
            f"`nx show projects --json` printed something other than a list of project "
            f"names: {shown.stdout}"
        )
        raise TierError(msg)
    return set(listed)


def release_branch_prefix(repo: Repo) -> str:
    """The head-branch prefix of the pull request `release-plz` opens.

    Raises:
        TierError: If `release-plz.toml` names none, so no pull request could be
            told apart as the release pull request.
    """
    workspace = repo.read_toml("release-plz.toml").get("workspace", {})
    prefix = workspace.get(RELEASE_BRANCH_KEY) if isinstance(workspace, dict) else None
    if not isinstance(prefix, str) or not prefix:
        msg = (
            f"release-plz.toml's [workspace] names no `{RELEASE_BRANCH_KEY}`, so the release "
            f"pull request the sweep runs on cannot be told apart from any other"
        )
        raise TierError(msg)
    return prefix


def select(repo: Repo, environ: Mapping[str, str] | None = None) -> Tier:
    """The tier a continuous-integration run of the gate is for.

    The sweep runs on the release pull request, which is where a batched
    release is gated before it ships; an ordinary pull request and a push to
    the base branch run the affected tier. Any other event — a hand dispatch of
    one gate cell, or a run nothing here recognises — takes the sweep.

    Raises:
        TierError: If `release-plz.toml` names no release branch prefix.
    """
    environment = os.environ if environ is None else environ
    match environment.get("GITHUB_EVENT_NAME", ""):
        case "pull_request":
            head = environment.get("GITHUB_HEAD_REF", "")
            return ALL if head.startswith(release_branch_prefix(repo)) else AFFECTED
        case "push":
            return AFFECTED
        case _:
            return ALL


def main(argv: list[str] | None = None) -> int:
    """`run` targets over a tier, or `select` the tier a CI run is for."""
    parser = argparse.ArgumentParser(prog="gate-tier", description=__doc__)
    parser.add_argument("verb", choices=("run", "select"))
    parser.add_argument("tier", nargs="?", help=f"one of {', '.join(TIERS)}, for run")
    parser.add_argument("targets", nargs="*", help="the Nx targets to run, for run")
    parser.add_argument("--root", default=".", help="the tree to read (default: the cwd)")
    parsed = parser.parse_args(argv)
    repo = Repo(Path(parsed.root))

    try:
        if parsed.verb == "select":
            print(select(repo))
            return 0
        if parsed.tier is None or not parsed.targets:
            parser.error("run needs a tier and at least one target")
        scope = resolve(repo, parsed.tier)
    except TierError as error:
        print(f"gate-tier: refused: {error}", file=sys.stderr)
        return REFUSED
    command = scope.nx(*parsed.targets)
    print(f"gate-tier: {parsed.tier}: {scope.reason}", file=sys.stderr)
    print(f"gate-tier: {' '.join(command)}", file=sys.stderr)
    return run([*command, "--output-style=stream"], cwd=repo.root, capture=False).returncode


if __name__ == "__main__":
    sys.exit(main())
