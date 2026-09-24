"""Bootstrap and the install journey agree on which PowerShells a host provides."""

from __future__ import annotations

from collections.abc import Callable

import pytest
from held_toolchain import held_by_verb
from repo_checks.__main__ import main
from repo_checks.expect import contains, equal
from repo_checks.model import Repo
from treecopy import Tree


def test_the_committed_provider_contract_passes(committed: Repo) -> None:
    """The public repository check accepts the policy and journey shipped together."""
    equal(main(["powershell-providers", "--root", str(committed.root)]), 0)


def test_bootstrap_dropping_a_powershell_the_journey_uses_is_refused(
    tree: Callable[[], Tree], capsys: pytest.CaptureFixture[str]
) -> None:
    """A Windows host with only `powershell` must still satisfy bootstrap."""
    broken = tree()
    broken.edit(
        "repo-policy.toml",
        'provided_by = ["pwsh", "powershell"]',
        'provided_by = ["pwsh"]',
    )

    equal(main(["powershell-providers", "--root", str(broken.root)]), 1)

    contains(capsys.readouterr().err, "pwsh provided_by")


def test_a_malformed_installer_journey_is_a_check_finding(
    tree: Callable[[], Tree], capsys: pytest.CaptureFixture[str]
) -> None:
    """A damaged source file should make the repository check fail cleanly."""
    broken = tree()
    broken.edit(
        "tools/release-artifacts/src/release_artifacts/installing.py",
        'POWERSHELLS = ("pwsh", "powershell")',
        "POWERSHELLS = (",
    )

    equal(main(["powershell-providers", "--root", str(broken.root)]), 1)

    contains(capsys.readouterr().err, "could not be parsed")


INSTALLING = "tools/release-artifacts/src/release_artifacts/installing.py"

#: The release the committed policy holds `pwsh` at.
PWSH = next(held.version for held in held_by_verb() if held.command == "pwsh")


@pytest.mark.parametrize(
    ("old", "new", "said"),
    [
        ("POWERSHELLS = (", "SHELLS = (", "declares no literal POWERSHELLS tuple"),
        (
            'POWERSHELLS = ("pwsh", "powershell")',
            "POWERSHELLS = tuple(SHELLS)",
            "declares no literal POWERSHELLS tuple",
        ),
        (
            'POWERSHELLS = ("pwsh", "powershell")',
            'POWERSHELLS = ["pwsh", "powershell"]',
            "declares no literal POWERSHELLS tuple",
        ),
        (
            'POWERSHELLS = ("pwsh", "powershell")',
            'POWERSHELLS = ("pwsh", 7)',
            "declares no literal POWERSHELLS tuple",
        ),
    ],
    ids=["no such name", "not a literal", "not a tuple", "not all names"],
)
def test_a_journey_declaring_no_literal_tuple_of_powershells_is_refused(
    tree: Callable[[], Tree], capsys: pytest.CaptureFixture[str], old: str, new: str, said: str
) -> None:
    """What the journey runs is read as written, and anything else is named rather than guessed."""
    broken = tree()
    broken.edit(INSTALLING, old, new)

    equal(main(["powershell-providers", "--root", str(broken.root)]), 1)

    contains(capsys.readouterr().err, f"{INSTALLING} {said}")


def test_a_tree_carrying_no_installer_journey_is_refused_naming_it(
    tree: Callable[[], Tree], capsys: pytest.CaptureFixture[str]
) -> None:
    """With the source gone there is nothing to hold bootstrap to, and that is said."""
    broken = tree()
    broken.remove(INSTALLING)

    equal(main(["powershell-providers", "--root", str(broken.root)]), 1)

    contains(capsys.readouterr().err, f"{INSTALLING} is absent")


def test_a_policy_holding_no_pwsh_is_refused(
    tree: Callable[[], Tree], capsys: pytest.CaptureFixture[str]
) -> None:
    """A policy that stops declaring `pwsh` provides the journey no PowerShell at all."""
    broken = tree()
    broken.edit(
        "repo-policy.toml",
        f'command = "pwsh"\nversion = "{PWSH}"\nprovided_by = ["pwsh", "powershell"]\n',
        f'command = "pwsh-retired"\nversion = "{PWSH}"\n',
    )

    equal(main(["powershell-providers", "--root", str(broken.root)]), 1)

    contains(capsys.readouterr().err, "repo-policy.toml declares no pwsh toolchain tool")
