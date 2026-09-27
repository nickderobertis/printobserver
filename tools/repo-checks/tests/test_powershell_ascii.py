"""Every PowerShell script the tree carries reads the same under every encoding."""

from __future__ import annotations

from collections.abc import Callable

import pytest
from repo_checks.__main__ import main
from repo_checks.expect import contains, equal
from repo_checks.model import Repo
from treecopy import Tree


def test_the_committed_scripts_pass(committed: Repo) -> None:
    """The installers shipped in this tree are ASCII."""
    equal(main(["powershell-ascii", "--root", str(committed.root)]), 0)


def test_an_em_dash_in_an_installer_is_refused_naming_the_line(
    tree: Callable[[], Tree], capsys: pytest.CaptureFixture[str]
) -> None:
    """One character Windows PowerShell 5.1 misreads is a finding at its line."""
    broken = tree()
    script = broken.root / "scripts" / "install-service.ps1"
    lines = script.read_text(encoding="utf-8").splitlines(keepends=True)
    lines.insert(1, "# a comment with an em dash — in it\n")
    script.write_text("".join(lines), encoding="utf-8")

    equal(main(["powershell-ascii", "--root", str(broken.root)]), 1)

    said = capsys.readouterr().err
    contains(said, "scripts/install-service.ps1:2")
    contains(said, "outside ASCII")


def test_a_script_under_a_directory_a_tool_owns_is_not_read(tree: Callable[[], Tree]) -> None:
    """What an install put in the tree is not this repository's PowerShell."""
    copied = tree()
    installed = copied.root / "node_modules" / "some-package" / "install.ps1"
    installed.parent.mkdir(parents=True)
    installed.write_text("# —\n", encoding="utf-8")

    equal(main(["powershell-ascii", "--root", str(copied.root)]), 0)
