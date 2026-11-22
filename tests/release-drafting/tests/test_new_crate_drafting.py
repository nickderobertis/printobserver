"""A crate added after a release is drafted, rather than wedging the release pull request.

`release-plz release-pr` computes each package's next version by comparing it
against what the registry serves, and it stops the whole draft on a package the
registry does not carry while the tag that package's own tag name renders
exists. Under one workspace-wide tag name, `v<version>`, every crate added after
a release found that release's tag and stopped it: `printobserver-panels`
landed on top of `v0.13.1` and no release pull request was drafted after.

This journey drives the real drafting computation — `release-plz update`, which
is what `release-pr` runs before it opens anything — under the committed
`release-plz.toml`, over a fixture workspace carrying one crate for every
package that configuration declares. Those crates are served at the released
version by a loopback registry, `v<version>` is tagged, and then a new crate
lands beside them and a fix lands in a released one. The draft has to reach the
new crate and draft it, and every changelog section it writes has to link the
tags a release here is actually cut under.

The workspace is a fixture rather than a copy of this tree because what is under
test is the release configuration and not any crate's code: the fixture reads
no crate of this workspace, so no change to one can make this run, and nothing
reaches crates.io.
"""

from __future__ import annotations

import hashlib
import json
import os
import re
import shutil
import threading
import tomllib
from collections.abc import Iterator
from http.server import BaseHTTPRequestHandler
from pathlib import Path

import pytest
from repo_checks.expect import absent, contains, equal, passing, truth
from repo_checks.loopback import LoopbackServer
from repo_checks.shell import run

#: The repository this journey reads its one input from: `release-plz.toml`.
REPO_ROOT = Path(__file__).resolve().parents[3]

#: The name the loopback registry is configured under in the fixture.
STANDIN = "standin"

#: The version the fixture's crates were released at, and its tag.
RELEASED = "0.1.0"

#: A crate the workspace gains after the release, as `printobserver-panels` was
#: gained on top of `v0.13.1`: in the one version group, and on no registry.
ADDED_AFTER = "printobserver-added-after-release"

#: The released crate a fix lands in after the release.
FIXED = "printobserver-types"

#: What the drafting tool says of a package the registry does not carry while
#: the tag that package's own tag name renders exists.
WEDGED = "not found in the registry, but the git tag"

#: A changelog section heading as release-plz writes one: `## [<version>](<link>)`.
SECTION_LINK = re.compile(r"^## \[(?P<version>[^\]]+)\]\((?P<link>[^)]+)\)", re.MULTILINE)

#: The one tag name a release of this workspace is cut under.
RELEASE_TAG = re.compile(r"v\d+\.\d+\.\d+")

#: A bound on a hang rather than a budget: drafting a dozen empty crates took
#: well under a minute where this was written.
DRAFT_TIMEOUT_SECONDS = 600

#: The identity the fixture's commits are made under.
COMMIT_IDENTITY = ("-c", "user.email=e2e@printobserver.test", "-c", "user.name=e2e")

pytestmark = pytest.mark.skipif(
    shutil.which("release-plz") is None,
    reason=(
        "release-plz is installed by `just install-tools release-plz`, which "
        "`just test-e2e` runs before this tier"
    ),
)


def committed_configuration() -> str:
    """The `release-plz.toml` this repository releases under, as committed."""
    return (REPO_ROOT / "release-plz.toml").read_text(encoding="utf-8")


def declared_packages(configuration: str) -> list[str]:
    """Every package the configuration declares a `[[package]]` table for, in its order."""
    return [str(package["name"]) for package in tomllib.loads(configuration)["package"]]


def repository_url() -> str:
    """The forge address the workspace declares, which the changelog's links are under."""
    with (REPO_ROOT / "Cargo.toml").open("rb") as handle:
        return str(tomllib.load(handle)["workspace"]["package"]["repository"]).removesuffix("/")


def sparse_path(name: str) -> str:
    """Where a sparse index keeps the entry for one crate name, by the index's own rule."""
    lowered = name.lower()
    match len(lowered):
        case 1:
            return f"1/{lowered}"
        case 2:
            return f"2/{lowered}"
        case 3:
            return f"3/{lowered[0]}/{lowered}"
        case _:
            return f"{lowered[:2]}/{lowered[2:4]}/{lowered}"


class Registry:
    """A sparse cargo registry on loopback serving the `.crate` files it was handed.

    The real read protocol: `config.json`, one index line per version at the
    sparse path whose checksum is the archive's SHA-256, and `/dl/` serving the
    archive back. A crate it was never handed answers `404`, as a registry that
    has never seen it does.
    """

    def __init__(self) -> None:
        """Start answering on a port the operating system chooses, carrying nothing."""
        self._crates: dict[tuple[str, str], bytes] = {}
        self._index: dict[str, list[dict[str, object]]] = {}
        self._server = LoopbackServer(self._handler())
        self._serving = threading.Thread(target=self._server.serve_forever, daemon=True)
        self._serving.start()

    @property
    def index(self) -> str:
        """The index URL cargo is pointed at, in the sparse protocol's spelling."""
        host, port = self._server.server_address[:2]
        return f"sparse+http://{host!s}:{port}/"

    def serve(self, name: str, version: str, crate: bytes) -> None:
        """Serve `crate` as `name` at `version`."""
        self._crates[(name, version)] = crate
        self._index.setdefault(name, []).append(
            {
                "name": name,
                "vers": version,
                "deps": [],
                "cksum": hashlib.sha256(crate).hexdigest(),
                "features": {},
                "yanked": False,
            }
        )

    def stop(self) -> None:
        """Release the port."""
        self._server.shutdown()
        self._server.server_close()

    def _handler(self) -> type[BaseHTTPRequestHandler]:
        registry = self

        class Handler(BaseHTTPRequestHandler):
            def do_GET(self) -> None:
                base = registry.index.removeprefix("sparse+").removesuffix("/")
                if self.path == "/config.json":
                    self._answer(200, json.dumps({"dl": f"{base}/dl/{{crate}}/{{version}}"}))
                    return
                if self.path.startswith("/dl/"):
                    name, _, version = self.path.removeprefix("/dl/").partition("/")
                    crate = registry._crates.get((name, version))
                    self._answer(404, "") if crate is None else self._answer(200, crate)
                    return
                name = self.path.rsplit("/", 1)[-1]
                lines = registry._index.get(name)
                if lines is None or self.path != f"/{sparse_path(name)}":
                    self._answer(404, "")
                    return
                self._answer(200, "".join(f"{json.dumps(line)}\n" for line in lines))

            def _answer(self, status: int, body: str | bytes) -> None:
                data = body.encode() if isinstance(body, str) else body
                self.send_response(status)
                self.send_header("Content-Length", str(len(data)))
                self.end_headers()
                self.wfile.write(data)

            def log_message(self, format: str, *args: object) -> None:
                """Say nothing per request; the journey reads what the tool said."""

        return Handler


@pytest.fixture
def registry() -> Iterator[Registry]:
    """A loopback registry, stopped whether the journey passed or raised."""
    serving = Registry()
    try:
        yield serving
    finally:
        serving.stop()


def environment(home: Path) -> dict[str, str]:
    """The caller's environment, with cargo's home and build directory the fixture's own.

    Every registry read lands in a home of the journey's own rather than the
    developer's, and a `CARGO_TARGET_DIR` the caller set cannot move the
    fixture's packaging out from under it.
    """
    variables = {
        name: value
        for name, value in os.environ.items()
        if name != "CARGO_TARGET_DIR" and not name.startswith("CARGO_REGISTRIES_")
    }
    if "RUSTUP_HOME" not in variables:
        variables["RUSTUP_HOME"] = str(Path.home() / ".rustup")
    variables["CARGO_HOME"] = str(home)
    return variables


def git(root: Path, *arguments: str) -> None:
    """Run one git command in the fixture, failing the journey naming it if it fails."""
    result = run(["git", *COMMIT_IDENTITY, *arguments], cwd=root)
    passing(result, describing=f"`git {' '.join(arguments)}` in the fixture")


def write_crate(root: Path, name: str) -> None:
    """One crate drawn the way every crate of this workspace is: on the workspace's fields."""
    crate = root / "crates" / name
    (crate / "src").mkdir(parents=True)
    (crate / "Cargo.toml").write_text(
        f'[package]\nname = "{name}"\ndescription = "A fixture crate."\n'
        "version.workspace = true\nedition.workspace = true\n"
        "license.workspace = true\nrepository.workspace = true\n",
        encoding="utf-8",
    )
    (crate / "src" / "lib.rs").write_text(f"//! The fixture's `{name}`.\n", encoding="utf-8")


def released_workspace(root: Path, packages: list[str], configuration: str) -> None:
    """The fixture at its release: one crate per declared package, committed and tagged.

    `semver_check` is turned off in the fixture's copy of the configuration: it
    builds the API of every changed crate twice, which is cargo-semver-checks'
    work and none of what is asked here. Nothing else of the committed
    configuration is changed.
    """
    root.mkdir()
    (root / "Cargo.toml").write_text(
        '[workspace]\nmembers = ["crates/*"]\nresolver = "2"\n\n'
        f'[workspace.package]\nversion = "{RELEASED}"\nedition = "2021"\n'
        f'license = "MIT"\nrepository = "{repository_url()}"\n',
        encoding="utf-8",
    )
    for name in packages:
        write_crate(root, name)
    (root / "release-plz.toml").write_text(
        configuration.replace("semver_check = true", "semver_check = false"), encoding="utf-8"
    )
    (root / ".gitignore").write_text("/target\n", encoding="utf-8")
    git(root, "init", "-q", "-b", "main")
    git(root, "add", "-A")
    git(root, "commit", "-qm", "feat: the release")
    git(root, "tag", f"v{RELEASED}")


def publish(root: Path, packages: list[str], registry: Registry, env: dict[str, str]) -> None:
    """Serve every released crate from the registry, packaged by the real `cargo package`."""
    target = root.parent / "packaged"
    result = run(
        [
            "cargo",
            "package",
            "--workspace",
            "--no-verify",
            "--offline",
            "--target-dir",
            str(target),
        ],
        cwd=root,
        env=env,
        timeout=DRAFT_TIMEOUT_SECONDS,
    )
    passing(result, describing="packaging the released fixture crates")
    for name in packages:
        archive = target / "package" / f"{name}-{RELEASED}.crate"
        registry.serve(name, RELEASED, archive.read_bytes())


def add_crate_after_the_release(root: Path) -> None:
    """Commit a new crate in the one version group, then a fix to a released crate."""
    write_crate(root, ADDED_AFTER)
    configuration = root / "release-plz.toml"
    configuration.write_text(
        configuration.read_text(encoding="utf-8")
        + f'\n[[package]]\nname = "{ADDED_AFTER}"\nversion_group = "printobserver"\n',
        encoding="utf-8",
    )
    git(root, "add", "-A")
    git(root, "commit", "-qm", "feat: add a crate after the release")
    (root / "crates" / FIXED / "touched.md").write_text("A change.\n", encoding="utf-8")
    git(root, "add", "-A")
    git(root, "commit", "-qm", "fix(types): a change the next release carries")


def drafted_links(root: Path) -> dict[str, tuple[str, str]]:
    """Each crate's drafted changelog section: the version it heads and the link it carries."""
    drafted: dict[str, tuple[str, str]] = {}
    for changelog in sorted(root.glob("crates/*/CHANGELOG.md")):
        found = SECTION_LINK.search(changelog.read_text(encoding="utf-8"))
        if found is not None:
            drafted[changelog.parent.name] = (found["version"], found["link"])
    return drafted


def refs_of(link: str, repository: str) -> list[str]:
    """The refs a release link names: both ends of a comparison, or the one tag released."""
    for route in ("/compare/", "/releases/tag/"):
        prefix = f"{repository}{route}"
        if link.startswith(prefix):
            return link.removeprefix(prefix).split("...")
    msg = f"{link!r} is not a comparison or a release under {repository}"
    raise AssertionError(msg)


def test_a_crate_added_after_a_release_is_drafted_rather_than_wedging_the_draft(
    tmp_path: Path, registry: Registry
) -> None:
    """The state `main` was in after `printobserver-panels` landed on top of `v0.13.1`."""
    configuration = committed_configuration()
    packages = declared_packages(configuration)
    contains(packages, FIXED, describing="the packages `release-plz.toml` declares")
    root = tmp_path / "workspace"
    env = environment(tmp_path / "cargo-home")
    released_workspace(root, packages, configuration)
    (root / ".cargo").mkdir()
    (root / ".cargo" / "config.toml").write_text(
        f'[registries.{STANDIN}]\nindex = "{registry.index}"\n', encoding="utf-8"
    )
    publish(root, packages, registry, env)
    add_crate_after_the_release(root)
    repository = repository_url()

    result = run(
        ["release-plz", "update", "--registry", STANDIN, "--repo-url", repository],
        cwd=root,
        env=env,
        timeout=DRAFT_TIMEOUT_SECONDS,
    )
    said = result.stdout + result.stderr

    passing(result, describing="drafting over a crate added after the release")
    absent(said, WEDGED, describing="what the drafting tool said")
    contains(said, f"{ADDED_AFTER}: next version is", describing="what the tool drafted")
    contains(said, f"{FIXED}: next version is", describing="what the tool drafted")

    drafted = drafted_links(root)
    contains(list(drafted), ADDED_AFTER, describing="the crates given a changelog section")
    contains(list(drafted), FIXED, describing="the crates given a changelog section")
    tags = set(run(["git", "tag", "--list"], cwd=root).stdout.split())
    for crate, (drafted_version, link) in drafted.items():
        for ref in refs_of(link, repository):
            truth(
                RELEASE_TAG.fullmatch(ref) is not None,
                describing=f"`{crate}`'s section to link `v<version>` tags, not {ref!r}: {link}",
            )
            truth(
                ref in tags or ref == f"v{drafted_version}",
                describing=f"`{crate}`'s section to link a tag the fixture carries or the one "
                f"drafted, not {ref!r}: {link} (the fixture carries {sorted(tags)})",
            )
    drafted_version, link = drafted[FIXED]
    equal(
        link,
        f"{repository}/compare/v{RELEASED}...v{drafted_version}",
        describing="the released crate's section link",
    )
