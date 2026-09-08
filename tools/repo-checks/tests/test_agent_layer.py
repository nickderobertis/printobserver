"""The agent layer: AGENTS.md, the CLAUDE.md symlink, and the composition record."""

from __future__ import annotations

from collections.abc import Callable

from repo_checks.checks_repo import agent_layer
from repo_checks.model import Repo
from treecopy import Tree


def test_the_committed_tree_is_accepted(committed: Repo) -> None:
    """The record this repository ships passes the check it is held to."""
    assert agent_layer(committed) == []


def test_an_absent_symlink_is_refused(tree: Callable[[], Tree]) -> None:
    """A CLAUDE.md that is not a symlink is a copy that will drift."""
    broken = tree()
    broken.remove("CLAUDE.md")
    broken.write("CLAUDE.md", "# a copy, not a link\n")

    findings = agent_layer(broken.repo)

    assert any("not a symbolic link" in finding for finding in findings), findings


def test_a_symlink_pointing_elsewhere_is_refused(tree: Callable[[], Tree]) -> None:
    """A symlink to another file is the same drift with an extra step."""
    broken = tree()
    broken.remove("CLAUDE.md")
    (broken.root / "CLAUDE.md").symlink_to("README.md")

    findings = agent_layer(broken.repo)

    assert any("points at README.md" in finding for finding in findings), findings


def test_an_entry_without_a_disposition_is_refused(tree: Callable[[], Tree]) -> None:
    """An entry that names a piece of guidance but not what was done with it."""
    broken = tree()
    broken.edit(
        "AGENTS.md",
        "- `shape:library` — excluded:",
        "- `shape:library` — we thought about it:",
    )

    findings = agent_layer(broken.repo)

    assert any("carries no disposition" in finding for finding in findings), findings


def test_a_placeholder_entry_is_refused(tree: Callable[[], Tree]) -> None:
    """A disposition with a TODO behind it records nothing."""
    broken = tree()
    text = broken.read("AGENTS.md")
    start = text.index("- `shape:web-app` — excluded:")
    end = text.index("- `shape:react`")
    broken.write(
        "AGENTS.md",
        text[:start] + "- `shape:web-app` — excluded: TODO\n" + text[end:],
    )

    findings = agent_layer(broken.repo)

    assert any("is a placeholder" in finding for finding in findings), findings


def test_an_empty_record_is_refused(tree: Callable[[], Tree]) -> None:
    """A bare heading with no entries under it is not a record."""
    broken = tree()
    text = broken.read("AGENTS.md")
    start = text.index("[//]: # (BEGIN composition-record)")
    end = text.index("[//]: # (END composition-record)")
    broken.write(
        "AGENTS.md",
        text[: start + len("[//]: # (BEGIN composition-record)")] + "\n" + text[end:],
    )

    findings = agent_layer(broken.repo)

    assert any("empty block" in finding for finding in findings), findings
