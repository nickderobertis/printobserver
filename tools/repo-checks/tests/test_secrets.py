"""Every secret a workflow names is one the manifest declares, spelled as it spells it."""

# `assert` is how pytest states an assertion and how it produces the failure
# message a reader acts on; suppressions.toml carries the reason.

from __future__ import annotations

from collections.abc import Callable

from closures import closure
from repo_checks.checks_ci import secrets
from repo_checks.expect import accepted, contains, refused
from repo_checks.model import Repo
from treecopy import Tree

#: The files the checks this module drives read; see `closures`.
CLOSURE = closure("secrets")

CI = ".github/workflows/ci.yml"
RELEASE = ".github/workflows/release-plz.yml"
JUDGE_ENV = "          ANTHROPIC_API_KEY: ${{ secrets.ANTHROPIC_API_KEY }}\n"


def test_the_committed_workflows_are_accepted(committed: Repo) -> None:
    """Every reference lands inside gh-secrets.json."""
    accepted(secrets(committed))


def test_a_secret_outside_the_manifest_is_refused(tree: Callable[[], Tree]) -> None:
    """A workflow naming a secret this repository does not hold fails after a merge."""
    broken = tree()
    broken.edit(CI, JUDGE_ENV, JUDGE_ENV + "          SLACK_TOKEN: ${{ secrets.SLACK_TOKEN }}\n")

    findings = secrets(broken.repo)

    refused(findings, "SLACK_TOKEN")


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

    refused(findings, "built-in token")
    refused(findings, "RELEASE_PLZ_TOKEN")


def test_a_publishing_job_handed_the_built_in_token_is_refused(tree: Callable[[], Tree]) -> None:
    """The job that tags a release is refused the token as the drafting job is."""
    broken = tree()
    broken.edit(
        RELEASE,
        "          GIT_TOKEN: ${{ secrets.RELEASE_PLZ_TOKEN }}\n"
        "          CARGO_REGISTRY_TOKEN: ${{ secrets.CARGO_REGISTRY_TOKEN }}\n"
        "      - id: answer",
        "          GIT_TOKEN: ${{ github.token }}\n"
        "          CARGO_REGISTRY_TOKEN: ${{ secrets.CARGO_REGISTRY_TOKEN }}\n"
        "      - id: answer",
    )

    refused(secrets(broken.repo), "job `release` runs the release program")


def test_the_built_in_token_declared_for_the_whole_workflow_is_refused(
    tree: Callable[[], Tree],
) -> None:
    """A workflow-level `env` reaches every job, the drafting and publishing ones among them."""
    broken = tree()
    broken.edit(
        RELEASE,
        "permissions:\n  contents: read\n",
        "permissions:\n  contents: read\n\nenv:\n  GITHUB_TOKEN: ${{ github.token }}\n",
    )

    refused(secrets(broken.repo), "outside its jobs")


def test_the_reporting_job_alone_may_hand_the_built_in_token_to_a_step(committed: Repo) -> None:
    """The committed reporting job names it, under `issues: write`, and is accepted."""
    workflow = committed.path(RELEASE).read_text(encoding="utf-8")
    reporting = workflow[workflow.index("  release-health:") : workflow.index("  artifacts:")]

    contains(reporting, "GITHUB_TOKEN: ${{ github.token }}", describing="the reporting job")
    accepted(secrets(committed))


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

    refused(findings, "no crate-publishing credential")


def test_a_judged_lint_job_with_no_judge_credential_is_refused(
    tree: Callable[[], Tree],
) -> None:
    """A judge that cannot authenticate reports success having judged nothing."""
    broken = tree()
    broken.edit(CI, JUDGE_ENV, '          LLMLINT_QUIET: "1"\n')

    findings = secrets(broken.repo)

    refused(findings, "authenticates with 0 of")


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

    refused(findings, "authenticates with 2 of")


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

    refused(findings, "authenticates with 3 of")
