"""A stand-in for the part of GitHub's REST API the release workflow's tracking issue uses.

Nothing in this repository may open an issue on the real forge to prove a
point, so `tracking` is proven against this: one real HTTP server answering
the requests it makes — listing a repository's open issues by label, reading
and creating a label, opening an issue, commenting on one and closing it — in
GitHub's own paths and shapes, under one repository and one token. What it
holds is kept between requests, so a sequence of workflow runs driven against
one stand-in leaves the issue each run left; every request is recorded with
the credential it carried, and a caller can make any one path refused with a
status and a body of its own choosing.

Like the forge, it lists a pull request among a repository's issues, marked by
a `pull_request` field, and a caller can seed one carrying the label.
"""

from __future__ import annotations

import json
import threading
from dataclasses import dataclass, field
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from typing import Self
from urllib.parse import parse_qs, unquote, urlsplit

#: The most this reads of one request body; every document `tracking` sends
#: is a few hundred bytes.
MAX_BODY_BYTES = 1024 * 1024


@dataclass(slots=True)
class Issue:
    """One issue the stand-in holds, as the forge would show it."""

    number: int
    title: str
    body: str
    labels: list[str]
    state: str = "open"
    state_reason: str | None = None
    comments: list[str] = field(default_factory=list)
    pull_request: bool = False

    def document(self, html: str) -> dict[str, object]:
        """The issue in the forge's shape."""
        shown: dict[str, object] = {
            "number": self.number,
            "title": self.title,
            "body": self.body,
            "state": self.state,
            "state_reason": self.state_reason,
            "labels": [{"name": name} for name in self.labels],
            "html_url": f"{html}/issues/{self.number}",
            "comments": len(self.comments),
        }
        if self.pull_request:
            shown["pull_request"] = {"html_url": f"{html}/pull/{self.number}"}
        return shown


@dataclass(frozen=True, slots=True)
class Asked:
    """One request the stand-in took: its method, path and query, and its credential."""

    method: str
    path: str
    token: str


class IssuesForge:
    """GitHub's issues API for one repository, on a port the operating system chooses."""

    def __init__(self, repository: str, token: str) -> None:
        """Start answering for `repository` (`owner/name`), under `token` alone."""
        self.repository, self.token = repository, token
        self.issues: dict[int, Issue] = {}
        self.labels: set[str] = set()
        self.asked: list[Asked] = []
        self._refused: dict[tuple[str, str], tuple[int, str]] = {}
        self._garbled: set[tuple[str, str]] = set()
        self._lock = threading.Lock()
        self._server = ThreadingHTTPServer(("127.0.0.1", 0), _handler(self))
        self._serving = threading.Thread(target=self._server.serve_forever, daemon=True)
        self._serving.start()

    @property
    def api(self) -> str:
        """The API address a client is pointed at, as `GITHUB_API_URL` names one."""
        host, port = self._server.server_address[:2]
        return f"http://{host}:{port}"

    @property
    def html(self) -> str:
        """The repository's address on the forge, which every document links to."""
        return f"https://github.com/{self.repository}"

    def seed(self, title: str, labels: list[str], *, pull_request: bool = False) -> int:
        """Hold an open issue or pull request before anything asks, answering its number."""
        with self._lock:
            number = len(self.issues) + 1
            self.issues[number] = Issue(number, title, "", list(labels), pull_request=pull_request)
            self.labels |= set(labels)
            return number

    def refuse(self, method: str, path: str, status: int, body: str) -> None:
        """Answer every `method` request to `path` with `status` and `body` from now on."""
        self._refused[(method, path)] = (status, body)

    def garble(self, method: str, path: str) -> None:
        """Answer every `method` request to `path` with a body that is not JSON from now on."""
        self._garbled.add((method, path))

    def open_labelled(self, label: str) -> list[Issue]:
        """Every open issue carrying `label`, oldest first."""
        return [
            issue
            for _, issue in sorted(self.issues.items())
            if issue.state == "open" and label in issue.labels and not issue.pull_request
        ]

    def stop(self) -> None:
        """Stop answering."""
        self._server.shutdown()
        self._server.server_close()

    def __enter__(self) -> Self:
        """Serve for the duration of a `with` block."""
        return self

    def __exit__(self, *_: object) -> None:
        """Stop serving when the block ends."""
        self.stop()

    def answer(self, method: str, target: str, document: dict[str, object]) -> tuple[int, object]:
        """GitHub's status and document for one authenticated request."""
        split = urlsplit(target)
        path = unquote(split.path)
        refused = self._refused.get((method, path))
        if refused is not None:
            return refused[0], {"message": refused[1]}
        prefix = f"/repos/{self.repository}"
        if not path.startswith(f"{prefix}/"):
            return 404, {"message": "Not Found"}
        rest = path.removeprefix(prefix).strip("/").split("/")
        with self._lock:
            return self._route(method, rest, parse_qs(split.query), document)

    def _route(
        self,
        method: str,
        rest: list[str],
        query: dict[str, list[str]],
        document: dict[str, object],
    ) -> tuple[int, object]:
        match (method, rest):
            case ("GET", ["issues"]):
                return 200, self._listed(query)
            case ("POST", ["issues"]):
                return self._opened(document)
            case ("POST", ["issues", number, "comments"]) if self._held(number):
                body = str(document.get("body", ""))
                self.issues[int(number)].comments.append(body)
                return 201, {"id": len(self.issues[int(number)].comments), "body": body}
            case ("PATCH", ["issues", number]) if self._held(number):
                issue = self.issues[int(number)]
                issue.state = str(document.get("state", issue.state))
                reason = document.get("state_reason")
                issue.state_reason = str(reason) if reason is not None else None
                return 200, issue.document(self.html)
            case ("GET", ["labels", name]) if name in self.labels:
                return 200, {"name": name}
            case ("POST", ["labels"]):
                name = str(document.get("name", ""))
                if not name or name in self.labels:
                    return 422, {"message": "Validation Failed"}
                self.labels.add(name)
                return 201, {"name": name, "color": document.get("color")}
            case _:
                return 404, {"message": "Not Found"}

    def _held(self, number: str) -> bool:
        return number.isascii() and number.isdigit() and int(number) in self.issues

    def _listed(self, query: dict[str, list[str]]) -> list[dict[str, object]]:
        wanted = {name for value in query.get("labels", []) for name in value.split(",") if name}
        state = query.get("state", ["open"])[0]
        return [
            issue.document(self.html)
            for _, issue in sorted(self.issues.items())
            if (state == "all" or issue.state == state) and wanted <= set(issue.labels)
        ]

    def _opened(self, document: dict[str, object]) -> tuple[int, object]:
        title = document.get("title")
        labels = document.get("labels", [])
        if not isinstance(title, str) or not title or not isinstance(labels, list):
            return 422, {"message": "Validation Failed"}
        number = len(self.issues) + 1
        named = [str(label) for label in labels]
        self.labels |= set(named)
        self.issues[number] = Issue(number, title, str(document.get("body", "")), named)
        return 201, self.issues[number].document(self.html)


def _handler(forge: IssuesForge) -> type[BaseHTTPRequestHandler]:
    class Handler(BaseHTTPRequestHandler):
        """Answer each request as `IssuesForge.answer` says, under its token."""

        protocol_version = "HTTP/1.1"

        def _document(self) -> dict[str, object] | None:
            """The request's JSON object, `{}` for none, or `None` where it is not one."""
            declared = self.headers.get("Content-Length") or "0"
            if not (declared.isascii() and declared.isdigit()) or int(declared) > MAX_BODY_BYTES:
                return None
            raw = self.rfile.read(int(declared))
            if not raw:
                return {}
            try:
                parsed = json.loads(raw)
            except ValueError:
                return None
            return parsed if isinstance(parsed, dict) else None

        def _any(self) -> None:
            carried = self.headers.get("Authorization", "").removeprefix("Bearer ")
            forge.asked.append(Asked(self.command, self.path, carried))
            document = self._document()
            if carried != forge.token:
                code, answered = 401, {"message": "Bad credentials"}
            elif document is None:
                code, answered = 400, {"message": "Problems parsing JSON"}
            else:
                code, answered = forge.answer(self.command, self.path, document)
            garbled = (self.command, unquote(urlsplit(self.path).path)) in forge._garbled
            payload = b"<html>not JSON</html>" if garbled else json.dumps(answered).encode()
            self.send_response(code)
            self.send_header("Content-Type", "application/json")
            self.send_header("Content-Length", str(len(payload)))
            self.end_headers()
            self.wfile.write(payload)

        def do_GET(self) -> None:
            """Answer one read."""
            self._any()

        def do_POST(self) -> None:
            """Answer one creation."""
            self._any()

        def do_PATCH(self) -> None:
            """Answer one update."""
            self._any()

        def log_message(self, format: str, *args: object) -> None:
            """Say nothing: a stand-in whose log is the output is not signal."""

    return Handler
