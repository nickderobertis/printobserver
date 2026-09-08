"""The declared jobs are drawn from the recipe set and the install-path section."""

# `assert` is how pytest states an assertion and how it produces the failure
# message a reader acts on; suppressions.toml carries the reason.
# ruff: noqa: S101

from __future__ import annotations

from collections.abc import Callable

from repo_checks.checks_ci import continuous_integration
from repo_checks.model import Repo
from treecopy import Tree

CI = ".github/workflows/ci.yml"
INSTALL = ".github/workflows/install-path.yml"
FOURTH_ROUTE = """
#### Route 4 — a distribution channel nobody built

Made executable by nobody.

```console
brew install printobserver
```
"""


def test_the_committed_configuration_is_accepted(committed: Repo) -> None:
    """The jobs this repository ships agree with the recipes and the section."""
    assert continuous_integration(committed) == []


def test_a_missing_gate_job_is_refused(tree: Callable[[], Tree]) -> None:
    """A configuration with no complete-gate job proves nothing."""
    broken = tree()
    broken.edit(CI, "      - run: just check\n", "")

    findings = continuous_integration(broken.repo)

    assert any("no complete-gate job" in finding for finding in findings), findings


def test_a_gate_job_omitting_the_bootstrap_recipe_is_refused(
    tree: Callable[[], Tree],
) -> None:
    """A gate that never bootstrapped is a gate over somebody else's environment."""
    broken = tree()
    broken.edit(CI, "      - run: just bootstrap\n", "      - run: just install-tools\n")

    findings = continuous_integration(broken.repo)

    assert any("no complete-gate job" in finding for finding in findings), findings


def test_a_gate_step_naming_an_undeclared_recipe_is_refused(
    tree: Callable[[], Tree],
) -> None:
    """A gate job cannot invoke a recipe the set does not declare."""
    broken = tree()
    broken.edit(
        CI, "      - run: just check\n", "      - run: just check\n      - run: just verify\n"
    )

    findings = continuous_integration(broken.repo)

    assert any("which the recipe set does not declare" in finding for finding in findings), findings


def test_a_gate_step_running_something_that_is_not_a_recipe_is_refused(
    tree: Callable[[], Tree],
) -> None:
    """The gate job's steps are drawn from the recipe set, not composed beside it."""
    broken = tree()
    broken.edit(
        CI,
        "      - run: just check\n",
        "      - run: just check\n      - run: cargo test --workspace\n",
    )

    findings = continuous_integration(broken.repo)

    assert any("is not one of this repository's recipes" in finding for finding in findings), (
        findings
    )


def test_a_missing_judged_lint_job_is_refused(tree: Callable[[], Tree]) -> None:
    """The judged tier has to exist somewhere."""
    broken = tree()
    broken.edit(CI, "      - run: just lint-llm-diff\n", "")

    findings = continuous_integration(broken.repo)

    assert any("no judged-lint job" in finding for finding in findings), findings


def test_the_judged_tier_as_a_step_of_the_gate_job_is_refused(
    tree: Callable[[], Tree],
) -> None:
    """A non-deterministic tier inside the gate job stops the gate being deterministic."""
    broken = tree()
    broken.edit(
        CI,
        "      - run: just check\n",
        "      - run: just check\n      - run: just lint-llm-diff\n",
    )

    findings = continuous_integration(broken.repo)

    assert any("rather than a job of its own" in finding for finding in findings), findings


def test_a_route_with_no_install_job_is_refused(tree: Callable[[], Tree]) -> None:
    """The section is the source: a route it gains needs a job of its own."""
    broken = tree()
    broken.edit(
        "AGENTS.md",
        "### Then, in order — two commands",
        FOURTH_ROUTE + "\n### Then, in order — two commands",
    )

    findings = continuous_integration(broken.repo)

    assert any(
        "for which the committed configuration declares no install job" in finding
        for finding in findings
    ), findings


def test_a_missing_install_job_is_refused(tree: Callable[[], Tree]) -> None:
    """One job per route, because the routes are alternatives to one another."""
    broken = tree()
    broken.edit(INSTALL, "      - run: npm install -g printobserver-cli\n", "")

    findings = continuous_integration(broken.repo)

    assert any("npm install -g printobserver-cli" in finding for finding in findings), findings


def test_an_install_job_omitting_one_of_the_two_commands_is_refused(
    tree: Callable[[], Tree],
) -> None:
    """A route proven without the commands after it proves half the path."""
    broken = tree()
    broken.edit(
        INSTALL,
        "      - run: pip install printobserver-cli\n"
        "      - run: curl -fsSL https://raw.githubusercontent.com/nickderobertis/"
        "printobserver/main/scripts/install-service.sh | sudo sh\n",
        "      - run: pip install printobserver-cli\n",
    )

    findings = continuous_integration(broken.repo)

    assert any("omits `curl -fsSL" in finding for finding in findings), findings


def test_an_install_step_the_section_does_not_state_is_refused(
    tree: Callable[[], Tree],
) -> None:
    """An install job's steps are derived from the section, not composed beside it."""
    broken = tree()
    broken.edit(
        INSTALL,
        "      - run: pip install printobserver-cli\n",
        "      - run: pip install printobserver-cli\n      - run: printobserver --version\n",
    )

    findings = continuous_integration(broken.repo)

    assert any("does not state" in finding for finding in findings), findings


def test_a_spelling_disagreement_is_refused(tree: Callable[[], Tree]) -> None:
    """The section and the job cannot disagree on how a command is spelled."""
    broken = tree()
    broken.edit(
        INSTALL,
        "      - run: sudo systemctl enable --now printobserver.service\n\n  install-route-npm:",
        "      - run: systemctl enable --now printobserver.service\n\n  install-route-npm:",
    )

    findings = continuous_integration(broken.repo)

    assert any("does not state" in finding for finding in findings), findings
