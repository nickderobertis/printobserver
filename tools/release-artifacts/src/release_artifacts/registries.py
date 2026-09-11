"""Proving each end-user route against what its own registry actually serves.

`installing.py` proves an artifact **built from the committed tree**, which is
the proof a change can run before anything is published. This module proves the
other half, and it is the half a user meets: that the registry the install-path
section points them at is serving the version under test, that what it serves
installs on a host with no Rust toolchain, and that the program the install put
on a path runs and reports that version.

Three outcomes, and only the first is a pass:

* `SERVED AND PROVEN` — the registry served the version under test, it
  installed, and the program it left reported that version.
* `SERVED AND NOT PROVEN` — the registry served it and something after that
  failed. An artifact that does not work.
* `NOT SERVED` — the registry serves nothing for the version under test. A
  publish that did not happen.

Those are two different repairs, which is why they are two answers rather than
one failure: the first is somebody's build, the second is somebody's release.
A pass is one line saying so; a failure is every fact this run knew, one to a
line, because that is what a reader chasing one of the two repairs needs.

**The version under test is never this tree's own.** What a user gets is
whatever the registry is serving, and the number in the workspace is whatever
release automation last wrote there — so a proof keyed on it would pass over a
registry serving nothing. It is the version the caller names, and the newest the
registry serves when the caller names none. `release` names the newest release
the forge has published, which is what a run by hand keys on.

**And a release's own proof is keyed on the release that run cut**, rather than
on the newest of them. `cut_at` reads that version off the tag release
automation left at the run's own commit, and the one concrete version it
answers is what every route proof is then given — so the three cannot resolve
three different releases between them, and a run cannot report green over a
release somebody else's run published while its own went unproven. The forge's
listing cannot make that binding: it says which releases exist and not which
run cut which.

**Every registry is reachable somewhere other than the real one**, through
`PRINTOBSERVER_PROOF_REGISTRIES`. Nothing here may publish to a registry in
order to prove a point, so the proof has to be drivable against a registry
serving nothing, one serving something broken, and one serving something that
works — which `standin.py` stands up.
"""

from __future__ import annotations

import json
import re
import shutil
import urllib.error
import urllib.request
from dataclasses import dataclass
from enum import StrEnum
from pathlib import Path
from typing import Protocol
from urllib.parse import urlsplit

from repo_checks import install_path
from repo_checks.model import Repo
from repo_checks.shell import run

from release_artifacts import targets
from release_artifacts.build import PROGRAM
from release_artifacts.installing import (
    INSTALL_SCRIPT,
    SCRIPT_DIRECTORY,
    TOOLCHAIN,
    TOOLCHAIN_REPORT,
    InstallError,
    ran,
    without_rust,
)

#: Where every registry is read from when nothing points them elsewhere.
PRINTOBSERVER_PROOF_REGISTRIES = "PRINTOBSERVER_PROOF_REGISTRIES"

#: Which version this run proves: a version, `release`, or nothing at all.
PRINTOBSERVER_PROOF_VERSION = "PRINTOBSERVER_PROOF_VERSION"

#: What the install script reads the release location off, which is its own.
PRINTOBSERVER_RELEASE_BASE = "PRINTOBSERVER_RELEASE_BASE"

#: The version this proof takes from the newest release the forge published,
#: rather than from a number a caller typed.
RELEASE = "release"

#: What a version of this repository looks like, and so what this proof can
#: select and install: three numbers, which is every version release automation
#: writes into the workspace under pre-1.0 Cargo rules. A registry may serve
#: anything at all beside them — a pre-release, a yanked-and-renamed
#: distribution, a tag somebody typed — and none of those is a version a run of
#: this proof may pick as "the newest" or hand to a package manager.
SUPPORTED = re.compile(r"^v?\d+\.\d+\.\d+$")

#: What a commit a release-time run ran at looks like, and so what this will
#: ask a checkout about. It arrives from a workflow's own event payload and
#: reaches `git` as an argument, so it stops being an arbitrary string here.
COMMIT = re.compile(r"^[0-9a-f]{7,40}$")

#: The field a release-time run publishes the version it cut under. Written as
#: `<field>=<version>`, which is the one line a job reads an output from — so
#: the run that resolves it and the jobs that prove it name one version, and
#: the three routes cannot each resolve a different one.
VERSION_FIELD = "version"

#: The field the publishing job publishes what it released under. Written as
#: `<field>=<tag> <tag>...`, and `<field>=` where it released nothing, because
#: that empty field is what the artifact build and the artifact publish are
#: gated on: `release-plz release` exits zero having released nothing, which is
#: every ordinary push under `release_always`, and the registries refuse a
#: version they already serve.
RELEASED_FIELD = "released"

#: How long a registry is given to say what it serves.
ASK_TIMEOUT_SECONDS = 60

#: How long a checkout is given to say which release was cut at a commit.
CHECKOUT_TIMEOUT_SECONDS = 60

#: What this proof calls itself when it asks a registry. A forge answers an
#: anonymous read and expects to be told who is asking.
AGENT = "printobserver-install-proof"


class Outcome(StrEnum):
    """What a route's own proof found, in the words its report names it by."""

    PROVEN = "SERVED AND PROVEN"
    NOT_PROVEN = "SERVED AND NOT PROVEN"
    NOT_SERVED = "NOT SERVED"


#: The process exit each outcome answers with. Only a pass is zero, and the two
#: failures are told apart by the status as well as by the words, so a caller
#: driving this needs no output parsing to know which repair it is looking at.
EXIT = {Outcome.PROVEN: 0, Outcome.NOT_PROVEN: 1, Outcome.NOT_SERVED: 3}

#: And the exit a registry that could not be ASKED answers with, which is none
#: of the three: an unreachable or refusing registry is a network rather than a
#: release or a build, and sharing an exit with `SERVED AND NOT PROVEN` would
#: send a reader to repair an artifact nothing here even read.
UNREADABLE = 4


#: What to do about each of the two failures, beside the words naming them. A
#: report that tells a reader which repair they are looking at and not where to
#: start it has stopped one step short of being any use — and this tier is read
#: by somebody chasing a broken release, who has the least time to work it out.
NEXT_BUILD = (
    "Next: this is a build to repair rather than a release. `just prove-route-*` proves "
    "the same route over an artifact built from the committed tree, which reproduces this "
    "without waiting for a publish."
)
NEXT_PUBLISH = (
    "Next: the `release-plz` run that cut v{version} is where this was to be published — "
    "its `artifacts` and `publish` jobs. Re-running them publishes what {where} is missing."
)
#: And for the answer that is neither: a body no registry protocol describes.
#: Nothing was proven or disproven, so the next step is at the address rather
#: than in this repository — this proof reads those documents and cannot repair
#: one.
NEXT_MALFORMED = (
    "Next: nothing was proven or disproven here. Ask that address for the same document "
    "by hand: what answered is the registry — or whatever `{standin}` pointed this at — "
    "rather than anything this repository publishes."
)
#: And for a checkout that cannot say which release the run at a commit cut.
#: Every one of these is a question about the clone rather than about a release,
#: and answering it needs the commit and the tags that name it.
NEXT_CHECKOUT = (
    "Next: this needs a checkout carrying that commit and its tags — `fetch-depth: 0` on "
    "the job's own checkout step, or `git fetch --tags` on a clone made by hand."
)


class RegistryError(RuntimeError):
    """A registry could not be asked what it serves."""


@dataclass(frozen=True, slots=True)
class Bases:
    """Where each of the three registries is read from.

    One stand-in address covers all three, because a proof that read one
    registry from a stand-in and another from the real internet would be a
    proof of neither.
    """

    #: The Python package registry, whose own paths are `/pypi/<name>/json`
    #: and `/simple`.
    pypi: str
    #: The JavaScript package registry, which serves a packument per name.
    npm: str
    #: Where the forge lists this repository's releases.
    listing: str
    #: Where the install script downloads a release's artifacts from.
    releases: str

    @classmethod
    def read(cls, repo: Repo, environment: dict[str, str]) -> Bases:
        """The real registries, or the stand-in one address names.

        Raises:
            RegistryError: If `repo-policy.toml` names no repository for the
                forge's own paths to be composed from.
        """
        declared = repo.policy.get("repository", {})
        owner = str(declared.get("owner", "")).strip()
        name = str(declared.get("name", "")).strip()
        if not owner or not name:
            msg = (
                "`repo-policy.toml` declares no `repository.owner` and "
                "`repository.name`, so nothing can say where this repository's "
                "own releases are listed. Next: declare both in that file's "
                "`[repository]` table, which is the one place they are written."
            )
            raise RegistryError(msg)
        standing_in = environment.get(PRINTOBSERVER_PROOF_REGISTRIES, "").strip().rstrip("/")
        if standing_in:
            return cls(
                pypi=f"{standing_in}/pypi",
                npm=f"{standing_in}/npm",
                listing=f"{standing_in}/forge/releases",
                releases=f"{standing_in}/forge/releases",
            )
        return cls(
            pypi="https://pypi.org",
            npm="https://registry.npmjs.org",
            listing=f"https://api.github.com/repos/{owner}/{name}/releases",
            releases=f"https://github.com/{owner}/{name}/releases",
        )

    def of(self, registry: str) -> str:
        """The address the registry serving one target answers on."""
        return {"pypi": self.pypi, "npm": self.npm, "release": self.listing}.get(registry, "")


@dataclass(frozen=True, slots=True)
class Selected:
    """The version under test, and where it was taken from."""

    version: str
    whence: str


@dataclass(frozen=True, slots=True)
class Proof:
    """What one route's proof against its own registry found."""

    target: str
    outcome: Outcome
    #: Everything a reader needs to act on it, one fact to a line.
    report: str

    @property
    def exit_status(self) -> int:
        """The process exit this proof answers with."""
        return EXIT[self.outcome]


def supported_version(version: str) -> str:
    """One version this proof can select, or nothing where it is not one.

    Every version reaching this comes from outside — a registry's metadata, a
    forge's release list, or the variable a caller named — so this is where a
    string stops being arbitrary and becomes a version something will be asked
    to install.
    """
    named = version.strip()
    return named.removeprefix("v") if SUPPORTED.match(named) else ""


def cut_at(root: Path, commit: str) -> str:
    """The version the release-time run at `commit` cut, or nothing where it cut none.

    Read from the tag that names that commit in the checkout, because that tag
    is what release automation left behind at the moment it cut the release —
    and because the forge's own listing cannot answer this. A listing says
    which releases exist and not which run cut which, so a run keyed on the
    newest of them proves whichever release finished last: two runs minutes
    apart, and the earlier one reports green over an artifact it never looked
    at. `release_always` makes that ordinary rather than rare — every push to
    the base branch finishes a run, and all but the release ones cut nothing at
    all.

    Raises:
        RegistryError: If the commit is not one a checkout can be asked about;
            if this checkout does not carry it, which a shallow clone does not
            and which would otherwise answer "no release" for every commit
            there is and pass over every publish; or if more than one version
            tag names it, which leaves which release that run cut unanswerable.
    """
    named = commit.strip()
    if not COMMIT.match(named):
        msg = (
            f"`{named}` is no commit to key a release's own proof on: it must be a "
            f"hexadecimal object name, as the forge's own event payload states one"
        )
        raise RegistryError(msg)
    carried = run(
        ["git", "cat-file", "-e", f"{named}^{{commit}}"],
        cwd=root,
        timeout=CHECKOUT_TIMEOUT_SECONDS,
    )
    if carried.returncode != 0:
        msg = (
            f"{root} does not carry the commit {named}, so it cannot say which release "
            f"the run at it cut. A clone without that commit and its tags answers `no "
            f"release` for every commit there is, which passes over every publish "
            f"rather than failing:\n{carried.stderr}\n{NEXT_CHECKOUT}"
        )
        raise RegistryError(msg)
    listed = run(["git", "tag", "--points-at", named], cwd=root, timeout=CHECKOUT_TIMEOUT_SECONDS)
    if listed.returncode != 0:
        msg = (
            f"{root} could not be asked which tags name {named}:\n{listed.stderr}\n{NEXT_CHECKOUT}"
        )
        raise RegistryError(msg)
    cut = sorted(
        {supported_version(tag) for tag in listed.stdout.split() if supported_version(tag)},
        key=ordered,
    )
    if len(cut) > 1:
        msg = (
            f"{named} is named by {', '.join(cut)}, and which release the run at that "
            f"commit cut is then not something a tag can answer. Next: that run's own "
            f"`release-plz` log says which version it cut — prove that one by naming it "
            f"in `{PRINTOBSERVER_PROOF_VERSION}`."
        )
        raise RegistryError(msg)
    return cut[0] if cut else ""


def released_by(answer: str) -> tuple[str, ...]:
    """The tags `release-plz release --output json` says it released, in order.

    That answer is one JSON object, `{"releases": [...]}`, with one entry per
    package released carrying its `package_name`, `tag` and `version` — and an
    empty list where the run released nothing. A workspace releasing thirteen
    crates under one version names one tag thirteen times, so what is answered
    is the distinct tags, each once.

    Raises:
        RegistryError: If the answer is not one that program writes. An answer
            nothing can read is refused rather than read as "released nothing",
            because the jobs gated on this skip on an empty field — and a
            release skipped over an unreadable answer is one nobody can install
            and nothing reported.
    """
    try:
        parsed = json.loads(answer)
    except json.JSONDecodeError as malformed:
        msg = f"the release program's answer is not JSON: {malformed}\n{answer!r}"
        raise RegistryError(msg) from malformed
    releases = parsed.get("releases") if isinstance(parsed, dict) else None
    if not isinstance(releases, list):
        msg = (
            f"the release program's answer carries no `releases` list, which is what "
            f"`release-plz release --output json` writes:\n{answer!r}"
        )
        raise RegistryError(msg)
    tags: list[str] = []
    for release in releases:
        tag = release.get("tag") if isinstance(release, dict) else None
        if not isinstance(tag, str) or not tag.strip():
            msg = f"a release in the release program's answer names no tag:\n{release!r}"
            raise RegistryError(msg)
        # Narrowed to a tag release automation writes before it reaches a job
        # output file: that file is read a line at a time as `name=value`, so
        # a tag carrying a newline would write a second output nothing named.
        if not SUPPORTED.match(tag):
            msg = (
                f"a release in the release program's answer names the tag {tag!r}, which is "
                f"not one release automation writes (`v<major>.<minor>.<patch>`)"
            )
            raise RegistryError(msg)
        if tag not in tags:
            tags.append(tag)
    return tuple(tags)


def ordered(version: str) -> tuple[int, ...]:
    """One version as it sorts against another.

    Every version this repository publishes is three numbers — pre-1.0 Cargo
    rules, written by release automation — so anything else sorts before all of
    them rather than being guessed at: a proof must not pick a pre-release
    nobody meant to install as "the newest".
    """
    parts = version.removeprefix("v").split(".")
    if len(parts) != 3 or not all(part.isdigit() for part in parts):
        return (-1,)
    return tuple(int(part) for part in parts)


def _asked(url: str) -> bytes:
    """What one registry answered, or nothing where it serves no such name.

    Raises:
        RegistryError: If the address is not one this asks over, or the
            registry could not be reached or refused the read. An unreachable
            registry is not a registry serving nothing: one is a network and
            the other is a missing publish, and reporting the first as the
            second would send a reader to repair a release that is fine.
    """
    if urlsplit(url).scheme not in {"http", "https"}:
        msg = (
            f"{url} is not an address this asks a registry over. Next: "
            f"`{PRINTOBSERVER_PROOF_REGISTRIES}` is what points the three registries "
            f"somewhere other than the real ones, and it names one `http` or `https` base."
        )
        raise RegistryError(msg)
    # llmlint: ignore[async_typed_clients_at_boundaries] suppressions.toml has the reason.
    request = urllib.request.Request(  # noqa: S310
        url, headers={"Accept": "application/json", "User-Agent": AGENT}
    )
    try:
        # llmlint: ignore[async_typed_clients_at_boundaries] The same one site as above.
        with urllib.request.urlopen(request, timeout=ASK_TIMEOUT_SECONDS) as answer:  # noqa: S310
            read: bytes = answer.read()
    except urllib.error.HTTPError as refused:
        if refused.code == 404:
            return b""
        msg = (
            f"{url} refused the read that asks what it serves ({refused}). Nothing was "
            f"proven or disproven here: re-run this once that registry answers."
        )
        raise RegistryError(msg) from refused
    except (urllib.error.URLError, TimeoutError, OSError) as unreachable:
        msg = (
            f"{url} could not be reached to ask what it serves ({unreachable}). Nothing "
            f"was proven or disproven here: re-run this once that registry answers."
        )
        raise RegistryError(msg) from unreachable
    return read


def _answered(url: str) -> object:
    """One registry's answer, parsed.

    Raises:
        RegistryError: If what it answered is not the JSON its own protocol
            says it answers.
    """
    raw = _asked(url)
    if not raw:
        return None
    try:
        return json.loads(raw)
    except (UnicodeError, json.JSONDecodeError) as unreadable:
        msg = (
            f"{url} answered something other than the JSON its protocol serves. "
            f"{NEXT_MALFORMED.format(standin=PRINTOBSERVER_PROOF_REGISTRIES)}"
        )
        raise RegistryError(msg) from unreadable


def _versions(url: str, field: str) -> list[str]:
    """The versions one registry's own metadata document lists under `field`.

    Refused rather than read past where the body is not the shape that
    registry's protocol declares. A malformed answer read as an empty mapping
    would come back as `NOT SERVED`, which sends a reader to repair a publish
    that happened — the one confusion this whole proof exists to remove.

    Raises:
        RegistryError: If the registry could not be asked, or answered
            something other than a document with that mapping in it.
    """
    answer = _answered(url)
    if answer is None:
        return []
    if not isinstance(answer, dict):
        msg = (
            f"{url} answered something other than the metadata document its protocol "
            f"serves. {NEXT_MALFORMED.format(standin=PRINTOBSERVER_PROOF_REGISTRIES)}"
        )
        raise RegistryError(msg)
    listed = answer.get(field, {})
    if not isinstance(listed, dict):
        msg = (
            f"{url} answered a `{field}` that is not the mapping of versions its protocol "
            f"serves. {NEXT_MALFORMED.format(standin=PRINTOBSERVER_PROOF_REGISTRIES)}"
        )
        raise RegistryError(msg)
    # Only the versions this proof can select. A registry serving a
    # pre-release beside the real ones is ordinary, and one of those is
    # neither what "the newest" means here nor something to hand a package
    # manager — so it is dropped where it arrives rather than carried to
    # whichever line would have tripped over it.
    return [
        supported_version(str(version)) for version in listed if supported_version(str(version))
    ]


def served(bases: Bases, target: targets.Target) -> tuple[str, ...]:
    """Every version the registry serving one target serves, newest last.

    Raises:
        RegistryError: If the registry could not be asked, answered something
            other than its own protocol, or is one nothing here knows how to
            ask.
    """
    match target.registry:
        case "pypi":
            versions = _versions(f"{bases.pypi}/pypi/{target.name}/json", "releases")
        case "npm":
            versions = _versions(f"{bases.npm}/{target.name}", "versions")
        case "release":
            versions = list(released(bases))
        case _:
            msg = (
                f"nothing here knows how to ask what serves `{target.id}`. Next: "
                f"`release-targets.toml` declares that target's `registry`, and the "
                f"registries this proof reads are `pypi`, `npm` and `release`."
            )
            raise RegistryError(msg)
    return tuple(sorted(set(versions), key=ordered))


def released(bases: Bases) -> tuple[str, ...]:
    """Every release the forge lists that a run may be keyed on, newest last.

    Raises:
        RegistryError: If the forge could not be asked, or answered a listing
            of something other than releases.
    """
    answer = _answered(bases.listing)
    if answer is None:
        return ()
    if not isinstance(answer, list):
        msg = (
            f"{bases.listing} answered something other than a list of releases. "
            f"{NEXT_MALFORMED.format(standin=PRINTOBSERVER_PROOF_REGISTRIES)}"
        )
        raise RegistryError(msg)
    tags: set[str] = set()
    for entry in answer:
        # Dropped silently, this is a release the proof would go on to say
        # nothing about — and where the dropped one was the newest, a
        # release-time run would prove the one before it.
        if not isinstance(entry, dict) or not isinstance(entry.get("tag_name"), str):
            msg = (
                f"{bases.listing} lists {entry!r}, which is not a release with a tag. "
                f"{NEXT_MALFORMED.format(standin=PRINTOBSERVER_PROOF_REGISTRIES)}"
            )
            raise RegistryError(msg)
        # Neither a draft nor a pre-release is a release a run of this may be
        # keyed on: the forge marks both, and what a release-time run proves is
        # what an ordinary user's own install would resolve to. Read by
        # truthiness, a `"false"` somebody's mirror answered with would be a
        # release skipped, so each is the boolean its protocol serves or the
        # listing is refused.
        marked = False
        for flag in ("draft", "prerelease"):
            reading = entry.get(flag, False)
            if not isinstance(reading, bool):
                msg = (
                    f"{bases.listing} lists {entry['tag_name']} with a `{flag}` of "
                    f"{reading!r}, which is not the boolean its protocol serves. "
                    f"{NEXT_MALFORMED.format(standin=PRINTOBSERVER_PROOF_REGISTRIES)}"
                )
                raise RegistryError(msg)
            marked = marked or reading
        if marked:
            continue
        # And a tag naming no version this proof can select is not one either.
        if tag := supported_version(entry["tag_name"]):
            tags.add(tag)
    return tuple(sorted(tags, key=ordered))


def select(bases: Bases, target: targets.Target, wanted: str) -> Selected:
    """The version under test, and where it came from.

    Raises:
        RegistryError: If a registry or the forge could not be asked.
    """
    named = wanted.strip()
    if named and named != RELEASE:
        version = supported_version(named)
        if not version:
            msg = (
                f"`{PRINTOBSERVER_PROOF_VERSION}={named}` is no version to prove: it must "
                f"be three numbers, as `0.1.0` or `v0.1.0`, or `{RELEASE}` for the newest "
                f"release the forge published"
            )
            raise RegistryError(msg)
        return Selected(version, f"named by the caller as `{named}`")
    if named == RELEASE:
        tags = released(bases)
        if not tags:
            return Selected("", f"the newest release {bases.listing} lists, and it lists none")
        return Selected(tags[-1], "the newest release the forge published")
    available = served(bases, target)
    if not available:
        return Selected("", f"the newest {bases.of(target.registry)} serves, and it serves none")
    return Selected(available[-1], f"the newest {bases.of(target.registry)} serves")


def _pypi_route(repo: Repo, target: targets.Target, version: str, into: Path, bases: Bases) -> Path:
    """Route 1, taken from the Python package registry with `pip` itself.

    A virtual environment of this proof's own, seeded with `pip`, and then the
    route's own install pinned to the version under test — with no Rust
    toolchain anywhere on the path it runs under.

    Raises:
        InstallError: If the environment could not be made or the install
            failed.
    """
    environment = into / "env"
    ran(
        ["uv", "venv", "--seed", "--clear", str(environment)],
        cwd=into,
        describing="making a Python environment holding no copy of these sources",
    )
    pinned = f"{target.name}=={version}"
    ran(
        [str(environment / "bin/pip"), "install", pinned],
        cwd=into,
        env=without_rust(
            {
                "PIP_INDEX_URL": f"{bases.pypi}/simple",
                "PIP_TRUSTED_HOST": urlsplit(bases.pypi).netloc,
                "PIP_NO_CACHE_DIR": "1",
                "PIP_DISABLE_PIP_VERSION_CHECK": "1",
            }
        ),
        describing=f"`pip install {pinned}` from {bases.pypi}",
    )
    return environment / "bin" / PROGRAM


def _npm_route(repo: Repo, target: targets.Target, version: str, into: Path, bases: Bases) -> Path:
    """Route 2, taken from the JavaScript package registry with `npm` itself.

    A global install into a prefix of this proof's own, pinned to the version
    under test, resolving from the registry the caller named.

    Raises:
        InstallError: If the install failed.
    """
    environment = into / "env"
    environment.mkdir(parents=True, exist_ok=True)
    pinned = f"{target.name}@{version}"
    ran(
        [
            "npm",
            "install",
            "--global",
            "--prefix",
            str(environment),
            "--no-audit",
            "--no-fund",
            pinned,
        ],
        cwd=into,
        env=without_rust(
            {
                "npm_config_registry": bases.npm,
                "npm_config_cache": str(into / "npm-cache"),
                "npm_config_update_notifier": "false",
            }
        ),
        describing=f"`npm install -g {pinned}` from {bases.npm}",
    )
    return environment / "bin" / PROGRAM


def _script_route(
    repo: Repo, target: targets.Target, version: str, into: Path, bases: Bases
) -> Path:
    """Route 3, taken by the committed install script against the real releases.

    The script is driven with `sh`, exactly as that route's own one-line
    command drives it, pinned to the version under test and pointed at where
    the releases are. What is being proven here is the download, the
    verification and the install — the fetch of the script itself is what the
    install job's own run of that one-line command proves.

    Raises:
        InstallError: If the script refused, which it does before anything
            reaches a path.
    """
    directory = into / "env" / SCRIPT_DIRECTORY
    ran(
        [
            "sh",
            str(repo.path(INSTALL_SCRIPT)),
            "--version",
            f"v{version}",
            "--to",
            str(directory),
        ],
        cwd=into,
        env=without_rust({PRINTOBSERVER_RELEASE_BASE: bases.releases}),
        describing=f"the committed install script against {bases.releases}",
    )
    return directory / PROGRAM


class Route(Protocol):
    """How one route is taken from its own registry."""

    def __call__(
        self, repo: Repo, target: targets.Target, version: str, into: Path, bases: Bases
    ) -> Path:
        """Take it, and answer the program the install left on a path."""


#: How each route is taken from its own registry, by the target it is. Which
#: identifiers belong here is NOT this list's to say — `release-targets.toml`
#: declares which targets carry a route, and `routed` below holds these keys to
#: that declaration on every run rather than trusting them.
ROUTES: dict[str, Route] = {
    "pypi:printobserver-cli": _pypi_route,
    "npm:printobserver-cli": _npm_route,
    "release:printobserver": _script_route,
}


def routed(repo: Repo) -> dict[str, Route]:
    """How each declared route is taken, reconciled with the declaration itself.

    A dispatch table keyed by target identifiers is a second copy of the set
    `release-targets.toml` declares, and the two drift in both directions with
    nothing to say so. A target that gains a route and nothing here takes is a
    route this tier reports nothing at all about — the silence this whole tier
    exists to remove — and a key here no declaration names is a route nothing
    installs, so the entry beside it is dead.

    Which FUNCTION takes each route cannot be derived from a declaration, so
    what is reconciled is the key set; the bodies stay where they are written.

    Raises:
        RegistryError: If the two disagree, naming which side is missing what.
    """
    declared = {target.id for target in targets.declared(repo.root) if target.route}
    untaken = sorted(declared - set(ROUTES))
    undeclared = sorted(set(ROUTES) - declared)
    if untaken or undeclared:
        msg = (
            f"the routes this proof takes and the routes `release-targets.toml` declares "
            f"disagree: {', '.join(untaken) or 'nothing'} is declared with a route and "
            f"nothing here takes it, and {', '.join(undeclared) or 'nothing'} is taken "
            f"here and declared with no route"
        )
        raise RegistryError(msg)
    return ROUTES


def take(repo: Repo, target: targets.Target, version: str, into: Path, bases: Bases) -> Path:
    """Take one route from its own registry, and answer the program it left.

    Raises:
        InstallError: If nothing here takes that route, or the install failed.
        RegistryError: If the routes taken here and the routes declared differ.
    """
    route = routed(repo).get(target.id)
    if route is None:
        msg = f"nothing here takes `{target.id}` the way an end user takes it"
        raise InstallError(msg)
    into.mkdir(parents=True, exist_ok=True)
    return route(repo, target, version, into, bases)


def _reported(program: Path, cwd: Path) -> str:
    """What the program a route installed says its own version is.

    Raises:
        InstallError: If the route put no program on the path it was given, or
            what it put there does not run.
    """
    if not program.exists():
        msg = f"the route put no {PROGRAM} at {program}"
        raise InstallError(msg)
    return ran(
        [str(program), "--version"],
        cwd=cwd,
        env=without_rust(),
        describing=f"{program} reporting its own version",
    ).strip()


def _answers(version: str) -> str:
    """The whole answer an installed `printobserver` gives `--version`.

    Its own name and the version it is, which is the one response that command
    line contracts to give — and so the one thing a route's proof can read as
    the artifact working rather than as the version appearing somewhere.
    """
    return f"{PROGRAM} {version}"


def _reported_as(version: str, said: str) -> bool:
    """Whether the installed program ANSWERED `--version` with the version under test.

    The whole answer, and not the version found somewhere in what the program
    printed. Two ways a looser comparison passes over an artifact that does not
    work, and both are the failure this tier exists to catch reporting itself
    green: a substring lets `printobserver 10.3.0` prove `0.3.0`, and a version
    among the words printed lets a program that ran and failed — one answering
    `unexpected error 0.3.0` and exiting zero — prove the release it was
    complaining about.
    """
    return said.strip() == _answers(version)


def prove(repo: Repo, identifier: str, into: Path, environment: dict[str, str]) -> Proof:
    """Take one route from its own registry and prove what it served.

    Raises:
        RegistryError: If the registry could not be asked what it serves. That
            is neither of the two failures this reports: one is a network and
            the others are a release and a build.
        InstallError: If the declaration names no such target.
    """
    target = targets.named(repo.root, identifier)
    # Before any outcome is reached, because a run that answers `NOT SERVED`
    # never takes a route and would never otherwise look at whether the set it
    # can take is still the set that is declared.
    routed(repo)
    bases = Bases.read(repo, environment)
    where = bases.of(target.registry)
    selected = select(bases, target, environment.get(PRINTOBSERVER_PROOF_VERSION, ""))
    stated = _stated_command(repo, target)
    preamble = [
        f"version under test: {selected.version or '(none)'} ({selected.whence})",
        f"route: {target.route or target.id} — `{stated}`" if stated else f"route: {target.id}",
        f"registry: {where}",
    ]

    available = served(bases, target)
    if not selected.version or selected.version not in available:
        return _refused(target, selected, where, available, preamble)

    into.mkdir(parents=True, exist_ok=True)
    try:
        installed = take(repo, target, selected.version, into, bases)
        version = _reported(installed, into)
    except InstallError as refused:
        return Proof(
            target.id,
            Outcome.NOT_PROVEN,
            _rendered(
                target,
                Outcome.NOT_PROVEN,
                [
                    *preamble,
                    f"{where} serves {selected.version}, and what it serves did not work here:",
                    str(refused),
                    NEXT_BUILD,
                ],
            ),
        )
    if not _reported_as(selected.version, version):
        return Proof(
            target.id,
            Outcome.NOT_PROVEN,
            _rendered(
                target,
                Outcome.NOT_PROVEN,
                [
                    *preamble,
                    f"installed: {installed}",
                    f"reported: {version}",
                    f"which is not the version under test: an installed "
                    f"{PROGRAM} answers `--version` with `{_answers(selected.version)}`",
                    NEXT_BUILD,
                ],
            ),
        )
    reached = [name for name in TOOLCHAIN if shutil.which(name, path=without_rust()["PATH"])]
    # A pass is ONE line: a tool that worked says so and stops. Everything a
    # reader of a pass needs is on it — which version was proven, where that
    # version came from, what installed it and what the program said — and
    # everything else this run knows is what a reader of a FAILURE needs, which
    # is why the two are not the same report.
    return Proof(
        target.id,
        Outcome.PROVEN,
        f"{target.id}: {Outcome.PROVEN} {selected.version} ({selected.whence}): "
        f"`{stated or target.id}` installed {version} — "
        f"{TOOLCHAIN_REPORT.format(', '.join(reached) or 'none')}",
    )


def _refused(
    target: targets.Target,
    selected: Selected,
    where: str,
    available: tuple[str, ...],
    preamble: list[str],
) -> Proof:
    """The answer where the registry serves nothing for the version under test."""
    serves = ", ".join(available) if available else "no version at all"
    return Proof(
        target.id,
        Outcome.NOT_SERVED,
        _rendered(
            target,
            Outcome.NOT_SERVED,
            [
                *preamble,
                f"{where} serves {serves} for `{target.name}`",
                "Nothing was installed: this is a publish that did not happen rather "
                "than an artifact that does not work.",
                NEXT_PUBLISH.format(version=selected.version or "it", where=where),
            ],
        ),
    )


def _rendered(target: targets.Target, outcome: Outcome, lines: list[str]) -> str:
    """One FAILING proof's whole answer, as a reader chasing it reads it.

    Every fact this run knew, one to a line, because what a reader of a failure
    is doing is finding out which of the two repairs it is. A pass is one line
    and is written where it is decided.
    """
    return "\n".join([f"{target.id}: {outcome}", *(f"  {line}" for line in lines)])


def _stated_command(repo: Repo, target: targets.Target) -> str:
    """The command `AGENTS.md`'s install-path section states for this route.

    Read rather than restated: the section is the authoritative source of the
    three routes, and a proof naming a command of its own would be a second
    statement of one of them.
    """
    for route in install_path.parse(repo.agents_md).routes:
        if route.heading == target.route:
            return route.command
    return ""
