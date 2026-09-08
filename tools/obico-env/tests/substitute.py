"""A stand-in for the Obico stack, so the tier itself can be driven end to end.

The tier's own claim is that the body it compares is the body it captured. A
suite that fed an altered body straight to the comparator would prove nothing
about that claim — a tier that captured the real payload and compared something
else would pass it. So this stand-in takes the place of the *stack*: it is asked
to post, exactly as the real stack is asked to raise an alert, and every
alteration therefore travels through the tier's own capture before anything
compares it.

It stands in for the two things the stack does and nothing else: it posts a body
to the webhook address, and it serves the image that body points at.
"""

from __future__ import annotations

import json
import threading
import urllib.request
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from typing import Any

CONTROL_PATH = "/control/alert"
IMAGE_PATH = "/snapshot.jpg"


class Substitute:
    """An HTTP server that posts what it is told to, and serves the image it points at."""

    def __init__(self, webhook_url: str, image: bytes) -> None:
        """Take the address to post to and the image bytes to serve."""
        self.webhook_url = webhook_url
        self.image = image
        self.body: bytes | None = b"{}"
        self.posted: list[int] = []
        stand_in = self

        class Handler(BaseHTTPRequestHandler):
            """The stack's two behaviours, and no others."""

            def do_POST(self) -> None:
                """Post the prepared body to the webhook, as the plugin would."""
                if self.path != CONTROL_PATH:
                    self._answer(404, b"no", "text/plain")
                    return
                # A stand-in told to post nothing answers as a stack that took
                # the request and raised no alert, which is what the tier's own
                # timeout is for.
                status = stand_in.post_now() if stand_in.body is not None else 0
                self._answer(200, json.dumps({"posted": status}).encode(), "application/json")

            def do_GET(self) -> None:
                """Serve the image the posted body points at."""
                if self.path.split("?")[0] != IMAGE_PATH:
                    self._answer(404, b"no", "text/plain")
                    return
                self._answer(200, stand_in.image, "image/jpeg")

            def _answer(self, status: int, body: bytes, content_type: str) -> None:
                """One answer, with its length, so no client waits for the close."""
                self.send_response(status)
                self.send_header("Content-Type", content_type)
                self.send_header("Content-Length", str(len(body)))
                self.end_headers()
                self.wfile.write(body)

            def log_message(self, format: str, *args: object) -> None:
                """Say nothing: the journey's own assertions are the signal."""

        self.server = ThreadingHTTPServer(("127.0.0.1", 0), Handler)
        self.port = int(self.server.server_address[1])
        self.thread = threading.Thread(target=self.server.serve_forever, daemon=True)

    def __enter__(self) -> Substitute:
        """Start answering."""
        self.thread.start()
        return self

    def __exit__(self, *_: object) -> None:
        """Stop answering, leaving no thread behind."""
        self.server.shutdown()
        self.server.server_close()
        self.thread.join(timeout=10)

    @property
    def base(self) -> str:
        """Where this stand-in answers."""
        return f"http://127.0.0.1:{self.port}"

    @property
    def control_url(self) -> str:
        """The address the tier asks it to post at, in place of causing a real alert."""
        return f"{self.base}{CONTROL_PATH}"

    @property
    def image_url(self) -> str:
        """The address it serves the image at, which a posted body points at."""
        return f"{self.base}{IMAGE_PATH}"

    def will_post(self, body: dict[str, Any] | None) -> None:
        """Set the body it posts when it is next asked, or nothing to post nothing."""
        self.body = None if body is None else json.dumps(body).encode("utf-8")

    def post_now(self) -> int:
        """Post the prepared body to the webhook address, as Obico's plugin does."""
        request = urllib.request.Request(  # noqa: S310
            self.webhook_url,
            data=self.body,
            method="POST",
            headers={"Content-Type": "application/json"},
        )
        with urllib.request.urlopen(request, timeout=30) as answer:  # noqa: S310
            self.posted.append(answer.status)
            return int(answer.status)
