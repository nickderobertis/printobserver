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

import json
import shutil
import threading
import tomllib
from collections.abc import Callable
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from typing import Self

import pytest
from journey import REPO_ROOT, GateCopy, capture, clean_environment, output, run
from repo_checks.expect import absent, contains, equal, failing, passing, truth
from repo_checks.shell import run as shell_run

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
    reason="release-plz is installed by `just install-tools`; run bootstrap first",
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
