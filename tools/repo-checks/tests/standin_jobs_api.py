"""GitHub's jobs API, served over real HTTP on loopback from a recorded document.

`repo_checks gate_time_budget` reads the current run's jobs from
`GET {GITHUB_API_URL}/repos/{owner}/{repo}/actions/runs/{id}/attempts/{n}/jobs`,
paging by `per_page` and `page` as the forge does. This answers that path from
one jobs-API document a suite hands it — a recorded one under `recorded/`, or
one derived from it — and 404s every other path, so a command pointed at the
wrong run reads nothing. It records each path asked, so a suite can state what
was read.
"""

from __future__ import annotations

import json
import threading
import urllib.parse
from collections.abc import Iterator, Mapping
from contextlib import contextmanager
from dataclasses import dataclass, field
from http.server import BaseHTTPRequestHandler
from typing import Any

from repo_checks.loopback import LoopbackServer


@dataclass(slots=True)
class Jobs:
    """One run attempt's jobs document, and every path asked of the stand-in."""

    repository: str
    run_id: int
    attempt: int
    #: The document as the forge answers it: `total_count` and `jobs`.
    document: Mapping[str, Any]
    #: A body answered in place of the document, for a forge answering garbage.
    raw: bytes | None = None
    #: A status answered in place of the document, for a forge refusing the read.
    status: int = 200
    asked: list[str] = field(default_factory=list)
    #: The `Authorization` header of each request, or none where it carried none.
    authorized: list[str | None] = field(default_factory=list)


@contextmanager
def serving(jobs: Jobs) -> Iterator[str]:
    """Answer `jobs` on loopback, yielding the address to hand as `GITHUB_API_URL`."""
    path = f"/repos/{jobs.repository}/actions/runs/{jobs.run_id}/attempts/{jobs.attempt}/jobs"

    class Handler(BaseHTTPRequestHandler):
        def do_GET(self) -> None:
            jobs.asked.append(self.path)
            jobs.authorized.append(self.headers.get("Authorization"))
            parsed = urllib.parse.urlsplit(self.path)
            if parsed.path != path:
                self.send_error(404)
                return
            if jobs.status != 200:
                self.send_error(jobs.status)
                return
            query = urllib.parse.parse_qs(parsed.query)
            asked = (query.get("per_page", ["30"])[0], query.get("page", ["1"])[0])
            if not all(value.isdigit() and int(value) > 0 for value in asked):
                self.send_error(400, "per_page and page are positive whole numbers")
                return
            per_page, page = (int(value) for value in asked)
            listed = list(jobs.document.get("jobs", []))
            answer = {**jobs.document, "jobs": listed[(page - 1) * per_page : page * per_page]}
            body = jobs.raw if jobs.raw is not None else json.dumps(answer).encode("utf-8")
            self.send_response(200)
            self.send_header("Content-Type", "application/json")
            self.send_header("Content-Length", str(len(body)))
            self.end_headers()
            self.wfile.write(body)

        def log_message(self, format: str, *args: object) -> None:
            """Say nothing: the suite's own assertions are the signal."""

    server = LoopbackServer(Handler)
    thread = threading.Thread(target=server.serve_forever, daemon=True)
    thread.start()
    try:
        yield server.url
    finally:
        server.shutdown()
        server.server_close()
