"""Arming auto-merge on the release pull request release automation just drafted.

`release-plz release-pr` opens or refreshes one release pull request, under a
token of its own so that the pull request's gating workflows run. Nothing then
merges it unless somebody does, and a release waits on that person. So the
drafting job hands the program's own answer — `release-plz release-pr --output
json`, which names the pull request it opened or refreshed — to `arm`, which
arms the forge's auto-merge on exactly that pull request and on nothing else:
the forge merges it once its required checks pass, and the push that merge
makes to `main` is the one that cuts the release.

It cannot loop. The merge leaves one `chore(...): release v<version>` commit on
`main`, and `release-plz.toml`'s `release_commits` releases on `feat`, `fix`
and `perf` alone, so that commit drafts no further release pull request.

The answer's shape is the program's own, read from release-plz 0.3.167's
`main.rs` (`{"prs": [...]}`, empty when it drafted nothing) and its core's
`ReleasePr` (`head_branch`, `base_branch`, `html_url`, `number`, `releases`).
`samples/release-plz-release-pr.json` is a committed copy of it.
"""

from __future__ import annotations

import json
import subprocess
from dataclasses import dataclass
from pathlib import Path

from repo_checks.model import Repo
from repo_checks.shell import run

#: A committed answer of `release-plz release-pr --output json`, as the program
#: writes one having refreshed a release pull request.
DRAFTED_SAMPLE = Path("tools/release-artifacts/samples/release-plz-release-pr.json")

#: What arms one pull request: the forge's own auto-merge, squashing, as every
#: pull request of this repository is merged. The pull request is named by its
#: URL, which names the repository too, so nothing is inferred from a remote.
ARM = ("gh", "pr", "merge", "--auto", "--squash")

#: How long the forge is given to answer one arming.
TIMEOUT_SECONDS = 120

#: The forge a release pull request of this repository is on.
FORGE = "https://github.com"

#: The branches release-plz drafts a release pull request on: its default
#: prefix, and the one it still refreshes an older pull request under
#: (`release_plz_core::pr::{DEFAULT_BRANCH_PREFIX, OLD_BRANCH_PREFIX}`).
RELEASE_BRANCHES = ("release-plz-", "release-plz/")


class ArmingError(RuntimeError):
    """The release pull request could not be named, or could not be armed."""


@dataclass(frozen=True, slots=True)
class Drafted:
    """One release pull request the program says it opened or refreshed."""

    number: int
    url: str


@dataclass(frozen=True, slots=True)
class Repository:
    """Where a release pull request of this repository is, as `repo-policy.toml` says."""

    owner: str
    name: str
    base_branch: str

    def pull(self, number: int) -> str:
        """The URL the forge gives this repository's pull request `number`."""
        return f"{FORGE}/{self.owner}/{self.name}/pull/{number}"


def repository(repo: Repo) -> Repository:
    """This repository, as its `repo-policy.toml` declares it.

    Raises:
        ArmingError: If the policy declares no owner, name or base branch, which
            is a checkout of something other than this repository.
    """
    try:
        declared = repo.policy.get("repository", {})
    except OSError as unreadable:
        msg = f"{repo.root} carries no readable `repo-policy.toml`: {unreadable}"
        raise ArmingError(msg) from unreadable
    fields = [
        declared.get(key) if isinstance(declared, dict) else None
        for key in ("owner", "name", "base_branch")
    ]
    if not all(isinstance(value, str) and value.strip() for value in fields):
        msg = (
            f"{repo.root}'s `repo-policy.toml` does not declare `repository.owner`, `.name` "
            f"and `.base_branch`, so nothing says which pull request is this repository's"
        )
        raise ArmingError(msg)
    owner, name, base = (str(value).strip() for value in fields)
    return Repository(owner, name, base)


def drafted(answer: str, where: str, ours: Repository) -> tuple[Drafted, ...]:
    """Every release pull request one answer names, in the order it names them.

    Each must be this repository's own, drafted by release-plz on one of its
    branches into the declared base branch: the step arms nothing it cannot
    tell is the release pull request.

    Raises:
        ArmingError: If the answer is not the program's shape, or names a pull
            request that is not this repository's release pull request. Read as
            "none", an answer nothing could parse would leave the release
            blocked with nothing said.
    """
    try:
        parsed = json.loads(answer)
    except json.JSONDecodeError as error:
        msg = f"{where} is not the JSON `release-plz release-pr --output json` writes: {error}"
        raise ArmingError(msg) from error
    # llmlint: ignore[contracts_have_one_source_or_a_drift_gate] suppressions.toml has the reason.
    prs = parsed.get("prs") if isinstance(parsed, dict) else None
    if not isinstance(prs, list):
        msg = f"{where} carries no `prs` list, which `release-plz release-pr` always answers"
        raise ArmingError(msg)
    named: list[Drafted] = []
    for entry in prs:
        fields = entry if isinstance(entry, dict) else {}
        number = fields.get("number")
        url = fields.get("html_url")
        head = fields.get("head_branch")
        base = fields.get("base_branch")
        if (
            not isinstance(number, int)
            or isinstance(number, bool)
            or number < 1
            or url != ours.pull(number)
            or not isinstance(head, str)
            or not head.startswith(RELEASE_BRANCHES)
            or base != ours.base_branch
        ):
            msg = (
                f"{where} names {entry!r}, which is not a release pull request of "
                f"{FORGE}/{ours.owner}/{ours.name} from a release-plz branch into "
                f"`{ours.base_branch}`, so it is not armed"
            )
            raise ArmingError(msg)
        named.append(Drafted(number, url))
    return tuple(named)


def arm(answer: Path, repo: Repo) -> list[str]:
    """Arm auto-merge on every release pull request `answer` names, saying what was done.

    Every one named is attempted, and every refusal is collected rather than
    raised at the first.

    Raises:
        ArmingError: If the answer cannot be read, or the forge refused to arm
            a pull request it names — naming each, in the forge's own words.
    """
    try:
        text = answer.read_text(encoding="utf-8")
    except (OSError, UnicodeDecodeError) as error:
        msg = (
            f"{answer} could not be read as what `release-plz release-pr --output json` "
            f"answered: {error}"
        )
        raise ArmingError(msg) from error
    pulls = drafted(text, str(answer), repository(repo))
    if not pulls:
        return ["release-plz drafted no release pull request, so there is nothing to arm"]
    said: list[str] = []
    refused: list[str] = []
    for pull in pulls:
        try:
            # llmlint: ignore[async_typed_clients_at_boundaries] suppressions.toml has the reason.
            done = run([*ARM, pull.url], timeout=TIMEOUT_SECONDS)
        except subprocess.TimeoutExpired:
            refused.append(f"#{pull.number} ({pull.url}): no answer within {TIMEOUT_SECONDS} s")
            continue
        if done.returncode != 0:
            words = (done.stderr or done.stdout).strip() or f"exit {done.returncode}"
            refused.append(f"#{pull.number} ({pull.url}): {words}")
            continue
        said.append(f"armed #{pull.number} ({pull.url}) to squash-merge once its checks pass")
    if refused:
        msg = (
            "the forge refused to arm auto-merge on the release pull request, so no release "
            "follows until somebody merges it: " + "; ".join(refused)
        )
        if said:
            msg += ". What was armed: " + "; ".join(said)
        raise ArmingError(msg)
    return said
