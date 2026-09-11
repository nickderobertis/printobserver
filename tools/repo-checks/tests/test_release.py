"""What this repository publishes, and whether the path that publishes it can run."""

# `assert` is how pytest states an assertion and how it produces the failure
# message a reader acts on; suppressions.toml carries the reason.

from __future__ import annotations

from collections.abc import Callable

from repo_checks.checks_release import (
    _publishable_crates,
    release_automation,
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
        broken.read(RELEASE).replace("      - run: release-plz ", "      # was: release-plz "),
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
        "      - id: answer\n        run: just release-answer",
        "      - run: true\n      - run: just release-answer",
    )
    broken.edit(RELEASE, "released: ${{ steps.answer.outputs.released }}", "released: ''")

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
        broken.read(RELEASE).replace("      - run: release-plz ", "      # was: release-plz "),
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
