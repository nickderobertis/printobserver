"""A release, served to an installer over real HTTP on loopback.

Every installer in `repo_checks` fetches a producer's release and refuses
anything it cannot vouch for, and what proves one is its own download, digest
check, extraction and placement — not a patched `urlopen`. So each suite stands
one of these up, publishes the files the real release would publish for the host
it is running on, and points the installer at it with `--releases`.

It records the path of every request, so a suite can state what the installer
asked for and in which order: a digest read after the artifact it vouches for
would be no verification at all.
"""

from __future__ import annotations

import threading
from collections.abc import Iterator
from contextlib import contextmanager
from dataclasses import dataclass, field
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer


@dataclass(slots=True)
class Release:
    """A release served over loopback HTTP: the files, by name, and what was asked.

    Not frozen: a suite publishes a file, drives an installer, and republishes
    a different one under the same name to drive the next outcome.
    """

    #: What the release publishes, by the name the forge serves it under.
    files: dict[str, bytes] = field(default_factory=dict)
    #: The path of every request made of it, in the order they were made.
    asked: list[str] = field(default_factory=list)
    #: A name the release answers with a redirect rather than with bytes, and
    #: the address it names. A real release asset is answered this way — the
    #: forge redirects to its own asset store — so this is what lets a suite
    #: drive both the redirect an installer follows and the one it refuses.
    redirects: dict[str, str] = field(default_factory=dict)


@contextmanager
def serving(prefix: str, release: Release) -> Iterator[str]:
    """Answer `release`'s files under `prefix`, yielding the address to download from.

    Anything outside `prefix`, and any name the release does not publish, is a
    404 — which is what an installer meets when a producer publishes no artifact
    for a host, and is a case each suite drives. A name in `redirects` is
    answered `302` to the address it names, as the forge answers for a release
    asset.
    """

    class Handler(BaseHTTPRequestHandler):
        def do_GET(self) -> None:
            release.asked.append(self.path)
            name = self.path.rsplit("/", 1)[-1]
            if self.path.startswith(prefix) and name in release.redirects:
                self.send_response(302)
                self.send_header("Location", release.redirects[name])
                self.send_header("Content-Length", "0")
                self.end_headers()
                return
            body = release.files.get(name) if self.path.startswith(prefix) else None
            if body is None:
                self.send_error(404)
                return
            self.send_response(200)
            self.send_header("Content-Length", str(len(body)))
            self.end_headers()
            self.wfile.write(body)

        def log_message(self, format: str, *args: object) -> None:
            """Say nothing: the suite's own assertions are the signal."""

    server = ThreadingHTTPServer(("127.0.0.1", 0), Handler)
    thread = threading.Thread(target=server.serve_forever, daemon=True)
    thread.start()
    try:
        yield f"http://127.0.0.1:{server.server_address[1]}"
    finally:
        server.shutdown()
        server.server_close()
