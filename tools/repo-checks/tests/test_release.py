"""What this repository publishes, and whether the path that publishes it can run."""

# `assert` is how pytest states an assertion and how it produces the failure
# message a reader acts on; suppressions.toml carries the reason.

from __future__ import annotations

from collections.abc import Callable

import pytest
from repo_checks.checks_release import (
    _publishable_crates,
    release_automation,
    release_dispatch,
    release_gating,
    release_targets,
)
from repo_checks.expect import accepted, equal, refused
from repo_checks.model import Repo
from treecopy import Tree

RELEASE = ".github/workflows/release-plz.yml"
TARGETS = "release-targets.toml"


def test_the_committed_declaration_is_accepted(committed: Repo) -> None:
    """One crate target per publishable crate, and nothing else at this node."""
    accepted(release_targets(committed))


def test_the_publishable_set_is_derived_from_the_workspace(committed: Repo) -> None:
    """The check reads the workspace, not the declaration it is comparing against."""
    equal(sorted(_publishable_crates(committed)), sorted(committed.crate_names))


def test_a_declaration_omitting_a_publishable_crate_is_refused(
    tree: Callable[[], Tree],
) -> None:
    """A crate nobody declared is a crate consumers cannot name."""
    broken = tree()
    broken.edit(
        TARGETS,
        '[[target]]\nid = "crate:printobserver-obico"\n'
        'manifest = "crates/printobserver-obico/Cargo.toml"\n',
        "",
    )

    findings = release_targets(broken.repo)

    refused(findings, "omits publishable crate")


def test_a_declaration_naming_an_unbacked_target_is_refused(
    tree: Callable[[], Tree],
) -> None:
    """A target no workspace member backs is a promise nothing can keep."""
    broken = tree()
    broken.write(
        TARGETS,
        broken.read(TARGETS) + '\n[[target]]\nid = "crate:printobserver-ghost"\n'
        'manifest = "crates/printobserver-types/Cargo.toml"\n',
    )

    findings = release_targets(broken.repo)

    refused(findings, "no workspace member backs")


def test_a_target_of_a_registry_nothing_publishes_to_is_refused(
    tree: Callable[[], Tree],
) -> None:
    """A target nothing knows how to publish is a name with no path to a consumer."""
    broken = tree()
    broken.write(
        TARGETS,
        broken.read(TARGETS)
        + '\n[[target]]\nid = "conda:printobserver"\ndescription = "Somewhere else."\n',
    )

    findings = release_targets(broken.repo)

    refused(findings, "conda")


def test_automation_that_is_not_conventional_commit_driven_is_refused(
    tree: Callable[[], Tree],
) -> None:
    """A release rule that refuses `feat` refuses the subjects this plan publishes under."""
    broken = tree()
    broken.edit("release-plz.toml", "^(feat|fix|perf)", "^(fix|perf)")

    findings = release_targets(broken.repo)

    refused(findings, "does not admit `feat`")


def test_a_release_rule_admitting_a_non_releasing_type_is_refused(
    tree: Callable[[], Tree],
) -> None:
    """A `chore` that releases opens an auto-merge loop with itself."""
    broken = tree()
    broken.edit("release-plz.toml", "^(feat|fix|perf)", "^(feat|fix|perf|chore)")

    findings = release_targets(broken.repo)

    refused(findings, "releases from `chore`")


def test_a_version_field_outside_the_automation_owned_set_is_refused(
    tree: Callable[[], Tree],
) -> None:
    """A version a person would hand-maintain is the thing releasing exists to remove."""
    broken = tree()
    broken.edit(
        "npm/printobserver-sdk/package.json",
        '"private": true,',
        '"private": true,\n  "version": "0.1.0",',
    )

    findings = release_targets(broken.repo)

    refused(findings, "carries a version field")


def test_the_committed_release_path_is_accepted(committed: Repo) -> None:
    """The release path exists, fires by itself, and covers every declared target."""
    accepted(release_automation(committed))


def test_no_release_workflow_at_all_is_refused(tree: Callable[[], Tree]) -> None:
    """An inert configuration is not a release path."""
    broken = tree()
    broken.remove(RELEASE)

    findings = release_automation(broken.repo)

    refused(findings, "no committed workflow performs releases")


def test_a_manual_only_release_workflow_is_refused(tree: Callable[[], Tree]) -> None:
    """A release somebody has to press is a manual deploy step."""
    broken = tree()
    broken.edit(RELEASE, "on:\n  push:\n    branches: [main]\n", "on:\n  workflow_dispatch:\n")

    findings = release_automation(broken.repo)

    refused(findings, "only trigger is manual invocation")


def test_a_release_workflow_with_no_release_step_is_refused(
    tree: Callable[[], Tree],
) -> None:
    """A workflow that describes a release in prose releases nothing."""
    broken = tree()
    broken.write(
        RELEASE,
        broken.read(RELEASE).replace("run: release-plz ", "run: true # was: release-plz "),
    )

    findings = release_automation(broken.repo)

    refused(findings, "no committed workflow performs releases")


def test_a_release_that_builds_nothing_beside_the_crates_is_refused(
    tree: Callable[[], Tree],
) -> None:
    """The three end-user routes are artifacts release automation has to build."""
    broken = tree()
    broken.edit(".github/workflows/release-plz.yml", "- run: just build-artifacts", "- run: true")

    findings = release_automation(broken.repo)

    refused(findings, "no committed job builds the artifacts beside the crates")


def test_a_step_that_halts_for_a_person_is_refused(tree: Callable[[], Tree]) -> None:
    """Nothing may stop between a merge and a target reaching its registry."""
    broken = tree()
    broken.edit(
        RELEASE,
        "  release:\n    name: release\n",
        "  release:\n    name: release\n    environment: production\n",
    )

    findings = release_automation(broken.repo)

    refused(findings, "halt for a person")


def test_a_release_program_the_toolchain_does_not_install_is_refused(
    tree: Callable[[], Tree],
) -> None:
    """A workflow naming a program nothing installs fails after a merge."""
    broken = tree()
    broken.edit(
        "repo-policy.toml",
        'command = "release-plz"',
        'command = "cargo-release"',
    )

    findings = release_automation(broken.repo)

    refused(findings, "toolchain does not install")


def test_the_committed_gating_is_accepted(committed: Repo) -> None:
    """Publishing waits on no drafting, and the artifacts follow the answer."""
    accepted(release_gating(committed))


def test_a_publishing_job_that_waits_on_the_drafting_job_is_refused(
    tree: Callable[[], Tree],
) -> None:
    """A drafting job that cannot draft must not be able to stop a publication that is ready."""
    broken = tree()
    broken.edit(
        RELEASE,
        "  release:\n    name: release\n    runs-on: ubuntu-24.04\n",
        "  release:\n    name: release\n    needs: release-pr\n    runs-on: ubuntu-24.04\n",
    )

    findings = release_gating(broken.repo)

    refused(findings, "waits on `release-pr`, which drafts the next one")


def test_a_release_step_that_does_not_say_what_it_released_is_refused(
    tree: Callable[[], Tree],
) -> None:
    """Without the answer there is nothing to gate the artifacts on but an exit status."""
    broken = tree()
    broken.edit(
        RELEASE,
        "release-plz release --backend github --output json >",
        "release-plz release --backend github >",
    )

    findings = release_gating(broken.repo)

    refused(findings, "without `--output json`")


def test_a_publishing_job_that_reads_its_answer_into_no_output_is_refused(
    tree: Callable[[], Tree],
) -> None:
    """The answer has to reach the jobs gated on it, and a job output is how."""
    broken = tree()
    broken.edit(
        RELEASE,
        "      - id: answer\n        if: github.event_name == 'push'\n"
        "        run: just release-answer",
        "      - run: true\n      - if: github.event_name == 'push'\n"
        "        run: just release-answer",
    )
    broken.edit(
        RELEASE,
        "released: ${{ steps.answer.outputs.released || steps.dispatched.outputs.released }}",
        "released: ${{ steps.dispatched.outputs.released }}",
    )

    findings = release_gating(broken.repo)

    refused(findings, "carries no `id`")
    refused(findings, "publishes no output `released`")


def test_an_artifact_build_that_runs_whether_or_not_a_release_was_cut_is_refused(
    tree: Callable[[], Tree],
) -> None:
    """The registries refuse a version they already serve, so the next push goes red."""
    broken = tree()
    broken.edit(
        RELEASE,
        "    if: needs.release.outputs.released != ''\n    strategy:\n",
        "    strategy:\n",
    )

    findings = release_gating(broken.repo)

    refused(findings, "runs `just build-artifacts` and is not gated on")


def test_an_artifact_publish_gated_on_the_exit_status_is_refused(
    tree: Callable[[], Tree],
) -> None:
    """The exit status is zero whether or not a release was cut, so it gates nothing."""
    broken = tree()
    broken.edit(
        RELEASE,
        "    needs: [release, artifacts]\n    if: needs.release.outputs.released != ''\n",
        "    needs: [release, artifacts]\n    if: needs.release.result == 'success'\n",
    )

    findings = release_gating(broken.repo)

    refused(findings, "runs `just publish-artifacts` and is not gated on")
    refused(findings, "reads a job's exit status rather than what it answered")


def test_an_artifact_publish_that_does_not_wait_on_the_publishing_job_is_refused(
    tree: Callable[[], Tree],
) -> None:
    """A job reads the outputs of the jobs it names and no others."""
    broken = tree()
    broken.edit(RELEASE, "    needs: [release, artifacts]\n", "    needs: artifacts\n")

    findings = release_gating(broken.repo)

    refused(findings, "does not wait on `release`")


def test_a_reader_that_no_longer_declares_the_gated_field_is_refused(
    tree: Callable[[], Tree],
) -> None:
    """The workflow, the policy and the module that answers the field name one field."""
    broken = tree()
    broken.edit(
        "tools/release-artifacts/src/release_artifacts/registries.py",
        'RELEASED_FIELD = "released"',
        'RELEASED_FIELD = "cut"',
    )

    findings = release_gating(broken.repo)

    refused(findings, "declares no `released`")


def test_a_gating_recipe_the_justfile_does_not_declare_is_refused(
    tree: Callable[[], Tree],
) -> None:
    """A workflow step running a recipe nothing declares fails after a merge."""
    broken = tree()
    broken.edit("justfile", "release-answer ANSWER:", "release-read ANSWER:")

    findings = release_gating(broken.repo)

    refused(findings, "declares no `release-answer` recipe")


def test_a_gating_check_with_no_publishing_step_at_all_is_refused(
    tree: Callable[[], Tree],
) -> None:
    """A workflow that publishes nothing has nothing to gate the artifacts on."""
    broken = tree()
    broken.write(
        RELEASE,
        broken.read(RELEASE).replace("run: release-plz ", "run: true # was: release-plz "),
    )

    findings = release_gating(broken.repo)

    refused(findings, "no committed job runs `release-plz release`")


def test_a_publishing_job_with_no_reading_step_is_refused(tree: Callable[[], Tree]) -> None:
    """An answer nothing reads gates nothing."""
    broken = tree()
    broken.edit(RELEASE, "        run: just release-answer", "        run: just release-dry-run")

    findings = release_gating(broken.repo)

    refused(findings, "runs no `just release-answer` step")


def test_a_reading_step_that_reaches_no_job_output_is_refused(tree: Callable[[], Tree]) -> None:
    """The one line the recipe answers has to land in the job's output file."""
    broken = tree()
    broken.edit(
        RELEASE, '"$RUNNER_TEMP/released.json" >> "$GITHUB_OUTPUT"', '"$RUNNER_TEMP/released.json"'
    )

    findings = release_gating(broken.repo)

    refused(findings, "does not append to `$GITHUB_OUTPUT`")


def test_a_gating_policy_naming_no_reader_is_refused(tree: Callable[[], Tree]) -> None:
    """The policy names the module that reads the answer, and it has to be there."""
    broken = tree()
    broken.edit(
        "repo-policy.toml",
        'answer_source = "tools/release-artifacts/src/release_artifacts/registries.py"',
        'answer_source = "tools/release-artifacts/src/release_artifacts/answers.py"',
    )

    findings = release_gating(broken.repo)

    refused(findings, "commits no such file")


def test_a_gating_policy_missing_a_name_is_refused(tree: Callable[[], Tree]) -> None:
    """A check cannot hold the workflow to a name the policy does not declare."""
    broken = tree()
    broken.edit("repo-policy.toml", 'answer_output = "released"\n', "")

    findings = release_gating(broken.repo)

    refused(findings, "declares no `release.answer_output` string")


#: Fragments of the committed release workflow the dispatched shape is made of,
#: as the workflow spells them; each test below takes one away or changes it.
PUSH_ONLY = "    if: github.event_name == 'push'\n"
DISPATCHED_STEP = (
    "      - id: dispatched\n"
    "        if: github.event_name == 'workflow_dispatch'\n"
    "        env:\n"
    "          TAG: ${{ inputs.tag }}\n"
    "          REF: ${{ github.ref }}\n"
    '        run: just release-dispatched "$TAG" . "$RUNNER_TEMP/dispatched-release/version"'
    ' "$REF" >> "$GITHUB_OUTPUT"\n'
)
RELEASED_OUTPUT = (
    "released: ${{ steps.answer.outputs.released || steps.dispatched.outputs.released }}"
)
BUILD_CHECKOUT = (
    "      - uses: actions/checkout@v5\n        with:\n          ref: ${{ inputs.tag }}\n"
)
PUBLISH_VERSION = "          PRINTOBSERVER_PUBLISH_VERSION: ${{ inputs.tag }}\n"
RECORD_UPLOAD = "          name: dispatched-release\n"
WHOLE_HISTORY = (
    "      - uses: actions/checkout@v5\n"
    "        with:\n"
    "          fetch-depth: 0\n"
    "          token: ${{ secrets.RELEASE_PLZ_TOKEN }}\n"
    "      - uses: extractions/setup-just@v3\n"
)


def test_the_committed_dispatched_shape_is_accepted(committed: Repo) -> None:
    """A dispatch finishes an existing release through the same jobs a push runs."""
    accepted(release_dispatch(committed))


def test_a_dispatch_that_takes_no_required_tag_is_refused(tree: Callable[[], Tree]) -> None:
    """The tag is the whole of what a dispatcher names, so it is required and a string."""
    optional = tree()
    optional.edit(
        RELEASE, "        required: true\n        type: string\n", "        type: string\n"
    )
    refused(release_dispatch(optional.repo), "is not `required: true`")

    renamed = tree()
    renamed.edit(RELEASE, "    inputs:\n      tag:\n", "    inputs:\n      ref:\n")
    refused(release_dispatch(renamed.repo), "declares no `tag` input")

    untyped = tree()
    untyped.edit(RELEASE, "        type: string\n", "        type: boolean\n")
    refused(release_dispatch(untyped.repo), "is not `type: string`")


@pytest.mark.parametrize(
    ("program", "job"),
    [("release-plz release-pr", "release-pr"), ("release-plz release", "release")],
)
def test_a_release_program_that_runs_on_a_dispatch_is_refused(
    program: str, job: str, tree: Callable[[], Tree]
) -> None:
    """A dispatch finishes a release that exists: it drafts nothing and cuts nothing."""
    broken = tree()
    if job == "release-pr":
        broken.edit(RELEASE, f"    name: release-pr\n{PUSH_ONLY}", "    name: release-pr\n")
    else:
        broken.edit(
            RELEASE,
            f"      - {PUSH_ONLY.strip()}\n        run: {program} ",
            f"      - run: {program} ",
        )

    findings = release_dispatch(broken.repo)

    refused(findings, f"job `{job}` runs `{program}` without `github.event_name == 'push'`")


def test_a_dispatched_tag_that_reaches_the_artifact_jobs_unverified_is_refused(
    tree: Callable[[], Tree],
) -> None:
    """Nothing is built or published for a tag the verifying recipe did not answer."""
    unverified = tree()
    unverified.edit(RELEASE, DISPATCHED_STEP, "      - run: true\n")
    refused(release_dispatch(unverified.repo), "no step runs `just release-dispatched`")

    unread = tree()
    unread.edit(RELEASE, RELEASED_OUTPUT, "released: ${{ steps.answer.outputs.released }}")
    refused(release_dispatch(unread.repo), "publishes no output `released` from `steps.dispatched")

    nameless = tree()
    nameless.edit(RELEASE, "      - id: dispatched\n        if:", "      - if:")
    refused(release_dispatch(nameless.repo), "carries no `id`")

    ungated = tree()
    ungated.edit(
        RELEASE,
        "      - id: dispatched\n        if: github.event_name == 'workflow_dispatch'\n",
        "      - id: dispatched\n",
    )
    refused(
        release_dispatch(ungated.repo),
        "not conditioned on `github.event_name == 'workflow_dispatch'`",
    )

    unpublished = tree()
    unpublished.edit(RELEASE, '"$REF" >> "$GITHUB_OUTPUT"', '"$REF"')
    refused(release_dispatch(unpublished.repo), "does not append to `$GITHUB_OUTPUT`")


def test_a_verifying_step_outside_the_job_the_artifacts_are_gated_on_is_refused(
    tree: Callable[[], Tree],
) -> None:
    """The artifact jobs read one job's output; an answer elsewhere reaches them from nowhere."""
    broken = tree()
    broken.edit(RELEASE, DISPATCHED_STEP, "      - run: true\n")
    broken.edit(
        RELEASE,
        "  artifacts:\n",
        "  verify:\n    runs-on: ubuntu-24.04\n    outputs:\n"
        "      released: ${{ steps.dispatched.outputs.released }}\n    steps:\n"
        "      - uses: actions/checkout@v5\n        with:\n          fetch-depth: 0\n"
        f"{DISPATCHED_STEP}  artifacts:\n",
    )

    findings = release_dispatch(broken.repo)

    refused(findings, "is in a job the artifact jobs are not gated on")


def test_a_build_that_does_not_check_out_the_dispatched_tag_is_refused(
    tree: Callable[[], Tree],
) -> None:
    """A dispatch would otherwise build `main`'s tree and publish it as the tag's release."""
    broken = tree()
    broken.edit(RELEASE, BUILD_CHECKOUT, "      - uses: actions/checkout@v5\n")

    findings = release_dispatch(broken.repo)

    refused(findings, "runs `just build-artifacts` and its checkout does not take `ref:")


def test_a_publish_not_handed_the_dispatched_version_is_refused(tree: Callable[[], Tree]) -> None:
    """A dispatch would otherwise publish the tag's artifacts under `main`'s own version."""
    broken = tree()
    broken.edit(RELEASE, PUBLISH_VERSION, "")

    findings = release_dispatch(broken.repo)

    refused(findings, "does not hand the recipe `PRINTOBSERVER_PUBLISH_VERSION` from `inputs.tag`")


def test_a_record_uploaded_under_another_name_is_refused(tree: Callable[[], Tree]) -> None:
    """The proof downloads the record by the declared name, and would find nothing."""
    renamed = tree()
    renamed.edit(RELEASE, RECORD_UPLOAD, "          name: dispatched\n")
    refused(
        release_dispatch(renamed.repo), "uploads the dispatched release's record as `dispatched`"
    )

    missing = tree()
    missing.edit(
        RELEASE,
        "      - uses: actions/upload-artifact@v4\n"
        "        if: github.event_name == 'workflow_dispatch'\n",
        "      - uses: actions/upload-artifact@v4\n        if: github.event_name == 'push'\n",
    )
    refused(release_dispatch(missing.repo), "uploads no artifact on a dispatch")


def test_a_verifying_job_checking_out_less_than_the_whole_history_is_refused(
    tree: Callable[[], Tree],
) -> None:
    """A shallow checkout of `main` carries no tag, and would refuse every dispatch there is.

    The copy the end-to-end tier drives cannot tell this defect from the
    committed shape — it carries its tags already — so this is where it is
    caught.
    """
    shallow = tree()
    shallow.edit(
        RELEASE,
        WHOLE_HISTORY,
        "      - uses: actions/checkout@v5\n        with:\n"
        "          token: ${{ secrets.RELEASE_PLZ_TOKEN }}\n"
        "      - uses: extractions/setup-just@v3\n",
    )
    refused(release_dispatch(shallow.repo), "does not carry `fetch-depth: 0`")

    narrowed = tree()
    narrowed.edit(
        RELEASE, WHOLE_HISTORY, WHOLE_HISTORY.replace("fetch-depth: 0", "fetch-depth: 50")
    )
    refused(release_dispatch(narrowed.repo), "does not carry `fetch-depth: 0`")


def test_a_concurrency_that_no_longer_keys_on_the_ref_is_refused(tree: Callable[[], Tree]) -> None:
    """A dispatched run and a push-triggered run of one ref must not publish at once."""
    unkeyed = tree()
    unkeyed.edit(RELEASE, "  group: release-plz-${{ github.ref }}\n", "  group: release-plz\n")
    refused(release_dispatch(unkeyed.repo), "does not read `github.ref`")

    cancelling = tree()
    cancelling.edit(RELEASE, "  cancel-in-progress: false\n", "  cancel-in-progress: true\n")
    refused(release_dispatch(cancelling.repo), "cancels a run in progress")


def test_a_dispatch_recipe_the_justfile_does_not_declare_is_refused(
    tree: Callable[[], Tree],
) -> None:
    """A workflow step running a recipe nothing declares fails after a merge."""
    verifying = tree()
    verifying.edit(
        "justfile",
        "release-dispatched TAG ROOT RECORD REF:",
        "release-verify TAG ROOT RECORD REF:",
    )
    refused(release_dispatch(verifying.repo), "declares no `release-dispatched` recipe")

    reading = tree()
    reading.edit("justfile", "release-version-dispatched RECORD:", "release-recorded RECORD:")
    refused(release_dispatch(reading.repo), "declares no `release-version-dispatched` recipe")


def test_a_publisher_reading_another_variable_is_refused(tree: Callable[[], Tree]) -> None:
    """Workflow, policy and publisher name one variable, or a dispatch publishes nothing."""
    renamed = tree()
    renamed.edit(
        "tools/release-artifacts/src/release_artifacts/publishing.py",
        'PRINTOBSERVER_PUBLISH_VERSION = "PRINTOBSERVER_PUBLISH_VERSION"',
        'PRINTOBSERVER_PUBLISH_VERSION = "PRINTOBSERVER_RELEASE_VERSION"',
    )
    refused(release_dispatch(renamed.repo), "declares no `PRINTOBSERVER_PUBLISH_VERSION`")

    elsewhere = tree()
    elsewhere.edit(
        "repo-policy.toml",
        'publish_version_source = "tools/release-artifacts/src/release_artifacts/publishing.py"',
        'publish_version_source = "tools/release-artifacts/src/release_artifacts/publisher.py"',
    )
    refused(release_dispatch(elsewhere.repo), "commits no such file")


def test_a_dispatch_policy_missing_a_name_is_refused(tree: Callable[[], Tree]) -> None:
    """A check cannot hold the workflow to a name the policy does not declare."""
    broken = tree()
    broken.edit("repo-policy.toml", 'record_artifact = "dispatched-release"\n', "")

    findings = release_dispatch(broken.repo)

    refused(findings, "declares no `release.record_artifact` string")


def test_a_dispatch_check_with_no_publishing_step_at_all_is_refused(
    tree: Callable[[], Tree],
) -> None:
    """A workflow that publishes nothing has no dispatched shape to hold."""
    broken = tree()
    broken.write(
        RELEASE,
        broken.read(RELEASE).replace("run: release-plz ", "run: true # was: release-plz "),
    )

    findings = release_dispatch(broken.repo)

    refused(findings, "no committed job runs `release-plz release`")
