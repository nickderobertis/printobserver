"""What this repository publishes, and whether the path that publishes it can run."""

# `assert` is how pytest states an assertion and how it produces the failure
# message a reader acts on; suppressions.toml carries the reason.

from __future__ import annotations

from collections.abc import Callable

from repo_checks.checks_release import _publishable_crates, release_automation, release_targets
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


def test_a_non_crate_target_is_refused_at_this_node(tree: Callable[[], Tree]) -> None:
    """The distributions and the release artifacts belong to the `sdks` node."""
    broken = tree()
    broken.write(
        TARGETS,
        broken.read(TARGETS)
        + '\n[[target]]\nid = "pypi:printobserver-cli"\nmanifest = "pyproject.toml"\n',
    )

    findings = release_targets(broken.repo)

    refused(findings, "publishes crates and nothing else")


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


def test_a_target_with_no_publishing_step_is_refused(tree: Callable[[], Tree]) -> None:
    """Every declared target reaches its registry from a committed step."""
    broken = tree()
    broken.write(
        TARGETS,
        broken.read(TARGETS) + '\n[[target]]\nid = "npm:printobserver-cli"\n'
        'manifest = "npm/printobserver-sdk/package.json"\n',
    )

    findings = release_automation(broken.repo)

    refused(findings, "no committed publishing step covers")


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
