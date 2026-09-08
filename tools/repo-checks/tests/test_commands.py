"""The commands the recipes and hooks run that do something rather than check it."""

from __future__ import annotations

import subprocess
from collections.abc import Callable
from pathlib import Path

import pytest
from conftest import Tree
from repo_checks.commands import coverage, install_hooks, install_tools
from repo_checks.model import Repo

POLICY = """
schema_version = 1

[gate.coverage]
rust = 95
python = 95
typescript = 95

[[toolchain.tool]]
command = "{command}"
install = "{install}"
"""


def test_install_hooks_points_git_at_the_committed_hooks(
    tree: Callable[[], Tree],
) -> None:
    """A clean clone's bootstrap wires the hooks it ships with."""
    fresh = tree()
    subprocess.run(["git", "init", "-q", "-b", "main"], cwd=fresh.root, check=True)

    assert install_hooks(fresh.repo) == 0

    configured = subprocess.run(
        ["git", "config", "core.hooksPath"],
        cwd=fresh.root,
        capture_output=True,
        text=True,
        check=True,
    ).stdout.strip()
    assert configured == ".githooks"


def test_install_hooks_is_a_no_op_outside_a_git_repository(
    tree: Callable[[], Tree],
) -> None:
    """The bootstrap journey copies the tree without its history and must still run."""
    fresh = tree()

    assert install_hooks(fresh.repo) == 0


def test_install_tools_skips_a_tool_already_on_the_path(tmp_path: Path) -> None:
    """Bootstrap is idempotent: a present tool is not reinstalled."""
    root = tmp_path / "tree"
    root.mkdir()
    (root / "repo-policy.toml").write_text(
        POLICY.format(command="git", install="false"), encoding="utf-8"
    )

    assert install_tools(Repo(root)) == 0


def test_install_tools_reports_an_install_it_could_not_do(
    tmp_path: Path, capsys: pytest.CaptureFixture[str]
) -> None:
    """A failed install names the command to run by hand rather than failing silently."""
    root = tmp_path / "tree"
    root.mkdir()
    (root / "repo-policy.toml").write_text(
        POLICY.format(command="a-tool-that-does-not-exist", install="false"),
        encoding="utf-8",
    )

    assert install_tools(Repo(root)) == 1
    assert "Run `false` by hand" in capsys.readouterr().err


def test_coverage_fails_where_no_coverage_was_measured(
    tmp_path: Path, capsys: pytest.CaptureFixture[str]
) -> None:
    """A run with nothing to report is below the floor, not above it."""
    root = tmp_path / "tree"
    root.mkdir()
    (root / "repo-policy.toml").write_text(
        POLICY.format(command="git", install="false"), encoding="utf-8"
    )

    assert coverage(Repo(root)) == 1
    assert "below the" in capsys.readouterr().err
