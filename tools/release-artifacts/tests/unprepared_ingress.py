"""A stand-in supervisor whose ingress accepts an alert and never finishes preparing it.

Run as `<program> server --config <path>`, the way a world runs the real one.
It says where it serves the way the real one does, and on the first alert
records a print in the store beside that configuration and answers 202 — and
then never stores an image or finishes a supervision turn, so a world waiting
for the print and image waits for something that never comes.
"""

from __future__ import annotations

import socketserver
import sqlite3
import sys
from http.server import BaseHTTPRequestHandler, HTTPServer
from pathlib import Path

SERVING_ON = "printobserver is serving on "
STORE = Path(sys.argv[3]).parent / "state" / "printobserver.sqlite3"


class Ingress(BaseHTTPRequestHandler):
    """Takes the alert, records a print and stops short of an image."""

    def do_POST(self) -> None:
        """Record a print and answer that the alert was taken."""
        self.rfile.read(int(self.headers["Content-Length"]))
        store = sqlite3.connect(STORE)
        store.executescript(
            "CREATE TABLE prints (id TEXT); CREATE TABLE images (id TEXT);"
            " CREATE TABLE events (id TEXT, kind TEXT, payload TEXT);"
            " INSERT INTO prints VALUES ('a-print');"
        )
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
