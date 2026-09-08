"""The command-line surface `just check-repo` and the committed hooks reach."""

# `assert` is how pytest states an assertion and how it produces the failure
# message a reader acts on; suppressions.toml carries the reason.

from __future__ import annotations

from collections.abc import Callable
from pathlib import Path

import pytest
from repo_checks.__main__ import main
from repo_checks.expect import contains, equal
from repo_checks.shell import run
from treecopy import REPO_ROOT, Tree

# The subject git writes for the merge every publication of this repository makes
# before it pushes. Nobody types it, and the hook rules on the commit's state
# rather than on this text — which is why the same text is refused below.
MERGE_SUBJECT = "Merge remote-tracking branch 'origin/main' into work"


@pytest.fixture
def merging(tree: Callable[[], Tree]) -> Tree:
    """A real repository part-way through the merge a publication starts with.

    A real `git merge --no-commit` of a real remote-tracking ref, left where git
    leaves it while it is writing the merge commit: `MERGE_HEAD` in place and
    nothing committed yet. That is the state the `commit-msg` hook runs in, and
    `git merge --abort` takes the same repository back out of it.
    """
    copy = tree()

    def git(*args: str) -> None:
        run(["git", *args], cwd=copy.root, check=True)

    for args in (
        ("init", "-q", "-b", "main"),
        ("config", "user.email", "test@example.com"),
        ("config", "user.name", "test"),
        ("add", "-A"),
        ("commit", "-q", "-m", "chore: the committed tree, copied"),
        ("checkout", "-q", "-b", "work"),
    ):
        git(*args)

    copy.write("on-the-branch", "the finished work\n")
    git("add", "-A")
    git("commit", "-q", "-m", "feat(server): the finished work")

    git("checkout", "-q", "main")
    copy.write("on-the-base", "what landed meanwhile\n")
    git("add", "-A")
    git("commit", "-q", "-m", "fix(core): what landed meanwhile")
    git("update-ref", "refs/remotes/origin/main", "HEAD")

    git("checkout", "-q", "work")
    git("merge", "--no-commit", "--no-ff", "origin/main")
    return copy


def test_all_over_the_committed_tree_reports_nothing(capsys: pytest.CaptureFixture[str]) -> None:
    """Silent on success is the contract every script here holds to."""
    equal(main(["all", "--root", str(REPO_ROOT)]), 0)
    equal(capsys.readouterr().err, "")


def test_one_check_can_be_run_on_its_own() -> None:
    """A single check is reachable by name."""
    equal(main(["workspace", "--root", str(REPO_ROOT)]), 0)


def test_the_workflow_tier_is_its_own_group() -> None:
    """`just lint-workflows` runs the workflow checks, `just check-repo` runs the rest."""
    equal(main(["workflows", "--root", str(REPO_ROOT)]), 0)


def test_an_unknown_check_is_refused() -> None:
    """A typo is a failure, not a silent no-op."""
    with pytest.raises(SystemExit):
        main(["not-a-check", "--root", str(REPO_ROOT)])


def test_a_failing_check_prints_every_finding(
    tree: Callable[[], Tree], capsys: pytest.CaptureFixture[str]
) -> None:
    """On failure the exact disagreement reaches the next reader."""
    broken = tree()
    broken.remove("CLAUDE.md")

    equal(main(["agent-layer", "--root", str(broken.root)]), 1)
    contains(capsys.readouterr().err, "agent-layer")


def test_the_suppression_check_takes_a_base_revision(tree: Callable[[], Tree]) -> None:
    """The change-scoped half is reachable from the command line."""
    allowed = tree()
    for args in (
        ["init", "-q", "-b", "main"],
        ["config", "user.email", "test@example.com"],
        ["config", "user.name", "test"],
        ["add", "-A"],
        ["commit", "-q", "-m", "chore: the base"],
    ):
        run(["git", *args], cwd=allowed.root, check=True)

    equal(main(["suppressions", "--root", str(allowed.root), "--base", "HEAD"]), 0)


def test_commit_msg_needs_a_message_file() -> None:
    """A hook invoked wrongly says so rather than passing."""
    with pytest.raises(SystemExit):
        main(["commit-msg", "--root", str(REPO_ROOT)])


def test_the_committed_hook_admits_a_releasing_subject(tmp_path: Path) -> None:
    """`feat` is a subject this repository's own plan publishes under."""
    message = tmp_path / "COMMIT_EDITMSG"
    message.write_text("feat(server): supervise a print\n", encoding="utf-8")

    equal(main(["commit-msg", str(message), "--root", str(REPO_ROOT)]), 0)


@pytest.mark.parametrize("subject", ["fix: a thing", "perf: faster", "chore: tidy up"])
def test_the_committed_hook_admits_every_declared_type(subject: str, tmp_path: Path) -> None:
    """Every type `repo-policy.toml` declares is a subject the hook accepts."""
    message = tmp_path / "COMMIT_EDITMSG"
    message.write_text(subject + "\n", encoding="utf-8")

    equal(main(["commit-msg", str(message), "--root", str(REPO_ROOT)]), 0)


def test_a_merge_git_is_completing_carries_the_subject_git_wrote(
    merging: Tree, tmp_path: Path
) -> None:
    """`MERGE_HEAD` is present, so this commit is one git is writing rather than a person."""
    message = tmp_path / "COMMIT_EDITMSG"
    message.write_text(MERGE_SUBJECT + "\n", encoding="utf-8")

    equal(main(["commit-msg", str(message), "--root", str(merging.root)]), 0)


def test_the_same_wording_is_refused_when_no_merge_is_in_progress(
    merging: Tree, tmp_path: Path, capsys: pytest.CaptureFixture[str]
) -> None:
    """The narrowing, in the same repository: only the state differs, not the subject.

    A person can type `Merge remote-tracking branch 'origin/main' into work` as
    easily as git can generate it, so a rule reading the subject cannot tell the
    two apart and would hand anybody a bypass of the whole convention. This is
    the case that fails if the exemption ever moves back onto the wording.
    """
    run(["git", "merge", "--abort"], cwd=merging.root, check=True)
    message = tmp_path / "COMMIT_EDITMSG"
    message.write_text(MERGE_SUBJECT + "\n", encoding="utf-8")

    equal(main(["commit-msg", str(message), "--root", str(merging.root)]), 1)
    contains(capsys.readouterr().err, "not a Conventional Commit")


def test_a_merge_shaped_pull_request_title_is_refused(
    monkeypatch: pytest.MonkeyPatch, capsys: pytest.CaptureFixture[str]
) -> None:
    """The exemption is the commit hook's alone: a title is always authored.

    Under squash-merge the title becomes the subject release automation reads,
    and no merge commit of this repository ever reaches `main` to carry one.
    """
    monkeypatch.setenv("PR_TITLE", "Merge remote-tracking branch 'origin/main' into work")

    equal(main(["pr-title", "--root", str(REPO_ROOT)]), 1)
    contains(capsys.readouterr().err, "not a Conventional Commit")


def test_the_committed_hook_refuses_an_unconventional_subject(
    tmp_path: Path, capsys: pytest.CaptureFixture[str]
) -> None:
    """A subject release automation cannot read is refused where it is written."""
    message = tmp_path / "COMMIT_EDITMSG"
    message.write_text("made some changes\n", encoding="utf-8")

    equal(main(["commit-msg", str(message), "--root", str(REPO_ROOT)]), 1)
    contains(capsys.readouterr().err, "not a Conventional Commit")


def test_the_committed_hook_refuses_an_undeclared_type(
    tmp_path: Path, capsys: pytest.CaptureFixture[str]
) -> None:
    """A type nobody declared is one release automation has no rule for."""
    message = tmp_path / "COMMIT_EDITMSG"
    message.write_text("wip: halfway there\n", encoding="utf-8")

    equal(main(["commit-msg", str(message), "--root", str(REPO_ROOT)]), 1)
    contains(capsys.readouterr().err, "does not admit")


def test_a_comment_only_message_is_left_alone(tmp_path: Path) -> None:
    """An aborted commit is git's business, not the hook's."""
    message = tmp_path / "COMMIT_EDITMSG"
    message.write_text("# please enter a message\n", encoding="utf-8")

    equal(main(["commit-msg", str(message), "--root", str(REPO_ROOT)]), 0)


def test_the_pull_request_title_lint_reads_the_title_from_the_environment(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    """The title is attacker-controlled, so it arrives through the environment."""
    monkeypatch.setenv("PR_TITLE", "feat(gate): add a tier")

    equal(main(["pr-title", "--root", str(REPO_ROOT)]), 0)


def test_an_empty_pull_request_title_is_refused(
    monkeypatch: pytest.MonkeyPatch, capsys: pytest.CaptureFixture[str]
) -> None:
    """A missing title is a misconfigured job, not a pass."""
    monkeypatch.setenv("PR_TITLE", "")

    equal(main(["pr-title", "--root", str(REPO_ROOT)]), 1)
    contains(capsys.readouterr().err, "PR_TITLE is empty")


def test_an_unconventional_pull_request_title_is_refused(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    """Under squash-merge the title is the commit release automation reads."""
    monkeypatch.setenv("PR_TITLE", "some changes")

    equal(main(["pr-title", "--root", str(REPO_ROOT)]), 1)
