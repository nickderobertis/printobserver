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
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer


class Release:
    """A release served over loopback HTTP: the files, by name, and what was asked."""

    def __init__(self, files: dict[str, bytes] | None = None) -> None:
        """A release publishing `files`, which nothing has asked for yet."""
        self.files = files if files is not None else {}
        self.asked: list[str] = []


@contextmanager
def serving(prefix: str, release: Release) -> Iterator[str]:
    """Answer `release`'s files under `prefix`, yielding the address to download from.

    Anything outside `prefix`, and any name the release does not publish, is a
    404 — which is what an installer meets when a producer publishes no artifact
    for a host, and is a case each suite drives.
    """

    class Handler(BaseHTTPRequestHandler):
        def do_GET(self) -> None:
            release.asked.append(self.path)
            name = self.path.rsplit("/", 1)[-1]
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
