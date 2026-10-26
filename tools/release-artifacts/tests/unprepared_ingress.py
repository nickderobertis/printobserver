"""A stand-in supervisor whose ingress accepts an alert and never finishes preparing it.

Run as `<program> server --config <path>`, the way a world runs the real one.
It says where it serves the way the real one does, and on the first alert
answers 202 and leaves the store beside that configuration short of ready in the
way `UNPREPARED_STORE` names, so a world waiting for the print and image waits
for something that never comes:

- `recorded` (the default): a print is recorded, and no image, alert or
  finished supervision turn ever is;
- `absent`: no store is ever written;
- `tableless`: a store holding a table of prints and nothing else.
"""

from __future__ import annotations

import os
import socketserver
import sqlite3
import sys
from http.server import BaseHTTPRequestHandler, HTTPServer
from pathlib import Path

SERVING_ON = "printobserver is serving on "
STORE = Path(sys.argv[3]).parent / "state" / "printobserver.sqlite3"
SHORT_OF_READY = {
    "recorded": (
        "CREATE TABLE prints (id TEXT); CREATE TABLE images (id TEXT);"
        " CREATE TABLE events (id TEXT, kind TEXT, payload TEXT);"
        " INSERT INTO prints VALUES ('a-print');"
    ),
    "tableless": "CREATE TABLE prints (id TEXT);",
    "absent": None,
}[os.environ.get("UNPREPARED_STORE", "recorded")]


class Ingress(BaseHTTPRequestHandler):
    """Takes the alert and leaves the store short of a prepared print."""

    def do_POST(self) -> None:
        """Leave the store short of ready and answer that the alert was taken."""
        self.rfile.read(int(self.headers["Content-Length"]))
        if SHORT_OF_READY is not None:
            # llmlint: ignore[async_typed_clients_at_boundaries] suppressions.toml has the reason.
            store = sqlite3.connect(STORE)
            store.executescript(SHORT_OF_READY)
            store.close()
        self.send_response(202)
        self.send_header("Content-Length", "0")
        self.end_headers()


class Loopback(HTTPServer):
    """Binds its numeric address and asks no name service what it is called."""

    def server_bind(self) -> None:
        """Bind, and name the server by the address it bound."""
        socketserver.TCPServer.server_bind(self)
        host, port = self.server_address[:2]
        self.server_name, self.server_port = str(host), int(port)


server = Loopback(("127.0.0.1", 0), Ingress)
sys.stderr.write(f"{SERVING_ON}127.0.0.1:{server.server_port}\n")
sys.stderr.flush()
server.serve_forever()
