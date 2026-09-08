"""The command-line surface `just check-repo` and the committed hooks reach."""

from __future__ import annotations

import subprocess
from collections.abc import Callable
from pathlib import Path

import pytest
from conftest import REPO_ROOT, Tree
from repo_checks.__main__ import main


def test_all_over_the_committed_tree_reports_nothing(capsys: pytest.CaptureFixture[str]) -> None:
    """Silent on success is the contract every script here holds to."""
    assert main(["all", "--root", str(REPO_ROOT)]) == 0
    assert capsys.readouterr().err == ""


def test_one_check_can_be_run_on_its_own() -> None:
    """A single check is reachable by name."""
    assert main(["workspace", "--root", str(REPO_ROOT)]) == 0


def test_the_workflow_tier_is_its_own_group() -> None:
    """`just lint-workflows` runs the workflow checks, `just check-repo` runs the rest."""
    assert main(["workflows", "--root", str(REPO_ROOT)]) == 0


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

    assert main(["agent-layer", "--root", str(broken.root)]) == 1
    assert "agent-layer" in capsys.readouterr().err


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
        subprocess.run(["git", *args], cwd=allowed.root, check=True, capture_output=True)

    assert main(["suppressions", "--root", str(allowed.root), "--base", "HEAD"]) == 0


def test_commit_msg_needs_a_message_file() -> None:
    """A hook invoked wrongly says so rather than passing."""
    with pytest.raises(SystemExit):
        main(["commit-msg", "--root", str(REPO_ROOT)])


def test_the_committed_hook_admits_a_releasing_subject(tmp_path: Path) -> None:
    """`feat` is a subject this repository's own plan publishes under."""
    message = tmp_path / "COMMIT_EDITMSG"
    message.write_text("feat(server): supervise a print\n", encoding="utf-8")

    assert main(["commit-msg", str(message), "--root", str(REPO_ROOT)]) == 0


@pytest.mark.parametrize("subject", ["fix: a thing", "perf: faster", "chore: tidy up"])
def test_the_committed_hook_admits_every_declared_type(subject: str, tmp_path: Path) -> None:
    """Every type `repo-policy.toml` declares is a subject the hook accepts."""
    message = tmp_path / "COMMIT_EDITMSG"
    message.write_text(subject + "\n", encoding="utf-8")

    assert main(["commit-msg", str(message), "--root", str(REPO_ROOT)]) == 0


def test_the_committed_hook_refuses_an_unconventional_subject(
    tmp_path: Path, capsys: pytest.CaptureFixture[str]
) -> None:
    """A subject release automation cannot read is refused where it is written."""
    message = tmp_path / "COMMIT_EDITMSG"
    message.write_text("made some changes\n", encoding="utf-8")

    assert main(["commit-msg", str(message), "--root", str(REPO_ROOT)]) == 1
    assert "not a Conventional Commit" in capsys.readouterr().err


def test_the_committed_hook_refuses_an_undeclared_type(
    tmp_path: Path, capsys: pytest.CaptureFixture[str]
) -> None:
    """A type nobody declared is one release automation has no rule for."""
    message = tmp_path / "COMMIT_EDITMSG"
    message.write_text("wip: halfway there\n", encoding="utf-8")

    assert main(["commit-msg", str(message), "--root", str(REPO_ROOT)]) == 1
    assert "does not admit" in capsys.readouterr().err


def test_a_comment_only_message_is_left_alone(tmp_path: Path) -> None:
    """An aborted commit is git's business, not the hook's."""
    message = tmp_path / "COMMIT_EDITMSG"
    message.write_text("# please enter a message\n", encoding="utf-8")

    assert main(["commit-msg", str(message), "--root", str(REPO_ROOT)]) == 0


def test_the_pull_request_title_lint_reads_the_title_from_the_environment(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    """The title is attacker-controlled, so it arrives through the environment."""
    monkeypatch.setenv("PR_TITLE", "feat(gate): add a tier")

    assert main(["pr-title", "--root", str(REPO_ROOT)]) == 0


def test_an_empty_pull_request_title_is_refused(
    monkeypatch: pytest.MonkeyPatch, capsys: pytest.CaptureFixture[str]
) -> None:
    """A missing title is a misconfigured job, not a pass."""
    monkeypatch.setenv("PR_TITLE", "")

    assert main(["pr-title", "--root", str(REPO_ROOT)]) == 1
    assert "PR_TITLE is empty" in capsys.readouterr().err


def test_an_unconventional_pull_request_title_is_refused(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    """Under squash-merge the title is the commit release automation reads."""
    monkeypatch.setenv("PR_TITLE", "some changes")

    assert main(["pr-title", "--root", str(REPO_ROOT)]) == 1
