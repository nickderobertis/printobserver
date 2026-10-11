"""A stand-in for the part of GitHub's REST API the release workflow's tracking issue uses.

Nothing in this repository may open an issue on the real forge to prove a
point, so `tracking` is proven against this: one real HTTP server answering
exactly the operations `github_issues` declares — which that module's drift
gate holds to GitHub's own published description — with the documents and
refusals it declares, under one repository and one token. A request whose
document carries a field its operation does not declare, or one of another
type, is refused as the forge refuses it. What it holds is kept between
requests, so a sequence of workflow runs driven against one stand-in leaves
the issue each run left; every request is recorded with the credential it
carried, and a caller can make any one path refused with a status and a body
of its own choosing.

Like the forge, it lists a pull request among a repository's issues, marked by
a `pull_request` field, and a caller can seed one carrying the label.
"""

from __future__ import annotations

import json
import threading
from collections.abc import Callable
from dataclasses import dataclass, field
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from typing import Self
from urllib.parse import parse_qs, unquote, urlsplit

from release_artifacts.github_issues import (
    CREATE_COMMENT,
    CREATE_ISSUE,
    CREATE_LABEL,
    GET_LABEL,
    HEXADECIMAL_DIGITS,
    INVALID,
    ISSUE,
    LIST_ISSUES,
    LISTED_STATES,
    NOT_FOUND,
    OPERATIONS,
    PER_PAGE_DEFAULT,
    PER_PAGE_MAX,
    UPDATE_ISSUE,
    IssueNumber,
    Operation,
    Reason,
    State,
    conforms,
)
from release_artifacts.registries import Token

#: The most this reads of one request body; every document `tracking` sends
#: is a few hundred bytes.
MAX_BODY_BYTES = 1024 * 1024

#: One answer: a status, and the document sent with it.
Answer = tuple[int, object]

#: What the forge answers a request whose document it will not take, and one
#: naming nothing it serves.
REFUSED_INVALID: Answer = (INVALID, {"message": "Validation Failed"})
REFUSED_ABSENT: Answer = (NOT_FOUND, {"message": "Not Found"})


@dataclass(slots=True)
class Issue:
    """One issue the stand-in holds, as the forge would show it."""

    number: IssueNumber
    title: str
    body: str
    labels: list[str]
    state: State = State.OPEN
    state_reason: Reason | None = None
    comments: list[str] = field(default_factory=list)
    pull_request: bool = False

    def document(self, html: str) -> dict[str, object]:
        """The issue in the forge's shape: the fields `github_issues.ISSUE` declares."""
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
        return {name: value for name, value in shown.items() if name in ISSUE}


@dataclass(frozen=True, slots=True)
class Asked:
    """One request the stand-in took: its method, path and query, and its credential."""

    method: str
    path: str
    token: Token


#: What answers one operation: the stand-in, the path's values, the query and
#: the document sent.
Handler = Callable[
    ["IssuesForge", dict[str, str], dict[str, list[str]], dict[str, object]], "Answer"
]


class IssuesForge:
    """GitHub's issues API for one repository, on a port the operating system chooses."""

    def __init__(self, repository: str, token: Token) -> None:
        """Start answering for `repository` (`owner/name`), under `token` alone."""
        self.repository, self.token = repository, token
        self.issues: dict[IssueNumber, Issue] = {}
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

    def seed(self, title: str, labels: list[str], *, pull_request: bool = False) -> IssueNumber:
        """Hold an open issue or pull request before anything asks, answering its number."""
        with self._lock:
            number = IssueNumber(len(self.issues) + 1)
            self.issues[number] = Issue(number, title, "", list(labels), pull_request=pull_request)
            self.labels |= set(labels)
            return number

    def refuse(self, method: str, path: str, status: int, body: str) -> None:
        """Answer every `method` request to `path` with `status` and `body` from now on."""
        self._refused[(method, path)] = (status, body)

    def accept(self, method: str, path: str) -> None:
        """Answer `method` requests to `path` as the forge does again."""
        self._refused.pop((method, path), None)

    def garble(self, method: str, path: str) -> None:
        """Answer every `method` request to `path` with a body that is not JSON from now on."""
        self._garbled.add((method, path))

    def garbled(self, method: str, path: str) -> bool:
        """Whether `method` requests to `path` are answered with a body that is not JSON."""
        return (method, path) in self._garbled

    def open_labelled(self, label: str) -> list[Issue]:
        """Every open issue carrying `label`, oldest first."""
        return [
            issue
            for _, issue in sorted(self.issues.items())
            if issue.state is State.OPEN and label in issue.labels and not issue.pull_request
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

    def answer(self, method: str, target: str, document: dict[str, object]) -> Answer:
        """GitHub's status and document for one authenticated request.

        The operation is the one of `github_issues.OPERATIONS` whose method and
        path the request names, for this repository; anything else is `404`,
        and a document carrying a field the operation does not declare, or one
        of another type, is `422`.
        """
        split = urlsplit(target)
        path = unquote(split.path)
        refused = self._refused.get((method, path))
        if refused is not None:
            return refused[0], {"message": refused[1]}
        owner, name = self.repository.split("/", 1)
        for operation in OPERATIONS:
            values = operation.matched(method, path)
            if values is None or (values["owner"], values["repo"]) != (owner, name):
                continue
            if not conforms(document, operation):
                return REFUSED_INVALID
            with self._lock:
                return HANDLERS[operation](self, values, parse_qs(split.query), document)
        return REFUSED_ABSENT

    def _held(self, values: dict[str, str]) -> Issue | None:
        number = values.get("issue_number", "")
        if not (number.isascii() and number.isdigit()):
            return None
        return self.issues.get(IssueNumber(int(number)))

    def _listed(
        self, _: dict[str, str], query: dict[str, list[str]], __: dict[str, object]
    ) -> Answer:
        """One page of the issues carrying every label asked for, in the state asked for."""
        wanted = {name for value in query.get("labels", []) for name in value.split(",") if name}
        state = query.get("state", [State.OPEN.value])[0]
        size = query.get("per_page", [str(PER_PAGE_DEFAULT)])[0]
        page = query.get("page", ["1"])[0]
        if state not in LISTED_STATES or not all(
            value.isascii() and value.isdigit() and int(value) > 0 for value in (size, page)
        ):
            return REFUSED_INVALID
        size_of, page_of = min(int(size), PER_PAGE_MAX), int(page)
        matching = [
            issue.document(self.html)
            for _, issue in sorted(self.issues.items())
            if state in (issue.state, "all") and wanted <= set(issue.labels)
        ]
        return LIST_ISSUES.answers, matching[(page_of - 1) * size_of : page_of * size_of]

    def _label(
        self, values: dict[str, str], _: dict[str, list[str]], __: dict[str, object]
    ) -> Answer:
        """One label, by name."""
        if values["name"] not in self.labels:
            return REFUSED_ABSENT
        return GET_LABEL.answers, {"name": values["name"]}

    def _labelled(
        self, _: dict[str, str], __: dict[str, list[str]], document: dict[str, object]
    ) -> Answer:
        """Create a label from a name not yet taken and a colour in the forge's spelling."""
        name, colour = document.get("name"), document.get("color")
        if not isinstance(name, str) or not name or name in self.labels:
            return REFUSED_INVALID
        if not isinstance(colour, str) or not _hexadecimal(colour):
            return REFUSED_INVALID
        self.labels.add(name)
        return CREATE_LABEL.answers, {"name": name, "color": colour}

    def _opened(
        self, _: dict[str, str], __: dict[str, list[str]], document: dict[str, object]
    ) -> Answer:
        """Open an issue from a document carrying a title, a body and a list of label names."""
        title, body, labels = (document.get(key) for key in ("title", "body", "labels"))
        if not isinstance(title, str) or not title or not isinstance(body, str):
            return REFUSED_INVALID
        if not isinstance(labels, list) or not all(isinstance(name, str) for name in labels):
            return REFUSED_INVALID
        number = IssueNumber(len(self.issues) + 1)
        self.labels |= set(labels)
        self.issues[number] = Issue(number, title, body, list(labels))
        return CREATE_ISSUE.answers, self.issues[number].document(self.html)

    def _commented(
        self, values: dict[str, str], _: dict[str, list[str]], document: dict[str, object]
    ) -> Answer:
        """Leave a comment on an issue it holds."""
        issue = self._held(values)
        if issue is None:
            return REFUSED_ABSENT
        body = document.get("body")
        if not isinstance(body, str) or not body:
            return REFUSED_INVALID
        issue.comments.append(body)
        return CREATE_COMMENT.answers, {"id": len(issue.comments), "body": body}

    def _updated(
        self, values: dict[str, str], _: dict[str, list[str]], document: dict[str, object]
    ) -> Answer:
        """Move an issue it holds to the state, and for the reason, a document names."""
        issue = self._held(values)
        if issue is None:
            return REFUSED_ABSENT
        state, reason = document.get("state"), document.get("state_reason")
        if state not in {value.value for value in State}:
            return REFUSED_INVALID
        if reason is not None and reason not in {value.value for value in Reason}:
            return REFUSED_INVALID
        issue.state = State(str(state))
        issue.state_reason = Reason(str(reason)) if reason is not None else None
        return UPDATE_ISSUE.answers, issue.document(self.html)


#: The answer to each operation `github_issues` declares, and to no other.
HANDLERS: dict[Operation, Handler] = {
    LIST_ISSUES: IssuesForge._listed,
    GET_LABEL: IssuesForge._label,
    CREATE_LABEL: IssuesForge._labelled,
    CREATE_ISSUE: IssuesForge._opened,
    CREATE_COMMENT: IssuesForge._commented,
    UPDATE_ISSUE: IssuesForge._updated,
}


def _hexadecimal(colour: str) -> bool:
    """Whether `colour` is written in the hexadecimal digits the forge takes a colour in."""
    return bool(colour) and set(colour) <= HEXADECIMAL_DIGITS


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
            carried = Token(self.headers.get("Authorization", "").removeprefix("Bearer "))
            forge.asked.append(Asked(self.command, self.path, carried))
            document = self._document()
            if carried != forge.token:
                code, answered = 401, {"message": "Bad credentials"}
            elif document is None:
                code, answered = 400, {"message": "Problems parsing JSON"}
            else:
                code, answered = forge.answer(self.command, self.path, document)
            garbled = forge.garbled(self.command, unquote(urlsplit(self.path).path))
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
