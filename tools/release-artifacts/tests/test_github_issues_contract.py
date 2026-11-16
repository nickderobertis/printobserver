"""The drift gate for `github_issues`: every part of it is in GitHub's own description.

`github_issues` declares the slice of GitHub's REST API the release workflow's
tracking issue uses, and `tracking` and `issues_standin` are both built from
it. Here that declaration is held to GitHub's published OpenAPI description of
its REST API — `github/rest-api-description` at the pinned revision, downloaded
from the forge and verified against the pinned SHA-256 before a byte of it is
read: every operation's method and path, every query parameter it is asked
with, every field it sends and reads with a compatible JSON type, every status
it succeeds or is refused with, the issue and refusal documents the stand-in
writes, and each enumerated value either side uses. One the description lacks
fails here, naming it.
"""

from __future__ import annotations

import hashlib
import json
from collections.abc import Iterator
from typing import Any

import pytest
from conftest import REPO_ROOT
from release_artifacts.github_issues import (
    CREATE_LABEL,
    DESCRIPTION_COMMIT,
    DESCRIPTION_FILE,
    DESCRIPTION_SHA256,
    DESCRIPTION_URL,
    HEXADECIMAL_DIGITS,
    ISSUE,
    LIST_ISSUES,
    LISTED_STATES,
    OPERATIONS,
    PER_PAGE_DEFAULT,
    PER_PAGE_MAX,
    REFUSAL,
    UPDATE_ISSUE,
    JsonType,
    Operation,
    Reason,
    State,
)
from release_artifacts.issues_standin import HANDLERS
from release_artifacts.tracking import MEDIA_TYPE, Outcome
from repo_checks.expect import contains, equal, truth
from repo_checks.verified_download import verified

#: One OpenAPI object as the JSON reader hands it back: its keys are the
#: description's own, so there is no narrower shape to read it as, and each
#: reader below narrows the one value it needs.
Described = dict[str, Any]

#: Where a verified copy is kept between runs: the recipes' own scratch space,
#: which `.gitignore` keeps out of the tree, named by the revision it is of.
CACHED = REPO_ROOT / ".just-tmp" / f"rest-api-description-{DESCRIPTION_COMMIT}.json"


@pytest.fixture(scope="module")
def description() -> Described:
    """GitHub's description at the pinned revision, verified against the pinned digest."""
    if CACHED.is_file() and hashlib.sha256(CACHED.read_bytes()).hexdigest() == DESCRIPTION_SHA256:
        payload = CACHED.read_bytes()
    else:
        # llmlint: ignore[async_typed_clients_at_boundaries] suppressions.toml has the reason.
        payload = verified(
            DESCRIPTION_URL,
            name=DESCRIPTION_FILE,
            expected=DESCRIPTION_SHA256,
            source="`github_issues.DESCRIPTION_SHA256`",
        )
        CACHED.parent.mkdir(parents=True, exist_ok=True)
        CACHED.write_bytes(payload)
    return json.loads(payload)


def resolved(description: Described, schema: object) -> Described:
    """A schema with every `$ref` it is followed to its target."""
    while isinstance(schema, dict) and "$ref" in schema:
        target: object = description
        for part in str(schema["$ref"]).removeprefix("#/").split("/"):
            target = target[part] if isinstance(target, dict) else None
        schema = target
    truth(isinstance(schema, dict), describing=f"a schema to resolve to an object: {schema!r}")
    return schema if isinstance(schema, dict) else {}


def alternatives(description: Described, schema: object) -> Iterator[Described]:
    """The schema and every `oneOf`, `anyOf` and `allOf` branch beneath it, resolved."""
    here = resolved(description, schema)
    yield here
    for key in ("oneOf", "anyOf", "allOf"):
        for branch in here.get(key, []):
            yield from alternatives(description, branch)


def types(description: Described, schema: object) -> set[str]:
    """Every JSON type a schema admits, across its branches."""
    found: set[str] = set()
    for branch in alternatives(description, schema):
        match branch.get("type"):
            case list() as several:
                found |= {str(kind) for kind in several}
            case str() as one:
                found.add(one)
            case _:
                pass
    return found


def properties(description: Described, schema: object) -> dict[str, object]:
    """Every property a schema's object carries, across its branches; an array's items'."""
    found: dict[str, object] = {}
    for branch in alternatives(description, schema):
        if "items" in branch:
            found |= properties(description, branch["items"])
        found |= branch.get("properties", {})
    return found


def operation_in(description: Described, operation: Operation) -> Described:
    """The description's own object for one operation, or a failure naming it."""
    paths = description["paths"]
    truth(operation.path in paths, describing=f"`{operation.path}` among the described paths")
    methods = paths[operation.path]
    method = operation.method.lower()
    truth(method in methods, describing=f"`{operation.method} {operation.path}` to be described")
    return methods[method]


def body_schema(description: Described, described: Described, status: str) -> object:
    """The JSON schema an operation answers with under `status`."""
    response = resolved(description, described["responses"][status])
    content = response.get("content", {})
    truth(
        MEDIA_TYPE in content,
        describing=f"`{status}` to answer `{MEDIA_TYPE}`, as the client asks: {sorted(content)}",
    )
    return content[MEDIA_TYPE]["schema"]


def compatible(
    description: Described, declared: dict[str, object], fields: dict[str, JsonType], where: str
) -> None:
    """Every field of `fields` is one `declared` carries, admitting the JSON type it names."""
    for name, kind in fields.items():
        truth(name in declared, describing=f"`{name}` among the fields {where}: {sorted(declared)}")
        admitted = types(description, declared[name])
        truth(
            kind.value in admitted,
            describing=f"`{name}` {where} to admit `{kind.value}`; it admits {sorted(admitted)}",
        )


def named(operation: Operation) -> str:
    """One operation, as a test id."""
    return f"{operation.method} {operation.path}"


@pytest.mark.parametrize("operation", OPERATIONS, ids=named)
def test_every_operation_and_its_query_is_described(
    description: Described, operation: Operation
) -> None:
    """The method, the path, and every query parameter it is asked with."""
    described = operation_in(description, operation)
    query = {
        resolved(description, parameter)["name"]
        for parameter in described.get("parameters", [])
        if resolved(description, parameter).get("in") == "query"
    }
    for parameter in operation.query:
        truth(parameter in query, describing=f"`{parameter}` among {named(operation)}'s query")


@pytest.mark.parametrize("operation", OPERATIONS, ids=named)
def test_every_field_an_operation_sends_and_reads_is_described_with_its_type(
    description: Described, operation: Operation
) -> None:
    """The request document's fields, and the answered document's, with their JSON types."""
    described = operation_in(description, operation)
    if operation.sends:
        request = resolved(description, described["requestBody"])
        schema = request["content"]["application/json"]["schema"]
        compatible(description, properties(description, schema), dict(operation.sends), "it sends")
    answered = body_schema(description, described, str(operation.answers))
    compatible(description, properties(description, answered), dict(operation.reads), "it reads")


@pytest.mark.parametrize("operation", OPERATIONS, ids=named)
def test_every_field_sent_as_null_is_one_the_description_admits_null_for(
    description: Described, operation: Operation
) -> None:
    """A `nullable` field is one the description marks `nullable`, or types as `null`."""
    if not operation.nullable:
        return
    request = resolved(description, operation_in(description, operation)["requestBody"])
    declared = properties(description, request["content"]["application/json"]["schema"])
    for name in operation.nullable:
        branches = list(alternatives(description, declared[name]))
        truth(
            any(branch.get("nullable") is True for branch in branches)
            or "null" in types(description, declared[name]),
            describing=f"`{name}` of {named(operation)} to admit `null`",
        )


@pytest.mark.parametrize("operation", OPERATIONS, ids=named)
def test_every_status_an_operation_is_answered_with_is_described(
    description: Described, operation: Operation
) -> None:
    """The status it succeeds with, and each refusal a caller acts on."""
    responses = operation_in(description, operation)["responses"]
    for status in (operation.answers, *operation.refusals):
        truth(str(status) in responses, describing=f"`{status}` among {sorted(responses)}")


def test_the_issue_document_the_stand_in_writes_is_the_described_issue(
    description: Described,
) -> None:
    """Every field of `ISSUE`, as the listing and the update answer an issue."""
    for operation in (LIST_ISSUES, UPDATE_ISSUE):
        schema = body_schema(description, operation_in(description, operation), "200")
        compatible(description, properties(description, schema), ISSUE, f"of {named(operation)}")


def test_the_refusal_document_is_the_described_error(description: Described) -> None:
    """The stand-in's `404` and `422` documents, as the description's errors carry them."""
    for error in ("basic-error", "validation-error"):
        schema = description["components"]["schemas"][error]
        compatible(description, properties(description, schema), REFUSAL, f"of `{error}`")


def test_every_enumerated_value_is_one_the_description_names(description: Described) -> None:
    """Issue states and reasons, the listing's states, its page, and a label's colour."""
    update = resolved(
        description,
        operation_in(description, UPDATE_ISSUE)["requestBody"]["content"]["application/json"][
            "schema"
        ],
    )["properties"]
    for value in State:
        contains(resolved(description, update["state"])["enum"], value.value, describing="state")
    for value in Reason:
        contains(
            resolved(description, update["state_reason"])["enum"], value.value, describing="reason"
        )
    query = {
        resolved(description, parameter)["name"]: resolved(description, parameter)
        for parameter in operation_in(description, LIST_ISSUES)["parameters"]
    }
    equal(tuple(query["state"]["schema"]["enum"]), LISTED_STATES, describing="the listed states")
    equal(query["per_page"]["schema"]["default"], PER_PAGE_DEFAULT, describing="the default page")
    contains(query["per_page"]["description"], f"max {PER_PAGE_MAX}", describing="the page")
    colour = properties(
        description,
        operation_in(description, CREATE_LABEL)["requestBody"]["content"]["application/json"][
            "schema"
        ],
    )["color"]
    said = str(resolved(description, colour).get("description", ""))
    contains(said, "hexadecimal color code", describing="how a label's colour is written")
    contains(said, "without the leading `#`", describing="how a label's colour is written")
    equal(
        HEXADECIMAL_DIGITS,
        frozenset("0123456789abcdefABCDEF"),
        describing="the digits a hexadecimal colour is written in",
    )


def test_every_job_outcome_is_a_described_job_conclusion(description: Described) -> None:
    """What `needs.<job>.result` can carry, among the conclusions the forge gives a job."""
    conclusion = description["components"]["schemas"]["job"]["properties"]["conclusion"]
    for value in Outcome:
        contains(conclusion["enum"], value.value, describing="a workflow job's conclusions")


def test_the_stand_in_answers_every_declared_operation_and_no_other() -> None:
    """The stand-in's routes are the declaration's, so what it answers is what is held here."""
    equal(set(HANDLERS), set(OPERATIONS), describing="the operations the stand-in answers")
