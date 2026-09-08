"""Every secret a workflow names is one the manifest declares, spelled as it spells it."""

# `assert` is how pytest states an assertion and how it produces the failure
# message a reader acts on; suppressions.toml carries the reason.
# ruff: noqa: S101

from __future__ import annotations

from collections.abc import Callable

from repo_checks.checks_ci import secrets
from repo_checks.model import Repo
from treecopy import Tree

CI = ".github/workflows/ci.yml"
RELEASE = ".github/workflows/release-plz.yml"
JUDGE_ENV = "          ANTHROPIC_API_KEY: ${{ secrets.ANTHROPIC_API_KEY }}\n"


def test_the_committed_workflows_are_accepted(committed: Repo) -> None:
    """Every reference lands inside gh-secrets.json."""
    assert secrets(committed) == []


def test_a_secret_outside_the_manifest_is_refused(tree: Callable[[], Tree]) -> None:
    """A workflow naming a secret this repository does not hold fails after a merge."""
    broken = tree()
    broken.edit(CI, JUDGE_ENV, JUDGE_ENV + "          SLACK_TOKEN: ${{ secrets.SLACK_TOKEN }}\n")

    findings = secrets(broken.repo)

    assert any("SLACK_TOKEN" in finding for finding in findings), findings


def test_release_automation_naming_the_built_in_token_is_refused(
    tree: Callable[[], Tree],
) -> None:
    """A change request opened by the built-in token does not trigger its own gates."""
    broken = tree()
    broken.write(
        RELEASE,
        broken.read(RELEASE).replace("secrets.RELEASE_PLZ_TOKEN", "secrets.GITHUB_TOKEN"),
    )

    findings = secrets(broken.repo)

    assert any("built-in token" in finding for finding in findings), findings
    assert any("RELEASE_PLZ_TOKEN" in finding for finding in findings), findings


def test_release_automation_without_a_crate_credential_is_refused(
    tree: Callable[[], Tree],
) -> None:
    """A release that reaches no registry is not a release."""
    broken = tree()
    broken.write(
        RELEASE,
        "\n".join(
            line for line in broken.read(RELEASE).splitlines() if "CARGO_REGISTRY_TOKEN" not in line
        )
        + "\n",
    )

    findings = secrets(broken.repo)

    assert any("no crate-publishing credential" in finding for finding in findings), findings


def test_a_judged_lint_job_with_no_judge_credential_is_refused(
    tree: Callable[[], Tree],
) -> None:
    """A judge that cannot authenticate reports success having judged nothing."""
    broken = tree()
    broken.edit(CI, JUDGE_ENV, '          LLMLINT_QUIET: "1"\n')

    findings = secrets(broken.repo)

    assert any("authenticates with 0 of" in finding for finding in findings), findings


def test_a_judged_lint_job_with_two_judge_credentials_is_refused(
    tree: Callable[[], Tree],
) -> None:
    """Two ways over is as far from one declared credential as none."""
    broken = tree()
    broken.edit(
        CI,
        JUDGE_ENV,
        JUDGE_ENV + "          OPENAI_API_KEY: ${{ secrets.OPENAI_API_KEY }}\n",
    )

    findings = secrets(broken.repo)

    assert any("authenticates with 2 of" in finding for finding in findings), findings


def test_a_judged_lint_job_with_three_judge_credentials_is_refused(
    tree: Callable[[], Tree],
) -> None:
    """And so is all three."""
    broken = tree()
    broken.edit(
        CI,
        JUDGE_ENV,
        JUDGE_ENV
        + "          OPENAI_API_KEY: ${{ secrets.OPENAI_API_KEY }}\n"
        + "          CLAUDE_CODE_OAUTH_TOKEN: ${{ secrets.CLAUDE_CODE_OAUTH_TOKEN }}\n",
    )

    findings = secrets(broken.repo)

    assert any("authenticates with 3 of" in finding for finding in findings), findings
