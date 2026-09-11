"""A stand-in for every registry the three end-user routes are taken from.

Nothing in this repository may publish to a registry in order to prove a point,
and the proof of the three routes has to be falsifiable in both directions: a
registry serving nothing for the version under test, a registry serving
something that cannot be installed or cannot be run, and a registry serving
something that works. So this stands the registries up — one real HTTP server
speaking each of their own protocols, serving packages assembled by the same
writers the published artifacts are assembled by.

What stands in is the **registry**. The program each served package carries is a
small one of this module's own that says which version it is, because what a
proof driven against this is about is the resolution, the install and the run
rather than the program inside. The *shape* each registry serves is the
published one: route 2's launcher and the per-platform packages it resolves
through are served under two names, because the failure that shape has and no
other is reachable from a registry alone — a launcher published without the
package beside it installs clean, since an optional dependency nothing serves
is one `npm` skips, and leaves a program on the path that cannot run. Nothing
proving a local build ever resolves anything, so nothing there can see it.

One base address covers all three, because a proof that read one registry from a
stand-in and another from the real internet would be a proof of neither: the
paths below it are each registry's own.

**And each registry takes a write as the real one takes it**: the Python
registry's legacy multipart upload, the JavaScript registry's publish document
and the forge's asset upload and deletion, each on that registry's own path
under the same base. What a write lands is then served by the same documents a
read asks for — the JSON document, the packument, the release document — so a
publish driven against this is proven by the reads that decide what to publish
next time. A caller can make any one artifact refused, with a status and a body
of its own choosing, and every write is recorded with the credential it
carried, so that a journey can say which token reached which registry.
"""

from __future__ import annotations

import base64
import binascii
import hashlib
import json
import threading
from collections.abc import Mapping
from dataclasses import dataclass
from email.parser import BytesParser
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from pathlib import Path
from typing import NewType
from urllib.parse import parse_qs, unquote, urlsplit

from repo_checks.model import Repo

from release_artifacts import packages, platforms, targets
from release_artifacts.build import CHECKSUMS, LAUNCHER, PROGRAM

# The one version ordering. A stand-in that sorted versions its own way could
# serve a newest the proof selecting from it disagreed about.
from release_artifacts.publishing import CREDENTIALS
from release_artifacts.registries import (
    STANDIN_FORGE,
    STANDIN_NPM,
    STANDIN_PYPI,
    STANDIN_PYPI_UPLOAD,
    UPLOADED,
    AssetId,
    ReleaseId,
    ordered,
    pypi_name,
)

#: The program a served package carries: it runs, and it answers `--version`
#: with whatever the caller asked it to, which is the whole of what a route's
#: own proof reads back from it. The answer is a caller's rather than composed
#: here, because a route is proven by the program giving the ONE answer its
#: command line contracts to give — and a stand-in that could only ever give
#: that answer could not falsify the comparison that reads it.
STAND_IN = """#!/bin/sh
if [ "${{1:-}}" = "--version" ]; then
    echo "{answer}"
    exit 0
fi
echo "{PROGRAM}: a stand-in program, which does nothing else" >&2
exit 1
"""

#: A program that installs and does not run, which is what a registry serving a
#: broken artifact looks like from the outside.
BROKEN = f"""#!/bin/sh
echo "{PROGRAM}: this program cannot run on this host" >&2
exit 1
"""

#: Where each registry answers under the one base, and where the Python one
#: takes an upload — the addresses `Bases` composes a stand-in's from, so that
#: what is served here and what a publish is pointed at cannot drift apart.
PYPI_PREFIX = STANDIN_PYPI
NPM_PREFIX = STANDIN_NPM
FORGE_PREFIX = STANDIN_FORGE
PYPI_UPLOAD = STANDIN_PYPI_UPLOAD

#: The state an interrupted upload leaves an asset in, beside `UPLOADED` — the
#: forge's own, read from the module that reads a release document.
INTERRUPTED = "starter"

#: The largest body one write may carry, which is far above any artifact this
#: repository publishes and far below what a stand-in should hold in memory.
LARGEST_WRITE = 256 * 1024 * 1024

#: The registries a write is recorded under: exactly the ones the publisher
#: holds a credential for, so a registry gained there gains a stand-in rather
#: than a silent gap no journey can drive.
REGISTRIES = tuple(CREDENTIALS)

#: The `Authorization` header one write arrived with, verbatim: `Basic` and
#: what `uv` encodes under it, or `Bearer` and the token `npm` or a forge
#: upload sends. Its own type because it is the one thing a write records
#: that is a secret, and a journey reads it back to say which token reached
#: which registry — it is not a name to be compared with one.
Authorization = NewType("Authorization", str)


class StandinError(ValueError):
    """A caller asked the stand-in registries to serve something they cannot."""


@dataclass(frozen=True, slots=True)
class Answer:
    """What this stand-in answers one request with."""

    content_type: str
    body: bytes
    status: int = 200


@dataclass(frozen=True, slots=True)
class Field:
    """One field of a multipart form: its bytes, and the file name it carried."""

    content: bytes = b""
    #: Empty for a field that carried no file, which is every field of the
    #: legacy upload form but the wheel itself.
    file_name: str = ""


@dataclass(frozen=True, slots=True)
class Write:
    """One write a registry here took, or refused, and the credential it carried."""

    registry: str
    method: str
    #: The wheel's file name, the package's name, or the asset's name.
    name: str
    #: The credential as it arrived, which is how the real registries take
    #: one: `Basic` from `uv`, `Bearer` from `npm` and from a forge upload.
    credential: Authorization
    #: Whether a refusal this stand-in was TOLD to make met this write. A write
    #: that met none may still be answered as malformed, so this says what the
    #: caller arranged rather than what the whole answer was.
    refused: bool


@dataclass(frozen=True, slots=True)
class _Refusal:
    """What a caller arranged one artifact's writes to be answered with."""

    answer: Answer
    #: The one method it meets, or empty for every write of the artifact.
    method: str

    def meets(self, method: str) -> bool:
        """Whether a write by one method is the one this refuses."""
        return self.method in {"", method}


@dataclass(slots=True)
class _Asset:
    """One asset a release here carries."""

    id: AssetId
    content: bytes
    state: str = UPLOADED


class Registries:
    """The three registries a route is taken from, answering on one address."""

    def __init__(self, repo: Repo, into: Path) -> None:
        """Start answering on a port the operating system chooses.

        Args:
            repo: The tree the served distributions' own names are declared in.
            into: A directory the assembled packages are written under.
        """
        self.repo = repo
        self.into = into
        self.asked: list[str] = []
        #: Every write a registry here was sent, accepted or refused, in the
        #: order it arrived.
        self.written: list[Write] = []
        self._answers: dict[str, Answer] = {}
        #: Every file the Python registry serves, by the distribution's
        #: normalized name, then by version, then by the file's own name: the
        #: JSON document lists a version's files, and a publish that died
        #: between two per-platform wheels is told apart by the file.
        self._files: dict[str, dict[str, dict[str, bytes]]] = {}
        #: Every package of the JavaScript registry this serves, by its own
        #: name and then by version: the launcher and the per-platform
        #: packages beside it are separate names with separate packuments,
        #: exactly as the published ones are.
        self._manifests: dict[str, dict[str, dict[str, object]]] = {}
        self._tags: list[str] = []
        #: Every asset each release carries, by tag and then by name, with
        #: the forge's own numbering of releases and of assets.
        self._assets: dict[str, dict[str, _Asset]] = {}
        self._release_ids: dict[str, ReleaseId] = {}
        self._next_id = 1
        #: What a write of one artifact is refused with, by registry and name.
        self._refusals: dict[tuple[str, str], _Refusal] = {}
        self._server = ThreadingHTTPServer(("127.0.0.1", 0), _handler(self))
        self._serving = threading.Thread(target=self._server.serve_forever, daemon=True)
        self._serving.start()

    @property
    def base(self) -> str:
        """The one address every registry below it is reached under."""
        host, port = self._server.server_address[:2]
        return f"http://{host!s}:{port}"

    @property
    def names(self) -> dict[str, str]:
        """What each route's own registry serves its distribution under.

        Read out of `release-targets.toml`, which is where a route's target is
        declared, so a stand-in cannot serve a name no route installs.
        """
        return {
            target.registry: target.name
            for target in targets.declared(self.repo.root)
            if target.route
        }

    def serve(
        self,
        version: str,
        *,
        reported: str = "",
        says: str = "",
        broken: bool = False,
        listed: bool = True,
        carries_program: bool = True,
        platform_package: bool = True,
    ) -> None:
        """Serve one version from every registry, and list its release.

        Args:
            version: The version each registry serves it as.
            reported: What the program it carries says its own version is,
                which is `version` unless a caller asks for a mislabelled one.
            says: The whole line that program answers `--version` with, where
                a caller wants one that is not this program's own response at
                all. An installed program that exits zero and prints something
                else — a diagnostic naming the version it could not run as —
                is an artifact that does not work, and a proof reading a
                version out of it rather than reading its answer would call
                that a pass.
            broken: Serve a program that installs and does not run.
            listed: List a release for it on the forge. A package registry
                serving a version the forge never released is a publish that
                reached one place and not the other, which is a state the
                version selection has to be drivable against.
            carries_program: Serve a package that installs and leaves no
                program on the path at all, which is the other way an artifact
                a registry serves can be broken.
            platform_package: Serve the per-platform packages the launcher of
                route 2 resolves the program through. Serving the launcher
                without them is a publish that reached one package and not the
                one beside it — and it is the failure of that route that hides
                best, because an optional dependency nothing published is one
                `npm` skips: the install reports success, and what it left on
                the path cannot run.
        """
        answer = says or f"{PROGRAM} {reported or version}"
        body = BROKEN if broken else STAND_IN.format(PROGRAM=PROGRAM, answer=answer)
        program = self.into / f"program-{version}" / PROGRAM
        program.parent.mkdir(parents=True, exist_ok=True)
        program.write_text(body, encoding="utf-8")
        program.chmod(0o755)

        carried = program if carries_program else None
        self._serve_wheel(version, carried)
        self._serve_package(version, carried, per_platform=platform_package)
        self._serve_release(version, carried)
        if listed:
            self.release(f"v{version}")

    def release(self, tag: str) -> None:
        """List one release on the forge, whether or not anything serves it.

        A tag nothing published an artifact for is what a release whose publish
        did not happen looks like from the outside, which is a case a proof of
        these routes has to be drivable against.
        """
        if tag not in self._tags:
            self._tags.append(tag)
            self._release_ids[tag] = ReleaseId(self._numbered())
        self.answers(
            FORGE_PREFIX,
            json.dumps(
                [
                    {"tag_name": tag, "draft": False, "prerelease": False}
                    for tag in sorted(self._tags, key=ordered, reverse=True)
                ]
            ).encode(),
        )
        self._serve_release_document(tag)

    def refuse(
        self,
        registry: str,
        name: str,
        *,
        status: int,
        body: bytes,
        content_type: str = "application/json",
        method: str = "",
    ) -> None:
        """Refuse every write of one artifact with exactly this answer.

        By the registry and the artifact's name as a write states it: a
        wheel's file name, a package's name, or an asset's name. The status and
        the body are the caller's, because what a publish reports of a refusal
        is the registry's own words, and the words this repository's release of
        2026-09-11 met were `404` and `Scope not found`. Given a `method`, only
        that write of the artifact is refused — which is how a forge that took
        the deletion of an asset and then refused the upload over it is stood
        in for, since the two are two requests the forge answers apart.

        Raises:
            StandinError: If the registry is not one a write is recorded under.
        """
        if registry not in REGISTRIES:
            msg = f"`{registry}` is not a registry here: they are {', '.join(REGISTRIES)}"
            raise StandinError(msg)
        self._refusals[registry, name] = _Refusal(Answer(content_type, body, status), method)

    def accept(self, registry: str, name: str) -> None:
        """Stop refusing one artifact, which is the organization coming to exist."""
        self._refusals.pop((registry, name), None)

    def interrupted(self, tag: str, name: str) -> None:
        """List one asset of a release as an upload that never finished.

        The forge lists an asset whose upload was interrupted under its name in
        a state other than `uploaded`, and a download of the release does not
        find it — so a publish that read the name alone would skip an asset
        nobody can download. Both halves are stood in for: the listing changes
        state, and the download address stops answering until an upload over
        it lands.

        Raises:
            StandinError: If the release carries no such asset.
        """
        asset = self._assets.get(tag, {}).get(name)
        if asset is None:
            msg = f"{tag} carries no asset {name} to have interrupted"
            raise StandinError(msg)
        asset.state = INTERRUPTED
        self._answers.pop(f"{FORGE_PREFIX}/download/{tag}/{name}", None)
        self._serve_release_document(tag)

    def assets_of(self, tag: str) -> dict[str, bytes]:
        """Every asset one release carries, by name, as a download would read it."""
        return {name: asset.content for name, asset in self._assets.get(tag, {}).items()}

    def _numbered(self) -> int:
        """The next number the forge here gives a release or an asset."""
        numbered = self._next_id
        self._next_id += 1
        return numbered

    def answers(
        self,
        path: str,
        body: bytes,
        *,
        content_type: str = "application/json",
        status: int = 200,
    ) -> None:
        """Answer one path with exactly this, whatever its own protocol serves.

        A registry answering a body its protocol does not describe, or refusing
        the read outright, is a state a proof of these routes has to be
        drivable against: what a reader needs then is a stop naming the
        registry rather than an outcome about a publish nothing could read.
        """
        self._answers[path] = Answer(content_type, body, status)

    def stop(self) -> None:
        """Stop answering, leaving no thread behind."""
        self._server.shutdown()
        self._server.server_close()
        self._serving.join(timeout=10)

    def answer(self, path: str) -> Answer:
        """What this stand-in answers one read with.

        Decoded before it is looked up, because `npm` asks for a scoped
        package under its name percent-encoded — `@printobserver%2fcli-...` —
        and what is served is the name itself; and read without its query,
        which is a reader's own and names nothing served here.
        """
        self.asked.append(path)
        found = self._answers.get(_served_path(path))
        if found is None:
            return Answer("text/plain", f"{path} is not served here\n".encode(), 404)
        return found

    def take(self, method: str, path: str, headers: Mapping[str, str], body: bytes) -> Answer:
        """Take one write as the registry it is addressed to takes it.

        Three writes and one deletion, each on its own registry's path: the
        Python registry's legacy upload, the JavaScript registry's publish
        document, and the forge's asset upload and asset deletion. Anything
        else is answered as a path nothing here serves. The headers arrive
        with their names in lower case, as one client spells them and another
        does not.
        """
        where = _served_path(path)
        credential = Authorization(headers.get("authorization", ""))
        if method == "POST" and where == PYPI_UPLOAD.rstrip("/"):
            return self._take_wheel(headers.get("content-type", ""), body, credential)
        if method == "PUT" and where.startswith(f"{NPM_PREFIX}/"):
            return self._take_package(where.removeprefix(f"{NPM_PREFIX}/"), body, credential)
        if method == "POST" and where.startswith(f"{FORGE_PREFIX}/") and where.endswith("/assets"):
            numbered = where.removeprefix(f"{FORGE_PREFIX}/").removesuffix("/assets")
            named = parse_qs(urlsplit(path).query).get("name", [""])[0]
            return self._take_asset(numbered, named, body, credential)
        if method == "DELETE" and where.startswith(f"{FORGE_PREFIX}/assets/"):
            return self._delete_asset(where.removeprefix(f"{FORGE_PREFIX}/assets/"), credential)
        return Answer("text/plain", f"{method} {path} is not served here\n".encode(), 404)

    def _refused(
        self, registry: str, name: str, method: str, credential: Authorization
    ) -> Answer | None:
        """The refusal one write meets, recording it either way."""
        refusal = self._refusals.get((registry, name))
        met = refusal is not None and refusal.meets(method)
        self.written.append(Write(registry, method, name, credential, met))
        return refusal.answer if refusal is not None and met else None

    def _take_wheel(self, content_type: str, body: bytes, credential: Authorization) -> Answer:
        """Take the legacy upload form `uv publish` posts, and serve the file it carries."""
        fields = _form(content_type, body)
        name = _text(fields.get("name", Field()).content)
        version = _text(fields.get("version", Field()).content)
        digest = _text(fields.get("sha256_digest", Field()).content)
        uploaded = fields.get("content", Field())
        content, file_name = uploaded.content, uploaded.file_name
        if name is None or version is None or digest is None:
            return Answer("text/plain", b"the upload carries a field that is not text\n", 400)
        if not name or not version or not file_name:
            return Answer("text/plain", b"the upload names no file\n", 400)
        refusal = self._refused("pypi", file_name, "POST", credential)
        if refusal is not None:
            return refusal
        if digest and digest != hashlib.sha256(content).hexdigest():
            return Answer("text/plain", b"the digest does not match the file\n", 400)
        self._serve_file(pypi_name(name), version, file_name, content)
        return Answer("text/plain", b"")

    def _take_package(self, name: str, body: bytes, credential: Authorization) -> Answer:
        """Take the publish document `npm publish` puts, and serve the version in it.

        As the registry takes it: a version's manifest has to name the package
        the document was put at and the version it is listed under, or the
        publish is refused — a packument serving a manifest that says it is
        some other package is not one any install could resolve through.
        """
        refusal = self._refused("npm", name, "PUT", credential)
        if refusal is not None:
            return refusal
        try:
            document = json.loads(body)
        except UnicodeError, json.JSONDecodeError:
            return Answer("application/json", b'{"error": "not a publish document"}', 400)
        versions = document.get("versions") if isinstance(document, dict) else None
        attachments = document.get("_attachments") if isinstance(document, dict) else None
        if not isinstance(versions, dict) or not isinstance(attachments, dict) or not versions:
            return Answer("application/json", b'{"error": "not a publish document"}', 400)
        for version, manifest in versions.items():
            if not isinstance(manifest, dict):
                return Answer("application/json", b'{"error": "not a manifest"}', 400)
            if manifest.get("name") != name or manifest.get("version") != version:
                return Answer("application/json", b'{"error": "not the package named"}', 400)
            file_name, attached = next(iter(attachments.items()), ("", {}))
            data = attached.get("data", "") if isinstance(attached, dict) else ""
            raw = _attached(file_name, data)
            if raw is None:
                return Answer("application/json", b'{"error": "not an attachment"}', 400)
            self._serve_version(name, version, manifest, file_name, raw)
        return Answer("application/json", b'{"ok": true}')

    def _take_asset(
        self, numbered: str, name: str, body: bytes, credential: Authorization
    ) -> Answer:
        """Take one asset's bytes on a release's own upload address."""
        tag = next((tag for tag, id in self._release_ids.items() if str(id) == numbered), "")
        if not tag or not name:
            return Answer("application/json", b'{"message": "Not Found"}', 404)
        refusal = self._refused("release", name, "POST", credential)
        if refusal is not None:
            return refusal
        assets = self._assets.setdefault(tag, {})
        if name in assets:
            return Answer("application/json", b'{"message": "already_exists"}', 422)
        asset = _Asset(AssetId(self._numbered()), body)
        assets[name] = asset
        self._serve_asset(tag, name)
        return Answer("application/json", json.dumps(self._listed(name, asset)).encode(), 201)

    def _delete_asset(self, numbered: str, credential: Authorization) -> Answer:
        """Remove one asset by the number the forge here gave it."""
        for tag, assets in self._assets.items():
            for name, asset in assets.items():
                if str(asset.id) == numbered:
                    refusal = self._refused("release", name, "DELETE", credential)
                    if refusal is not None:
                        return refusal
                    del assets[name]
                    self._answers.pop(f"{FORGE_PREFIX}/download/{tag}/{name}", None)
                    self._serve_release_document(tag)
                    return Answer("application/json", b"", 204)
        return Answer("application/json", b'{"message": "Not Found"}', 404)

    def _serve_wheel(self, version: str, program: Path | None) -> None:
        """Assemble a wheel and serve it, with the index an installer reads."""
        from release_artifacts import wheels

        name = self.names["pypi"]
        distribution = wheels.Distribution(
            name=name,
            version=version,
            summary="A stand-in for the command-line distribution.",
            requires_python=">=3.9",
            license="MIT",
            homepage=self.base,
        )
        wheel = wheels.Wheel(distribution, wheels.PURE_TAG)
        if program is not None:
            wheel.add_script(PROGRAM, program)
        written = wheel.write(self.into / "pypi")
        self._serve_file(pypi_name(name), version, written.name, written.read_bytes())

    def _serve_file(self, name: str, version: str, file_name: str, content: bytes) -> None:
        """Serve one file of the Python registry, and the two documents listing it.

        The JSON document lists each version's FILES, as the real one does: a
        document whose `releases` listed none would be one no reader could
        tell a half-published version from a whole one by.
        """
        self._files.setdefault(name, {}).setdefault(version, {})[file_name] = content
        self.answers(
            f"{PYPI_PREFIX}/files/{file_name}", content, content_type="application/octet-stream"
        )
        versions = self._files[name]
        self.answers(
            f"{PYPI_PREFIX}/pypi/{name}/json",
            json.dumps(
                {
                    "info": {"name": name, "version": _newest(versions)},
                    "releases": {
                        served: [
                            {
                                "filename": file_name,
                                "url": f"{self.base}{PYPI_PREFIX}/files/{file_name}",
                                "size": len(files[file_name]),
                            }
                            for file_name in sorted(files)
                        ]
                        for served, files in versions.items()
                    },
                }
            ).encode(),
        )
        links = "\n".join(
            f'<a href="{self.base}{PYPI_PREFIX}/files/{file_name}">{file_name}</a><br>'
            for file_name in sorted(file_name for files in versions.values() for file_name in files)
        )
        self.answers(
            f"{PYPI_PREFIX}/simple/{name}",
            f"<!DOCTYPE html>\n<html><body>\n{links}\n</body></html>\n".encode(),
            content_type="text/html",
        )

    def _serve_package(self, version: str, program: Path | None, *, per_platform: bool) -> None:
        """Assemble route 2's packages and serve a packument for each name.

        What that route publishes is a **launcher** and one package per
        supported platform beside it: the launcher carries no program of its
        own and names those packages as optional dependencies, and the
        caller's own package manager resolves whichever one their operating
        system and processor select. A stand-in serving one package with the
        program in it would serve a shape nothing publishes, and the failure
        it could then never produce is the one this route hides best — the
        launcher published and its platform package missing, which installs
        clean and leaves a program on the path that cannot run.
        """
        supported = platforms.supported(self.repo)
        if program is not None and per_platform:
            for platform in supported:
                system, processor = platform.npm
                carried = packages.Archive()
                carried.add(
                    f"{packages.PACKAGE_ROOT}/bin/{PROGRAM}", program.read_bytes(), executable=True
                )
                self._publish(
                    self._package(platform.npm_package, version),
                    carried,
                    os=[system],
                    cpu=[processor],
                    bin={PROGRAM: f"bin/{PROGRAM}"},
                    files=["bin"],
                )

        beside = packages.Archive()
        declared: dict[str, object] = {
            "type": "module",
            "files": ["bin"],
            # Named whether or not anything serves them: what makes an absent
            # platform package the quiet failure it is, is that the launcher
            # goes on declaring one.
            "optionalDependencies": {platform.npm_package: version for platform in supported},
        }
        if program is not None:
            beside.add(
                f"{packages.PACKAGE_ROOT}/bin/{PROGRAM}.mjs",
                self.repo.read(LAUNCHER).encode(),
                executable=True,
            )
            declared["bin"] = {PROGRAM: f"bin/{PROGRAM}.mjs"}
        self._publish(self._package(self.names["npm"], version), beside, **declared)

    def _package(self, name: str, version: str) -> packages.NodePackage:
        """What one package of the JavaScript registry says about itself."""
        return packages.NodePackage(
            name=name,
            version=version,
            description="A stand-in for the command-line distribution.",
            license="MIT",
            repository=self.base,
        )

    def _publish(
        self, package: packages.NodePackage, archive: packages.Archive, **declared: object
    ) -> None:
        """Serve one package's tarball, and the packument every version of it is in."""
        manifest = package.manifest(**declared)
        written = packages.packed(package, manifest, archive, self.into / "npm")
        self._serve_version(
            package.name, package.version, manifest, written.name, written.read_bytes()
        )

    def _serve_version(
        self, name: str, version: str, manifest: dict[str, object], file_name: str, raw: bytes
    ) -> None:
        """Serve one version of one package: its tarball, and the packument listing it."""
        self.answers(
            f"{NPM_PREFIX}/{name}/-/{file_name}",
            raw,
            content_type="application/octet-stream",
        )
        versions = self._manifests.setdefault(name, {})
        versions[version] = {
            **manifest,
            "dist": {
                "tarball": f"{self.base}{NPM_PREFIX}/{name}/-/{file_name}",
                # SHA-1 because `dist.shasum` is SHA-1 by the JavaScript
                # registry's own protocol, and `npm` refuses to install from a
                # packument carrying anything else. suppressions.toml has the
                # whole reason; `integrity` beside it carries SHA-512.
                "shasum": hashlib.sha1(raw).hexdigest(),  # noqa: S324
                "integrity": "sha512-"
                + base64.b64encode(hashlib.sha512(raw).digest()).decode("ascii"),
            },
        }
        self.answers(
            f"{NPM_PREFIX}/{name}",
            json.dumps(
                {
                    "_id": name,
                    "name": name,
                    "dist-tags": {"latest": _newest(versions)},
                    "versions": versions,
                }
            ).encode(),
        )

    def _serve_release(self, version: str, program: Path | None) -> None:
        """Publish the release artifacts the install script downloads."""
        asset = f"{PROGRAM}-{platforms.host(self.repo).id}.tar.gz"
        archive = packages.Archive()
        if program is None:
            archive.add("README", b"a release artifact carrying no program\n")
        else:
            archive.add(PROGRAM, program.read_bytes(), executable=True)
        written = archive.write(self.into / "releases" / version / asset)
        tag = f"v{version}"
        assets = self._assets.setdefault(tag, {})
        assets[asset] = _Asset(AssetId(self._numbered()), written.read_bytes())
        assets[CHECKSUMS] = _Asset(AssetId(self._numbered()), packages.checksums([written]))
        for name in (asset, CHECKSUMS):
            self._serve_asset(tag, name)
            self.answers(
                f"{FORGE_PREFIX}/latest/download/{name}",
                assets[name].content,
                content_type=_asset_type(name),
            )

    def _serve_asset(self, tag: str, name: str) -> None:
        """Serve one asset where the install script downloads it, and list it."""
        self.answers(
            f"{FORGE_PREFIX}/download/{tag}/{name}",
            self._assets[tag][name].content,
            content_type=_asset_type(name),
        )
        self._serve_release_document(tag)

    def _listed(self, name: str, asset: _Asset) -> dict[str, object]:
        """One asset as the release document lists it."""
        return {"id": asset.id, "name": name, "size": len(asset.content), "state": asset.state}

    def _serve_release_document(self, tag: str) -> None:
        """Serve the release document the forge answers for one tag, if it is listed."""
        numbered = self._release_ids.get(tag)
        if numbered is None:
            return
        self.answers(
            f"{FORGE_PREFIX}/tags/{tag}",
            json.dumps(
                {
                    "id": numbered,
                    "tag_name": tag,
                    "upload_url": f"{self.base}{FORGE_PREFIX}/{numbered}/assets{{?name,label}}",
                    "assets": [
                        self._listed(name, asset)
                        for name, asset in self._assets.get(tag, {}).items()
                    ],
                }
            ).encode(),
        )


def _served_path(path: str) -> str:
    """The path one request names, as it is served here."""
    return unquote(urlsplit(path).path).rstrip("/") or "/"


def _asset_type(name: str) -> str:
    """What one release asset is served as."""
    return "text/plain" if name == CHECKSUMS else "application/octet-stream"


def _form(content_type: str, body: bytes) -> dict[str, Field]:
    """The fields of one multipart form, each with the file name it carried, if any."""
    message = BytesParser().parsebytes(
        f"Content-Type: {content_type}\r\nMIME-Version: 1.0\r\n\r\n".encode() + body
    )
    fields: dict[str, Field] = {}
    for part in message.walk():
        if part.is_multipart():
            continue
        name = part.get_param("name", header="content-disposition")
        if not isinstance(name, str):
            continue
        payload = part.get_payload(decode=True)
        fields[name] = Field(
            payload if isinstance(payload, bytes) else b"", part.get_filename() or ""
        )
    return fields


def _text(raw: bytes) -> str | None:
    """One form field as text, or nothing where what arrived is not text at all."""
    try:
        return raw.decode("utf-8")
    except UnicodeDecodeError:
        return None


def _attached(file_name: object, data: object) -> bytes | None:
    """The tarball one publish document attaches, or nothing where it attaches none."""
    if not isinstance(file_name, str) or not file_name or not isinstance(data, str):
        return None
    try:
        return base64.b64decode(data, validate=True)
    except binascii.Error:
        return None


def _length(declared: str | None) -> int | None:
    """How many bytes a write says it carries, or nothing where that is not a length.

    A write with no `Content-Length` carries none, which is what a `DELETE`
    here is. Anything that is not a number, is negative, or is larger than
    this stand-in will hold is refused rather than read.
    """
    if declared is None:
        return 0
    try:
        length = int(declared)
    except ValueError:
        return None
    return length if 0 <= length <= LARGEST_WRITE else None


def _newest(served: Mapping[str, object]) -> str:
    """The newest version a registry below serves, as its own answer states it."""
    return max(served, key=ordered, default="")


def _handler(registries: Registries) -> type[BaseHTTPRequestHandler]:
    """The request handler every registry answers through."""

    class Handler(BaseHTTPRequestHandler):
        """Answer a read of a package, an index, a packument or a release, or take a write."""

        protocol_version = "HTTP/1.1"

        def do_GET(self) -> None:
            """Answer whatever an installer asked for."""
            self._answer(registries.answer(self.path))

        def do_POST(self) -> None:
            """Take a wheel's legacy upload form, or one release asset's bytes."""
            self._take("POST")

        def do_PUT(self) -> None:
            """Take a package's publish document, which is how that registry is written."""
            self._take("PUT")

        def do_DELETE(self) -> None:
            """Take the removal of a release asset, which an upload over one does first."""
            self._take("DELETE")

        def _take(self, method: str) -> None:
            """Read the whole body a write carries and answer as its registry would."""
            length = _length(self.headers.get("Content-Length"))
            if length is None:
                self._answer(
                    Answer("text/plain", b"the request declares no length this can read\n", 400)
                )
                return
            body = self.rfile.read(length)
            headers = {name.lower(): value for name, value in self.headers.items()}
            self._answer(registries.take(method, self.path, headers, body))

        def _answer(self, answer: Answer) -> None:
            """Answer, declaring the length every client reads the body by."""
            self.send_response(answer.status)
            self.send_header("Content-Type", answer.content_type)
            self.send_header("Content-Length", str(len(answer.body)))
            self.end_headers()
            self.wfile.write(answer.body)

        def log_message(self, format: str, *args: object) -> None:
            """Say nothing: a stand-in whose log is the output is not signal."""

    return Handler
