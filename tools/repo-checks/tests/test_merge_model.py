"""The recorded merge model names jobs the configuration actually declares."""

from __future__ import annotations

from collections.abc import Callable

from repo_checks.checks_ci import merge_model
from repo_checks.model import Repo
from treecopy import Tree

BLOCK_START = "[//]: # (BEGIN required-checks)"
BLOCK_END = "[//]: # (END required-checks)"


def test_the_committed_record_is_accepted(committed: Repo) -> None:
    """Every required name has a job behind it."""
    assert merge_model(committed) == []


def _replace_required(tree: Tree, names: list[str]) -> None:
    text = tree.read("AGENTS.md")
    start = text.index(BLOCK_START) + len(BLOCK_START)
    end = text.index(BLOCK_END)
    listing = "".join(f"\n- `{name}`" for name in names)
    tree.write("AGENTS.md", text[:start] + listing + "\n" + text[end:])


def test_a_required_name_with_no_job_behind_it_is_refused(
    tree: Callable[[], Tree],
) -> None:
    """A required context nothing reports blocks every pull request forever."""
    broken = tree()
    _replace_required(broken, ["gate", "llmlint", "pr-title", "smoke"])

    findings = merge_model(broken.repo)

    assert any("no committed workflow declares a job" in finding for finding in findings), findings


def test_a_record_naming_no_required_job_is_refused(tree: Callable[[], Tree]) -> None:
    """A merge path with nothing required on it is not a gate."""
    broken = tree()
    _replace_required(broken, [])

    findings = merge_model(broken.repo)

    assert any("records no required check at all" in finding for finding in findings), findings


def test_a_record_omitting_the_gate_job_is_refused(tree: Callable[[], Tree]) -> None:
    """The complete-gate job is one of the two the gate is made of."""
    broken = tree()
    _replace_required(broken, ["llmlint", "pr-title"])

    findings = merge_model(broken.repo)

    assert any("omit the complete-gate job" in finding for finding in findings), findings


def test_a_record_omitting_the_judged_lint_job_is_refused(
    tree: Callable[[], Tree],
) -> None:
    """So is the judged-lint job."""
    broken = tree()
    _replace_required(broken, ["gate", "pr-title"])

    findings = merge_model(broken.repo)

    assert any("omit the judged-lint job" in finding for finding in findings), findings


def test_the_record_states_that_the_base_branch_takes_no_direct_push(
    committed: Repo,
) -> None:
    """How a change reaches the base branch is written down."""
    assert "takes no direct push" in committed.agents_md
    assert "squash-merged" in committed.agents_md
