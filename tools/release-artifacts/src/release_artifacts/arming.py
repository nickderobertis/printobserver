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
and `perf` alone, so that commit drafts no further release pull request. That
rule also releases on `BREAKING CHANGE:` anywhere in a message, body included,
and a squash commit's body is by default the pull request's description — here
release-plz's changelog, which quotes other commits' words. So the arming names
the merge commit's body itself, `SQUASH_BODY`, and what the merge leaves on
`main` never depends on what the changelog happens to say.

The answer's shape is the program's own, read from release-plz 0.3.167's
`main.rs` (`{"prs": [...]}`, empty when it drafted nothing) and its core's
`ReleasePr` (`head_branch`, `base_branch`, `html_url`, `number`, `releases`).
`samples/release-plz-release-pr.json` is a committed copy of it, and
`tests/repo-e2e/tests/test_release_path_journey.py` holds that copy to the
fields and types the held release actually writes, by running it against a
stand-in forge and arming what it answered.
"""

from __future__ import annotations

import json
import subprocess
from dataclasses import dataclass
from pathlib import Path

from repo_checks.model import Repo
from repo_checks.shell import run

from release_artifacts import targets

#: A committed answer of `release-plz release-pr --output json`, as the program
#: writes one having refreshed a release pull request.
DRAFTED_SAMPLE = Path("tools/release-artifacts/samples/release-plz-release-pr.json")

#: What arms one pull request: the forge's own auto-merge, squashing, as every
#: pull request of this repository is merged, with the merge commit's body
#: given as `SQUASH_BODY`. The pull request is named by its URL after that, which
#: names the repository too, so nothing is inferred from a remote.
ARM = ("gh", "pr", "merge", "--auto", "--squash", "--body")

#: The body of the commit the armed merge leaves on `main`, in place of the
#: pull request's description. It names no commit type and carries no
#: `BREAKING CHANGE:`, so `release_commits` drafts nothing for it.
SQUASH_BODY = (
    "Release automation's release pull request, merged by the forge once its "
    "required checks passed. What it releases is in each crate's CHANGELOG.md."
)

#: How long the forge is given to answer one arming before that pull request is
#: reported refused: a hung `gh` must fail the step naming it rather than hold
#: the job until the runner's own limit, which would say nothing about why.
TIMEOUT_SECONDS = 120

#: Every pull request this repository has is on github.com, and `ARM` names one
#: by its URL there, so an answer naming any other host is never one of ours.
FORGE = "https://github.com"

#: The keys of the drafting program's answer this reads, and no other: the list
#: of pull requests, and of each its number, URL, base branch and the packages
#: it releases, and of each of those its name. `test_arming.py` holds the
#: committed sample to carrying every one.
PRS, NUMBER, URL, BASE, RELEASES, PACKAGE = (
    "prs",
    "number",
    "html_url",
    "base_branch",
    "releases",
    "package_name",
)


#: What to do about an answer that is not the program's, or names a pull request
#: this step may not arm: nothing was armed, and the release waits on it.
ANSWER_NEXT = (
    "Next: read what release-plz answered in the drafting step's log above. If the held "
    "release changed the shape of that answer, `tests/repo-e2e/tests/"
    "test_release_path_journey.py`'s drift gate names the field that moved: update "
    "`samples/release-plz-release-pr.json` and this reader to it, then re-run the job."
)

#: What to do about a pull request the forge would not arm.
REFUSED_NEXT = (
    "Next: merge it by hand once its checks pass, or allow auto-merge in the repository's "
    "settings and let RELEASE_PLZ_TOKEN merge pull requests, then re-run the job."
)


class ArmingError(RuntimeError):
    """The release pull request could not be named, or could not be armed."""


@dataclass(frozen=True, slots=True)
class Drafted:
    """One release pull request the program says it opened or refreshed."""

    number: int
    url: str


@dataclass(frozen=True, slots=True)
class Repository:
    """Where a release pull request of this repository is, and what it can release.

    The owner, name and base branch as `repo-policy.toml` declares them, and
    the crates as `release-targets.toml` does.
    """

    owner: str
    name: str
    base_branch: str
    crates: frozenset[str]

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
        msg = (
            f"{repo.root} carries no readable `repo-policy.toml`: {unreadable}. Next: run "
            f"the step from a checkout of this repository."
        )
        raise ArmingError(msg) from unreadable
    fields = [
        declared.get(key) if isinstance(declared, dict) else None
        for key in ("owner", "name", "base_branch")
    ]
    if not all(isinstance(value, str) and value.strip() for value in fields):
        msg = (
            f"{repo.root}'s `repo-policy.toml` does not declare `repository.owner`, `.name` "
            f"and `.base_branch`, so nothing says which pull request is this repository's. "
            f"Next: run the step from a checkout of this repository."
        )
        raise ArmingError(msg)
    owner, name, base = (str(value).strip() for value in fields)
    crates = frozenset(
        target.name for target in targets.declared(repo.root) if target.registry == "crate"
    )
    return Repository(owner, name, base, crates)


def drafted(answer: str, where: str, ours: Repository) -> tuple[Drafted, ...]:
    """Every release pull request one answer names, in the order it names them.

    Each must be this repository's own, into the declared base branch, and a
    release pull request: one releasing at least one package, every one of
    them a crate this repository publishes. The step arms nothing else.

    Raises:
        ArmingError: If the answer is not the program's shape, or names a pull
            request that is not this repository's release pull request. Read as
            "none", an answer nothing could parse would leave the release
            blocked with nothing said.
    """
    try:
        parsed = json.loads(answer)
    except json.JSONDecodeError as error:
        msg = (
            f"{where} is not the JSON `release-plz release-pr --output json` writes: "
            f"{error}. {ANSWER_NEXT}"
        )
        raise ArmingError(msg) from error
    prs = parsed.get(PRS) if isinstance(parsed, dict) else None
    if not isinstance(prs, list):
        msg = (
            f"{where} carries no `{PRS}` list, which `release-plz release-pr` always "
            f"answers. {ANSWER_NEXT}"
        )
        raise ArmingError(msg)
    named: list[Drafted] = []
    for entry in prs:
        fields = entry if isinstance(entry, dict) else {}
        number = fields.get(NUMBER)
        url = fields.get(URL)
        if (
            not isinstance(number, int)
            or isinstance(number, bool)
            or number < 1
            or url != ours.pull(number)
            or fields.get(BASE) != ours.base_branch
        ):
            msg = (
                f"{where} names {entry!r}, which is not a pull request of "
                f"{FORGE}/{ours.owner}/{ours.name} into `{ours.base_branch}`, so it is "
                f"not armed. {ANSWER_NEXT}"
            )
            raise ArmingError(msg)
        # llmlint: ignore[boundary_inputs_validated] suppressions.toml has the reason.
        released = fields.get(RELEASES)
        packages = [
            release.get(PACKAGE) if isinstance(release, dict) else None
            for release in (released if isinstance(released, list) else [])
        ]
        if not packages or not all(
            isinstance(package, str) and package in ours.crates for package in packages
        ):
            msg = (
                f"{where} names #{number}, which releases {packages!r}: a release pull "
                f"request releases at least one package and only crates "
                f"`release-targets.toml` declares, so it is not armed. {ANSWER_NEXT}"
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
            f"answered: {error}. {ANSWER_NEXT}"
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
            done = run([*ARM, SQUASH_BODY, pull.url], timeout=TIMEOUT_SECONDS)
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
        raise ArmingError(f"{msg}. {REFUSED_NEXT}")
    return said
