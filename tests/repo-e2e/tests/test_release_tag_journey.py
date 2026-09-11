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
import re
import secrets
import shutil
import struct
import threading
import urllib.error
import urllib.request
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from pathlib import Path
from typing import NamedTuple, NewType, Protocol, Self

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

#: The repository the program is told it is releasing. It derives every API
#: route from these, so the forge stand-in answers under this path and no other.
OWNER, NAME = "nickderobertis", "printobserver"

#: What GitHub answered the second `POST …/git/refs` for `refs/tags/v0.2.0`,
#: recorded verbatim from the `release` job's log of workflow run 34578287017
#: on 2026-09-11 — the line release-plz prints as `Response body: …` when a
#: forge refuses it (`gh run view 34578287017 --log`). The forge stand-in
#: answers this recording rather than a restatement of it, and the journey
#: over the former configuration holds the program to reporting it exactly as
#: that run did. It has no drift gate against a live GitHub, because
#: reconciling it means creating one ref twice on the real repository.
# llmlint: ignore[contracts_have_one_source_or_a_drift_gate] suppressions.toml has the reason.
REFERENCE_EXISTS = (
    '{"message":"Reference already exists","documentation_url":'
    '"https://docs.github.com/rest/git/refs#create-a-reference","status":"422"}'
)

#: What the program says of a package the registry already carries.
ALREADY_PUBLISHED = "{crate} {version}: already published"

#: What the program says when the forge refuses the ref, naming it.
FAILED_REF = "failed to create ref refs/tags/v"

#: The edits that put the former configuration back — creation on for every
#: package — which is the one the `0.2.0` release died under.
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

#: What a crate name looks like where cargo writes it into a sparse-index
#: path: lowercased, and nothing outside letters, digits, `-` and `_`. A path
#: whose last segment is not one of these names nothing in any registry.
CRATE_NAME = re.compile(r"[a-z0-9_-]{1,64}")

#: What a stand-in authenticates its writer by: minted per stand-in, never
#: written in a source file, and a type of its own so a path or a body cannot
#: be handed where one is expected.
Credential = NewType("Credential", str)


def minted() -> Credential:
    """A fresh credential for one stand-in's lifetime."""
    return Credential(secrets.token_hex(16))


DEPENDENCY_KINDS = ("normal", "build", "dev")

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


class CopiesTheTree(Protocol):
    """What the `gate_copy` fixture is: a factory for copies of the committed tree.

    `node_modules=False` is the copy a publication is cut from, and the one
    the release program is driven over here — it copies the whole tree aside
    to diff it, and refuses a symbolic link out of the tree.
    """

    def __call__(self, *, node_modules: bool = True) -> GateCopy:
        """Make one more copy, with or without the JavaScript dependencies linked in."""
        ...


def sparse_path(name: str) -> str:
    """Where a sparse index keeps `name`'s entry: cargo's own layout, by name length."""
    match len(name):
        case 1:
            return f"/1/{name}"
        case 2:
            return f"/2/{name}"
        case 3:
            return f"/3/{name[0]}/{name}"
        case _:
            return f"/{name[:2]}/{name[2:4]}/{name}"


def crate_named_by(path: str) -> str | None:
    """The crate whose sparse-index entry `path` is, or none if it is not such a path.

    A well-formed path is exactly `sparse_path` of a well-formed name — the
    prefix segments are derived from the name, so a path with the right shape
    and the wrong prefix names nothing either.
    """
    name = path.rsplit("/", 1)[-1]
    if CRATE_NAME.fullmatch(name) and sparse_path(name) == path:
        return name
    return None


def framed(metadata: object, crate: bytes) -> bytes:
    """The body `cargo publish` sends: two length-prefixed parts, metadata then crate."""
    document = json.dumps(metadata).encode()
    return struct.pack("<I", len(document)) + document + struct.pack("<I", len(crate)) + crate


def opened(
    url: str,
    *,
    data: bytes | None = None,
    method: str = "GET",
    authorization: str = "",
    timeout: int = 60,
) -> tuple[int, bytes]:
    """One HTTP exchange, as a status and a body, whatever the status was.

    The one place this module opens a URL: the stand-in registry forwarding a
    read to crates.io, and the journeys that drive the two stand-ins directly
    — sending `authorization` as the header's whole value where they give one,
    because the two speak different schemes: GitHub takes `Bearer <token>`
    and a cargo registry takes the bare token.
    """
    headers = {"User-Agent": "printobserver-repo-e2e (stand-in registry)"}
    if authorization:
        headers["Authorization"] = authorization
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
            # `publish` is null for a crate that may go anywhere, and a list
            # of the registries it may go to — empty for `publish = false`.
            and (
                package.get("publish") is None
                or (
                    isinstance(package["publish"], list)
                    and all(isinstance(registry, str) for registry in package["publish"])
                )
            )
            for package in listed
        ),
        describing="`cargo metadata` to answer named packages, each publishable, with kinded deps",
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
                if dependency.get("kind") != "dev"
            )
        )
        truth(ready, describing=f"a crate whose dependencies are all ordered: {ordered}")
        ordered.extend(ready)
    return tuple(ordered)


class Request(NamedTuple):
    """One request, tagged with the stand-in that took it.

    The tag is what lets a registry upload and a forge write be ordered against
    each other: both kinds sit in one sequence, and `who` tells them apart.
    """

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
        """Own the lock the two stand-ins' handler threads append under.

        Each stand-in serves on threads of its own, so without one lock the
        order this record ends up in would be the order two threads happened
        to interleave their appends, not the order the requests arrived.
        """
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
        """Bind a free loopback port and serve on a daemon thread from here on.

        Serving starts here rather than in `__enter__`, because a caller needs
        the port — to write it into the copy's cargo configuration — before the
        `with` block. The thread is a daemon so a journey that fails before
        `stop` cannot hang the process on it.
        """
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
        """Shut the serving thread down and release the port.

        Once, from `__exit__`: `shutdown` blocks until the serving loop has
        returned, and a second call would block forever on a loop that is not
        running.
        """
        self._server.shutdown()
        self._server.server_close()

    def __enter__(self) -> Self:
        """Hand the already-serving stand-in to the block; `__init__` started it."""
        return self

    def __exit__(self, *_: object) -> None:
        """Release the port whether the block passed or raised, leaving no listener behind."""
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
        """Send `document` as JSON, typed as such: cargo and release-plz both parse only that."""
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


def _typed[T](mapping: dict[str, object], field: str, kind: type[T]) -> T:
    """`mapping[field]`, once it is present and a `kind`.

    Raises:
        ValueError: Naming the field, if it is absent or of another type.
    """
    value = mapping.get(field)
    if not isinstance(value, kind):
        msg = f"`{field}` is missing or not {kind.__name__}"
        raise ValueError(msg)
    return value


def _nullable(mapping: dict[str, object], field: str) -> str | None:
    """`mapping[field]`, a string or null — and absent is null, which is how cargo sends one.

    A publish carries `explicit_name_in_toml`, `target`, `registry` and
    `links` only when they are set, so a missing one is the null case rather
    than a malformed body; anything present that is not a string is.
    """
    value = mapping.get(field)
    if not (value is None or isinstance(value, str)):
        msg = f"`{field}` is not a string or null"
        raise ValueError(msg)
    return value


def _strings(mapping: dict[str, object], field: str) -> list[str]:
    """`mapping[field]`, once it is a list of strings."""
    listed = _typed(mapping, field, list)
    if not all(isinstance(item, str) for item in listed):
        msg = f"`{field}` is not a list of strings"
        raise ValueError(msg)
    return [str(item) for item in listed]


class Published(NamedTuple):
    """What one publish's metadata says, as the index line will carry it."""

    name: str
    version: str
    dependencies: list[dict[str, object]]
    features: dict[str, list[str]]
    links: str | None


def _published(metadata: object) -> Published:
    """One publish's metadata, every field that reaches the index checked first.

    Each is held to the type cargo's publish metadata gives it, and every
    crate name to `CRATE_NAME`, before any of it is written where cargo will
    read it back.

    Raises:
        ValueError: Naming the first field that is not what cargo sends.
    """
    if not isinstance(metadata, dict):
        msg = "metadata is not a JSON object"
        raise ValueError(msg)
    name = _typed(metadata, "name", str)
    version = _typed(metadata, "vers", str)
    if not CRATE_NAME.fullmatch(name) or not version:
        msg = f"`{name}` at `{version}` is not a crate name and a version"
        raise ValueError(msg)
    dependencies: list[dict[str, object]] = []
    for listed in _typed(metadata, "deps", list):
        if not isinstance(listed, dict):
            msg = "a dependency is not a JSON object"
            raise ValueError(msg)
        dependency_name = _typed(listed, "name", str)
        renamed = _nullable(listed, "explicit_name_in_toml")
        kind = _typed(listed, "kind", str)
        if not CRATE_NAME.fullmatch(dependency_name) or kind not in DEPENDENCY_KINDS:
            msg = f"dependency `{dependency_name}` of kind `{kind}` is not one cargo would send"
            raise ValueError(msg)
        dependencies.append(
            {
                "name": renamed or dependency_name,
                "req": _typed(listed, "version_req", str),
                "features": _strings(listed, "features"),
                "optional": _typed(listed, "optional", bool),
                "default_features": _typed(listed, "default_features", bool),
                "target": _nullable(listed, "target"),
                "kind": kind,
                "registry": _nullable(listed, "registry"),
                "package": dependency_name if renamed else None,
            }
        )
    features: dict[str, list[str]] = {}
    for feature, enables in _typed(metadata, "features", dict).items():
        if not isinstance(feature, str) or not isinstance(enables, list):
            msg = "`features` is not a map of feature to the features it enables"
            raise ValueError(msg)
        features[feature] = _strings({"enables": enables}, "enables")
    return Published(name, version, dependencies, features, _nullable(metadata, "links"))


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
        """Own `owned` — every crate the workspace publishes — and carry none of them yet.

        Owning a name means never forwarding it: a read for an owned crate
        this registry has not taken answers `404`, as a registry that has
        never seen it would, rather than reaching crates.io for it.
        """
        self.owned = set(owned)
        #: The one credential this registry takes an upload under.
        self.credential = minted()
        self.uploads: list[tuple[str, str]] = []
        self.forwarded: list[str] = []
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

    def environment(self) -> dict[str, str]:
        """The credential, in the one variable `cargo publish --registry` reads it from.

        The release program is handed the same value on `--token`, so the two
        paths an upload can take here authenticate as one client.
        """
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
        published = _published(metadata)
        name, version = published.name, published.version
        self._crates[(name, version)] = crate
        self._index.setdefault(name, []).append(
            {
                "name": name,
                "vers": version,
                "deps": published.dependencies,
                "cksum": hashlib.sha256(crate).hexdigest(),
                "features": published.features,
                "yanked": False,
                "links": published.links,
                "v": 2,
            }
        )
        self.uploads.append((name, version))
        return name, version

    def _handler(self) -> type[BaseHTTPRequestHandler]:
        registry = self

        class Handler(_Handler):
            """The sparse index, the download path, and the publish endpoint.

            A read that is not `config.json`, a download of one named crate at
            one version, or a well-formed sparse-index path is `404` here and
            reaches crates.io no more than a read for an owned crate does.
            """

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
                    if len(parts) != 2 or not CRATE_NAME.fullmatch(parts[0]) or not parts[1]:
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
                name = crate_named_by(self.path)
                if name is None:
                    self.answer(404)
                    return
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
                registry.forwarded.append(self.path)
                self.answer(*opened(f"{CRATES_IO_INDEX}{self.path}"))

        return Handler


# llmlint: ignore[e2e_not_mocked] suppressions.toml has the reason.
# llmlint: ignore[contracts_have_one_source_or_a_drift_gate] suppressions.toml has the reason.
class StandInForge(_StandIn):
    """GitHub's git-data and release endpoints, refusing a ref it already holds.

    What `release-plz release` touches: the pull requests at a commit, a tag
    object, a ref, and a release. Each write is recorded in the shared record,
    and a second `POST …/git/refs` for a ref this forge already holds is
    answered `422` with `REFERENCE_EXISTS`, the body GitHub answered the
    release of `0.2.0` with.
    """

    def __init__(self, record: Record) -> None:
        """Start as the forge was before the release: no tag, no ref, no release.

        Nothing here is ever deleted: a ref once taken is held for the life of
        the stand-in, which is what makes the second request for it refusable.
        """
        self.refs: list[str] = []
        self.releases: list[dict[str, object]] = []
        self.tags: list[dict[str, object]] = []
        #: What the program authenticates to this forge with, required on
        #: every write as GitHub requires it.
        self.credential = minted()
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
                """A tag object, a ref, or a release — each only under the forge's credential."""
                forge.record.note(forge.who, "POST", self.path)
                payload = self.body()
                if payload is None:
                    return
                if self.headers.get("Authorization", "").split()[-1:] != [forge.credential]:
                    self.answer_json(401, {"message": "Bad credentials"})
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
                            self.answer(
                                422, REFERENCE_EXISTS.encode(), Content_Type="application/json"
                            )
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

    def __init__(self, gate_copy: CopiesTheTree, tags: tuple[str, ...] = ()) -> None:
        """Stand both stand-ins up, then copy the tree carrying `tags` and wire it to them.

        The stand-ins come first because the copy's `.cargo/config.toml`
        names the registry's port, and that port exists only once it is bound.
        The copy is pytest's to remove; the listeners are this stage's.
        """
        self.record = Record()
        self.registry = StandInRegistry(self.record, publishable_crates())
        self.forge = StandInForge(self.record)
        self.copy = tagged(gate_copy, tags)
        self.copy.write(".cargo/config.toml", self.registry.cargo_config())

    def __enter__(self) -> Self:
        """Hand the wired stage to the block; both stand-ins are already serving."""
        return self

    def __exit__(self, *_: object) -> None:
        """Release both ports whether the block passed or raised, the registry's first."""
        self.registry.stop()
        self.forge.stop()

    def environment(self) -> dict[str, str]:
        """The caller's environment, cleaned, plus the credential the registry uploads need."""
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
                self.forge.credential,
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
    gate_copy: CopiesTheTree, tmp_path: Path
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
    gate_copy: CopiesTheTree, tmp_path: Path
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
    gate_copy: CopiesTheTree, tmp_path: Path
) -> None:
    """The former configuration, put back: the release stops on the second package's tag.

    The same seeded registry and clean forge, over a copy whose release
    configuration lets every package create the tag. The program dies naming
    the ref it could not create and reporting the forge's body as the real
    run's log did, leaves at least one publishable crate unuploaded, and
    writes no answer — which the recipe refuses rather than reading as
    "released nothing".
    """
    version = workspace_version()
    with Stage(gate_copy) as stage:
        for old, new in CREATION_EVERYWHERE:
            stage.copy.edit("release-plz.toml", old, new)
        stage.seed(*PUBLISHED_FIRST)
        seeded = len(stage.registry.uploads)

        code, answer, said = stage.released()

        failing((code, said), naming=f"{FAILED_REF}{version}")
        contains(said, f"Response body: {REFERENCE_EXISTS}", describing="what the program said")
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
    means nothing without it — and a write carrying no credential takes
    nothing, so the case above cannot have tagged under one the program did
    not send.
    """
    record = Record()
    with StandInForge(record) as forge:
        url = f"{forge.base}/api/v3/repos/{OWNER}/{NAME}/git/refs"
        body = json.dumps({"ref": "refs/tags/v9.9.9", "sha": "0" * 40}).encode()
        equal(
            opened(url, data=body, method="POST")[0], 401, describing="a write with no credential"
        )
        equal(forge.refs, [], describing="the refs the forge holds after a refused write")

        def posted() -> tuple[int, bytes]:
            return opened(url, data=body, method="POST", authorization=f"Bearer {forge.credential}")

        equal(posted()[0], 201, describing="the first ref")
        status, refusal = posted()
        equal(status, 422, describing="the second ref")
        equal(refusal.decode(), REFERENCE_EXISTS, describing="the refusal's body")
        equal(forge.refs, ["refs/tags/v9.9.9"], describing="the refs the forge holds")
    equal(
        record.of("forge", "POST"),
        [(position, f"/api/v3/repos/{OWNER}/{NAME}/git/refs") for position in range(3)],
        describing="what the record holds",
    )


def test_the_registry_takes_only_a_publish_and_forwards_only_an_index_path() -> None:
    """What the registry stand-in refuses, and what it does not send to crates.io.

    Held here because the cases above read what this stand-in recorded: an
    upload it took while refusing nothing, or a read it forwarded for a path
    that names no crate, would make those records worth less than they claim.
    """
    record = Record()
    with StandInRegistry(record, ("printobserver-types",)) as registry:
        publish = f"{registry.base}/api/v1/crates/new"
        crate = b"not a real crate archive, and nothing here opens it"
        dependency = {
            "name": "serde",
            "version_req": "^1",
            "features": [],
            "optional": False,
            "default_features": True,
            "target": None,
            "kind": "normal",
            "registry": None,
            "explicit_name_in_toml": None,
        }
        metadata = {
            "name": "printobserver-types",
            "vers": "0.0.0-standin",
            "deps": [dependency],
            "features": {"default": []},
            "links": None,
        }

        def put(body: bytes, *, authorization: str = registry.credential) -> tuple[int, str]:
            status, said = opened(publish, data=body, method="PUT", authorization=authorization)
            return status, said.decode()

        for authorization in ("", f"Bearer {registry.credential}"):
            status, said = put(framed(metadata, crate), authorization=authorization)
            equal(status, 403, describing=f"a publish under {authorization!r}")
            contains(said, "must be logged in", describing="the refusal")
        malformed = (
            (framed(metadata, crate)[:-8], "crate length does not fill the body"),
            (b"\x00" * 3, "no metadata length"),
            (framed(["not", "an", "object"], crate), "metadata is not a JSON object"),
            (framed({**metadata, "name": "Print Observer"}, crate), "not a crate name"),
            (
                framed({**metadata, "deps": [{**dependency, "optional": "yes"}]}, crate),
                "`optional`",
            ),
            (framed({**metadata, "deps": [{**dependency, "kind": "runtime"}]}, crate), "`runtime`"),
            (framed({**metadata, "deps": [{**dependency, "target": 7}]}, crate), "`target`"),
            (framed({**metadata, "features": {"default": "serde"}}, crate), "`features`"),
            (framed({**metadata, "links": ["z"]}, crate), "`links`"),
        )
        for body, reason in malformed:
            status, said = put(body)
            equal(status, 400, describing=f"a publish refused for {reason!r}")
            contains(said, reason, describing="what the refusal named")
        equal(registry.uploads, [], describing="what was taken while everything was refused")
        truth(
            not registry.carries("printobserver-types", "0.0.0-standin"),
            describing="the refused crate to be served by nothing",
        )

        equal(put(framed(metadata, crate))[0], 200, describing="the well-formed publish")
        equal(registry.uploads, [("printobserver-types", "0.0.0-standin")], describing="uploads")
        status, line = opened(f"{registry.base}{sparse_path('printobserver-types')}")
        equal(status, 200, describing="the index entry")
        entry = json.loads(line)
        equal(entry["cksum"], hashlib.sha256(crate).hexdigest(), describing="the checksum")
        equal(entry["deps"][0]["req"], "^1", describing="the requirement carried over")
        status, served = opened(f"{registry.base}/dl/printobserver-types/0.0.0-standin")
        equal((status, served), (200, crate), describing="the download")

        # Not forwarded: a path that is not a sparse-index path, an owned crate
        # nothing has taken, and a download that names no crate at one version.
        for path in (
            "/etc/passwd",
            "/api/v1/crates/new",
            "/pr/in/printobserver-types/extra",
            "/xx/yy/printobserver-types",
            "/pr/in/Printobserver-Types",
            "/dl/printobserver-types",
            "/dl/../serde/1.0.0",
        ):
            equal(opened(f"{registry.base}{path}")[0], 404, describing=f"a read of {path}")
        equal(registry.forwarded, [], describing="what reached crates.io")
        truth(
            all(path == "/api/v1/crates/new" for _, path in record.of("registry", "PUT")),
            describing="every PUT to have been recorded at the publish endpoint",
        )
