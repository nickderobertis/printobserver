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
rather than the program inside. The shape of the published artifacts — the
per-platform wheel tag, the launcher and the platform package beside it — is
what `just prove-route-*` proves instead, over artifacts built from the
committed tree.

One base address covers all three, because a proof that read one registry from a
stand-in and another from the real internet would be a proof of neither: the
paths below it are each registry's own.
"""

from __future__ import annotations

import base64
import hashlib
import json
import threading
from collections.abc import Mapping
from dataclasses import dataclass
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from pathlib import Path

from repo_checks.model import Repo

from release_artifacts import packages, platforms, targets
from release_artifacts.build import CHECKSUMS, PROGRAM

# The one version ordering. A stand-in that sorted versions its own way could
# serve a newest the proof selecting from it disagreed about.
from release_artifacts.registries import ordered

#: The program a served package carries: it runs, and it says which version it
#: is, which is the whole of what a route's own proof reads back from it.
STAND_IN = """#!/bin/sh
if [ "${{1:-}}" = "--version" ]; then
    echo "{PROGRAM} {reported}"
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

#: Where each registry answers under the one base, as that registry's own
#: service spells its paths. `pypi` and `npm` are read as registry bases and the
#: forge's is read as both the release list and the download root, exactly as
#: the real ones are.
PYPI_PREFIX = "/pypi"
NPM_PREFIX = "/npm"
FORGE_PREFIX = "/forge/releases"


class StandinError(ValueError):
    """A caller asked the stand-in registries to serve something they cannot."""


@dataclass(frozen=True, slots=True)
class Answer:
    """What this stand-in answers one read with."""

    content_type: str
    body: bytes
    status: int = 200


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
        self._answers: dict[str, Answer] = {}
        self._wheels: dict[str, str] = {}
        self._manifests: dict[str, dict[str, object]] = {}
        self._tags: list[str] = []
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
        broken: bool = False,
        listed: bool = True,
        carries_program: bool = True,
    ) -> None:
        """Serve one version from every registry, and list its release.

        Args:
            version: The version each registry serves it as.
            reported: What the program it carries says its own version is,
                which is `version` unless a caller asks for a mislabelled one.
            broken: Serve a program that installs and does not run.
            listed: List a release for it on the forge. A package registry
                serving a version the forge never released is a publish that
                reached one place and not the other, which is a state the
                version selection has to be drivable against.
            carries_program: Serve a package that installs and leaves no
                program on the path at all, which is the other way an artifact
                a registry serves can be broken.
        """
        body = BROKEN if broken else STAND_IN.format(PROGRAM=PROGRAM, reported=reported or version)
        program = self.into / f"program-{version}" / PROGRAM
        program.parent.mkdir(parents=True, exist_ok=True)
        program.write_text(body, encoding="utf-8")
        program.chmod(0o755)

        self._serve_wheel(version, program if carries_program else None)
        self._serve_package(version, program if carries_program else None)
        self._serve_release(version, program if carries_program else None)
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
        self.answers(
            FORGE_PREFIX,
            json.dumps(
                [
                    {"tag_name": tag, "draft": False, "prerelease": False}
                    for tag in sorted(self._tags, key=ordered, reverse=True)
                ]
            ).encode(),
        )

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
        """What this stand-in answers one read with."""
        self.asked.append(path)
        found = self._answers.get(path.rstrip("/") or "/")
        if found is None:
            return Answer("text/plain", f"{path} is not served here\n".encode(), 404)
        return found

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
        self._wheels[version] = written.name
        self.answers(
            f"{PYPI_PREFIX}/files/{written.name}",
            written.read_bytes(),
            content_type="application/octet-stream",
        )
        self.answers(
            f"{PYPI_PREFIX}/pypi/{name}/json",
            json.dumps(
                {
                    "info": {"name": name, "version": _newest(self._wheels)},
                    "releases": {served: [] for served in self._wheels},
                }
            ).encode(),
        )
        links = "\n".join(
            f'<a href="{self.base}{PYPI_PREFIX}/files/{file_name}">{file_name}</a><br>'
            for file_name in sorted(self._wheels.values())
        )
        self.answers(
            f"{PYPI_PREFIX}/simple/{name}",
            f"<!DOCTYPE html>\n<html><body>\n{links}\n</body></html>\n".encode(),
            content_type="text/html",
        )

    def _serve_package(self, version: str, program: Path | None) -> None:
        """Assemble a package of the JavaScript registry and serve its packument."""
        name = self.names["npm"]
        package = packages.NodePackage(
            name=name,
            version=version,
            description="A stand-in for the command-line distribution.",
            license="MIT",
            repository=self.base,
        )
        archive = packages.Archive()
        carried: dict[str, object] = {"files": ["bin"]}
        if program is not None:
            archive.add(
                f"{packages.PACKAGE_ROOT}/bin/{PROGRAM}", program.read_bytes(), executable=True
            )
            carried["bin"] = {PROGRAM: f"bin/{PROGRAM}"}
        manifest = package.manifest(**carried)
        written = packages.packed(package, manifest, archive, self.into / "npm")
        raw = written.read_bytes()
        tarball = f"{self.base}{NPM_PREFIX}/{name}/-/{written.name}"
        self.answers(
            f"{NPM_PREFIX}/{name}/-/{written.name}",
            raw,
            content_type="application/octet-stream",
        )
        self._manifests[version] = {
            **manifest,
            "dist": {
                "tarball": tarball,
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
                    "dist-tags": {"latest": _newest(self._manifests)},
                    "versions": self._manifests,
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
        digests = packages.checksums([written])
        for under in (f"{FORGE_PREFIX}/download/v{version}", f"{FORGE_PREFIX}/latest/download"):
            self.answers(
                f"{under}/{asset}",
                written.read_bytes(),
                content_type="application/octet-stream",
            )
            self.answers(f"{under}/{CHECKSUMS}", digests, content_type="text/plain")


def _newest(served: Mapping[str, object]) -> str:
    """The newest version a registry below serves, as its own answer states it."""
    return max(served, key=ordered, default="")


def _handler(registries: Registries) -> type[BaseHTTPRequestHandler]:
    """The request handler every registry answers through."""

    class Handler(BaseHTTPRequestHandler):
        """Answer a read of a package, an index, a packument or a release."""

        protocol_version = "HTTP/1.1"

        def do_GET(self) -> None:
            """Answer whatever an installer asked for."""
            answer = registries.answer(self.path)
            self.send_response(answer.status)
            self.send_header("Content-Type", answer.content_type)
            self.send_header("Content-Length", str(len(answer.body)))
            self.end_headers()
            self.wfile.write(answer.body)

        def log_message(self, format: str, *args: object) -> None:
            """Say nothing: a stand-in whose log is the output is not signal."""

    return Handler
