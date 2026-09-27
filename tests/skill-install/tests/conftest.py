"""The GitHub CLI this project's journeys run: the release `repo-policy.toml` holds."""

from __future__ import annotations

import shutil
from pathlib import Path

import pytest
from repo_checks.model import Repo, toolchain_tools
from repo_checks.shell import run

#: The repository these journeys read.
REPO_ROOT = Path(__file__).resolve().parents[3]


def _held_release() -> str:
    """The release `repo-policy.toml` holds gh at."""
    held = {tool.command: tool.version for tool in toolchain_tools(Repo(REPO_ROOT))}
    release = held.get("gh")
    if release is None:
        message = "`repo-policy.toml` holds gh at no release, and this journey is about one"
        raise AssertionError(message)
    return release


@pytest.fixture(scope="session")
def gh() -> str:
    """The held `gh` on PATH, refusing — never skipping — where there is none."""
    release = _held_release()
    found = shutil.which("gh")
    if found is None:
        pytest.fail(
            f"`gh` is not on PATH. These journeys run the real `gh`, and need GitHub CLI "
            f"{release}: install it with `just install-tools gh`, or `just "
            f"install-gh {release}`, and put ~/.local/bin on PATH.",
            pytrace=False,
        )
    version = run([found, "--version"], timeout=60).stdout.strip()
    answered = run([found, "skill", "--help"], timeout=60)
    if answered.returncode != 0:
        pytest.fail(
            f"`gh skill` is missing from {found} ({version or 'no version'}): `gh skill` "
            f"arrived in GitHub CLI 2.100.0, and this repository holds gh at {release}. "
            f"Install it with `just install-tools gh`.",
            pytrace=False,
        )
    if f"gh version {release} " not in f"{version} ":
        pytest.fail(
            f"{found} answers {version.splitlines()[0] if version else 'no version'}, and this "
            f"journey is a claim about gh {release}, the release `repo-policy.toml` holds. "
            f"Install it with `just install-tools gh`.",
            pytrace=False,
        )
    return found
