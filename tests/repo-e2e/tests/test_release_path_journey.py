"""The release path can draft a release from this tree at all.

`release-plz release-pr` computes each package's next version by comparing it
against what the registry serves, and it stops dead on a package the registry
does not carry while a tag naming that package's version exists. This
repository's first release run left exactly that state behind: two scaffold
crates published at `0.1.0`, the tag `v0.1.0`, and eleven crates the registry
never saw. The repair was to move the workspace to a version no crate had
served and no tag named, and these journeys are what hold the tree to it.

The drafting tool is driven for real — `release-plz update` is the same
computation `release-pr` runs, minus opening the pull request — against a
stand-in registry that carries no entry for any crate this tree publishes, over
a copy carrying the tags this repository carried when the repair was made. A
tree that returned to a tagged version is refused there, naming the tag.
"""

# `assert` is how pytest states an assertion and how it produces the failure
# message a reader acts on; suppressions.toml carries the reason.

from __future__ import annotations

import difflib
import hashlib
import http.client
import json
import os
import secrets
import shutil
import threading
import tomllib
from collections.abc import Callable
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from pathlib import Path
from typing import Self
from urllib.parse import urlsplit

import pytest
from journey import COMMIT_IDENTITY, REPO_ROOT, GateCopy, capture, clean_environment, output, run
from release_artifacts import arming, bumping
from release_artifacts.arming import DRAFTED_SAMPLE
from repo_checks.expect import absent, contains, equal, failing, passing, truth
from repo_checks.model import Repo
from repo_checks.shell import run as shell_run
from test_release_program import step_arguments
from test_release_workflow_journey import gh_stand_in

#: The baseline of occupied versions, taken 2026-09-10 before the repair and
#: fixed here rather than re-read: the one tag this repository carried, which is
#: also the one version any of its crates had ever published. The tree's version
#: must be absent from it, and a copy carrying these tags is what the drafting
#: tool is driven over.
OCCUPIED = ("0.1.0",)

#: The name the stand-in registry is reached under. Cargo reads a registry's
#: index out of `CARGO_REGISTRIES_<NAME>_INDEX`, so this is the one name both
#: the environment and `--registry` spell.
STANDIN = "standin"

#: What the drafting tool says of a package the registry does not carry while
#: a tag naming its version exists.
WEDGED = "not found in the registry, but the git tag"

#: What it says as it reaches the computation that state stops, once per crate.
DETERMINING = "determining next version for"

#: The drafting computation takes a few seconds over this workspace; this is a
#: bound on a hang rather than a budget.
DRAFT_TIMEOUT_SECONDS = 120

pytestmark = pytest.mark.skipif(
    shutil.which("release-plz") is None,
    reason=(
        "release-plz is installed by `just install-tools release-plz`, which "
        "`just test-e2e` runs before this tier"
    ),
)


def workspace_version() -> str:
    """The one version the workspace declares for every crate."""
    with (REPO_ROOT / "Cargo.toml").open("rb") as handle:
        return str(tomllib.load(handle)["workspace"]["package"]["version"])


# llmlint: ignore[e2e_not_mocked] suppressions.toml has the reason.
class EmptyIndex:
    """A sparse cargo registry index carrying no crate at all.

    It answers `config.json`, which is what makes it a registry to cargo, and
    answers every other read — every crate a tool asks it for — with `404`,
    which is how a sparse index says a crate does not exist. What was asked for
    is recorded, so a journey can say the tool consulted this registry rather
    than the real one.
    """

    def __init__(self) -> None:
        """Start answering on a port the operating system chooses."""
        self.asked: list[str] = []
        self._server = ThreadingHTTPServer(("127.0.0.1", 0), self._handler())
        self._serving = threading.Thread(target=self._server.serve_forever, daemon=True)
        self._serving.start()

    @property
    def index(self) -> str:
        """The index URL cargo is pointed at, in the sparse protocol's spelling."""
        host, port = self._server.server_address[:2]
        return f"sparse+http://{host}:{port}/"

    def environment(self) -> dict[str, str]:
        """The environment that makes this the registry named `STANDIN`."""
        return {f"CARGO_REGISTRIES_{STANDIN.upper()}_INDEX": self.index}

    def stop(self) -> None:
        """Stop answering."""
        self._server.shutdown()
        self._server.server_close()

    def __enter__(self) -> Self:
        """Serve for the duration of a `with` block."""
        return self

    def __exit__(self, *_: object) -> None:
        """Stop serving when the block ends."""
        self.stop()

    def _handler(self) -> type[BaseHTTPRequestHandler]:
        registry = self

        class Handler(BaseHTTPRequestHandler):
            """Answer `config.json`, and answer every crate with `404`."""

            protocol_version = "HTTP/1.1"

            def do_GET(self) -> None:
                """Answer one read, recording what was asked for."""
                registry.asked.append(self.path)
                if self.path == "/config.json":
                    body = json.dumps(
                        {"dl": registry.index.removeprefix("sparse+") + "dl/{crate}/{version}"}
                    ).encode()
                    self.send_response(200)
                    self.send_header("Content-Type", "application/json")
                    self.send_header("Content-Length", str(len(body)))
                    self.end_headers()
                    self.wfile.write(body)
                    return
                self.send_response(404)
                self.send_header("Content-Length", "0")
                self.end_headers()

            def log_message(self, format: str, *args: object) -> None:
                """Say nothing: a stand-in whose log is the output is not signal."""

        return Handler


def tagged(gate_copy: Callable[..., GateCopy], versions: tuple[str, ...]) -> GateCopy:
    """A copy carrying the tags release automation would have left for `versions`.

    Without the installed JavaScript dependencies: the drafting tool copies the
    whole tree aside to diff it, and a symbolic link out of the tree is one it
    refuses to copy. Nothing it does reads them.
    """
    copy = gate_copy(node_modules=False)
    for version in versions:
        shell_run(["git", "tag", f"v{version}"], cwd=copy.root, check=True)
    return copy


# llmlint: ignore[tests_mirror_real_usage] suppressions.toml has the reason.
# llmlint: ignore[expensive_tests_stay_behind_their_own_edge] suppressions.toml has the reason.
def drafted(copy: GateCopy, registry: EmptyIndex) -> tuple[int, str]:
    """Drive the drafting tool's computation over the copy, against the stand-in.

    `update` is what `release-pr` runs before it opens anything: it downloads
    each published package from the registry, diffs it against the tree and
    writes the next version. `--no-changelog` because the copy has no remote
    for a changelog to link to, and nothing here is about the changelog.
    """
    result = capture(
        ["release-plz", "update", "--no-changelog", "--registry", STANDIN],
        copy.root,
        timeout=DRAFT_TIMEOUT_SECONDS,
        env=clean_environment(**registry.environment()),
    )
    return result.returncode, output(result)


def test_the_workspace_declares_one_version_no_occupied_version_names() -> None:
    """Every crate, every inter-crate requirement and the lockfile agree on one version.

    Read through `cargo metadata --locked`, which refuses a lockfile the
    manifests have moved away from, so the committed lockfile is proven to
    resolve the committed manifests without being amended.
    """
    version = workspace_version()
    absent(OCCUPIED, version, describing="the versions the registry or a tag already occupies")

    result = run(["cargo", "metadata", "--locked", "--format-version", "1"], cwd=REPO_ROOT)
    passing(result, describing="`cargo metadata --locked` over the committed tree")
    metadata = json.loads(result.stdout)
    members = [package for package in metadata["packages"] if package["source"] is None]
    names = {package["name"] for package in members}
    truth(len(members) > 1, describing="the workspace to hold more than one crate")
    for package in members:
        equal(package["version"], version, describing=f"the version `{package['name']}` declares")
        for dependency in package["dependencies"]:
            if dependency["name"] in names:
                equal(
                    dependency["req"],
                    f"^{version}",
                    describing=f"what `{package['name']}` requires of `{dependency['name']}`",
                )


def test_the_drafting_tool_finds_no_absent_registry_entry_to_compare_against(
    gate_copy: Callable[..., GateCopy],
) -> None:
    """Over the tree as committed, the tool drafts one next version for every crate.

    The copy carries the tags this repository carried when the repair was made,
    and the registry it is compared against carries none of its crates — which
    is the state the registry was in for eleven of the thirteen, and is a
    stricter one than the real registry's for the other two.
    """
    version = workspace_version()
    copy = tagged(gate_copy, OCCUPIED)

    with EmptyIndex() as registry:
        code, said = drafted(copy, registry)

    passing((code, said), describing="drafting a release from the committed tree")
    absent(said, WEDGED, describing="what the tool said")
    # Every crate of the workspace reached the computation the wedged state
    # stopped, and each was computed from the one version the tree declares.
    computed = [line for line in said.splitlines() if DETERMINING in line]
    crates = sorted(path.name for path in (REPO_ROOT / "crates").iterdir() if path.is_dir())
    equal(len(computed), len(crates), describing=f"the crates the tool computed: {computed}")
    for crate in crates:
        contains(said, f"{DETERMINING} {crate} {version}", describing="what the tool computed")
    truth(
        any(path.endswith("/printobserver") for path in registry.asked),
        describing=f"the stand-in to have been asked for a crate this tree publishes: "
        f"{registry.asked}",
    )


def test_a_tree_at_a_version_a_tag_already_names_cannot_be_drafted_from(
    gate_copy: Callable[..., GateCopy],
) -> None:
    """A tag naming the tree's own version, with no registry entry behind it, wedges it.

    This is the state the repair left behind, reproduced on the committed tree
    by giving the copy the tag release automation would leave for its version.
    The tool refuses to draft, and it names the tag rather than the network.
    """
    version = workspace_version()
    copy = tagged(gate_copy, (version,))

    with EmptyIndex() as registry:
        code, said = drafted(copy, registry)

    failing((code, said), naming=WEDGED)
    contains(said, f"v{version}", describing="the tag the tool named")


#: The subjects the merge of a release pull request can leave on `main`: every
#: title release-plz can give that pull request, which squash-merging makes the
#: subject. They are the three arms of `release_plz_core::pr::pr_title` at the
#: release `repo-policy.toml` holds, for a configuration naming no `pr_name`:
#: one package of several, packages at differing versions, and one version.
# llmlint: ignore[contracts_have_one_source_or_a_drift_gate] suppressions.toml has the reason.
RELEASE_SUBJECTS = (
    "chore(printobserver-types): release v{version}",
    "chore: release",
    "chore: release v{version}",
)

#: What the drafting tool says of a package no commit since its release names
#: under a subject `release_commits` releases on, and of one it will release —
#: the decision a release pull request is opened on, per package.
NO_RELEASE_COMMIT = "no commit matches the `release_commits` regex"
NEXT_VERSION = "next version is"


def merged_release(
    gate_copy: Callable[..., GateCopy],
    *after: str,
    subject: str = RELEASE_SUBJECTS[-1],
    body: str = arming.SQUASH_BODY,
) -> GateCopy:
    """A copy whose `main` ends at a merged release pull request, then at `after`.

    The copy's own history is one `chore:` commit; on it lands exactly what the
    release pull request carries — the workspace moved as release-plz moves it —
    under the subject that pull request is merged as and the body the arming
    names for it. Each subject in `after` then lands as a commit touching one
    crate's sources.
    """
    copy = gate_copy(node_modules=False)
    version = bumping.next_minor(bumping.workspace_version(copy.root))
    # llmlint: ignore[tests_mirror_real_usage] suppressions.toml has the reason.
    bumping.bump_workspace_version(copy.root, version)
    shell_run(
        ["git", *COMMIT_IDENTITY, "commit", "-qam", f"{subject.format(version=version)}\n\n{body}"],
        cwd=copy.root,
        check=True,
    )
    for index, subject in enumerate(after):
        touched = copy.root / "crates" / "printobserver-types" / "src" / f"touched_{index}.rs"
        touched.write_text("//! A change the next release would carry.\n", encoding="utf-8")
        shell_run(["git", "add", "-A"], cwd=copy.root, check=True)
        shell_run(["git", *COMMIT_IDENTITY, "commit", "-qm", subject], cwd=copy.root, check=True)
    shell_run(["git", "repack", "-ad"], cwd=copy.root, check=True)
    return copy


def decided(said: str) -> dict[str, str]:
    """What the drafting tool decided for each crate: `release` or `skip`."""
    decisions: dict[str, str] = {}
    for line in said.splitlines():
        for marker, decision in ((NO_RELEASE_COMMIT, "skip"), (NEXT_VERSION, "release")):
            if f": {marker}" in line:
                crate = line.split(f": {marker}")[0].split()[-1]
                decisions[crate] = decision
    return decisions


@pytest.mark.parametrize("subject", RELEASE_SUBJECTS)
# llmlint: ignore[expensive_tests_stay_behind_their_own_edge] suppressions.toml has the reason.
def test_the_commit_a_merged_release_pull_request_leaves_drafts_no_further_release(
    gate_copy: Callable[..., GateCopy], subject: str
) -> None:
    """Arming the release pull request's auto-merge cannot loop.

    The drafting tool, driven for real over `main` as a merged release pull
    request leaves it, proposes nothing: `release-plz.toml`'s `release_commits`
    does not release on a `chore`, so no second release pull request is opened
    for the auto-merge that follows to merge.
    """
    copy = merged_release(gate_copy, subject=subject)

    # llmlint: ignore[e2e_not_mocked] suppressions.toml has the reason.
    with EmptyIndex() as registry:
        code, said = drafted(copy, registry)

    passing((code, said), describing="drafting over a merged release pull request")
    crates = sorted(path.name for path in (copy.root / "crates").iterdir() if path.is_dir())
    equal(
        decided(said),
        dict.fromkeys(crates, "skip"),
        describing=f"what the tool decided for each crate, having said:\n{said}",
    )


# llmlint: ignore[expensive_tests_stay_behind_their_own_edge] suppressions.toml has the reason.
def test_a_fix_after_the_release_commit_is_drafted_as_the_next_release(
    gate_copy: Callable[..., GateCopy],
) -> None:
    """The same drive with a `fix:` after it releases that crate: the skip above is the guard."""
    copy = merged_release(gate_copy, "fix(types): a change worth releasing")

    # llmlint: ignore[e2e_not_mocked] suppressions.toml has the reason.
    with EmptyIndex() as registry:
        code, said = drafted(copy, registry)

    passing((code, said), describing="drafting over a fix after the release")
    equal(
        decided(said).get("printobserver-types"),
        "release",
        describing=f"what the tool decided for the crate the fix touched, having said:\n{said}",
    )


#: A release pull request's description as release-plz writes it, quoting a
#: commit whose own words carry the footer `release_commits` releases on. It is
#: what a squash merge would put in the body were the arming not to name one.
QUOTING_CHANGELOG = """## `printobserver-types`

### Fixed

- *(types)* read a `BREAKING CHANGE:` footer the way the convention spells it
"""


# llmlint: ignore[expensive_tests_stay_behind_their_own_edge] suppressions.toml has the reason.
def test_a_merge_body_quoting_the_breaking_footer_would_draft_a_release(
    gate_copy: Callable[..., GateCopy],
) -> None:
    """The control for naming the merge commit's body: a changelog body would loop.

    The same drive as the loop guard's, with the body a squash merge takes from
    the pull request's description by default. The drafting tool releases on
    it, which is why `arming.SQUASH_BODY` is what the merge leaves instead.
    """
    copy = merged_release(gate_copy, body=QUOTING_CHANGELOG)

    # llmlint: ignore[e2e_not_mocked] suppressions.toml has the reason.
    with EmptyIndex() as registry:
        code, said = drafted(copy, registry)

    passing((code, said), describing="drafting over a merge carrying the changelog")
    truth(
        "release" in decided(said).values(),
        describing=f"the tool to release on the quoted footer, having said:\n{said}",
    )


#: Every file a release pull request moves the version in, as the tree names them.
VERSIONED = ("Cargo.toml", "Cargo.lock", "crates/*/Cargo.toml")


def versioned(root: Path) -> dict[str, str]:
    """The text of every file a release pull request moves the version in, by path."""
    return {
        path.relative_to(root).as_posix(): path.read_text(encoding="utf-8")
        for pattern in VERSIONED
        for path in sorted(root.glob(pattern))
    }


# llmlint: ignore[expensive_tests_stay_behind_their_own_edge] suppressions.toml has the reason.
def test_the_bump_the_suites_apply_is_what_the_drafting_tool_writes(
    gate_copy: Callable[..., GateCopy],
) -> None:
    """The drift gate for `release_artifacts.bumping`, against the held release-plz.

    The drafting tool is driven for real over a copy whose `main` carries a
    `fix` in every crate, so that every crate is released as release pull
    request #41 released every one. It compares against a second, untouched copy
    standing where the registry's packages would — `--registry-manifest-path`,
    its own way of reading a release that is already available locally. The
    helper then moves an untouched copy to the version the tool chose, and
    every manifest and the lock file must read byte for byte as the tool wrote
    them.
    """
    released = gate_copy(node_modules=False)
    drafted_copy = gate_copy(node_modules=False)
    for crate in sorted((drafted_copy.root / "crates").glob("*/src")):
        touched = crate / "touched_by_fix.rs"
        touched.write_text("//! A change the release carries.\n", encoding="utf-8")
    shell_run(["git", "add", "-A"], cwd=drafted_copy.root, check=True)
    shell_run(
        ["git", *COMMIT_IDENTITY, "commit", "-qm", "fix: a change to every crate"],
        cwd=drafted_copy.root,
        check=True,
    )
    shell_run(["git", "repack", "-ad"], cwd=drafted_copy.root, check=True)
    was = bumping.workspace_version(released.root)

    result = capture(
        [
            "release-plz",
            "update",
            "--no-changelog",
            "--registry-manifest-path",
            str(released.root / "Cargo.toml"),
        ],
        drafted_copy.root,
        timeout=DRAFT_TIMEOUT_SECONDS,
        env=clean_environment(),
    )
    passing(result, describing="drafting a release of every crate")
    version = bumping.workspace_version(drafted_copy.root)
    truth(version != was, describing=f"the tool to have moved the workspace from {was}")

    equal(
        # llmlint: ignore[tests_mirror_real_usage] suppressions.toml has the reason.
        bumping.bump_workspace_version(released.root, version),
        was,
        describing="what the helper moved the untouched copy from",
    )
    by_helper, by_tool = versioned(released.root), versioned(drafted_copy.root)
    equal(sorted(by_helper), sorted(by_tool), describing="the versioned files of the two copies")
    differing = [path for path in by_tool if by_helper[path] != by_tool[path]]
    moved = "".join(
        "".join(
            difflib.unified_diff(
                by_tool[path].splitlines(keepends=True),
                by_helper[path].splitlines(keepends=True),
                f"{path} (release-plz)",
                f"{path} (bumping)",
            )
        )
        for path in differing
    )
    equal(
        differing,
        [],
        describing=f"the files the helper moved to {version} otherwise than the tool:\n{moved}",
    )


#: The most the drafting forge stand-in reads of one request body. The largest
#: the held release sends is the GraphQL commit, which carries every file the
#: release pull request changes, base64-encoded: well under a megabyte here.
MAX_FORGE_BODY_BYTES = 16 * 1024 * 1024

#: What the drafting forge stand-in authenticates the program by: minted when
#: this module is loaded, and never a real token.
FORGE_CREDENTIAL = f"drafting-{secrets.token_hex(8)}"


# llmlint: ignore[e2e_not_mocked] suppressions.toml has the reason.
class DraftingForge:
    """GitHub's API as `release-plz release-pr` reaches it to open a pull request.

    It answers the four requests the held release makes, in GitHub's shapes: no
    release pull request open yet, the branch's ref created, the release commit
    made on it through GraphQL's `createCommitOnBranch`, and the pull request
    opened — numbered, and at the URL GitHub gives a pull request of this
    repository. Anything else is `404`, and every request is recorded.
    """

    def __init__(self, owner: str, name: str) -> None:
        """Start answering on a port the operating system chooses."""
        self.owner, self.name = owner, name
        self.asked: list[str] = []
        self.opened: list[str] = []
        self._server = ThreadingHTTPServer(("127.0.0.1", 0), self._handler())
        self._serving = threading.Thread(target=self._server.serve_forever, daemon=True)
        self._serving.start()

    @property
    def repo_url(self) -> str:
        """The repository URL the program is pointed at, from which it derives the API."""
        host, port = self._server.server_address[:2]
        return f"http://{host}:{port}/{self.owner}/{self.name}"

    def __enter__(self) -> Self:
        """Serve for the duration of a `with` block."""
        return self

    def __exit__(self, *_: object) -> None:
        """Stop serving when the block ends."""
        self._server.shutdown()
        self._server.server_close()

    # llmlint: ignore[e2e_not_mocked] suppressions.toml has the reason.
    def answer(self, method: str, path: str, body: dict[str, object]) -> tuple[int, object]:
        """GitHub's status and document for one request, or `404`."""
        repository = f"/api/v3/repos/{self.owner}/{self.name}"
        digest = hashlib.sha1(json.dumps(body).encode(), usedforsecurity=False).hexdigest()
        pulls, refs = f"{repository}/pulls", f"{repository}/git/refs"
        match (method, path):
            case ("GET", _) if path == pulls:
                # llmlint: ignore[e2e_not_mocked] suppressions.toml has the reason.
                return 200, []
            case ("POST", _) if path == refs:
                # llmlint: ignore[e2e_not_mocked] suppressions.toml has the reason.
                return 201, {"ref": body.get("ref"), "object": {"sha": body.get("sha")}}
            case ("POST", "/api/graphql"):
                # llmlint: ignore[e2e_not_mocked] suppressions.toml has the reason.
                return 200, {"data": {"createCommitOnBranch": {"commit": {"oid": digest}}}}
            case ("POST", _) if path == pulls:
                number = len(self.opened) + 1
                url = f"https://github.com/{self.owner}/{self.name}/pull/{number}"
                self.opened.append(url)
                # llmlint: ignore[e2e_not_mocked] suppressions.toml has the reason.
                return 201, {
                    "id": number,
                    "node_id": f"PR_{number}",
                    "number": number,
                    "html_url": url,
                    "title": body.get("title"),
                    "body": body.get("body"),
                    "head": {"ref": body.get("head"), "sha": digest},
                    "base": {"ref": body.get("base")},
                    "user": {"login": "release-plz", "id": 1},
                    "labels": [],
                }
            case _:
                # llmlint: ignore[e2e_not_mocked] suppressions.toml has the reason.
                return 404, {"message": "Not Found"}

    def _handler(self) -> type[BaseHTTPRequestHandler]:
        forge = self

        class Handler(BaseHTTPRequestHandler):
            """Answer each request as `DraftingForge.answer` says, under its credential."""

            protocol_version = "HTTP/1.1"

            def _body(self) -> dict[str, object] | None:
                """The request's JSON object, `{}` for none, or `None` where it is not one."""
                declared = self.headers.get("Content-Length") or "0"
                # ASCII digits alone: `isdigit` also admits `²`, which `int` refuses.
                if not (declared.isascii() and declared.isdigit()):
                    return None
                if int(declared) > MAX_FORGE_BODY_BYTES:
                    return None
                raw = self.rfile.read(int(declared))
                if not raw:
                    return {}
                try:
                    parsed = json.loads(raw)
                except ValueError:
                    # Undecodable bytes and text that is not JSON: `loads`
                    # refuses both as a `ValueError`.
                    return None
                return parsed if isinstance(parsed, dict) else None

            def _any(self) -> None:
                """Record the request and send what the forge answers it."""
                path = self.path.split("?", 1)[0]
                forge.asked.append(f"{self.command} {path}")
                token = self.headers.get("Authorization", "").split()[-1:]
                body = self._body()
                if token != [FORGE_CREDENTIAL]:
                    code, document = 401, {"message": "Bad credentials"}
                elif body is None:
                    code, document = 400, {"message": "Problems parsing JSON"}
                else:
                    code, document = forge.answer(self.command, path, body)
                payload = json.dumps(document).encode()
                self.send_response(code)
                self.send_header("Content-Type", "application/json")
                self.send_header("Content-Length", str(len(payload)))
                self.end_headers()
                self.wfile.write(payload)

            def do_GET(self) -> None:
                """Answer one read."""
                self._any()

            def do_POST(self) -> None:
                """Answer one write."""
                self._any()

            def log_message(self, format: str, *args: object) -> None:
                """Say nothing: a stand-in whose log is the output is not signal."""

        return Handler


@pytest.mark.parametrize("declared", ["\u00b2", "-1", "1e3", str(MAX_FORGE_BODY_BYTES + 1)])
def test_the_drafting_forge_answers_a_length_it_cannot_read_as_malformed(declared: str) -> None:
    """A `Content-Length` that is no ASCII count, or one past the bound, is GitHub's `400`.

    Sent over a real connection with the stand-in's own credential, so the only
    thing wrong with the request is the length it declares.
    """
    with DraftingForge("owner", "name") as forge:
        host, port = urlsplit(forge.repo_url).netloc.split(":")
        # llmlint: ignore[async_typed_clients_at_boundaries] suppressions.toml has the reason.
        connection = http.client.HTTPConnection(host, int(port), timeout=30)
        connection.putrequest("POST", "/api/graphql")
        connection.putheader("Authorization", f"Bearer {FORGE_CREDENTIAL}")
        connection.putheader("Content-Length", declared)
        connection.endheaders()
        answered = connection.getresponse()
        said = json.loads(answered.read())
        connection.close()

    equal(answered.status, 400, describing=f"the status for a length of {declared!r}")
    equal(said, {"message": "Problems parsing JSON"}, describing="what the stand-in said")


def shape(value: object) -> object:
    """The fields of a JSON document and the JSON type of each, with no values.

    A list is its first element's shape: every pull request and every release
    in the answer is one of a kind.
    """
    match value:
        case dict():
            return {key: shape(entry) for key, entry in sorted(value.items())}
        case list():
            return [shape(value[0])] if value else []
        case _:
            return type(value).__name__


# llmlint: ignore[expensive_tests_stay_behind_their_own_edge] suppressions.toml has the reason.
def test_the_committed_drafting_answer_is_what_the_held_program_writes_and_is_armed(
    gate_copy: Callable[..., GateCopy], tmp_path: Path
) -> None:
    """The drift gate for `samples/release-plz-release-pr.json`.

    The held `release-plz` runs the committed drafting step's own arguments over
    a copy carrying a `fix:` since its last release, against the empty stand-in
    registry and a stand-in GitHub, and opens its release pull request there.
    Its answer has to have the committed sample's fields and types, and the
    real `just release-pr-arm` has to arm that pull request and no other: a
    release of the program that renamed a field would fail here rather than
    leave the release pull request unarmed.
    """
    ours = arming.repository(Repo(REPO_ROOT))
    copy = merged_release(gate_copy, "fix(types): a change worth releasing")

    # llmlint: ignore[e2e_not_mocked] suppressions.toml has the reason.
    with EmptyIndex() as registry, DraftingForge(ours.owner, ours.name) as forge:
        result = capture(
            [
                *step_arguments("release-plz release-pr"),
                "--registry",
                STANDIN,
                "--repo-url",
                forge.repo_url,
                "--git-token",
                FORGE_CREDENTIAL,
            ],
            copy.root,
            timeout=DRAFT_TIMEOUT_SECONDS,
            env=clean_environment(**registry.environment()),
        )

    passing((result.returncode, output(result)), describing="drafting against the stand-ins")
    equal(len(forge.opened), 1, describing=f"the pull requests opened, having asked {forge.asked}")
    answer = json.loads(result.stdout)
    sample = json.loads((REPO_ROOT / DRAFTED_SAMPLE).read_text(encoding="utf-8"))
    equal(shape(answer), shape(sample), describing="the program's answer against the sample")

    stand_ins = tmp_path / "stand-ins"
    stand_ins.mkdir()
    # llmlint: ignore[e2e_not_mocked] suppressions.toml has the reason.
    gh_stand_in(stand_ins)
    armings = stand_ins / "armed"
    answered = tmp_path / "release-pr.json"
    answered.write_text(result.stdout, encoding="utf-8")
    armed = capture(
        ["just", "release-pr-arm", str(answered)],
        copy.root,
        timeout=DRAFT_TIMEOUT_SECONDS,
        env=clean_environment(
            # llmlint: ignore[e2e_not_mocked] suppressions.toml has the reason.
            PATH=f"{stand_ins}{os.pathsep}{os.environ['PATH']}",
            UV_PROJECT_ENVIRONMENT=str(copy.shared_venv),
            GH_TOKEN=FORGE_CREDENTIAL,
            # llmlint: ignore[e2e_not_mocked] suppressions.toml has the reason.
            GH_STANDIN_RECORD=str(armings),
        ),
    )

    passing((armed.returncode, output(armed)), describing="`just release-pr-arm` over the answer")
    equal(
        [json.loads(line)["argv"] for line in armings.read_text(encoding="utf-8").splitlines()],
        [[*arming.ARM[1:], arming.SQUASH_BODY, *forge.opened]],
        describing="every call arming made to the forge's CLI",
    )
