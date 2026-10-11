"""The slice of GitHub's REST issues API the release workflow's tracking issue uses, declared once.

`tracking` asks the forge through these operations and no other, sending only
the fields each declares and reading only the fields each declares; and
`issues_standin` answers exactly these operations, with these documents and
these refusals. So this module is the one statement in the repository of what
that slice of GitHub's API is, and both sides are built from it.

It is held to GitHub's own published description of its REST API —
`github/rest-api-description`, at `DESCRIPTION_COMMIT`, the file
`DESCRIPTION_FILE`, verified against `DESCRIPTION_SHA256` before it is read —
by `tools/release-artifacts/tests/test_github_issues_contract.py`, which
fails naming any operation, query parameter, request field, response field,
status or enumerated value declared here that the description does not carry
with a compatible type. Moving the pin is how a change of GitHub's API reaches
this repository.
"""

from __future__ import annotations

import re
from collections.abc import Mapping
from dataclasses import dataclass, field
from enum import StrEnum
from typing import NewType
from urllib.parse import quote

#: An issue's number on the forge: what it is commented on and closed by.
IssueNumber = NewType("IssueNumber", int)

#: The revision of `github/rest-api-description` this slice is held to, and
#: the description in it: the REST API at version `2022-11-28`, which is the
#: version GitHub answers a request naming none — as `tracking`'s do.
DESCRIPTION_COMMIT = "58b1e0c00b39b9e46c24dc4a0ba6a2669c90e9c1"
DESCRIPTION_FILE = "descriptions/api.github.com/api.github.com.2022-11-28.json"
DESCRIPTION_SHA256 = "b7ded4ae746e70e0c196927534cbfd77a4ecf125bb33f0298a9d218a20e3fc57"
DESCRIPTION_URL = (
    f"https://github.com/github/rest-api-description/raw/{DESCRIPTION_COMMIT}/{DESCRIPTION_FILE}"
)


class JsonType(StrEnum):
    """The JSON type of one field, as the description's `type` spells it."""

    STRING = "string"
    INTEGER = "integer"
    ARRAY = "array"
    OBJECT = "object"


#: The statuses the client acts on rather than failing: a label the
#: repository does not carry, and a document the forge would not take — which,
#: for a label, includes one a concurrent run has just created.
NOT_FOUND, INVALID = 404, 422

#: A template's `{name}`, which a path names a value of.
PLACEHOLDER = re.compile(r"\{([a-z_]+)\}")


@dataclass(frozen=True, slots=True, eq=False)
class Operation:
    """One request of the forge's REST API: its method, path, and the fields on each side.

    `query` is every query parameter it is asked with; `sends`, every field of
    the document it is sent; `reads`, every field read off the document it
    answers — of each entry, where the answer is a list. `answers` is the
    status it succeeds with, and `refusals` the statuses a caller acts on.
    `nullable` names each field it sends that may be sent as `null`.
    Each is one of the constants below and is compared as itself, which is
    what lets the stand-in key its answers by operation.
    """

    method: str
    path: str
    answers: int
    query: tuple[str, ...] = ()
    sends: Mapping[str, JsonType] = field(default_factory=dict)
    reads: Mapping[str, JsonType] = field(default_factory=dict)
    refusals: tuple[int, ...] = ()
    nullable: frozenset[str] = frozenset()

    def target(self, values: Mapping[str, str]) -> str:
        """The path with each `{name}` replaced by its value, quoted as one segment."""
        return PLACEHOLDER.sub(lambda match: quote(values[match.group(1)], safe=""), self.path)

    def matched(self, method: str, path: str) -> dict[str, str] | None:
        """The value each `{name}` takes in `path`, or `None` where this is not its operation."""
        if method != self.method:
            return None
        pattern = PLACEHOLDER.sub(lambda match: f"(?P<{match.group(1)}>[^/]+)", self.path)
        found = re.fullmatch(pattern, path)
        return found.groupdict() if found else None


#: The open issues of a repository carrying a label, one page at a time.
LIST_ISSUES = Operation(
    "GET",
    "/repos/{owner}/{repo}/issues",
    200,
    query=("labels", "state", "per_page", "page"),
    reads={"number": JsonType.INTEGER, "pull_request": JsonType.OBJECT},
)

#: One label of a repository, by name.
GET_LABEL = Operation(
    "GET",
    "/repos/{owner}/{repo}/labels/{name}",
    200,
    reads={"name": JsonType.STRING},
    refusals=(NOT_FOUND,),
)

#: A label created in a repository.
CREATE_LABEL = Operation(
    "POST",
    "/repos/{owner}/{repo}/labels",
    201,
    sends={"name": JsonType.STRING, "color": JsonType.STRING, "description": JsonType.STRING},
    reads={"name": JsonType.STRING, "color": JsonType.STRING},
    refusals=(INVALID,),
)

#: An issue opened in a repository.
CREATE_ISSUE = Operation(
    "POST",
    "/repos/{owner}/{repo}/issues",
    201,
    sends={"title": JsonType.STRING, "body": JsonType.STRING, "labels": JsonType.ARRAY},
    reads={"number": JsonType.INTEGER},
    refusals=(INVALID,),
)

#: A comment left on an issue.
CREATE_COMMENT = Operation(
    "POST",
    "/repos/{owner}/{repo}/issues/{issue_number}/comments",
    201,
    sends={"body": JsonType.STRING},
    reads={"id": JsonType.INTEGER, "body": JsonType.STRING},
    refusals=(NOT_FOUND, INVALID),
)

#: An issue moved to a state, for a reason.
UPDATE_ISSUE = Operation(
    "PATCH",
    "/repos/{owner}/{repo}/issues/{issue_number}",
    200,
    sends={"state": JsonType.STRING, "state_reason": JsonType.STRING},
    refusals=(NOT_FOUND, INVALID),
    nullable=frozenset({"state_reason"}),
)

#: Every operation, which is every request `tracking` makes and every one the
#: stand-in answers.
OPERATIONS = (LIST_ISSUES, GET_LABEL, CREATE_LABEL, CREATE_ISSUE, CREATE_COMMENT, UPDATE_ISSUE)

#: The document the forge answers an issue with, by field, as far as anything
#: here writes or reads one.
ISSUE = {
    "number": JsonType.INTEGER,
    "title": JsonType.STRING,
    "body": JsonType.STRING,
    "state": JsonType.STRING,
    "state_reason": JsonType.STRING,
    "labels": JsonType.ARRAY,
    "html_url": JsonType.STRING,
    "comments": JsonType.INTEGER,
    "pull_request": JsonType.OBJECT,
}

#: The document the forge refuses a request with, by field.
REFUSAL = {"message": JsonType.STRING}


class State(StrEnum):
    """Whether an issue is open, as `state` says on an issue and on its update."""

    OPEN = "open"
    CLOSED = "closed"


class Reason(StrEnum):
    """Why an issue was closed or reopened, as `state_reason` says."""

    COMPLETED = "completed"
    NOT_PLANNED = "not_planned"
    REOPENED = "reopened"


#: The `state` a listing is asked for: either state, or both.
LISTED_STATES = ("open", "closed", "all")

#: The digits the description says a label's colour is written in, with no
#: leading `#`.
HEXADECIMAL_DIGITS = frozenset("0123456789abcdefABCDEF")

#: The listing's page: how many entries one holds unless asked otherwise, and
#: the most it holds however it is asked.
PER_PAGE_DEFAULT, PER_PAGE_MAX = 30, 100


def conforms(document: Mapping[str, object], operation: Operation) -> bool:
    """Whether every field of `document` is one `operation` sends, of the JSON type it declares.

    `null` is admitted for a field the operation declares `nullable`, and for
    no other; a boolean is not an integer.
    """
    fields = operation.sends
    shapes: dict[JsonType, type | tuple[type, ...]] = {
        JsonType.STRING: str,
        JsonType.INTEGER: int,
        JsonType.ARRAY: list,
        JsonType.OBJECT: dict,
    }
    for name, value in document.items():
        declared = fields.get(name)
        if declared is None:
            return False
        if value is None:
            if name not in operation.nullable:
                return False
            continue
        if isinstance(value, bool) or not isinstance(value, shapes[declared]):
            return False
    return True
