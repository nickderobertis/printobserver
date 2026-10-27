"""A stand-in for a running instance's API, answering one status to every command.

Started by a test as a process of its own, so the script finds it the way it
finds an instance it started: a live process, recorded with the address it
answers on. It answers the script's liveness probe, `GET /api/version`, as an
instance does, and answers every `POST` — a line typed at the terminal, or a
reconnect — with the status it was started with, and an instance's own words for
a refusal. It prints the port it bound on its standard output, logs each request
on its standard error as the standard library's server does, and serves until
it is stopped.

Usage: `python answering_host.py <status>`.
"""

from __future__ import annotations

import json
import sys
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer

#: What an instance says when it will not take a line or a reconnect.
REFUSAL = {"error": "Printer is not operational"}


def handler_answering(status: int) -> type[BaseHTTPRequestHandler]:
    """A request handler answering every `POST` with `status`."""

    class Answering(BaseHTTPRequestHandler):
        """The stand-in's one handler."""

        def do_GET(self) -> None:
            """Answer the liveness probe as an instance does, and nothing else."""
            if self.path != "/api/version":
                self._send(404, {"error": "Not found"})
                return
            self._send(200, {"api": "0.1", "server": "1.11.2", "text": "OctoPrint 1.11.2"})

        def do_POST(self) -> None:
            """Answer a line or a reconnect with the status this host was started with."""
            length = int(self.headers.get("Content-Length") or 0)
            self.rfile.read(length)
            if status == 204:
                self.send_response(204)
                self.end_headers()
                return
            self._send(status, REFUSAL)

        def _send(self, code: int, document: object) -> None:
            """One JSON answer."""
            body = json.dumps(document).encode("utf-8")
            self.send_response(code)
            self.send_header("Content-Type", "application/json")
            self.send_header("Content-Length", str(len(body)))
            self.end_headers()
            self.wfile.write(body)

    return Answering


def main() -> int:
    """Bind a port the system chooses, say which, and serve until stopped."""
    status = int(sys.argv[1])
    server = ThreadingHTTPServer(("127.0.0.1", 0), handler_answering(status))
    print(server.server_address[1], flush=True)
    server.serve_forever()
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
