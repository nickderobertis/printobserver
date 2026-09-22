"""Every Python project type-checks for its own host and for every declared platform."""

from __future__ import annotations

from collections.abc import Callable

import pytest
from repo_checks.checks_repo import python_typecheck_platforms
from repo_checks.expect import accepted, refused, refused_naming
from repo_checks.model import Repo
from treecopy import Tree

SDK = "python/printobserver-sdk/project.json"
HOST_INVOCATION = "uv run -q ty check python/printobserver-sdk"
WIN32_INVOCATION = "uv run -q ty check --python-platform win32 python/printobserver-sdk"


def test_the_committed_python_projects_are_accepted(committed: Repo) -> None:
    """Every project this repository ships runs both of its passes."""
    accepted(python_typecheck_platforms(committed))


def test_a_project_running_no_win32_pass_is_refused(tree: Callable[[], Tree]) -> None:
    """A target back at one pass reports a POSIX-only attribute from no Unix host."""
    broken = tree()
    broken.edit(SDK, f"{HOST_INVOCATION} && {WIN32_INVOCATION}", HOST_INVOCATION)

    findings = python_typecheck_platforms(broken.repo)

    refused_naming(findings, "printobserver-sdk-python:typecheck", "no `win32` pass")


def test_a_project_running_the_win32_pass_alone_is_refused(tree: Callable[[], Tree]) -> None:
    """The second pass is beside the host's own rather than instead of it."""
    broken = tree()
    broken.edit(SDK, f"{HOST_INVOCATION} && {WIN32_INVOCATION}", WIN32_INVOCATION)

    findings = python_typecheck_platforms(broken.repo)

    refused(findings, "runs no `ty check python/printobserver-sdk` pass for this host")


def test_a_project_checking_another_project_for_win32_is_refused(
    tree: Callable[[], Tree],
) -> None:
    """A pass over somebody else's root leaves this project's own code unchecked."""
    broken = tree()
    broken.edit(
        SDK,
        WIN32_INVOCATION,
        "uv run -q ty check --python-platform win32 tools/repo-checks",
    )

    findings = python_typecheck_platforms(broken.repo)

    refused_naming(findings, "no `win32` pass over python/printobserver-sdk")


def test_a_python_project_declaring_no_typecheck_target_is_refused(
    tree: Callable[[], Tree],
) -> None:
    """A project joins the graph by declaring the target every other project declares."""
    broken = tree()
    broken.edit(
        "tools/octoprint-env/project.json",
        '    "typecheck": {\n      "command": "uv run -q ty check tools/octoprint-env '
        '&& uv run -q ty check --python-platform win32 tools/octoprint-env"\n    },\n',
        "",
    )

    findings = python_typecheck_platforms(broken.repo)

    refused(findings, "octoprint-env is a Python project declaring no `typecheck` command")


def test_a_platform_added_to_the_policy_is_required_of_every_project(
    tree: Callable[[], Tree],
) -> None:
    """The platforms are the policy's, so every target is held to the list it names."""
    broken = tree()
    broken.edit("repo-policy.toml", 'platforms = ["win32"]', 'platforms = ["win32", "darwin"]')

    findings = python_typecheck_platforms(broken.repo)

    refused_naming(findings, "printobserver-sdk-python:typecheck", "no `darwin` pass")


@pytest.mark.parametrize("declared", ['platforms = "win32"', "platforms = []"])
def test_a_policy_declaring_no_platform_list_is_refused(
    tree: Callable[[], Tree], declared: str
) -> None:
    """The check reads one declaration, and says so rather than raising when it is not one."""
    broken = tree()
    broken.edit("repo-policy.toml", 'platforms = ["win32"]', declared)

    findings = python_typecheck_platforms(broken.repo)

    refused(findings, "declares no `toolchain.python_typecheck.platforms` list")


def test_a_project_carrying_no_python_tag_is_left_alone(tree: Callable[[], Tree]) -> None:
    """What a project is held to is the language it declares itself in."""
    relabelled = tree()
    relabelled.edit(SDK, '"lang:python"', '"lang:ruby"')
    relabelled.edit(SDK, f"{HOST_INVOCATION} && {WIN32_INVOCATION}", HOST_INVOCATION)

    accepted(python_typecheck_platforms(relabelled.repo))
