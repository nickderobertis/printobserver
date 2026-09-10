"""The registry install-path proof: one recipe per route, on a release and a schedule.

Every journey here drives the committed check over a real copy of the committed
tree with one defect in it. Nothing is mocked: the check reads the recipe set,
the release declaration, the committed workflow and `AGENTS.md`, and those are
the files this repository ships.
"""

# `assert` is how pytest states an assertion and how it produces the failure
# message a reader acts on; suppressions.toml carries the reason.

from __future__ import annotations

from collections.abc import Callable

from repo_checks.checks_install_proof import install_proof
from repo_checks.expect import accepted, refused
from repo_checks.model import Repo
from treecopy import Tree

WORKFLOW = ".github/workflows/install-path.yml"
POLICY = "repo-policy.toml"
JUSTFILE = "justfile"
AGENTS = "AGENTS.md"

#: The success gate every job of the proof carries, as the workflow spells it.
GATE = (
    "    if: github.event_name != 'workflow_run' || "
    "github.event.workflow_run.conclusion == 'success'\n"
)


def test_the_committed_tree_is_accepted(committed: Repo) -> None:
    """The tier this repository ships is declared, recorded and out of the gate."""
    accepted(install_proof(committed))


def test_a_tree_declaring_no_install_proof_is_refused(tree: Callable[[], Tree]) -> None:
    """Nothing here guesses which recipes prove the routes against their registries."""
    broken = tree()
    broken.edit(POLICY, "[install_proof]", "[install-proof-was-here]")

    refused(install_proof(broken.repo), "declares no `[install_proof]` section")


def test_a_declaration_missing_a_key_is_refused(tree: Callable[[], Tree]) -> None:
    """A half-declared tier is a check that would inspect nothing."""
    broken = tree()
    broken.edit(POLICY, 'release_workflow = "release-plz"', 'release_workflow = ""')

    refused(install_proof(broken.repo), "declares no release_workflow")


def test_a_route_with_no_registry_proof_is_refused(tree: Callable[[], Tree]) -> None:
    """A route nothing proves against its own registry is a route nothing proves."""
    broken = tree()
    broken.edit(
        JUSTFILE,
        "prove-registry-npm:\n    uv run -q python -m release_artifacts prove --registry "
        "--target npm:printobserver-cli --into dist/proof/registry-npm",
        "prove-registry-npm:\n    echo nothing",
    )

    refused(install_proof(broken.repo), "no `prove-registry-` recipe proves what")


def test_a_tier_that_leaves_one_route_out_is_refused(tree: Callable[[], Tree]) -> None:
    """A run of the tier by hand takes all three routes, not two of them."""
    broken = tree()
    broken.edit(JUSTFILE, "    just prove-registry-script\n", "")

    refused(install_proof(broken.repo), "leaves one of the three routes unproven")


def test_the_gate_declaring_this_tier_is_refused(tree: Callable[[], Tree]) -> None:
    """Over a change it could only report what was published before that change."""
    broken = tree()
    broken.edit(POLICY, '    "test-e2e",\n]', '    "test-e2e",\n    "test-install-proof",\n]')

    refused(install_proof(broken.repo), "which is the registry install-path proof")


def test_the_check_recipe_invoking_this_tier_is_refused(tree: Callable[[], Tree]) -> None:
    """The gate stays the tier a developer runs to know whether a change is ready."""
    broken = tree()
    broken.edit(
        JUSTFILE, "    just test-e2e\n", "    just test-e2e\n    just prove-registry-pypi\n"
    )

    refused(install_proof(broken.repo), "belongs to the registry install-path proof")


def test_a_trigger_that_fires_on_a_change_is_refused(tree: Callable[[], Tree]) -> None:
    """A change-fired copy of this tier reports what was published before the change."""
    broken = tree()
    broken.edit(WORKFLOW, "on:\n  workflow_dispatch:", "on:\n  pull_request:\n  workflow_dispatch:")

    refused(install_proof(broken.repo), "fires on `pull_request`")


def test_keying_the_proof_on_the_release_being_published_is_refused(
    tree: Callable[[], Tree],
) -> None:
    """The release is cut before its artifacts are built and published."""
    broken = tree()
    broken.edit(
        WORKFLOW,
        "  workflow_run:\n    workflows: [release-plz]\n    types: [completed]\n",
        "  release:\n    types: [published]\n",
    )

    refused(install_proof(broken.repo), "declares no `workflow_run` trigger")


def test_keying_the_proof_on_another_workflow_finishing_is_refused(
    tree: Callable[[], Tree],
) -> None:
    """A proof keyed anywhere earlier than the publish measures the version before it."""
    broken = tree()
    broken.edit(WORKFLOW, "    workflows: [release-plz]", "    workflows: [artifacts]")

    refused(install_proof(broken.repo), "having finished")


def test_naming_a_release_workflow_no_committed_workflow_carries_is_refused(
    tree: Callable[[], Tree],
) -> None:
    """A trigger naming a workflow that is not there fires on nothing."""
    broken = tree()
    broken.edit(POLICY, 'release_workflow = "release-plz"', 'release_workflow = "release-it"')
    broken.edit(WORKFLOW, "    workflows: [release-plz]", "    workflows: [release-it]")

    refused(install_proof(broken.repo), "no committed workflow carries that name")


def test_a_job_not_gated_on_the_release_succeeding_is_refused(
    tree: Callable[[], Tree],
) -> None:
    """A release run that failed published nothing, and proving it blames the artifact."""
    broken = tree()
    broken.edit(WORKFLOW, GATE, "")

    refused(install_proof(broken.repo), "is not gated on")


def test_a_manual_run_that_cannot_name_a_version_is_refused(
    tree: Callable[[], Tree],
) -> None:
    """The caller names the version a run proves, or nothing can prove an old one."""
    broken = tree()
    broken.edit(
        WORKFLOW,
        "  workflow_dispatch:\n    inputs:\n      version:",
        "  workflow_dispatch:\n    inputs:\n      which:",
    )

    refused(install_proof(broken.repo), "cannot name the version a run proves")


def test_a_version_that_ignores_what_the_caller_named_is_refused(
    tree: Callable[[], Tree],
) -> None:
    """A caller who names a version has it proven rather than the newest served."""
    broken = tree()
    broken.edit(WORKFLOW, "${{ inputs.version ||", "${{ ('' ||")

    refused(install_proof(broken.repo), "does not take the version a caller named")


def test_a_release_run_that_proves_whatever_is_newest_is_refused(
    tree: Callable[[], Tree],
) -> None:
    """A release's own proof proves that release, not whatever a registry serves."""
    broken = tree()
    broken.edit(WORKFLOW, "&& 'release' ||", "&& '' ||")

    refused(install_proof(broken.repo), "does not select `release`")


def test_a_version_that_cannot_tell_the_release_trigger_apart_is_refused(
    tree: Callable[[], Tree],
) -> None:
    """Which trigger fired is what decides where the version comes from."""
    broken = tree()
    broken.edit(
        WORKFLOW,
        "${{ inputs.version || (github.event_name == 'workflow_run' && 'release' || '') }}",
        "${{ inputs.version || (github.event_name == 'schedule' && '' || 'release') }}",
    )

    refused(install_proof(broken.repo), "does not tell the")


def test_a_job_declaring_a_version_of_its_own_is_refused(tree: Callable[[], Tree]) -> None:
    """The version under test is one answer for the whole run, not one per route."""
    broken = tree()
    broken.edit(
        WORKFLOW,
        "      - run: just prove-registry-npm\n",
        "      - run: just prove-registry-npm\n    env:\n"
        "      PRINTOBSERVER_PROOF_VERSION: 0.0.1\n",
    )

    refused(install_proof(broken.repo), "declares `PRINTOBSERVER_PROOF_VERSION` of its own")


def test_the_workflow_declaring_no_version_at_all_is_refused(
    tree: Callable[[], Tree],
) -> None:
    """The version under test has to reach the tier somehow."""
    broken = tree()
    broken.edit(WORKFLOW, "  PRINTOBSERVER_PROOF_VERSION: >-", "  UNREAD_BY_THE_TIER: >-")

    refused(install_proof(broken.repo), "which is how the version under test reaches")


def test_a_proof_recipe_with_no_job_is_refused(tree: Callable[[], Tree]) -> None:
    """A recipe nothing runs is a route nothing proves."""
    broken = tree()
    broken.edit(WORKFLOW, "      - run: just prove-registry-script\n", "      - run: true\n")

    refused(install_proof(broken.repo), "declares no job that runs `just prove-registry-script`")


def test_a_schedule_the_prose_does_not_record_is_refused(tree: Callable[[], Tree]) -> None:
    """A schedule recorded in prose cannot drift from the one that fires."""
    broken = tree()
    broken.edit(WORKFLOW, '    - cron: "0 6 * * 1"', '    - cron: "0 7 * * 2"')

    refused(install_proof(broken.repo), "does not record")


def test_a_recorded_schedule_the_workflow_does_not_declare_is_refused(
    tree: Callable[[], Tree],
) -> None:
    """And it cannot drift the other way either."""
    broken = tree()
    broken.edit(
        AGENTS,
        "[//]: # (BEGIN install-proof-schedule)\n- cron: `0 6 * * 1`",
        "[//]: # (BEGIN install-proof-schedule)\n- cron: `0 6 * * 1`\n- cron: `0 9 * * 4`",
    )

    refused(install_proof(broken.repo), "which .github/workflows/install-path.yml does not declare")


def test_a_schedule_recorded_in_another_form_is_refused(tree: Callable[[], Tree]) -> None:
    """One recorded form, so nothing has to guess what a line meant."""
    broken = tree()
    broken.edit(
        AGENTS,
        "[//]: # (BEGIN install-proof-schedule)\n- cron: `0 6 * * 1`",
        "[//]: # (BEGIN install-proof-schedule)\n- weekly, on Monday morning",
    )

    refused(install_proof(broken.repo), "which is not of the form")


def test_a_tree_recording_no_section_for_the_tier_is_refused(
    tree: Callable[[], Tree],
) -> None:
    """A tier outside the gate with nothing saying so is a tier nobody runs."""
    broken = tree()
    broken.edit(AGENTS, "## The registry install-path proof", "## Something else entirely")

    refused(install_proof(broken.repo), "carries no `## The registry install-path proof` section")


def test_a_section_that_does_not_say_how_to_run_it_by_hand_is_refused(
    tree: Callable[[], Tree],
) -> None:
    """A reader who has to reconstruct the command is a reader who runs one route."""
    broken = tree()
    # Every place the section states it, because a section that states it once
    # anywhere has told the reader how to run the tier.
    broken.write(
        AGENTS, broken.read(AGENTS).replace("just test-install-proof", "the install-proof tier")
    )

    refused(install_proof(broken.repo), "states no `just test-install-proof` command")


def test_a_section_that_does_not_say_how_the_version_is_named_is_refused(
    tree: Callable[[], Tree],
) -> None:
    """A run by hand proves a version, and the section says which and how."""
    broken = tree()
    broken.write(
        AGENTS,
        broken.read(AGENTS).replace("PRINTOBSERVER_PROOF_VERSION", "the version variable"),
    )

    refused(install_proof(broken.repo), "does not say how the version under test is named")


def test_a_workflow_that_is_not_there_is_refused(tree: Callable[[], Tree]) -> None:
    """A declared workflow that is absent is a tier on no trigger at all."""
    broken = tree()
    broken.remove(WORKFLOW)

    refused(install_proof(broken.repo), "which is not there")
