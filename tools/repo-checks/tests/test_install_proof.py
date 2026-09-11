"""The registry install-path proof: one recipe per route, on a release and a schedule.

Every journey here drives the committed check over a real copy of the committed
tree with one defect in it. Nothing is mocked: the check reads the recipe set,
the release declaration, the committed workflow and `AGENTS.md`, and those are
the files this repository ships.
"""

from __future__ import annotations

from collections.abc import Callable

from repo_checks.checks_install_proof import install_proof
from repo_checks.checks_release import release_dispatch
from repo_checks.expect import accepted, refused
from repo_checks.model import Repo
from treecopy import Tree

WORKFLOW = ".github/workflows/install-path.yml"
POLICY = "repo-policy.toml"
JUSTFILE = "justfile"
AGENTS = "AGENTS.md"

#: The gate every job of the proof carries, as the workflow spells it: the
#: release the triggering run cut, and not that run's conclusion.
GATE = "    if: github.event_name != 'workflow_run' || needs.resolve.outputs.version != ''\n"

#: The version every job proving a route takes, as the workflow spells it.
VERSION = (
    "      PRINTOBSERVER_PROOF_VERSION: ${{ inputs.version || needs.resolve.outputs.version }}\n"
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


def test_a_declaration_missing_the_resolving_output_is_refused(
    tree: Callable[[], Tree],
) -> None:
    """Nothing composes the gate from a declaration that names half of it."""
    broken = tree()
    broken.edit(POLICY, 'release_output = "version"', 'release_output = ""')

    refused(install_proof(broken.repo), "declares no release_output")


def test_a_declaration_whose_field_is_not_a_string_is_refused(
    tree: Callable[[], Tree],
) -> None:
    """A malformed value coerced to a string passes as a declaration and names nothing."""
    broken = tree()
    broken.edit(POLICY, 'release_job = "resolve"', 'release_job = ["resolve"]')

    refused(install_proof(broken.repo), "declares no release_job")


def test_a_trigger_declared_as_something_other_than_its_settings_is_refused(
    tree: Callable[[], Tree],
) -> None:
    """A finding rather than a traceback: this check answers about the file it reads.

    `workflow_run` takes a mapping of settings. Reached into without being
    narrowed first, anything else raises out of the middle of the check — and a
    check whose whole job is to answer with findings, answering with an
    exception, is one nobody can act on.
    """
    broken = tree()
    broken.edit(
        WORKFLOW,
        "  workflow_run:\n    workflows: [release-plz]\n    types: [completed]\n",
        "  workflow_run: [completed]\n",
    )

    refused(install_proof(broken.repo), "which is not the mapping of settings")


def test_a_manual_invocation_declared_as_something_other_than_its_settings_is_refused(
    tree: Callable[[], Tree],
) -> None:
    """And the same for the trigger a caller names a version on."""
    broken = tree()
    dispatch = broken.read(WORKFLOW).partition("on:\n")[2].partition("  workflow_run:")[0]
    broken.edit(WORKFLOW, dispatch, "  workflow_dispatch: [version]\n")

    refused(install_proof(broken.repo), "which is not the mapping of settings")


def test_a_route_with_no_registry_proof_is_refused(tree: Callable[[], Tree]) -> None:
    """A route nothing proves against its own registry is a route nothing proves."""
    broken = tree()
    broken.edit(
        JUSTFILE,
        "prove-registry-npm:\n    @uv run -q python -m release_artifacts prove --registry "
        "--target npm:printobserver-cli --into dist/proof/registry-npm",
        "prove-registry-npm:\n    echo nothing",
    )

    refused(install_proof(broken.repo), "no `prove-registry-` recipe proves what")


def test_a_tier_that_leaves_one_route_out_is_refused(tree: Callable[[], Tree]) -> None:
    """A run of the tier by hand takes all three routes, not two of them."""
    broken = tree()
    broken.edit(JUSTFILE, "    @just prove-registry-script\n", "")

    refused(install_proof(broken.repo), "leaves one of the three routes unproven")


def test_a_tier_that_proves_a_route_quietly_still_proves_it(tree: Callable[[], Tree]) -> None:
    """`@` decides whether `just` echoes the line, not what the line does.

    The three route proofs are invoked quietly because a proof that passed is
    one line and the echoed command beside it is the only other thing such a
    run prints. Read without stripping that prefix, this check would report a
    tier that proves every route as proving none.
    """
    quiet = tree()
    quiet.edit(JUSTFILE, "    @just prove-registry-npm\n", "    just prove-registry-npm\n")

    accepted(install_proof(quiet.repo), describing="a tier invoking one route noisily")


def test_a_tier_whose_route_proof_cannot_fail_it_is_refused(tree: Callable[[], Tree]) -> None:
    """`-`, the other prefix `just` takes, is a different invocation and not one of these.

    That one ignores the line's failure, so a tier carrying it runs the route
    proof and passes whatever the proof answered — which is a route this
    repository reports nothing about while appearing to prove it.
    """
    broken = tree()
    broken.edit(JUSTFILE, "    @just prove-registry-npm\n", "    -just prove-registry-npm\n")

    refused(install_proof(broken.repo), "leaves one of the three routes unproven")


def test_a_gate_invoking_this_tier_quietly_is_refused(tree: Callable[[], Tree]) -> None:
    """And the same prefix on the other side is a hole rather than a false finding.

    A `check` recipe that reached this tier quietly would put the registries in
    front of every change, and read without stripping the prefix nothing would
    say so.
    """
    broken = tree()
    broken.edit(
        JUSTFILE, "    just test-e2e\n", "    just test-e2e\n    @just test-install-proof\n"
    )

    refused(install_proof(broken.repo), "belongs to the registry install-path proof")


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


def test_a_job_not_gated_on_the_release_its_run_cut_is_refused(
    tree: Callable[[], Tree],
) -> None:
    """A run that cut no release would otherwise prove whatever was newest."""
    broken = tree()
    broken.edit(WORKFLOW, GATE, "")

    refused(install_proof(broken.repo), "is not gated on")


def test_a_job_gated_on_the_triggering_run_s_conclusion_is_refused(
    tree: Callable[[], Tree],
) -> None:
    """The run that cut a release and failed to publish it is the one this tier is for.

    Gated on the conclusion it is skipped, and the missing publish is reported
    by nothing at all — which is why a conclusion gate is refused rather than
    merely not asked for.
    """
    broken = tree()
    broken.edit(
        WORKFLOW,
        GATE,
        "    if: github.event.workflow_run.conclusion == 'success'\n",
    )

    refused(install_proof(broken.repo), "the one state this tier exists to find")


def test_a_workflow_with_no_job_resolving_the_release_is_refused(
    tree: Callable[[], Tree],
) -> None:
    """Nothing else can say which release the triggering run cut."""
    broken = tree()
    broken.edit(POLICY, 'release_job = "resolve"', 'release_job = "which-release"')

    refused(install_proof(broken.repo), "declares no `which-release` job")


def test_a_resolving_job_that_does_not_read_the_tag_is_refused(
    tree: Callable[[], Tree],
) -> None:
    """The release a run cut is read off the tag it left, by this repository's own recipe."""
    broken = tree()
    broken.edit(
        WORKFLOW,
        '        run: just release-version "$COMMIT" . >> "$GITHUB_OUTPUT"',
        "        run: just --list",
    )

    refused(install_proof(broken.repo), "does not run `just release-version`")


def test_a_resolving_recipe_the_recipe_set_does_not_declare_is_refused(
    tree: Callable[[], Tree],
) -> None:
    """A recipe named in policy and absent from the command surface reads nothing."""
    broken = tree()
    broken.edit(JUSTFILE, "release-version COMMIT ROOT:", "release-version-of COMMIT ROOT:")

    refused(install_proof(broken.repo), "is not a recipe the recipe set declares")


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
    """A release's own proof proves the release its run cut, not whatever is newest."""
    broken = tree()
    broken.edit(WORKFLOW, "|| needs.resolve.outputs.version }}", "|| '' }}")

    refused(install_proof(broken.repo), "not the release the triggering run cut")


def test_routes_taking_different_versions_are_refused(tree: Callable[[], Tree]) -> None:
    """The version under test is one answer for the whole run, not one per route."""
    broken = tree()
    broken.edit(WORKFLOW, VERSION, VERSION.replace("inputs.version ||", "inputs.version || '' ||"))

    refused(install_proof(broken.repo), "is one answer for the whole run")


def test_a_version_declared_for_the_whole_workflow_is_refused(
    tree: Callable[[], Tree],
) -> None:
    """The release it names is a job's answer, which nothing outside a job can read."""
    broken = tree()
    broken.edit(
        WORKFLOW,
        "permissions:\n  contents: read\n",
        "permissions:\n  contents: read\n\nenv:\n  PRINTOBSERVER_PROOF_VERSION: 0.0.1\n",
    )

    refused(install_proof(broken.repo), "declares `PRINTOBSERVER_PROOF_VERSION` for the whole")


def test_a_job_that_proves_no_route_declaring_a_version_is_refused(
    tree: Callable[[], Tree],
) -> None:
    """A version reaching a job that proves nothing with it says nothing about a run."""
    broken = tree()
    broken.edit(
        WORKFLOW,
        "    name: install-route-pypi (${{ matrix.platform.id }})\n",
        "    name: install-route-pypi (${{ matrix.platform.id }})\n"
        "    env:\n      PRINTOBSERVER_PROOF_VERSION: 0.0.1\n",
    )

    refused(install_proof(broken.repo), "and proves no route with it")


def test_a_proof_job_declaring_no_version_at_all_is_refused(
    tree: Callable[[], Tree],
) -> None:
    """The version under test has to reach the tier somehow."""
    broken = tree()
    broken.edit(WORKFLOW, VERSION, "      UNREAD_BY_THE_TIER: nothing\n")

    refused(install_proof(broken.repo), "declares no `PRINTOBSERVER_PROOF_VERSION`")


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


def test_a_tier_recipe_the_recipe_set_does_not_declare_is_refused(
    tree: Callable[[], Tree],
) -> None:
    """A tier declared in policy and absent from the command surface runs nothing."""
    broken = tree()
    broken.edit(JUSTFILE, "test-install-proof:", "test-registry-install-proof:")

    refused(install_proof(broken.repo), "which the recipe set does not declare")


def test_a_route_with_no_target_behind_it_is_refused(tree: Callable[[], Tree]) -> None:
    """A route with no declared target names no registry to prove it against."""
    broken = tree()
    broken.edit("release-targets.toml", 'route = "Route 2 — the JavaScript package registry"\n', "")

    refused(install_proof(broken.repo), "declares no target behind it")


def test_a_schedule_naming_no_cron_expression_is_refused(tree: Callable[[], Tree]) -> None:
    """A tier out of the gate and on no schedule is a tier nobody runs."""
    broken = tree()
    broken.edit(WORKFLOW, '  schedule:\n    - cron: "0 6 * * 1"\n', "  schedule:\n")

    refused(install_proof(broken.repo), "names no cron expression")


def test_a_tree_recording_no_schedule_block_is_refused(tree: Callable[[], Tree]) -> None:
    """The recorded schedule and the one that fires are checked against each other."""
    broken = tree()
    broken.edit(AGENTS, "[//]: # (BEGIN install-proof-schedule)", "[//]: # (a block, removed)")

    refused(install_proof(broken.repo), "carries no `install-proof-schedule` marker block")


def test_a_declaration_whose_triggers_are_not_a_list_of_events_is_refused(
    tree: Callable[[], Tree],
) -> None:
    """A malformed trigger list would leave the rule reaching no event at all."""
    broken = tree()
    broken.edit(
        POLICY,
        'triggers = ["workflow_run", "schedule", "workflow_dispatch"]',
        'triggers = "everything"',
    )

    refused(install_proof(broken.repo), "declares no triggers")


def test_a_consumer_naming_another_variable_is_refused(tree: Callable[[], Tree]) -> None:
    """The policy, the workflow and the prose could agree while the code did not."""
    broken = tree()
    broken.edit(
        "tools/release-artifacts/src/release_artifacts/registries.py",
        'PRINTOBSERVER_PROOF_REGISTRIES = "PRINTOBSERVER_PROOF_REGISTRIES"',
        'PRINTOBSERVER_PROOF_REGISTRIES = "PRINTOBSERVER_REGISTRY_STAND_IN"',
    )

    refused(install_proof(broken.repo), "declares no `PRINTOBSERVER_PROOF_REGISTRIES`")


def test_a_consumer_renaming_the_selector_out_from_under_the_policy_is_refused(
    tree: Callable[[], Tree],
) -> None:
    """The word a caller types to prove the newest release is one word, in one place.

    Moved on the code's side alone, `PRINTOBSERVER_PROOF_VERSION=release` is a
    version string nothing serves and the run answers `NOT SERVED` over a
    release that is fine.
    """
    broken = tree()
    broken.edit(
        "tools/release-artifacts/src/release_artifacts/registries.py",
        'RELEASE = "release"',
        'RELEASE = "newest"',
    )

    refused(install_proof(broken.repo), "and `repo-policy.toml` declares `release`")


def test_a_consumer_declaring_no_selector_constant_at_all_is_refused(
    tree: Callable[[], Tree],
) -> None:
    """A gate keyed on a constant that is not there reconciles nothing.

    The constant is what the declaration names, rather than the bare word:
    `"release"` is also the registry a forge listing is read from in that same
    module, so a check hunting the literal would find one of those.
    """
    broken = tree()
    broken.edit(
        "tools/release-artifacts/src/release_artifacts/registries.py",
        'RELEASE = "release"',
        'NEWEST = "release"',
    )

    refused(install_proof(broken.repo), "declares no `RELEASE` constant")


def test_a_declaration_naming_a_consumer_that_is_not_there_is_refused(
    tree: Callable[[], Tree],
) -> None:
    """A drift gate over a file nothing commits reconciles nothing."""
    broken = tree()
    broken.edit(
        POLICY,
        'version_source = "tools/release-artifacts/src/release_artifacts/registries.py"',
        'version_source = "tools/release-artifacts/src/release_artifacts/registry.py"',
    )

    refused(install_proof(broken.repo), "commits no such file")


#: How the resolving job reads a dispatched run's record, as the workflow
#: spells it; each test below takes one piece away.
RECORD_DOWNLOAD = "          name: dispatched-release\n"
RECORD_RUN_ID = "          run-id: ${{ github.event.workflow_run.id }}\n"
RECORD_STEP = (
    "      - id: dispatched\n"
    "        if: github.event_name == 'workflow_run' && "
    "github.event.workflow_run.event == 'workflow_dispatch'\n"
    '        run: just release-version-dispatched "$RUNNER_TEMP/dispatched-release/version"'
    ' >> "$GITHUB_OUTPUT"\n'
)
RESOLVED_OUTPUT = "version: ${{ steps.cut.outputs.version || steps.dispatched.outputs.version }}"


def test_the_resolving_job_reads_a_dispatched_runs_record_as_committed(committed: Repo) -> None:
    """After a dispatched publish, the proof runs for the version that run recorded."""
    accepted(release_dispatch(committed))


def test_a_record_downloaded_under_another_name_is_refused(tree: Callable[[], Tree]) -> None:
    """The two workflows name one artifact, or the proof downloads nothing and proves nothing."""
    renamed = tree()
    renamed.edit(WORKFLOW, RECORD_DOWNLOAD, "          name: dispatched\n")
    refused(
        release_dispatch(renamed.repo),
        "downloads no artifact named `dispatched-release` (`dispatched`)",
    )

    missing = tree()
    missing.edit(
        WORKFLOW,
        "      - uses: actions/download-artifact@v4\n",
        "      - uses: actions/setup-node@v5\n",
    )
    refused(
        release_dispatch(missing.repo),
        "downloads no artifact named `dispatched-release` (none at all)",
    )


def test_a_record_downloaded_from_this_run_rather_than_the_triggering_one_is_refused(
    tree: Callable[[], Tree],
) -> None:
    """The record is the release run's, and this run uploaded none."""
    broken = tree()
    broken.edit(WORKFLOW, RECORD_RUN_ID, "")

    refused(release_dispatch(broken.repo), "without `run-id: ${{ github.event.workflow_run.id }}`")


def test_a_resolving_job_that_does_not_read_the_record_into_its_version_is_refused(
    tree: Callable[[], Tree],
) -> None:
    """The version a dispatched run published reaches the route proofs through this output."""
    unread = tree()
    unread.edit(WORKFLOW, RECORD_STEP, "      - run: true\n")
    refused(release_dispatch(unread.repo), "runs no `just release-version-dispatched` step")

    nameless = tree()
    nameless.edit(WORKFLOW, "      - id: dispatched\n        if:", "      - if:")
    refused(
        release_dispatch(nameless.repo), "`just release-version-dispatched` step carries no `id`"
    )

    unpublished = tree()
    unpublished.edit(
        WORKFLOW,
        '"$RUNNER_TEMP/dispatched-release/version" >> "$GITHUB_OUTPUT"',
        '"$RUNNER_TEMP/dispatched-release/version"',
    )
    refused(
        release_dispatch(unpublished.repo),
        "`just release-version-dispatched` step does not append to `$GITHUB_OUTPUT`",
    )

    unrouted = tree()
    unrouted.edit(WORKFLOW, RESOLVED_OUTPUT, "version: ${{ steps.cut.outputs.version }}")
    refused(
        release_dispatch(unrouted.repo),
        "publishes no output `version` from `steps.dispatched.outputs.version`",
    )


def test_a_proof_workflow_with_no_resolving_job_is_refused_by_the_dispatch_check(
    tree: Callable[[], Tree],
) -> None:
    """Nothing else can read a dispatched run's record."""
    renamed = tree()
    renamed.edit(POLICY, 'release_job = "resolve"', 'release_job = "which-release"')
    refused(release_dispatch(renamed.repo), "declares no `which-release` job")

    absent = tree()
    absent.edit(POLICY, 'workflow = "install-path.yml"', 'workflow = "install-proof.yml"')
    refused(release_dispatch(absent.repo), "install-proof.yml, which is not there")
