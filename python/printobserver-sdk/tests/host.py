"""A stub supervisor on a real socket, for the generated walk to drive.

It is a **host** rather than a stand-in for the client's transport: the client
opens a real connection, writes a real request and reads a real answer, and
what this records is what arrived over that connection. The layer under test is
the client, and nothing here is inside it.

The real server is what the printer integration tier drives. This is what the
fast tier drives, because a walk over every operation needs an answer of every
shape and a real supervisor cannot be put into sixteen states in a second.
"""

from __future__ import annotations

import json
import threading
from dataclasses import dataclass
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from types import TracebackType

#: The media type every operation answers in.
MEDIA_TYPE = "application/json"

#: How long a caller waits for a call that should have arrived.
ARRIVAL_TIMEOUT_SECONDS = 10.0


@dataclass(frozen=True, slots=True)
class Received:
    """What arrived over one connection."""

    #: The method it was made by.
    method: str
    #: The whole request target, question mark and all.
    target: str
    #: The body it carried, empty where it carried none.
    body: str


class Host:
    """A host answering one canned document to whatever it is asked."""

    def __init__(self, status: int, answer: object) -> None:
        """Serve `answer` under `status`, on a port the system chooses."""
        self.status = status
        self.answer = json.dumps(answer).encode("utf-8")
        self.arrivals: list[Received] = []
        self._arrived = threading.Event()
        self._server = ThreadingHTTPServer(("127.0.0.1", 0), _handler(self))
        self._serving = threading.Thread(target=self._server.serve_forever, daemon=True)

    @property
    def address(self) -> str:
        """Where this host answers."""
        host, port = self._server.server_address[:2]
        return f"{host!s}:{port}"

    def __enter__(self) -> Host:
        """Start serving."""
        self._serving.start()
        return self

    def __exit__(
        self,
        kind: type[BaseException] | None,
        value: BaseException | None,
        traceback: TracebackType | None,
    ) -> None:
        """Stop serving, leaving no thread behind."""
        self._server.shutdown()
        self._server.server_close()
        self._serving.join(timeout=ARRIVAL_TIMEOUT_SECONDS)

    def received(self) -> Received:
        """What arrived over the one connection this host served.

        Returns:
            The one request that reached it.

        Raises:
            AssertionError: If nothing arrived, which is a call never made.
        """
        if not self._arrived.wait(timeout=ARRIVAL_TIMEOUT_SECONDS):
            msg = "no call reached this host"
            raise AssertionError(msg)
        return self.arrivals[0]

    def requests(self) -> int:
        """How many requests reached this host.

        Read without waiting: what a caller asks this is whether a call that
        should have been refused before it was made reached the wire, and
        waiting for one that never comes would be waiting for a timeout.
        """
        return len(self.arrivals)


def _handler(host: Host) -> type[BaseHTTPRequestHandler]:
    """The request handler one host answers through."""

    class Handler(BaseHTTPRequestHandler):
        """Record what arrived and answer the canned document."""

        protocol_version = "HTTP/1.1"

        def _serve(self) -> None:
            length = int(self.headers.get("Content-Length") or 0)
            body = self.rfile.read(length).decode("utf-8") if length else ""
            host.arrivals.append(Received(self.command, self.path, body))
            host._arrived.set()
            self.send_response(host.status)
            self.send_header("Content-Type", MEDIA_TYPE)
            self.send_header("Content-Length", str(len(host.answer)))
            self.end_headers()
            self.wfile.write(host.answer)

        def do_GET(self) -> None:
            """Answer a read."""
            self._serve()

        def do_POST(self) -> None:
            """Answer a request that changes something."""
            self._serve()

        def do_PUT(self) -> None:
            """Answer a write."""
            self._serve()

        def log_message(self, format: str, *args: object) -> None:
            """Say nothing: a suite's output is its assertions, not its traffic."""

    return Handler
