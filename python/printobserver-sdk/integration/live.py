"""What the walk against a real supervisor stands on.

Two things, and the second is what makes the first worth having.

`Proxy` sits between the client and the **real** supervisor and forwards every
request to it, recording what went and what came back. That is where the body a
comparison is against comes from: the answer this supervisor actually sent,
rather than a document a test wrote for it.

`matches` is the comparison. What a method answered is held against that
captured body, field for field — so a client that put anything of its own into
an answer fails on the equality rather than needing a probe that guesses what it
put there. `test_falsifying.py` drives two such clients through it.
"""

from __future__ import annotations

import http.client
import json
import threading
from dataclasses import dataclass
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from types import TracebackType
from urllib.parse import urlsplit

#: The media type every operation answers in.
MEDIA_TYPE = "application/json"

#: How long one forwarded call waits on the supervisor.
FORWARD_TIMEOUT_SECONDS = 60.0

#: How long the machine is given to reach a state a step needs.
PATIENCE_SECONDS = 180.0


@dataclass(frozen=True, slots=True)
class Exchange:
    """One request that went to the supervisor, and the answer it sent back."""

    #: The method the call was made by.
    method: str
    #: The whole request target, question mark and all.
    target: str
    #: The body the call carried, empty where it carried none.
    body: str
    #: The status the supervisor answered under.
    status: int
    #: The whole document the supervisor answered with.
    answer: str


class Proxy:
    """A recording proxy in front of one supervisor."""

    def __init__(self, server: str) -> None:
        """Forward to the supervisor at `server`, on a port the system chooses."""
        self.onward = urlsplit(server).netloc or server
        self.seen: list[Exchange] = []
        self._server = ThreadingHTTPServer(("127.0.0.1", 0), _handler(self))
        self._serving = threading.Thread(target=self._server.serve_forever, daemon=True)

    @property
    def url(self) -> str:
        """Where this proxy answers, as a client's own configuration writes it."""
        host, port = self._server.server_address[:2]
        return f"http://{host!s}:{port}"

    def __enter__(self) -> Proxy:
        """Start forwarding."""
        self._serving.start()
        return self

    def __exit__(
        self,
        kind: type[BaseException] | None,
        value: BaseException | None,
        traceback: TracebackType | None,
    ) -> None:
        """Stop forwarding, leaving no thread behind."""
        self._server.shutdown()
        self._server.server_close()
        self._serving.join(timeout=FORWARD_TIMEOUT_SECONDS)

    def last(self) -> Exchange:
        """The last exchange that went through this proxy.

        Returns:
            What went to the supervisor and what it sent back.

        Raises:
            AssertionError: If nothing has, which is a call never made.
        """
        if not self.seen:
            msg = "no call reached the supervisor through this proxy"
            raise AssertionError(msg)
        return self.seen[-1]

    def calls(self) -> int:
        """How many calls have gone through it."""
        return len(self.seen)


def _handler(proxy: Proxy) -> type[BaseHTTPRequestHandler]:
    """The request handler one proxy forwards through."""

    class Handler(BaseHTTPRequestHandler):
        """Forward one request to the supervisor and record both halves."""

        protocol_version = "HTTP/1.1"

        def _forward(self) -> None:
            length = int(self.headers.get("Content-Length") or 0)
            body = self.rfile.read(length).decode("utf-8") if length else ""
            headers = {"Accept": MEDIA_TYPE}
            if body:
                headers["Content-Type"] = MEDIA_TYPE
            upstream = http.client.HTTPConnection(proxy.onward, timeout=FORWARD_TIMEOUT_SECONDS)
            try:
                upstream.request(self.command, self.path, body=body or None, headers=headers)
                answered = upstream.getresponse()
                status = answered.status
                said = answered.read()
            finally:
                upstream.close()
            proxy.seen.append(
                Exchange(
                    method=self.command,
                    target=self.path,
                    body=body,
                    status=status,
                    answer=said.decode("utf-8", errors="replace"),
                )
            )
            self.send_response(status)
            self.send_header("Content-Type", MEDIA_TYPE)
            self.send_header("Content-Length", str(len(said)))
            self.end_headers()
            self.wfile.write(said)

        def do_GET(self) -> None:
            """Forward a read."""
            self._forward()

        def do_POST(self) -> None:
            """Forward a request that changes something."""
            self._forward()

        def do_PUT(self) -> None:
            """Forward a write."""
            self._forward()

        def log_message(self, format: str, *args: object) -> None:
            """Say nothing: a suite's output is its assertions, not its traffic."""

    return Handler


def matches(answered: object, captured: str) -> bool:
    """Whether what a method answered is, field for field, what was sent."""
    return answered == json.loads(captured)


def same(operation: str, answered: object, captured: str) -> None:
    """Assert that what a method answered is what the supervisor sent.

    Raises:
        AssertionError: If it is not, showing both.
    """
    if not matches(answered, captured):
        message = (
            f"`{operation}` answered something other than what the supervisor sent.\n"
            f"it answered: {json.dumps(answered, sort_keys=True)}\n"
            f"the supervisor sent: {captured}"
        )
        raise AssertionError(message)
