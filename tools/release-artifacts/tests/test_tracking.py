"""The one issue saying the release path on `main` is broken is opened, kept and closed.

Driven through the command line the release workflow's `just release-broken`
and `just release-mended` run, over the environment the forge hands a step.
GitHub's API is the one boundary: `issues_standin.IssuesForge` answers it over
real HTTP, keeping what each run left for the next, because the real one would
open a real issue.
"""

from __future__ import annotations

import json
import secrets
from collections.abc import Iterator

import pytest
from release_artifacts.__main__ import main
from release_artifacts.issues_standin import IssuesForge
from release_artifacts.registries import RefusedError, Token, exchange
from release_artifacts.tracking import LABEL, TITLE, outcomes
from repo_checks.expect import contains, equal, truth
from repo_checks.loopback import name_service_refused

REPOSITORY = "nickderobertis/printobserver"
#: What the stand-in authenticates the run by: minted per load, never a real token.
CREDENTIAL = Token(f"tracking-{secrets.token_hex(8)}")
RUN = "https://github.com/nickderobertis/printobserver/actions/runs/{}"


@pytest.fixture
def forge(monkeypatch: pytest.MonkeyPatch) -> Iterator[IssuesForge]:
    """A stand-in forge, and the environment the forge hands a step pointed at it."""
    # llmlint: ignore[e2e_not_mocked] suppressions.toml has the reason.
    with IssuesForge(REPOSITORY, CREDENTIAL) as standing:
        monkeypatch.setenv("GITHUB_API_URL", standing.api)
        monkeypatch.setenv("GITHUB_REPOSITORY", REPOSITORY)
        monkeypatch.setenv("GITHUB_TOKEN", CREDENTIAL)
        yield standing


def test_the_forge_is_ready_with_every_lookup_refused() -> None:
    """Started where nothing resolves, the stand-in forge answers its first request."""
    with name_service_refused():
        standing = IssuesForge(REPOSITORY, CREDENTIAL)
    try:
        answered = json.loads(
            exchange(f"{standing.api}/repos/{REPOSITORY}/issues", token=CREDENTIAL, expect=200)
        )
    finally:
        standing.stop()
    equal(answered, [], describing="the issues a forge with none answers")


def broken(run: int, *jobs: str) -> int:
    """Run `release-broken` for run number `run`, handed `jobs`."""
    return main(["release-broken", "--run-url", RUN.format(run), *(f"--job={job}" for job in jobs)])


def mended(run: int) -> int:
    """Run `release-mended` for run number `run`."""
    return main(["release-mended", "--run-url", RUN.format(run)])


def test_a_first_failure_opens_a_second_comments_and_a_success_closes(
    forge: IssuesForge, capsys: pytest.CaptureFixture[str]
) -> None:
    """The sequence the workflow exists for: one issue, kept by every run, closed once."""
    equal(broken(1, "release-pr=failure", "release=success"), 0, describing="the first failure")
    opened = forge.open_labelled(LABEL)
    equal(len(opened), 1, describing="the open tracking issues after a first failure")
    equal(opened[0].title, TITLE, describing="its title")
    contains(opened[0].body, RUN.format(1), describing="what it says")
    contains(opened[0].body, "`release-pr`", describing="the job it names as failed")
    truth("`release`" not in opened[0].body, describing="no succeeding job named as failed")
    truth(LABEL in forge.labels, describing="the label created for it")
    contains(capsys.readouterr().out, f"opened #{opened[0].number}", describing="what it said")

    equal(broken(2, "release-pr=failure", "release=failure"), 0, describing="a second failure")
    equal(
        [issue.number for issue in forge.open_labelled(LABEL)],
        [opened[0].number],
        describing="the open tracking issues after a second failure",
    )
    comment = forge.issues[opened[0].number].comments
    equal(len(comment), 1, describing="the comments a second failure left")
    contains(comment[0], RUN.format(2), describing="the run the comment names")
    contains(comment[0], "`release-pr`, `release`", describing="the jobs it names as failed")
    contains(capsys.readouterr().out, f"commented on #{opened[0].number}", describing="it said")

    equal(mended(3), 0, describing="a success")
    equal(forge.open_labelled(LABEL), [], describing="the open tracking issues after a success")
    closed = forge.issues[opened[0].number]
    equal((closed.state, closed.state_reason), ("closed", "completed"), describing="its state")
    contains(closed.comments[-1], RUN.format(3), describing="the run the closing names")
    contains(capsys.readouterr().out, f"closed #{opened[0].number}", describing="what it said")
    truth(
        all(asked.token == CREDENTIAL for asked in forge.asked),
        describing=f"every request to carry the run's token: {forge.asked}",
    )


def test_a_success_with_nothing_open_asks_the_forge_one_read(
    forge: IssuesForge, capsys: pytest.CaptureFixture[str]
) -> None:
    """The ordinary push: nothing broken, nothing written."""
    equal(mended(1), 0, describing="a success with no issue open")

    equal([asked.method for asked in forge.asked], ["GET"], describing="what was asked")
    contains(capsys.readouterr().out, "nothing to close", describing="what it said")


def test_a_pull_request_carrying_the_label_is_neither_commented_on_nor_closed(
    forge: IssuesForge,
) -> None:
    """The forge lists pull requests among issues; only an issue is the tracking one."""
    pull = forge.seed("A pull request somebody labelled", [LABEL], pull_request=True)

    equal(broken(1, "release=failure"), 0, describing="a failure")
    equal(mended(2), 0, describing="a success")

    equal(forge.issues[pull].comments, [], describing="what the pull request was told")
    equal(forge.issues[pull].state, "open", describing="the pull request's state")
    equal(
        [issue.state for number, issue in forge.issues.items() if number != pull],
        ["closed"],
        describing="the tracking issue the failure opened and the success closed",
    )


def test_an_existing_label_is_not_created_again(forge: IssuesForge) -> None:
    """A repository that already carries the label is asked for it and not asked to make it."""
    forge.labels.add(LABEL)

    equal(broken(1, "release-pr=failure"), 0, describing="a failure")

    absent_create = [asked for asked in forge.asked if asked.path.endswith("/labels")]
    equal(absent_create, [], describing="requests to create the label")


def test_a_refusing_forge_fails_the_step_in_its_own_words(
    forge: IssuesForge, capsys: pytest.CaptureFixture[str]
) -> None:
    """A token the forge will not take an issue from fails loudly, naming its words."""
    forge.refuse("POST", f"/repos/{REPOSITORY}/issues", 403, "Resource not accessible")

    equal(broken(1, "release-pr=failure"), 1, describing="the exit status")

    said = capsys.readouterr().err
    contains(said, "403", describing="the forge's status")
    contains(said, "Resource not accessible", describing="the forge's own words")
    contains(said, "issues: write", describing="what to check next")


def test_a_label_read_the_forge_refuses_otherwise_than_absent_fails_the_step(
    forge: IssuesForge, capsys: pytest.CaptureFixture[str]
) -> None:
    """Only a `404` is a label to create: anything else is the forge refusing."""
    forge.refuse("GET", f"/repos/{REPOSITORY}/labels/{LABEL}", 500, "Server Error")

    equal(broken(1, "release-pr=failure"), 1, describing="the exit status")
    contains(capsys.readouterr().err, "500", describing="the forge's status")
    equal(forge.issues, {}, describing="the issues opened")


def test_an_unreachable_forge_fails_the_step(
    forge: IssuesForge, monkeypatch: pytest.MonkeyPatch, capsys: pytest.CaptureFixture[str]
) -> None:
    """A forge nothing answers at is not a forge that has nothing open."""
    address = forge.api
    forge.stop()
    monkeypatch.setenv("GITHUB_API_URL", address)

    equal(mended(1), 1, describing="the exit status")
    contains(capsys.readouterr().err, "could not be reached", describing="what it said")


@pytest.mark.parametrize(
    ("variable", "value", "named"),
    [
        ("GITHUB_TOKEN", "", "`GITHUB_TOKEN`"),
        ("GITHUB_API_URL", "", "`GITHUB_API_URL`"),
        ("GITHUB_API_URL", "file:///etc", "not an `http` or `https` address"),
        ("GITHUB_API_URL", "https:///path", "not an `http` or `https` address"),
        ("GITHUB_REPOSITORY", "no-slash", "not an `owner/name`"),
    ],
)
def test_an_environment_the_forge_did_not_hand_is_refused_before_anything_is_asked(
    forge: IssuesForge,
    monkeypatch: pytest.MonkeyPatch,
    capsys: pytest.CaptureFixture[str],
    variable: str,
    value: str,
    named: str,
) -> None:
    """Every variable the forge hands a step is required, and each is held to its shape."""
    monkeypatch.setenv(variable, value)

    equal(broken(1, "release-pr=failure"), 1, describing="the exit status")
    contains(capsys.readouterr().err, named, describing="what it named")
    equal(forge.asked, [], describing="what was asked of the forge")


@pytest.mark.parametrize(
    ("arguments", "named"),
    [
        (["release-broken", "--run-url", RUN.format(1), "--job=release-pr"], "<job>=<result>"),
        (["release-broken", "--run-url", RUN.format(1), "--job=release=red"], "<job>=<result>"),
        (["release-broken", "--run-url", RUN.format(1), "--job=release=success"], "nothing to"),
        (["release-broken", "--run-url", "not-a-url", "--job=release=failure"], "not-a-url"),
        (["release-broken", "--run-url", "https://:", "--job=release=failure"], "https://:"),
        (["release-mended"], "--run-url"),
    ],
)
def test_what_the_step_hands_is_refused_where_it_is_not_an_outcome(
    forge: IssuesForge,
    capsys: pytest.CaptureFixture[str],
    arguments: list[str],
    named: str,
) -> None:
    """A job without a result, a result the forge never reports, no failure, no run.

    Each is a step written wrongly rather than a forge that could not be told,
    so each exits 2, before anything is asked of the forge.
    """
    status = main(arguments)

    equal(status, 2, describing=f"the exit status for {arguments}")
    contains(capsys.readouterr().err, named, describing="what it named")
    equal(forge.asked, [], describing="what was asked of the forge")


def test_every_result_the_forge_reports_is_an_outcome() -> None:
    """The four results `needs.<job>.result` can carry are each read as themselves."""
    handed = ["a=success", "b=failure", "c=cancelled", "d=skipped"]

    equal(
        outcomes(handed),
        {"a": "success", "b": "failure", "c": "cancelled", "d": "skipped"},
        describing="what each was read as",
    )


def test_a_forge_answering_something_that_is_not_json_fails_the_step(
    forge: IssuesForge, capsys: pytest.CaptureFixture[str]
) -> None:
    """An answer nothing can read is not a forge with nothing open."""
    forge.garble("GET", f"/repos/{REPOSITORY}/issues")

    equal(mended(1), 1, describing="the exit status")
    contains(capsys.readouterr().err, "not JSON", describing="what it said")


def test_a_repository_the_forge_does_not_carry_fails_the_step(
    forge: IssuesForge, monkeypatch: pytest.MonkeyPatch, capsys: pytest.CaptureFixture[str]
) -> None:
    """A read of a repository the stand-in does not hold is GitHub's `404`, and a failure."""
    monkeypatch.setenv("GITHUB_REPOSITORY", "somebody/else")

    equal(broken(1, "release=failure"), 1, describing="the exit status")
    contains(capsys.readouterr().err, "404", describing="the forge's status")
    equal(forge.issues, {}, describing="the issues opened")


@pytest.mark.parametrize(
    ("method", "path", "document", "credential", "status"),
    [
        ("GET", "/issues", None, "wrong", 401),
        ("POST", "/issues", b"[1]", CREDENTIAL, 400),
        ("POST", "/issues", b"{not json", CREDENTIAL, 400),
        ("POST", "/issues", b'{"labels": []}', CREDENTIAL, 422),
        ("POST", "/labels", b'{"name": ""}', CREDENTIAL, 422),
        ("POST", "/labels", b'{"name": "x", "color": "zzzzzz"}', CREDENTIAL, 422),
        ("GET", "/issues?state=sideways", None, CREDENTIAL, 422),
        ("GET", "/issues?per_page=0", None, CREDENTIAL, 422),
        ("GET", "/issues?page=x", None, CREDENTIAL, 422),
        ("POST", "/issues", b'{"title": "t", "body": "b", "labels": [1]}', CREDENTIAL, 422),
        ("POST", "/issues/1/comments", b'{"body": ""}', CREDENTIAL, 404),
        ("POST", "/issues", b'{"title": "t", "body": "b", "labels": [], "x": 1}', CREDENTIAL, 422),
        ("POST", "/issues", b'{"title": 5, "body": "b", "labels": []}', CREDENTIAL, 422),
        ("POST", "/issues", b'{"title": true, "body": "b", "labels": []}', CREDENTIAL, 422),
        ("POST", "/issues", b'{"title": null, "body": "b", "labels": []}', CREDENTIAL, 422),
        ("PATCH", "/issues/x", b"{}", CREDENTIAL, 404),
        ("POST", "/issues/9/comments", b'{"body": "x"}', CREDENTIAL, 404),
    ],
    # The credential is drawn afresh in each process, so an id spelling it would
    # differ between two xdist workers, which then refuse to run the session.
    ids=lambda value: "the-credential" if value == CREDENTIAL else None,
)
def test_the_stand_in_refuses_what_the_forge_refuses(
    forge: IssuesForge,
    method: str,
    path: str,
    document: bytes | None,
    credential: str,
    status: int,
) -> None:
    """Sent over a real connection: a bad token, a body that is no object, a missing field."""
    with pytest.raises(RefusedError) as refused:
        # llmlint: ignore[async_typed_clients_at_boundaries] suppressions.toml has the reason.
        exchange(
            f"{forge.api}/repos/{REPOSITORY}{path}",
            method=method,
            body=document,
            content_type="application/json",
            token=Token(credential),
        )

    equal(refused.value.status, status, describing=f"the status for {method} {path}")


def test_the_stand_in_refuses_a_label_it_already_carries(forge: IssuesForge) -> None:
    """Creating a label twice is GitHub's `422`, as a run racing another would meet it."""
    forge.labels.add(LABEL)

    with pytest.raises(RefusedError) as refused:
        # llmlint: ignore[async_typed_clients_at_boundaries] suppressions.toml has the reason.
        exchange(
            f"{forge.api}/repos/{REPOSITORY}/labels",
            method="POST",
            body=f'{{"name": "{LABEL}"}}'.encode(),
            content_type="application/json",
            token=Token(CREDENTIAL),
        )

    equal(refused.value.status, 422, describing="the status")


class RacedForge(IssuesForge):
    """The forge as a second run creating the label at the same moment leaves it.

    The label is created by that other run between this one's read and its
    creation, so this run's creation is refused as a label that exists.
    """

    def answer(self, method: str, target: str, document: dict[str, object]) -> tuple[int, object]:
        """Create the label for the other run, then answer as the forge does."""
        if (method, target) == ("POST", f"/repos/{self.repository}/labels"):
            self.labels.add(LABEL)
        return super().answer(method, target, document)


def test_a_label_another_run_created_at_the_same_moment_still_opens_the_issue(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    """The forge's `422` for a label a read then finds is the state the step asked for."""
    # llmlint: ignore[e2e_not_mocked] suppressions.toml has the reason.
    with RacedForge(REPOSITORY, CREDENTIAL) as raced:
        monkeypatch.setenv("GITHUB_API_URL", raced.api)
        monkeypatch.setenv("GITHUB_REPOSITORY", REPOSITORY)
        monkeypatch.setenv("GITHUB_TOKEN", CREDENTIAL)

        equal(broken(1, "release-pr=failure"), 0, describing="the exit status")
        equal(len(raced.open_labelled(LABEL)), 1, describing="the open tracking issues")


def test_a_label_creation_refused_as_invalid_fails_the_step(
    forge: IssuesForge, capsys: pytest.CaptureFixture[str]
) -> None:
    """A `422` after which no label is there is a document refused, not a race won."""
    forge.refuse("POST", f"/repos/{REPOSITORY}/labels", 422, "Validation Failed")

    equal(broken(1, "release-pr=failure"), 1, describing="the exit status")
    contains(capsys.readouterr().err, "404", describing="the read that found no label")
    equal(forge.issues, {}, describing="the issues opened")


def test_a_label_the_forge_refuses_to_create_fails_the_step_opening_nothing(
    forge: IssuesForge, capsys: pytest.CaptureFixture[str]
) -> None:
    """Any other refusal of the label is the forge refusing, and nothing is opened unlabelled."""
    forge.refuse("POST", f"/repos/{REPOSITORY}/labels", 403, "Resource not accessible")

    equal(broken(1, "release-pr=failure"), 1, describing="the exit status")
    contains(capsys.readouterr().err, "403", describing="the forge's status")
    equal(forge.issues, {}, describing="the issues opened")


def test_a_refused_comment_fails_the_step_and_the_next_failing_run_comments(
    forge: IssuesForge, capsys: pytest.CaptureFixture[str]
) -> None:
    """A run the forge would not take a comment from is loud, and changes nothing open."""
    equal(broken(1, "release-pr=failure"), 0, describing="the first failure")
    number = forge.open_labelled(LABEL)[0].number
    comments = f"/repos/{REPOSITORY}/issues/{number}/comments"
    forge.refuse("POST", comments, 502, "Bad Gateway")

    equal(broken(2, "release-pr=failure"), 1, describing="the refused second report")
    contains(capsys.readouterr().err, "502", describing="the forge's status")
    forge.accept("POST", comments)
    equal(broken(3, "release-pr=failure"), 0, describing="the third failure")

    equal(
        [issue.number for issue in forge.open_labelled(LABEL)],
        [number],
        describing="the open tracking issues",
    )
    equal(len(forge.issues[number].comments), 1, describing="the comments taken")
    contains(forge.issues[number].comments[0], RUN.format(3), describing="the run named")


def test_a_success_closes_every_open_tracking_issue_and_comments_on_the_oldest_first(
    forge: IssuesForge,
) -> None:
    """Two open, however that came about: a failure tells the oldest, a success closes both."""
    older = forge.seed(TITLE, [LABEL])
    newer = forge.seed(TITLE, [LABEL])

    equal(broken(1, "release=failure"), 0, describing="a failure")
    equal(len(forge.issues[older].comments), 1, describing="the oldest's comments")
    equal(forge.issues[newer].comments, [], describing="the newer's comments")
    equal(mended(2), 0, describing="a success")

    equal(forge.open_labelled(LABEL), [], describing="the open tracking issues")


def test_a_close_the_forge_refuses_partway_is_finished_by_the_next_success(
    forge: IssuesForge, capsys: pytest.CaptureFixture[str]
) -> None:
    """Every issue is attempted; the one left open is listed, and so closed, next time."""
    first = forge.seed(TITLE, [LABEL])
    second = forge.seed(TITLE, [LABEL])
    closing = f"/repos/{REPOSITORY}/issues/{first}"
    forge.refuse("PATCH", closing, 500, "Server Error")

    equal(mended(1), 1, describing="the partly refused close")
    said = capsys.readouterr().err
    contains(said, f"#{first}", describing="the issue the forge refused to close")
    contains(said, f"Closed: #{second}", describing="the issue it did close")
    equal(
        [issue.number for issue in forge.open_labelled(LABEL)],
        [first],
        describing="the tracking issues left open",
    )
    forge.accept("PATCH", closing)

    equal(mended(2), 0, describing="the next success")
    equal(forge.open_labelled(LABEL), [], describing="the tracking issues left open")


def test_every_page_of_open_tracking_issues_is_read(forge: IssuesForge) -> None:
    """More open than one page holds: the last page's are closed too."""
    numbers = [forge.seed(TITLE, [LABEL]) for _ in range(101)]

    equal(mended(1), 0, describing="a success")

    equal(forge.open_labelled(LABEL), [], describing="the tracking issues left open")
    truth(
        any("page=2" in asked.path for asked in forge.asked),
        describing="the second page to have been read",
    )
    equal(forge.issues[numbers[-1]].state, "closed", describing="the last page's issue")


@pytest.mark.parametrize(
    ("method", "path", "status", "named"),
    [
        ("GET", "/issues", 200, "with {'message'"),
        ("POST", "/issues", 201, "names no issue"),
    ],
)
def test_a_forge_answer_that_is_not_the_document_asked_for_fails_the_step(
    forge: IssuesForge,
    capsys: pytest.CaptureFixture[str],
    method: str,
    path: str,
    status: int,
    named: str,
) -> None:
    """A listing that is no list, and an issue opened that names no number, are refused."""
    forge.refuse(method, f"/repos/{REPOSITORY}{path}", status, "something else")

    equal(broken(1, "release-pr=failure"), 1, describing="the exit status")
    contains(capsys.readouterr().err, named, describing="what it said")


class MislistingForge(IssuesForge):
    """A forge whose listing names an issue by something that is no issue number."""

    def answer(self, method: str, target: str, document: dict[str, object]) -> tuple[int, object]:
        """Answer the listing with one entry numbered `0`, and everything else as the forge."""
        if method == "GET" and target.startswith(f"/repos/{self.repository}/issues?"):
            return 200, [{"number": 0, "title": TITLE}]
        return super().answer(method, target, document)


def test_a_listed_issue_naming_no_number_is_refused_before_anything_is_written(
    monkeypatch: pytest.MonkeyPatch, capsys: pytest.CaptureFixture[str]
) -> None:
    """An entry this could not comment on or close fails the step having written nothing."""
    # llmlint: ignore[e2e_not_mocked] suppressions.toml has the reason.
    with MislistingForge(REPOSITORY, CREDENTIAL) as mislisting:
        monkeypatch.setenv("GITHUB_API_URL", mislisting.api)
        monkeypatch.setenv("GITHUB_REPOSITORY", REPOSITORY)
        monkeypatch.setenv("GITHUB_TOKEN", CREDENTIAL)

        equal(broken(1, "release-pr=failure"), 1, describing="a failure's exit status")
        equal(mended(2), 1, describing="a success's exit status")
        contains(capsys.readouterr().err, "names no issue", describing="what it said")
        equal(
            [asked.method for asked in mislisting.asked],
            ["GET", "GET"],
            describing="what was asked of the forge",
        )


def test_a_refused_closing_comment_leaves_that_issue_open_for_the_next_success(
    forge: IssuesForge, capsys: pytest.CaptureFixture[str]
) -> None:
    """The issue whose comment was refused stays open, the next is still closed, and then both."""
    first = forge.seed(TITLE, [LABEL])
    second = forge.seed(TITLE, [LABEL])
    commenting = f"/repos/{REPOSITORY}/issues/{first}/comments"
    forge.refuse("POST", commenting, 503, "Service Unavailable")

    equal(mended(1), 1, describing="the partly refused close")
    said = capsys.readouterr().err
    contains(said, "503", describing="the forge's status")
    contains(said, f"Closed: #{second}", describing="the issue it did close")
    equal(
        [issue.number for issue in forge.open_labelled(LABEL)],
        [first],
        describing="the tracking issues left open",
    )
    forge.accept("POST", commenting)

    equal(mended(2), 0, describing="the next success")
    equal(forge.open_labelled(LABEL), [], describing="the tracking issues left open")


def test_the_stand_in_takes_a_null_where_the_forge_does(forge: IssuesForge) -> None:
    """A `null` reason closes an issue without one, as the description admits it."""
    number = forge.seed(TITLE, [LABEL])

    # llmlint: ignore[async_typed_clients_at_boundaries] suppressions.toml has the reason.
    answered = exchange(
        f"{forge.api}/repos/{REPOSITORY}/issues/{number}",
        method="PATCH",
        body=b'{"state": "closed", "state_reason": null}',
        content_type="application/json",
        token=CREDENTIAL,
    )

    equal(json.loads(answered)["state"], "closed", describing="the issue's state")
    equal(forge.issues[number].state_reason, None, describing="the reason it was closed for")


def test_a_label_read_answering_another_label_fails_the_step_opening_nothing(
    forge: IssuesForge, capsys: pytest.CaptureFixture[str]
) -> None:
    """A `200` that is not the tracking label is no confirmation that the label is there."""
    forge.refuse("GET", f"/repos/{REPOSITORY}/labels/{LABEL}", 200, "something else")

    equal(broken(1, "release-pr=failure"), 1, describing="the exit status")
    contains(capsys.readouterr().err, f"answered the `{LABEL}` label with", describing="it said")
    equal(forge.issues, {}, describing="the issues opened")


def test_a_comment_answered_with_a_status_it_does_not_succeed_with_fails_the_step(
    forge: IssuesForge, capsys: pytest.CaptureFixture[str]
) -> None:
    """A `202` is not the `201` a comment is taken with, so nothing is reported done."""
    number = forge.seed(TITLE, [LABEL])
    forge.refuse("POST", f"/repos/{REPOSITORY}/issues/{number}/comments", 202, "Accepted")

    equal(broken(1, "release-pr=failure"), 1, describing="the exit status")
    said = capsys.readouterr().err
    contains(said, "202", describing="the status the forge answered")
    contains(said, "where 201 was expected", describing="the status it succeeds with")
