"""The release path is one the release program actually accepts.

Reading a workflow cannot establish that its steps would run, so this journey
invokes the release program itself — with the argument list the committed step
carries, in the program's own mode that publishes nothing — over the committed
tree, and over a copy whose release configuration carries a setting the program
rejects.
"""

# `assert` is how pytest states an assertion and how it produces the failure
# message a reader acts on; suppressions.toml carries the reason.
# ruff: noqa: S101

from __future__ import annotations

import shutil
from collections.abc import Callable

import pytest
import yaml
from journey import REPO_ROOT, GateCopy, output, run

RELEASE_WORKFLOW = REPO_ROOT / ".github/workflows/release-plz.yml"
REJECTED_SETTING = "not_a_release_plz_setting"
ARGUMENT_ERRORS = ("unexpected argument", "unrecognized subcommand", "invalid value for")
CONFIG_ERRORS = ("invalid config file", "unknown field", "TOML parse error")


def release_step_arguments() -> list[str]:
    """The argument list the committed release step carries."""
    workflow = yaml.safe_load(RELEASE_WORKFLOW.read_text(encoding="utf-8"))
    for job in workflow["jobs"].values():
        for step in job.get("steps", []):
            command = str(step.get("run", "")).strip()
            if command.startswith("release-plz release "):
                return command.split()
    message = "no committed step runs `release-plz release`"
    raise AssertionError(message)


pytestmark = pytest.mark.skipif(
    shutil.which("release-plz") is None,
    reason="release-plz is installed by `just install-tools`; run bootstrap first",
)


def test_the_committed_step_runs_the_release_program_the_toolchain_installs() -> None:
    """The step names a program `just install-tools` puts on PATH."""
    assert release_step_arguments()[0] == "release-plz"
    assert "release-plz" in (REPO_ROOT / "repo-policy.toml").read_text(encoding="utf-8")


def test_the_program_accepts_the_committed_arguments_and_configuration() -> None:
    """Neither the argument list nor the release configuration is one it rejects."""
    result = run(
        [*release_step_arguments(), "--dry-run", "--git-token", "dry-run-has-no-token"],
        cwd=REPO_ROOT,
        timeout=300,
    )
    said = output(result)

    assert "using release-plz config file release-plz.toml" in said, said
    for error in ARGUMENT_ERRORS + CONFIG_ERRORS:
        assert error not in said, said


def test_a_configuration_the_program_rejects_is_refused_naming_it(
    gate_copy: Callable[[], GateCopy],
) -> None:
    """The same invocation over a tree whose release configuration it will not take."""
    broken = gate_copy()
    broken.edit(
        "release-plz.toml",
        "semver_check = true",
        f"semver_check = true\n{REJECTED_SETTING} = true",
    )

    result = run(
        [*release_step_arguments(), "--dry-run", "--git-token", "dry-run-has-no-token"],
        cwd=broken.root,
        timeout=300,
    )
    said = output(result)

    assert result.returncode != 0, said
    assert "invalid config file" in said, said
    assert REJECTED_SETTING in said, said
