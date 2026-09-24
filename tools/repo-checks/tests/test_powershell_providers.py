"""Bootstrap and the install journey agree on which PowerShells a host provides."""

from __future__ import annotations

from collections.abc import Callable

import pytest
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
