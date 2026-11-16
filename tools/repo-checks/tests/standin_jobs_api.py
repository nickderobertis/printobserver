"""GitHub's Actions API, served over real HTTP on loopback from recorded documents.

`repo_checks ci_budgets` reads three things from `{GITHUB_API_URL}/repos/{owner}/{repo}/actions/`:
the current run (`runs/{id}`), the runs of its commit and event (`runs?head_sha=…&event=…`,
paged by `per_page` and `page`), and each run's jobs
(`runs/{id}/attempts/{n}/jobs`, paged the same way). This answers those paths
from the runs a suite hands it — each a recorded jobs document under
`recorded/`, or one derived from it — and 404s every other path, so a command
pointed at the wrong run reads nothing. It records each path asked, and the
`Authorization` header each request carried, so a suite can state what was read
and with what.
"""

from __future__ import annotations

import copy
import json
import threading
import urllib.parse
from collections.abc import Iterator, Mapping, Sequence
from contextlib import contextmanager
from dataclasses import dataclass, field
from datetime import datetime, timedelta
from http.server import BaseHTTPRequestHandler
from typing import Any

from repo_checks.loopback import LoopbackServer


@dataclass(slots=True)
class Run:
    """One workflow run as the forge records it, and its attempt's jobs document."""

    run_id: int
    attempt: int
    head_sha: str
    event: str
    workflow: str
    #: The jobs document as the forge answers it: `total_count` and `jobs`.
    document: Mapping[str, Any]


@dataclass(slots=True)
class Forge:
    """The runs a stand-in answers for one repository, and every request asked of it."""

    repository: str
    runs: Sequence[Run]
    #: A status answered for every path, in place of the documents.
    status: int = 200
    #: A body answered for every jobs path, in place of the document.
    raw: bytes | None = None
    asked: list[str] = field(default_factory=list)
    #: The `Authorization` header of each request, or none where it carried none.
    authorized: list[str | None] = field(default_factory=list)


#: The fields of a job or a step that carry a time the forge recorded.
TIMES = ("created_at", "started_at", "completed_at")
#: How the forge spells those times.
SPELLING = "%Y-%m-%dT%H:%M:%SZ"


def slowed(document: Mapping[str, Any], job: str, seconds: int) -> dict[str, Any]:
    """`document` as the forge would answer it had job `job` run `seconds` longer.

    Every time that job recorded after it started, and every time any other
    job recorded from that job's end on, moves `seconds` later: the job's own
    span and every job waiting on it grow by exactly that much, a job that had
    ended before it does not move, and the order of what the run recorded is
    kept.
    """
    derived = copy.deepcopy(dict(document))
    (slow,) = (entry for entry in derived["jobs"] if entry["name"] == job)
    started = datetime.strptime(slow["started_at"], SPELLING)
    ended = datetime.strptime(slow["completed_at"], SPELLING)
    for entry in derived["jobs"]:
        after = (lambda at: at > started) if entry is slow else (lambda at: at >= ended)
        for record in (entry, *entry.get("steps", [])):
            for name in TIMES:
                value = record.get(name)
                if value is None:
                    continue
                at = datetime.strptime(value, SPELLING)
                if after(at):
                    record[name] = (at + timedelta(seconds=seconds)).strftime(SPELLING)
    return derived


def _paged(entries: list[Any], query: Mapping[str, list[str]]) -> list[Any] | None:
    """One page of `entries` as `per_page` and `page` ask, or none for a malformed ask."""
    asked = (query.get("per_page", ["30"])[0], query.get("page", ["1"])[0])
    if not all(value.isdigit() and int(value) > 0 for value in asked):
        return None
    per_page, page = (int(value) for value in asked)
    return entries[(page - 1) * per_page : page * per_page]


@contextmanager
def serving(forge: Forge) -> Iterator[str]:
    """Answer `forge` on loopback, yielding the address to hand as `GITHUB_API_URL`."""
    base = f"/repos/{forge.repository}/actions"

    class Handler(BaseHTTPRequestHandler):
        def answer(self, document: object, raw: bytes | None = None) -> None:
            body = raw if raw is not None else json.dumps(document).encode("utf-8")
            self.send_response(200)
            self.send_header("Content-Type", "application/json")
            self.send_header("Content-Length", str(len(body)))
            self.end_headers()
            self.wfile.write(body)

        def do_GET(self) -> None:
            forge.asked.append(self.path)
            forge.authorized.append(self.headers.get("Authorization"))
            parsed = urllib.parse.urlsplit(self.path)
            query = urllib.parse.parse_qs(parsed.query)
            if forge.status != 200:
                self.send_error(forge.status)
                return
            if parsed.path == f"{base}/runs":
                matching = [
                    {
                        "id": run.run_id,
                        "run_attempt": run.attempt,
                        "head_sha": run.head_sha,
                        "event": run.event,
                        "name": run.workflow,
                    }
                    for run in forge.runs
                    if run.head_sha == query.get("head_sha", [""])[0]
                    and run.event == query.get("event", [""])[0]
                ]
                page = _paged(matching, query)
                if page is None:
                    self.send_error(400, "per_page and page are positive whole numbers")
                    return
                self.answer({"total_count": len(matching), "workflow_runs": page})
                return
            for run in forge.runs:
                if parsed.path == f"{base}/runs/{run.run_id}":
                    self.answer(
                        {
                            "id": run.run_id,
                            "run_attempt": run.attempt,
                            "head_sha": run.head_sha,
                            "event": run.event,
                            "name": run.workflow,
                        }
                    )
                    return
                if parsed.path == f"{base}/runs/{run.run_id}/attempts/{run.attempt}/jobs":
                    page = _paged(list(run.document.get("jobs", [])), query)
                    if page is None:
                        self.send_error(400, "per_page and page are positive whole numbers")
                        return
                    self.answer({**run.document, "jobs": page}, forge.raw)
                    return
            self.send_error(404)

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
