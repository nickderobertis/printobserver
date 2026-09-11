"""The declared jobs are drawn from the recipe set and the install-path section."""

# `assert` is how pytest states an assertion and how it produces the failure
# message a reader acts on; suppressions.toml carries the reason.

from __future__ import annotations

from collections.abc import Callable

from repo_checks.checks_ci import continuous_integration
from repo_checks.expect import accepted, refused
from repo_checks.model import Repo
from treecopy import Tree

CI = ".github/workflows/ci.yml"
INSTALL = ".github/workflows/install-path.yml"
#: The install-service step of the first install job, as the workflow spells it.
SERVICE_STEP = (
    "      - id: install-service\n"
    "        continue-on-error: true\n"
    "        run: curl -fsSL https://raw.githubusercontent.com/nickderobertis/"
    "printobserver/main/scripts/install-service.sh | sudo sh\n"
)

#: The step that starts it, as the workflow spells it.
START_STEP = (
    "      - id: start-service\n"
    "        continue-on-error: true\n"
    "        run: sudo systemctl enable --now printobserver.service\n"
)

FOURTH_ROUTE = """
#### Route 4 — a distribution channel nobody built

Made executable by nobody.

```console
brew install printobserver
```
"""


def test_the_committed_configuration_is_accepted(committed: Repo) -> None:
    """The jobs this repository ships agree with the recipes and the section."""
    accepted(continuous_integration(committed))


def test_a_missing_gate_job_is_refused(tree: Callable[[], Tree]) -> None:
    """A configuration with no complete-gate job proves nothing."""
    broken = tree()
    broken.edit(CI, "      - run: just check\n", "")

    findings = continuous_integration(broken.repo)

    refused(findings, "no complete-gate job")


def test_a_gate_job_omitting_the_bootstrap_recipe_is_refused(
    tree: Callable[[], Tree],
) -> None:
    """A gate that never bootstrapped is a gate over somebody else's environment."""
    broken = tree()
    broken.edit(CI, "      - run: just bootstrap\n", "      - run: just install-tools\n")

    findings = continuous_integration(broken.repo)

    refused(findings, "no complete-gate job")


def test_a_gate_step_naming_an_undeclared_recipe_is_refused(
    tree: Callable[[], Tree],
) -> None:
    """A gate job cannot invoke a recipe the set does not declare."""
    broken = tree()
    broken.edit(
        CI, "      - run: just check\n", "      - run: just check\n      - run: just verify\n"
    )

    findings = continuous_integration(broken.repo)

    refused(findings, "which the recipe set does not declare")


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

    refused(findings, "is not one of this repository's recipes")


def test_a_missing_judged_lint_job_is_refused(tree: Callable[[], Tree]) -> None:
    """The judged tier has to exist somewhere."""
    broken = tree()
    broken.edit(CI, "      - run: just lint-llm-diff\n", "")

    findings = continuous_integration(broken.repo)

    refused(findings, "no judged-lint job")


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

    refused(findings, "rather than a job of its own")


def test_a_route_with_no_install_job_is_refused(tree: Callable[[], Tree]) -> None:
    """The section is the source: a route it gains needs a job of its own."""
    broken = tree()
    broken.edit(
        "AGENTS.md",
        "### Then, in order — two commands",
        FOURTH_ROUTE + "\n### Then, in order — two commands",
    )

    findings = continuous_integration(broken.repo)

    refused(findings, "for which the committed configuration declares no install job")


def test_a_missing_install_job_is_refused(tree: Callable[[], Tree]) -> None:
    """One job per route, because the routes are alternatives to one another."""
    broken = tree()
    broken.edit(INSTALL, "      - run: npm install -g printobserver-cli\n", "")

    findings = continuous_integration(broken.repo)

    refused(findings, "npm install -g printobserver-cli")


def test_an_install_job_omitting_one_of_the_two_commands_is_refused(
    tree: Callable[[], Tree],
) -> None:
    """A route proven without the commands after it proves half the path."""
    broken = tree()
    broken.edit(INSTALL, SERVICE_STEP, "")

    findings = continuous_integration(broken.repo)

    refused(findings, "omits `curl -fsSL")


def test_an_install_job_omitting_the_check_on_what_it_installed_is_refused(
    tree: Callable[[], Tree],
) -> None:
    """A job that only installs cannot tell a working install from a broken one.

    Which is the defect this whole workflow had: it took every route from every
    registry and never ran what any of them put on the path.
    """
    broken = tree()
    broken.edit(INSTALL, "      - run: printobserver --version\n", "")

    findings = continuous_integration(broken.repo)

    refused(findings, "omits `printobserver --version`")


def test_an_install_job_checking_before_the_route_it_installed_is_refused(
    tree: Callable[[], Tree],
) -> None:
    """Run first, the check reads whatever was already on the host."""
    broken = tree()
    broken.edit(
        INSTALL,
        "      - run: pip install printobserver-cli\n      - run: printobserver --version\n",
        "      - run: printobserver --version\n      - run: pip install printobserver-cli\n",
    )

    findings = continuous_integration(broken.repo)

    refused(findings, "before the route it is checking")


def test_an_install_job_checking_after_the_service_commands_is_refused(
    tree: Callable[[], Tree],
) -> None:
    """A program that does not run must not reach a service before anything looked."""
    broken = tree()
    broken.edit(INSTALL, "      - run: printobserver --version\n", "")
    broken.edit(INSTALL, START_STEP, START_STEP + "      - run: printobserver --version\n")

    findings = continuous_integration(broken.repo)

    refused(findings, "after the commands that put the service in place")


def test_an_install_job_running_the_two_commands_out_of_order_is_refused(
    tree: Callable[[], Tree],
) -> None:
    """Starting the service before the installer has written it starts nothing.

    AGENTS.md states the two in one order and says why they are two commands
    rather than one; a job running them the other way round proves a path
    nobody documented.
    """
    broken = tree()
    broken.edit(INSTALL, SERVICE_STEP + START_STEP, START_STEP + SERVICE_STEP)

    findings = continuous_integration(broken.repo)

    refused(findings, "out of the order AGENTS.md states")


def test_an_install_job_that_swallows_the_service_commands_silently_is_refused(
    tree: Callable[[], Tree],
) -> None:
    """The waiver's price: a completed run says what the two commands reached.

    Their failure not failing the job is what keeps this workflow from failing
    on every run whatever the world looks like. Unreported, that buys a green
    job saying nothing about whether the service was ever established — which
    is a credential-dependent failure disappearing.
    """
    broken = tree()
    broken.edit(
        INSTALL,
        "        run: 'echo \"install-route-pypi: service installation $INSTALLED, "
        'service startup $STARTED" >> "$GITHUB_STEP_SUMMARY"\'',
        "        run: true",
    )

    findings = continuous_integration(broken.repo)

    refused(findings, "cannot then tell a route whose service was established")


def test_an_install_job_whose_waived_step_is_fatal_is_refused(
    tree: Callable[[], Tree],
) -> None:
    """Made fatal, the job fails on every run — which is how it went unread."""
    broken = tree()
    broken.edit(INSTALL, START_STEP, START_STEP.replace("        continue-on-error: true\n", ""))

    findings = continuous_integration(broken.repo)

    refused(findings, "is fatal")


def test_an_install_job_whose_waived_step_carries_no_id_is_refused(
    tree: Callable[[], Tree],
) -> None:
    """Nothing of the run can report what a step it cannot name reached."""
    broken = tree()
    broken.edit(INSTALL, "      - id: install-service\n", "      - id: installs-the-service\n")

    findings = continuous_integration(broken.repo)

    refused(findings, "declares no `install-service` step")


def test_a_tree_declaring_no_waiver_at_all_is_refused(tree: Callable[[], Tree]) -> None:
    """Nothing here guesses which steps may fail without failing their job."""
    broken = tree()
    broken.edit(
        "repo-policy.toml",
        'waived_steps = ["install-service", "start-service"]',
        "waived_steps = []",
    )

    findings = continuous_integration(broken.repo)

    refused(findings, "declares no `waived_steps`")


def test_an_install_step_the_section_does_not_state_is_refused(
    tree: Callable[[], Tree],
) -> None:
    """An install job's steps are derived from the section, not composed beside it."""
    broken = tree()
    broken.edit(
        INSTALL,
        "      - run: pip install printobserver-cli\n",
        "      - run: pip install printobserver-cli\n      - run: printobserver server\n",
    )

    findings = continuous_integration(broken.repo)

    refused(findings, "does not state")


def test_a_spelling_disagreement_is_refused(tree: Callable[[], Tree]) -> None:
    """The section and the job cannot disagree on how a command is spelled."""
    broken = tree()
    broken.edit(
        INSTALL,
        "        run: sudo systemctl enable --now printobserver.service\n",
        "        run: systemctl enable --now printobserver.service\n",
    )

    findings = continuous_integration(broken.repo)

    refused(findings, "does not state")
