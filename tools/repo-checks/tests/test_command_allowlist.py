"""The command allowlist names exactly what the recipes and graph targets invoke."""

from __future__ import annotations

import json
from collections.abc import Callable

from repo_checks.checks_repo import _derived_programs, command_allowlist
from repo_checks.model import Repo
from treecopy import Tree


def test_the_committed_allowlist_is_accepted(committed: Repo) -> None:
    """The allowlist this repository ships names the set its own commands need."""
    assert command_allowlist(committed) == []


def test_the_derived_set_comes_from_the_recipes_and_targets(committed: Repo) -> None:
    """The set is derived from the committed files, not from a list kept beside them."""
    derived = _derived_programs(committed)

    assert "cargo" in derived
    assert "uv" in derived
    assert "bunx" in derived
    assert "just" in derived


def _allow(tree: Tree) -> list[str]:
    return json.loads(tree.read(".claude/settings.json"))["permissions"]["allow"]


def _write_allow(tree: Tree, entries: list[str]) -> None:
    settings = json.loads(tree.read(".claude/settings.json"))
    settings["permissions"]["allow"] = entries
    tree.write(".claude/settings.json", json.dumps(settings, indent=2) + "\n")


def test_an_allowlist_omitting_an_invoked_command_is_refused(
    tree: Callable[[], Tree],
) -> None:
    """A command the recipes run with no entry is an approval prompt every session."""
    broken = tree()
    _write_allow(broken, [e for e in _allow(broken) if not e.startswith("Bash(cargo")])

    findings = command_allowlist(broken.repo)

    assert any("`cargo` is invoked by" in finding for finding in findings), findings


def test_an_allowlist_naming_an_uninvoked_command_is_refused(
    tree: Callable[[], Tree],
) -> None:
    """An entry no recipe needs is a grant nothing in this repository asked for."""
    broken = tree()
    _write_allow(broken, [*_allow(broken), "Bash(curl:*)"])

    findings = command_allowlist(broken.repo)

    assert any("the allowlist names `curl`" in finding for finding in findings), findings
