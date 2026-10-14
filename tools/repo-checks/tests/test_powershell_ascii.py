"""Every PowerShell script the tree carries reads the same under every encoding."""

from __future__ import annotations

from collections.abc import Callable

import pytest
from repo_checks.__main__ import main
from repo_checks.expect import contains, equal
from repo_checks.model import Repo
from treecopy import Tree

#: The characters a script typed in an editor picks up, spelled by code point so
#: this file stays readable to the lint that flags them.
EM_DASH = chr(0x2014)
LEFT_QUOTE = chr(0x2018)
RIGHT_QUOTE = chr(0x2019)


def test_the_committed_scripts_pass(committed: Repo) -> None:
    """The installers shipped in this tree are ASCII."""
    equal(main(["powershell-ascii", "--root", str(committed.root)]), 0)


def test_an_em_dash_in_an_installer_is_refused_naming_the_line(
    tree: Callable[[], Tree], capsys: pytest.CaptureFixture[str]
) -> None:
    """One character Windows PowerShell 5.1 misreads is a finding at its line and column."""
    broken = tree()
    script = broken.root / "scripts" / "install-service.ps1"
    lines = script.read_text(encoding="utf-8").splitlines(keepends=True)
    lines.insert(1, f"# a comment with an em dash {EM_DASH} in it\n")
    script.write_text("".join(lines), encoding="utf-8")

    equal(main(["powershell-ascii", "--root", str(broken.root)]), 1)

    reported = capsys.readouterr().err
    contains(reported, "scripts/install-service.ps1:2:29 carries a byte outside ASCII")
    contains(reported, "Windows PowerShell 5.1")


def test_a_new_script_anywhere_in_the_tree_is_held_to_it(
    tree: Callable[[], Tree], capsys: pytest.CaptureFixture[str]
) -> None:
    """The rule is over every committed script, not over the two installers by name."""
    broken = tree()
    broken.write("tools/octoprint-env/helper.ps1", "Write-Output 'fine'\n")
    equal(main(["powershell-ascii", "--root", str(broken.root)]), 0)

    broken.write("tools/octoprint-env/helper.ps1", f"Write-Output {LEFT_QUOTE}curly{RIGHT_QUOTE}\n")
    equal(main(["powershell-ascii", "--root", str(broken.root)]), 1)
    contains(capsys.readouterr().err, "tools/octoprint-env/helper.ps1:1:14")


def test_a_byte_order_mark_is_refused_too(
    tree: Callable[[], Tree], capsys: pytest.CaptureFixture[str]
) -> None:
    """A mark a download may drop is no fix: the script is held to ASCII outright."""
    broken = tree()
    script = broken.root / "scripts" / "install.ps1"
    script.write_bytes(b"\xef\xbb\xbf" + script.read_bytes())

    equal(main(["powershell-ascii", "--root", str(broken.root)]), 1)
    contains(capsys.readouterr().err, "scripts/install.ps1:1:1")
