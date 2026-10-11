"""Saying, on the forge, that the release path on `main` is broken — and when it is mended.

The release workflow fires on every push to `main`, and a run of it that fails
is one red mark among many, which nobody reads. So the workflow's last job hands
the outcome of its drafting and publishing jobs to this module, which keeps
exactly one open issue labelled `LABEL` while either fails: `broken` opens it on the
first failing run and comments on it, naming the run, on every one after; and
`mended` closes it, naming the run, on the first run in which both succeed.

It speaks GitHub's REST API under the run's own token, through the operations
`github_issues` declares and that module's drift gate holds to GitHub's
published description, and through `registries.exchange` — the one place this
tool audits a URL's scheme and attaches a credential — at the API address the
forge hands every step as `GITHUB_API_URL`, for the repository it hands as
`GITHUB_REPOSITORY`.
`tests/repo-e2e/tests/test_release_workflow_journey.py` runs the committed
workflow's steps against a stand-in of that API and holds the sequence to it.
"""

from __future__ import annotations

import json
import re
from collections.abc import Mapping
from dataclasses import dataclass
from enum import StrEnum
from urllib.parse import urlencode, urlsplit

from release_artifacts.github_issues import (
    CREATE_COMMENT,
    CREATE_ISSUE,
    CREATE_LABEL,
    GET_LABEL,
    INVALID,
    LIST_ISSUES,
    NOT_FOUND,
    PER_PAGE_MAX,
    UPDATE_ISSUE,
    IssueNumber,
    Operation,
    Reason,
    State,
)
from release_artifacts.registries import RefusedError, RegistryError, Token, exchange

#: The label the one tracking issue carries, which is how it is found again.
LABEL = "release-broken"

#: The tracking issue's title, naming the release path it is about.
TITLE = "The release path on `main` is broken: release-plz cannot draft or publish"

#: The label's colour and description, for the run that has to create it.
LABEL_COLOUR = "b60205"
LABEL_DESCRIPTION = "The release workflow on main is failing; closed when it next succeeds."

#: What the forge hands every step, and this module reads.
API_URL, REPOSITORY, TOKEN = "GITHUB_API_URL", "GITHUB_REPOSITORY", "GITHUB_TOKEN"

#: The content type every operation `github_issues` declares answers in.
MEDIA_TYPE = "application/json"


class Outcome(StrEnum):
    """What the forge reports a job as, through `needs.<job>.result`.

    Every one is a conclusion GitHub's REST description gives a workflow job,
    which `test_github_issues_contract.py` holds them to.
    """

    SUCCESS = "success"
    FAILURE = "failure"
    CANCELLED = "cancelled"
    SKIPPED = "skipped"


#: An `owner/name` as the forge spells `GITHUB_REPOSITORY`.
REPOSITORY_NAME = re.compile(r"[A-Za-z0-9_.-]+/[A-Za-z0-9_.-]+")

#: The most open issues one read of the listing asks for; a shorter page is the
#: last one.
PAGE = PER_PAGE_MAX

#: What to do about a forge this module could not tell.
NEXT = (
    "Next: read the failed run named above by hand, and re-run this job once the forge "
    "answers; the job's `issues: write` permission is what lets it open the issue."
)


class TrackingError(RuntimeError):
    """The forge could not be told, or was handed something that is not an outcome.

    `status` is the forge's own refusal status where it refused, so a caller can
    tell a name it does not carry from a forge it could not reach.
    """

    def __init__(self, message: str, status: int | None = None) -> None:
        """Record what went wrong and, where the forge refused, its status."""
        super().__init__(message)
        self.status = status


@dataclass(frozen=True, slots=True)
class Forge:
    """GitHub's REST API for one repository, under one token."""

    api: str
    repository: str
    token: Token

    def ask(
        self,
        operation: Operation,
        values: Mapping[str, str] | None = None,
        *,
        query: Mapping[str, str] | None = None,
        document: Mapping[str, object] | None = None,
    ) -> object:
        """One operation of the repository's, and the JSON document it answered.

        `values` names the path's own placeholders beyond the repository's.

        Raises:
            TrackingError: If the forge refused the request or could not be
                reached, in its own words; or if the request carries a query
                parameter or a field its operation does not declare, which
                is a request nothing holds to GitHub's description.
        """
        undeclared = sorted(
            {*(query or {})} - {*operation.query} | {*(document or {})} - {*operation.sends}
        )
        if undeclared:
            msg = f"`{operation.method} {operation.path}` declares no {undeclared}"
            raise TrackingError(msg)
        owner, name = self.repository.split("/", 1)
        method = operation.method
        path = operation.target({"owner": owner, "repo": name, **(values or {})})
        if query:
            path = f"{path}?{urlencode(query)}"
        url = f"{self.api}{path}"
        try:
            # llmlint: ignore[async_typed_clients_at_boundaries] suppressions.toml has the reason.
            answered = exchange(
                url,
                method=method,
                body=None if document is None else json.dumps(document).encode(),
                content_type="" if document is None else "application/json",
                accept=MEDIA_TYPE,
                token=self.token,
            )
        except RegistryError as refused:
            msg = f"the forge did not take `{method} {path}`: {refused}. {NEXT}"
            status = refused.status if isinstance(refused, RefusedError) else None
            raise TrackingError(msg, status) from refused
        try:
            return json.loads(answered) if answered.strip() else None
        except ValueError as unreadable:
            msg = f"the forge answered `{method} {path}` with something that is not JSON. {NEXT}"
            raise TrackingError(msg) from unreadable


def forge(environment: Mapping[str, str]) -> Forge:
    """The forge the run's own environment names.

    Raises:
        TrackingError: If a variable the forge hands every step is absent or
            malformed, which is a step run somewhere other than the forge.
    """
    api = environment.get(API_URL, "").strip().rstrip("/")
    repository = environment.get(REPOSITORY, "").strip()
    token = environment.get(TOKEN, "").strip()
    missing = [
        name
        for name, value in ((API_URL, api), (REPOSITORY, repository), (TOKEN, token))
        if not value
    ]
    if missing:
        msg = (
            f"{', '.join(f'`{name}`' for name in missing)} not set, which the forge sets for "
            f"every step. Next: run this from the release workflow's own job, with `{TOKEN}` "
            f"set from `github.token`."
        )
        raise TrackingError(msg)
    parsed = urlsplit(api)
    if parsed.scheme not in {"http", "https"} or not parsed.hostname:
        msg = f"`{API_URL}` is {api!r}, which is not an `http` or `https` address. {NEXT}"
        raise TrackingError(msg)
    if not REPOSITORY_NAME.fullmatch(repository):
        msg = f"`{REPOSITORY}` is {repository!r}, which is not an `owner/name`. {NEXT}"
        raise TrackingError(msg)
    return Forge(api, repository, Token(token))


def outcomes(given: list[str]) -> dict[str, Outcome]:
    """Each `<job>=<result>` handed on the command line, by job.

    Raises:
        TrackingError: If one is not that shape, or names no result the forge
            reports a job as.
    """
    named: dict[str, Outcome] = {}
    known = {outcome.value for outcome in Outcome}
    for entry in given:
        job, separator, result = entry.partition("=")
        if not separator or not job.strip() or result not in known:
            msg = (
                f"{entry!r} is not `<job>=<result>` with a result among "
                f"{', '.join(sorted(known))}. Next: hand each job as "
                f"`<job>=${{{{ needs.<job>.result }}}}`."
            )
            raise TrackingError(msg)
        named[job.strip()] = Outcome(result)
    return named


def _number(entry: object, what: str) -> IssueNumber:
    """The issue number one document of the forge's carries.

    Raises:
        TrackingError: If it carries none a forge would give an issue: a
            positive integer, and not a boolean.
    """
    number = entry.get("number") if isinstance(entry, dict) else None
    if not isinstance(number, int) or isinstance(number, bool) or number < 1:
        msg = f"the forge answered {what} with {entry!r}, which names no issue. {NEXT}"
        raise TrackingError(msg)
    return IssueNumber(number)


def run_url(url: str) -> str:
    """The failing or succeeding run, as the address a reader opens.

    Raises:
        TrackingError: If it is not an `http` or `https` address.
    """
    parsed = urlsplit(url)
    if parsed.scheme not in {"http", "https"} or not parsed.hostname:
        msg = f"{url!r} is not the address of a workflow run. {NEXT}"
        raise TrackingError(msg)
    return url


def tracking(forge: Forge) -> list[IssueNumber]:
    """Every open tracking issue, oldest first, read page by page to the last.

    The forge lists a pull request among a repository's issues; one carrying
    the label is not an issue this keeps, and is left alone.

    Raises:
        TrackingError: If a page is not the forge's list of issues, or an entry
            names no issue.
    """
    numbers: list[IssueNumber] = []
    page = 1
    while True:
        listed = forge.ask(
            LIST_ISSUES,
            query={"labels": LABEL, "state": State.OPEN, "per_page": str(PAGE), "page": str(page)},
        )
        if not isinstance(listed, list):
            msg = f"the forge answered its open `{LABEL}` issues with {listed!r}. {NEXT}"
            raise TrackingError(msg)
        numbers.extend(
            _number(entry, "a listed issue")
            for entry in listed
            if not (isinstance(entry, dict) and "pull_request" in entry)
        )
        if len(listed) < PAGE:
            return sorted(numbers)
        page += 1


def _named(answered: object) -> None:
    """Confirm the forge answered the tracking label itself.

    Raises:
        TrackingError: If the answer is not a label named `LABEL`, which no
            read of that label answers.
    """
    if not (isinstance(answered, dict) and answered.get("name") == LABEL):
        msg = f"the forge answered the `{LABEL}` label with {answered!r}. {NEXT}"
        raise TrackingError(msg)


def _labelled(forge: Forge) -> None:
    """Create the tracking label where the repository does not carry it yet.

    A second run creating it at the same moment is refused as a label that
    already exists, which is the state this asks for once a read confirms it.
    """
    try:
        _named(forge.ask(GET_LABEL, {"name": LABEL}))
    except TrackingError as absent:
        if absent.status != NOT_FOUND:
            raise
        try:
            forge.ask(
                CREATE_LABEL,
                document={"name": LABEL, "color": LABEL_COLOUR, "description": LABEL_DESCRIPTION},
            )
        except TrackingError as raced:
            if raced.status != INVALID:
                raise
            # A `422` is also a document refused; only a label now there is the race.
            _named(forge.ask(GET_LABEL, {"name": LABEL}))


def broken(forge: Forge, run: str, results: Mapping[str, Outcome]) -> str:
    """Say that `run` failed: open the tracking issue, or comment on the open one.

    Returns:
        What was done, naming the issue.

    Raises:
        TrackingError: If no job handed failed, which is a step asked to say a
            run failed that did not, or the forge could not be told.
    """
    failed = [job for job, result in results.items() if result is Outcome.FAILURE]
    if not failed:
        msg = f"no job handed failed ({dict(results)}), so there is nothing to report. {NEXT}"
        raise TrackingError(msg)
    jobs = ", ".join(f"`{job}`" for job in failed)
    said = f"The release workflow failed on `main` at {jobs}: {run}"
    open_now = tracking(forge)
    if open_now:
        number = open_now[0]
        forge.ask(CREATE_COMMENT, {"issue_number": str(number)}, document={"body": said})
        return f"commented on #{number}: {said}"
    _labelled(forge)
    opened = forge.ask(
        CREATE_ISSUE,
        document={
            "title": TITLE,
            "labels": [LABEL],
            "body": (
                f"{said}\n\nNothing releases while this is open. This issue is kept by the "
                f"release workflow itself: every failing run on `main` comments here, and "
                f"the first run whose drafting and publishing both succeed closes it."
            ),
        },
    )
    return f"opened #{_number(opened, 'the issue it opened')}: {said}"


def mended(forge: Forge, run: str) -> str:
    """Say that `run` succeeded: close every open tracking issue, naming it.

    Every one is attempted whatever an earlier one answered, and one left open
    is what the next succeeding run closes, because it is still listed.

    Returns:
        What was done, naming each issue closed, or that none was open.

    Raises:
        TrackingError: If the forge refused to close one, naming each it
            refused and each it closed.
    """
    open_now = tracking(forge)
    if not open_now:
        return f"no open `{LABEL}` issue, so there is nothing to close"
    said = f"The release workflow drafted and published cleanly on `main`: {run}"
    closed: list[str] = []
    refused: list[str] = []
    for number in open_now:
        try:
            forge.ask(CREATE_COMMENT, {"issue_number": str(number)}, document={"body": said})
            forge.ask(
                UPDATE_ISSUE,
                {"issue_number": str(number)},
                document={"state": State.CLOSED, "state_reason": Reason.COMPLETED},
            )
        except TrackingError as refusal:
            refused.append(f"#{number}: {refusal}")
            continue
        closed.append(f"#{number}")
    if refused:
        msg = f"the forge refused to close {'; '.join(refused)}"
        if closed:
            msg += f". Closed: {', '.join(closed)}"
        raise TrackingError(msg)
    return f"closed {', '.join(closed)}: {said}"
