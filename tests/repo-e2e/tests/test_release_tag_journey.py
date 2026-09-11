"""One tag and one GitHub Release per workspace version, created after the last publish.

`release-plz release` tags and releases PER PACKAGE, in release order, and this
workspace names every package's tag `v<version>`: the second package to publish
under that name dies creating a ref the first already created, which is what
stopped the `0.2.0` release on 2026-09-11 after `printobserver-sdk` and
`printobserver-types`, with eleven crates unpublished. `release-plz.toml` now
turns tag and release creation off for the workspace and back on for the one
package release ordering publishes last, `printobserver`, and these journeys
hold it there.

The program is driven for real — the installed `release-plz release`, with the
argument list the committed step carries — over a copy of the tree, against
two stand-ins on loopback: a sparse cargo registry that takes real `cargo
publish` uploads and proxies every crate this workspace does not own to
crates.io, and a forge that answers GitHub's git-data and release endpoints
and refuses a ref it already holds, as GitHub does. Both write to one ordered
record, which is what the ordering across them is asserted from.

`--no-verify` is the one argument added beyond routing: without it every
`cargo publish` builds the crate in release mode, which is cargo's own
verification of the packaged source and not what is under test here. Nothing
below reaches a real registry's write path or a real forge; the one real
network is the public crates.io index and the third-party crates it serves,
which the workspace needs in order to resolve.
"""

from __future__ import annotations

import hashlib
import json
import secrets
import shutil
import struct
import threading
import urllib.error
import urllib.request
from collections.abc import Callable
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from pathlib import Path
from typing import NamedTuple, Self

import pytest
from journey import REPO_ROOT, GateCopy, capture, clean_environment, output
from release_artifacts.registries import released_by
from repo_checks.expect import absent, contains, equal, failing, passing, truth
from repo_checks.shell import run as shell_run
from test_release_path_journey import STANDIN, tagged, workspace_version
from test_release_program import release_step_arguments

#: The two crates the real `0.2.0` run published before it died, and so the
#: two the registry carries over the state `main` is in. Neither depends on
#: the other, so they seed in either order.
PUBLISHED_FIRST = ("printobserver-types", "printobserver-sdk")

#: Where the forge stand-in is this repository.
OWNER, NAME = "nickderobertis", "printobserver"

#: What GitHub answers a `POST …/git/refs` naming a ref it already holds.
REFERENCE_EXISTS = "Reference already exists"

#: What the program says of a package the registry already carries.
ALREADY_PUBLISHED = "{crate} {version}: already published"

#: What the program says when the forge refuses the ref, naming it.
FAILED_REF = "failed to create ref refs/tags/v"

#: The edits that put today's configuration back: creation on for every
#: package, exactly as `main` carries it.
CREATION_EVERYWHERE = (
    ("git_tag_enable = false\ngit_release_enable = false\n", ""),
    (
        '[[package]]\nname = "printobserver"\ngit_tag_enable = true\ngit_release_enable = true\n',
        "",
    ),
)

#: The real crates.io, which is where the stand-in sends every read for a crate
#: this workspace does not publish.
CRATES_IO_INDEX = "https://index.crates.io"
CRATES_IO_DOWNLOAD = "https://static.crates.io/crates/{name}/{version}/download"

#: The most a stand-in reads of one request body. A `.crate` of this workspace
#: is kilobytes and a forge write is one small JSON document; anything larger
#: is not a request either protocol makes.
BODY_LIMIT = 64 * 1024 * 1024

#: A crate publish over this workspace resolves the whole lockfile through the
#: stand-in the first time; a release of eleven crates was measured at under a
#: minute. Bounds on a hang, not budgets.
PUBLISH_TIMEOUT_SECONDS = 300
RELEASE_TIMEOUT_SECONDS = 900

pytestmark = pytest.mark.skipif(
    shutil.which("release-plz") is None,
    reason="release-plz is installed by `just install-tools`; run bootstrap first",
)


def opened(
    url: str, *, data: bytes | None = None, method: str = "GET", timeout: int = 60
) -> tuple[int, bytes]:
    """One HTTP exchange, as a status and a body, whatever the status was.

    The one place this module opens a URL: the stand-in registry forwarding a
    read to crates.io, and the journey that drives the stand-in forge directly.
    """
    headers = {"User-Agent": "printobserver-repo-e2e (stand-in registry)"}
    # llmlint: ignore[async_typed_clients_at_boundaries] suppressions.toml has the reason.
    request = urllib.request.Request(url, data=data, method=method, headers=headers)  # noqa: S310
    try:
        # llmlint: ignore[async_typed_clients_at_boundaries] The same one site as above.
        with urllib.request.urlopen(request, timeout=timeout) as answer:  # noqa: S310
            return answer.status, answer.read()
    except urllib.error.HTTPError as refused:
        return refused.code, refused.read()


def publishable_crates() -> tuple[str, ...]:
    """Every crate the workspace publishes, dependencies before dependents.

    That is the order a registry takes them in — crates.io and the stand-in
    alike refuse to package a crate whose sibling dependency it does not yet
    carry — and the order the release program publishes them in. Ties are
    broken by name, so the order is one thing on every run.
    """
    result = shell_run(
        ["cargo", "metadata", "--no-deps", "--locked", "--format-version", "1"],
        cwd=REPO_ROOT,
        check=True,
    )
    metadata = json.loads(result.stdout)
    listed = metadata.get("packages") if isinstance(metadata, dict) else None
    truth(
        isinstance(listed, list)
        and all(
            isinstance(package, dict)
            and isinstance(package.get("name"), str)
            and isinstance(package.get("dependencies"), list)
            and all(
                isinstance(dependency, dict)
                and isinstance(dependency.get("name"), str)
                and (dependency.get("kind") is None or isinstance(dependency["kind"], str))
                for dependency in package["dependencies"]
            )
            for package in listed
        ),
        describing="`cargo metadata` to answer named packages with named, kinded dependencies",
    )
    packages = {
        str(package["name"]): package for package in listed or [] if package.get("publish") != []
    }
    ordered: list[str] = []
    while len(ordered) < len(packages):
        ready = sorted(
            name
            for name, package in packages.items()
            if name not in ordered
            and all(
                dependency["name"] not in packages or dependency["name"] in ordered
                for dependency in package["dependencies"]
                if dependency["kind"] != "dev"
            )
        )
        truth(ready, describing=f"a crate whose dependencies are all ordered: {ordered}")
        ordered.extend(ready)
    return tuple(ordered)


class Request(NamedTuple):
    """One request a stand-in took, and which stand-in it was."""

    who: str
    method: str
    path: str


class Taken(NamedTuple):
    """One recorded request and its position in the shared record."""

    position: int
    path: str


class Record:
    """One ordered record shared by both stand-ins.

    Ordering across the registry and the forge is asserted from positions in
    this one sequence, rather than from two records and their timestamps.
    """

    def __init__(self) -> None:
        """Start with no request taken."""
        self.entries: list[Request] = []
        self._lock = threading.Lock()

    def note(self, who: str, method: str, path: str) -> None:
        """Take one request, under the lock both stand-ins' threads share."""
        with self._lock:
            self.entries.append(Request(who, method, path))

    def of(self, who: str, method: str) -> list[Taken]:
        """Each request `who` took by `method`, with its position in the record."""
        return [
            Taken(position, request.path)
            for position, request in enumerate(self.entries)
            if request.who == who and request.method == method
        ]


class _StandIn:
    """A loopback HTTP server whose handler is one of the two below."""

    def __init__(self, record: Record, who: str) -> None:
        """Start answering on a port the operating system chooses."""
        self.record = record
        self.who = who
        self._server = ThreadingHTTPServer(("127.0.0.1", 0), self._handler())
        self._serving = threading.Thread(target=self._server.serve_forever, daemon=True)
        self._serving.start()

    @property
    def base(self) -> str:
        """Where this stand-in answers, with no trailing slash."""
        host, port = self._server.server_address[:2]
        return f"http://{host}:{port}"

    def _handler(self) -> type[BaseHTTPRequestHandler]:
        raise NotImplementedError

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


class _Handler(BaseHTTPRequestHandler):
    """The two things every answer below needs: a body, and a recorded request."""

    protocol_version = "HTTP/1.1"

    def answer(self, status: int, body: bytes = b"", **headers: str) -> None:
        """Send one answer, `Content-Length` and all."""
        self.send_response(status)
        for name, value in headers.items():
            self.send_header(name.replace("_", "-"), value)
        self.send_header("Content-Length", str(len(body)))
        self.end_headers()
        self.wfile.write(body)

    def answer_json(self, status: int, document: object) -> None:
        """Send one JSON answer."""
        self.answer(status, json.dumps(document).encode(), Content_Type="application/json")

    def body(self) -> bytes | None:
        """The request's body, as long as its `Content-Length` says — or none, refused.

        A length that is not a number, is negative or exceeds `BODY_LIMIT` is
        answered `400` here, and the caller sends nothing more.
        """
        declared = self.headers.get("Content-Length", "0")
        length = int(declared) if declared.isdigit() else -1
        if not 0 <= length <= BODY_LIMIT:
            self.answer_json(400, {"errors": [{"detail": f"Content-Length {declared!r} refused"}]})
            return None
        return self.rfile.read(length)

    def log_message(self, format: str, *args: object) -> None:
        """Say nothing: a stand-in whose log is the output is not signal."""


# llmlint: ignore[e2e_not_mocked] suppressions.toml has the reason.
# llmlint: ignore[contracts_have_one_source_or_a_drift_gate] suppressions.toml has the reason.
class StandInRegistry(_StandIn):
    """A sparse cargo registry that owns this workspace's crates and proxies the rest.

    `cargo publish` uploads to it, `cargo info` reads its index and downloads
    from it, and both are the real protocol: a `PUT /api/v1/crates/new` whose
    body is the length-prefixed metadata and `.crate` bytes cargo sends, an
    index line at the sparse path whose checksum is those bytes' SHA-256, and
    `/dl/` serving them back. Every read for a crate outside `owned` — the
    third-party crates the workspace needs in order to resolve — is forwarded
    to crates.io, because cargo resolves this workspace's sibling dependencies
    from crates.io whatever `--registry` names, and a stand-in that owned the
    siblings but not the rest would resolve nothing.
    """

    def __init__(self, record: Record, owned: tuple[str, ...]) -> None:
        """Own `owned` — every crate the workspace publishes — and carry none of them yet."""
        self.owned = set(owned)
        self.uploads: list[tuple[str, str]] = []
        self._crates: dict[tuple[str, str], bytes] = {}
        self._index: dict[str, list[dict[str, object]]] = {}
        super().__init__(record, "registry")

    @property
    def index(self) -> str:
        """The index URL cargo is pointed at, in the sparse protocol's spelling."""
        return f"sparse+{self.base}/"

    def cargo_config(self) -> str:
        """The `.cargo/config.toml` that makes this the registry every crate resolves through.

        crates.io is replaced by the stand-in rather than the stand-in being a
        second registry beside it, because `[source]` cannot be set from the
        environment and no `printobserver-*` dependency carries a `registry`
        key: replacing the source is what makes the siblings resolve here.
        """
        return (
            f'[source.crates-io]\nreplace-with = "{STANDIN}"\n\n'
            f'[source.{STANDIN}]\nregistry = "{self.index}"\n\n'
            f'[registries.{STANDIN}]\nindex = "{self.index}"\n'
        )

    #: The one credential this registry takes an upload under.
    credential = "stand-in-credential"

    def environment(self) -> dict[str, str]:
        """The token cargo publishes to this registry under."""
        return {f"CARGO_REGISTRIES_{STANDIN.upper()}_TOKEN": self.credential}

    def carries(self, name: str, version: str) -> bool:
        """Whether `name` at `version` would be served: an upload of it was taken."""
        return (name, version) in self._crates

    def take(self, payload: bytes) -> tuple[str, str]:
        """Take one publish body, and index what it carried.

        Raises:
            ValueError: If the body is not the frame cargo sends — two
                length-prefixed parts that together fill it, the first a JSON
                object naming the crate, its version and its dependencies.
        """
        if len(payload) < 4:
            msg = "no metadata length"
            raise ValueError(msg)
        (metadata_length,) = struct.unpack("<I", payload[:4])
        crate_at = 8 + metadata_length
        if len(payload) < crate_at:
            msg = "metadata length exceeds the body"
            raise ValueError(msg)
        metadata = json.loads(payload[4 : 4 + metadata_length])
        (crate_length,) = struct.unpack("<I", payload[4 + metadata_length : crate_at])
        crate = payload[crate_at : crate_at + crate_length]
        if len(crate) != crate_length or crate_at + crate_length != len(payload):
            msg = "crate length does not fill the body"
            raise ValueError(msg)
        if (
            not isinstance(metadata, dict)
            or not isinstance(metadata.get("name"), str)
            or not isinstance(metadata.get("vers"), str)
            or not isinstance(metadata.get("deps"), list)
            or any(
                not isinstance(dependency, dict)
                or not isinstance(dependency.get("name"), str)
                or not isinstance(dependency.get("version_req"), str)
                for dependency in metadata["deps"]
            )
        ):
            msg = "metadata is not a crate with a name, a version and dependencies"
            raise ValueError(msg)
        name, version = str(metadata["name"]), str(metadata["vers"])
        self._crates[(name, version)] = crate
        self._index.setdefault(name, []).append(
            {
                "name": name,
                "vers": version,
                "deps": [
                    {
                        "name": dependency.get("explicit_name_in_toml") or dependency["name"],
                        "req": dependency["version_req"],
                        "features": dependency.get("features", []),
                        "optional": dependency.get("optional", False),
                        "default_features": dependency.get("default_features", True),
                        "target": dependency.get("target"),
                        "kind": dependency.get("kind", "normal"),
                        "registry": dependency.get("registry"),
                        "package": (
                            dependency["name"] if dependency.get("explicit_name_in_toml") else None
                        ),
                    }
                    for dependency in metadata["deps"]
                ],
                "cksum": hashlib.sha256(crate).hexdigest(),
                "features": metadata.get("features", {}),
                "yanked": False,
                "links": metadata.get("links"),
                "v": 2,
            }
        )
        self.uploads.append((name, version))
        return name, version

    def _handler(self) -> type[BaseHTTPRequestHandler]:
        registry = self

        class Handler(_Handler):
            """The sparse index, the download path, and the publish endpoint."""

            def do_GET(self) -> None:
                """Answer a read: owned from what was taken, the rest from crates.io."""
                registry.record.note(registry.who, "GET", self.path)
                if self.path == "/config.json":
                    self.answer_json(
                        200,
                        {"dl": f"{registry.base}/dl/{{crate}}/{{version}}", "api": registry.base},
                    )
                    return
                if self.path.startswith("/dl/"):
                    parts = self.path.removeprefix("/dl/").split("/")
                    if len(parts) != 2 or not all(parts):
                        self.answer(404)
                        return
                    name, version = parts
                    if name in registry.owned:
                        crate = registry._crates.get((name, version))
                        if crate is None:
                            self.answer(404)
                        else:
                            self.answer(200, crate, Content_Type="application/octet-stream")
                        return
                    self.answer(302, Location=CRATES_IO_DOWNLOAD.format(name=name, version=version))
                    return
                name = self.path.rsplit("/", 1)[-1]
                if name in registry.owned:
                    lines = registry._index.get(name)
                    if lines is None:
                        self.answer(404)
                    else:
                        self.answer(
                            200, "".join(f"{json.dumps(line)}\n" for line in lines).encode()
                        )
                    return
                self._forward()

            def do_PUT(self) -> None:
                """Take one publish, refusing one that carries no token as crates.io does."""
                registry.record.note(registry.who, "PUT", self.path)
                payload = self.body()
                if payload is None:
                    return
                if self.path != "/api/v1/crates/new":
                    self.answer(404)
                    return
                if self.headers.get("Authorization") != registry.credential:
                    self.answer_json(403, {"errors": [{"detail": "must be logged in"}]})
                    return
                try:
                    registry.take(payload)
                except (ValueError, json.JSONDecodeError, struct.error) as malformed:
                    self.answer_json(400, {"errors": [{"detail": f"not a publish: {malformed}"}]})
                    return
                self.answer_json(
                    200, {"warnings": {"invalid_categories": [], "invalid_badges": [], "other": []}}
                )

            def _forward(self) -> None:
                self.answer(*opened(f"{CRATES_IO_INDEX}{self.path}"))

        return Handler


# llmlint: ignore[e2e_not_mocked] suppressions.toml has the reason.
# llmlint: ignore[contracts_have_one_source_or_a_drift_gate] suppressions.toml has the reason.
class StandInForge(_StandIn):
    """GitHub's git-data and release endpoints, refusing a ref it already holds.

    What `release-plz release` touches: the pull requests at a commit, a tag
    object, a ref, and a release. Each write is recorded in the shared record,
    and a second `POST …/git/refs` for a ref this forge already holds is
    answered `422 Reference already exists`, which is GitHub's own answer and
    the one the release of `0.2.0` died on.
    """

    def __init__(self, record: Record) -> None:
        """Start as the forge was before the release: no tag, no ref, no release."""
        self.refs: list[str] = []
        self.releases: list[dict[str, object]] = []
        self.tags: list[dict[str, object]] = []
        super().__init__(record, "forge")

    @property
    def repo_url(self) -> str:
        """What the program is pointed at, from which it derives its API base."""
        return f"{self.base}/{OWNER}/{NAME}"

    def _handler(self) -> type[BaseHTTPRequestHandler]:
        forge = self
        prefix = f"/api/v3/repos/{OWNER}/{NAME}/"
        #: What GitHub requires of each write, as non-empty strings.
        required_of = {
            "git/tags": ("tag", "message", "object", "type"),
            "git/refs": ("ref", "sha"),
            "releases": ("tag_name",),
        }

        class Handler(_Handler):
            """The endpoints above, and `404` for anything else."""

            def do_GET(self) -> None:
                """A commit's pull requests: none."""
                forge.record.note(forge.who, "GET", self.path)
                if self.path.startswith(f"{prefix}commits/") and self.path.endswith("/pulls"):
                    self.answer_json(200, [])
                    return
                self.answer_json(404, {"message": "Not Found"})

            def do_POST(self) -> None:
                """A tag object, a ref, or a release."""
                forge.record.note(forge.who, "POST", self.path)
                payload = self.body()
                if payload is None:
                    return
                try:
                    document = json.loads(payload or b"{}")
                except json.JSONDecodeError:
                    document = None
                required = required_of.get(self.path.removeprefix(prefix), ())
                if not isinstance(document, dict) or any(
                    not isinstance(document.get(field), str) or not document[field]
                    for field in required
                ):
                    self.answer_json(422, {"message": f"Validation Failed: {required} required"})
                    return
                match self.path.removeprefix(prefix):
                    case "git/tags":
                        forge.tags.append(document)
                        self.answer_json(201, {**document, "sha": secrets.token_hex(20)})
                    case "git/refs":
                        ref = str(document["ref"])
                        if ref in forge.refs:
                            self.answer_json(422, {"message": REFERENCE_EXISTS})
                            return
                        forge.refs.append(ref)
                        self.answer_json(201, {"ref": ref, "object": {"sha": document["sha"]}})
                    case "releases":
                        forge.releases.append(document)
                        self.answer_json(
                            201,
                            {
                                **document,
                                "id": len(forge.releases),
                                "html_url": f"{forge.base}/{OWNER}/{NAME}/releases/tag/"
                                f"{document['tag_name']}",
                            },
                        )
                    case _:
                        self.answer_json(404, {"message": "Not Found"})

        return Handler


class Released(NamedTuple):
    """What one run of the release step came back with.

    `answer` is its standard output alone — the answer the workflow redirects
    into the file `just release-answer` reads — where `said` is both streams.
    """

    code: int
    answer: str
    said: str


class Stage:
    """A copy of the tree wired to a registry and a forge, sharing one record."""

    def __init__(self, gate_copy: Callable[..., GateCopy], tags: tuple[str, ...] = ()) -> None:
        """Copy the tree carrying `tags`, and stand both stand-ins up."""
        self.record = Record()
        self.registry = StandInRegistry(self.record, publishable_crates())
        self.forge = StandInForge(self.record)
        self.copy = tagged(gate_copy, tags)
        self.copy.write(".cargo/config.toml", self.registry.cargo_config())

    def __enter__(self) -> Self:
        """Serve for the duration of a `with` block."""
        return self

    def __exit__(self, *_: object) -> None:
        """Stop both stand-ins when the block ends."""
        self.registry.stop()
        self.forge.stop()

    def environment(self) -> dict[str, str]:
        """What every program run over the copy runs under."""
        return clean_environment(**self.registry.environment())

    def seed(self, *crates: str) -> None:
        """Put `crates` on the registry the way the real run did: a real `cargo publish` of each.

        In name order, which the stand-in — unlike crates.io — takes: what is
        being reproduced is the registry's state, not the order it reached it.
        """
        for crate in crates:
            result = capture(
                [
                    "cargo",
                    "publish",
                    "--registry",
                    STANDIN,
                    "--no-verify",
                    "--manifest-path",
                    f"crates/{crate}/Cargo.toml",
                ],
                self.copy.root,
                timeout=PUBLISH_TIMEOUT_SECONDS,
                env=self.environment(),
            )
            passing(result, describing=f"seeding `{crate}` onto the stand-in registry")
            truth(
                self.registry.carries(crate, workspace_version()),
                describing=f"the stand-in to carry `{crate}` after seeding it",
            )

    # llmlint: ignore[tests_mirror_real_usage] suppressions.toml has the reason.
    # llmlint: ignore[expensive_tests_stay_behind_their_own_edge] suppressions.toml has the reason.
    def released(self) -> Released:
        """Drive the committed release step over the copy, routed to the two stand-ins."""
        result = capture(
            [
                *release_step_arguments(),
                "--registry",
                STANDIN,
                "--token",
                self.registry.credential,
                "--repo-url",
                self.forge.repo_url,
                "--git-token",
                "stand-in-forge-token",
                "--no-verify",
            ],
            self.copy.root,
            timeout=RELEASE_TIMEOUT_SECONDS,
            env=self.environment(),
        )
        return Released(result.returncode, result.stdout, output(result))

    def answered(self, stdout: str, into: Path) -> tuple[int, str]:
        """What `just release-answer` says over the answer the program wrote.

        Written to a file first, because that is what the committed step hands
        the recipe: the program's standard output, redirected.
        """
        into.write_text(stdout, encoding="utf-8")
        result = capture(
            ["just", "release-answer", str(into)],
            self.copy.root,
            timeout=RELEASE_TIMEOUT_SECONDS,
            env=clean_environment(UV_PROJECT_ENVIRONMENT=str(self.copy.shared_venv)),
        )
        return result.returncode, output(result)


def test_a_partially_published_version_completes_under_one_tag_after_the_last_publish(
    gate_copy: Callable[..., GateCopy], tmp_path: Path
) -> None:
    """The state `main` is in: two crates on the registry, eleven not, no tag on the forge.

    The program skips the two, uploads each of the other eleven once, and the
    forge receives exactly one tag object, one ref and one release — all named
    `v<version>`, all after the last upload. Its answer names that tag for every
    package it released, and the recipe the publishing job reads it with folds
    those into `released=v<version>`.
    """
    version = workspace_version()
    crates = publishable_crates()
    with Stage(gate_copy) as stage:
        stage.seed(*PUBLISHED_FIRST)
        seeded = list(stage.registry.uploads)

        code, answer, said = stage.released()

        passing((code, said), describing="releasing the partially published version")
        uploads = stage.registry.uploads[len(seeded) :]
        for crate in PUBLISHED_FIRST:
            contains(
                said,
                ALREADY_PUBLISHED.format(crate=crate, version=version),
                describing="what the program said",
            )
        equal(
            sorted(uploads),
            sorted((crate, version) for crate in crates if crate not in PUBLISHED_FIRST),
            describing="the crates uploaded to the stand-in, each once",
        )
        equal(
            [tag["tag"] for tag in stage.forge.tags],
            [f"v{version}"],
            describing="the tag objects the forge received",
        )
        equal(stage.forge.refs, [f"refs/tags/v{version}"], describing="the refs the forge received")
        equal(
            [(release["tag_name"], release["name"]) for release in stage.forge.releases],
            [(f"v{version}", f"v{version}")],
            describing="the releases the forge received",
        )
        last_upload = max(position for position, _ in stage.record.of("registry", "PUT"))
        first_write = min(position for position, _ in stage.record.of("forge", "POST"))
        truth(
            first_write > last_upload,
            describing=f"every forge write to come after the last upload: {stage.record.entries}",
        )

        # `released_by` is the reader behind `just release-answer`: it refuses
        # an answer that is not the shape the program writes, and folds the
        # tags into the distinct ones.
        equal(released_by(answer), (f"v{version}",), describing="the tags the answer names")
        equal(
            sorted(str(entry["package_name"]) for entry in json.loads(answer)["releases"]),
            sorted(crate for crate in crates if crate not in PUBLISHED_FIRST),
            describing="the packages the program answered it released",
        )
        code, read = stage.answered(answer, tmp_path / "released.json")
        passing((code, read), describing="`just release-answer` over the program's answer")
        contains(read.splitlines(), f"released=v{version}", describing="what the recipe answered")


def test_a_fully_published_version_releases_nothing_and_writes_nothing(
    gate_copy: Callable[..., GateCopy], tmp_path: Path
) -> None:
    """After a completed release, as the workflow's checkout sees it: the tag present.

    Every crate on the registry and `v<version>` in the copy, the program
    uploads nothing, sends the forge no write, answers an empty release list,
    and the recipe reads that as `released=`.
    """
    version = workspace_version()
    with Stage(gate_copy, tags=(version,)) as stage:
        stage.seed(*publishable_crates())
        seeded = len(stage.registry.uploads)

        code, answer, said = stage.released()

        passing((code, said), describing="releasing an already released version")
        equal(stage.registry.uploads[seeded:], [], describing="what was uploaded")
        equal(stage.record.of("forge", "POST"), [], describing="what the forge was sent")
        equal(released_by(answer), (), describing="the tags the answer names")
        equal(json.loads(answer), {"releases": []}, describing="the program's answer")
        code, read = stage.answered(answer, tmp_path / "released.json")
        passing((code, read), describing="`just release-answer` over the program's answer")
        contains(read.splitlines(), "released=", describing="what the recipe answered")


def test_creation_enabled_for_every_package_dies_on_the_second_ref(
    gate_copy: Callable[..., GateCopy], tmp_path: Path
) -> None:
    """Today's configuration, put back: the release stops on the second package's tag.

    The same seeded registry and clean forge, over a copy whose release
    configuration lets every package create the tag. The program dies naming
    the ref it could not create, leaves at least one publishable crate
    unuploaded, and writes no answer — which the recipe refuses rather than
    reading as "released nothing".
    """
    version = workspace_version()
    with Stage(gate_copy) as stage:
        for old, new in CREATION_EVERYWHERE:
            stage.copy.edit("release-plz.toml", old, new)
        stage.seed(*PUBLISHED_FIRST)
        seeded = len(stage.registry.uploads)

        code, answer, said = stage.released()

        failing((code, said), naming=f"{FAILED_REF}{version}")
        uploaded = {name for name, _ in stage.registry.uploads[seeded:]}
        truth(
            any(
                crate not in uploaded
                for crate in publishable_crates()
                if crate not in PUBLISHED_FIRST
            ),
            describing=f"a publishable crate to have been left unpublished: {sorted(uploaded)}",
        )
        contains(
            stage.forge.refs, f"refs/tags/v{version}", describing="the ref the first package made"
        )
        equal(answer, "", describing="the answer the program wrote")
        code, read = stage.answered(answer, tmp_path / "released.json")
        failing((code, read), naming="not JSON")
        absent(read.splitlines(), "released=", describing="what the recipe answered")


def test_the_forge_refuses_a_ref_it_already_holds() -> None:
    """A second `POST …/git/refs` for one ref answers `422 Reference already exists`.

    This is the stand-in's own behaviour, held here because the case above
    means nothing without it.
    """
    record = Record()
    with StandInForge(record) as forge:
        url = f"{forge.base}/api/v3/repos/{OWNER}/{NAME}/git/refs"
        body = json.dumps({"ref": "refs/tags/v9.9.9", "sha": "0" * 40}).encode()
        equal(opened(url, data=body, method="POST")[0], 201, describing="the first ref")
        status, refusal = opened(url, data=body, method="POST")
        equal(status, 422, describing="the second ref")
        contains(refusal.decode(), REFERENCE_EXISTS, describing="the refusal's body")
        equal(forge.refs, ["refs/tags/v9.9.9"], describing="the refs the forge holds")
    equal(
        record.of("forge", "POST"),
        [
            (0, f"/api/v3/repos/{OWNER}/{NAME}/git/refs"),
            (1, f"/api/v3/repos/{OWNER}/{NAME}/git/refs"),
        ],
        describing="what the record holds",
    )
