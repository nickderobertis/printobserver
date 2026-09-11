"""The supported-platform list is the one source every CI matrix is derived from."""

# `assert` is how pytest states an assertion and how it produces the failure
# message a reader acts on; suppressions.toml carries the reason.

from __future__ import annotations

from collections.abc import Callable

import yaml
from repo_checks.checks_ci import platform_dependent_kinds, platforms, platforms_of
from repo_checks.expect import (
    absent,
    accepted,
    contains,
    equal,
    refused,
    refused_naming,
    truth,
)
from repo_checks.model import Repo
from treecopy import Tree

EMPTY_BLOCK = "[//]: # (BEGIN supported-platforms)\n[//]: # (END supported-platforms)"
AARCH64 = (
    "- `linux-aarch64` — runner `ubuntu-24.04-arm`, Rust target "
    "`aarch64-unknown-linux-gnu`, service manager `systemd`, install path: yes\n"
)


MATRIX = """    strategy:
      fail-fast: false
      matrix:
        platform:
          - id: linux-x86_64
            runner: ubuntu-24.04
          - id: linux-aarch64
            runner: ubuntu-24.04-arm
"""


def test_the_committed_tree_is_accepted(committed: Repo) -> None:
    """Every committed matrix agrees with the list it is derived from."""
    accepted(platforms(committed))


def test_the_platform_dependent_kinds_are_the_ones_that_build_or_install(
    committed: Repo,
) -> None:
    """The judged tier is not one of them: it reads a text diff."""
    dependent = platform_dependent_kinds(committed)

    contains(dependent, "gate", describing="the platform-dependent kinds")
    contains(dependent, "integration", describing="the platform-dependent kinds")
    contains(dependent, "install", describing="the platform-dependent kinds")
    absent(dependent, "llmlint", describing="the platform-dependent kinds")


def test_the_committed_judged_lint_job_declares_no_matrix(committed: Repo) -> None:
    """One change, one judged verdict, one status context carrying no platform."""
    workflow = yaml.safe_load(
        (committed.root / ".github/workflows/ci.yml").read_text(encoding="utf-8")
    )
    judged = workflow["jobs"]["llmlint"]

    absent(judged, "strategy", describing="the judged-lint job")
    equal(judged["runs-on"], "ubuntu-24.04", describing="the judged-lint job's runner")
    truth(
        "${{" not in str(judged["runs-on"]),
        describing="the judged-lint job to name its runner outright, not from a matrix",
    )


def test_a_policy_whose_table_is_not_a_table_is_refused(
    tree: Callable[[], Tree],
) -> None:
    """A malformed declaration is a finding, not an attribute error on the way to one."""
    broken = tree()
    # `workflows` becomes a top-level string rather than a table, which is what a
    # reader calling `.get` on it without narrowing would trip over.
    broken.edit(
        "repo-policy.toml", "schema_version = 1\n", 'schema_version = 1\nworkflows = "yes"\n'
    )
    broken.edit("repo-policy.toml", "\n[workflows]\n", "\n[workflow-settings]\n")

    findings = platforms(broken.repo)

    refused(findings, "declares no non-empty `workflows.platform_dependent_kinds` list")


def test_a_matrix_cell_whose_id_is_not_a_platform_name_is_refused(
    tree: Callable[[], Tree],
) -> None:
    """It is reported, rather than aborting the tier on an unhashable key."""
    broken = tree()
    broken.edit(
        ".github/workflows/ci.yml",
        "          - id: linux-x86_64\n            runner: ubuntu-24.04\n",
        "          - id: [linux, x86_64]\n            runner: ubuntu-24.04\n",
    )

    findings = platforms(broken.repo)

    refused_naming(findings, "job `gate`", "whose `id` is not a platform name")


def test_a_policy_declaring_no_platform_dependent_kinds_is_refused(
    tree: Callable[[], Tree],
) -> None:
    """A rule that reaches no job passes by inspecting nothing."""
    broken = tree()
    broken.edit(
        "repo-policy.toml",
        'platform_dependent_kinds = ["gate", "integration", "install", "artifact"]',
        "platform_dependent_kinds = []",
    )

    findings = platforms(broken.repo)

    refused(findings, "declares no non-empty `workflows.platform_dependent_kinds` list")


def test_a_policy_naming_something_that_is_not_a_job_kind_is_refused(
    tree: Callable[[], Tree],
) -> None:
    """A kind no job can ever be is refused rather than coerced into one."""
    broken = tree()
    broken.edit(
        "repo-policy.toml",
        'platform_dependent_kinds = ["gate", "integration", "install", "artifact"]',
        'platform_dependent_kinds = ["gate", "smoke"]',
    )

    findings = platforms(broken.repo)

    refused(findings, "which is not one of the job kinds these checks classify")


def test_a_matrix_on_the_judged_lint_job_is_refused(tree: Callable[[], Tree]) -> None:
    """Two cells over one text diff are two rolls of a non-deterministic judge."""
    broken = tree()
    broken.edit(
        ".github/workflows/ci.yml",
        "  llmlint:\n    name: llmlint\n",
        "  llmlint:\n    name: llmlint\n" + MATRIX,
    )

    findings = platforms(broken.repo)

    refused_naming(findings, "job `llmlint`", "declares a platform matrix")


def test_the_list_is_non_empty_and_names_the_install_paths_platform(
    committed: Repo,
) -> None:
    """The list names the operating system and service manager the unit is written for."""
    declared = platforms_of(committed)

    truth(declared, describing="a non-empty supported-platform list")
    truth(
        any(p.install_path and p.service_manager == "systemd" for p in declared),
        describing="a systemd platform the end-user install path targets",
    )


def _replace_block(tree: Tree, replacement: str) -> None:
    text = tree.read("AGENTS.md")
    start = text.index("[//]: # (BEGIN supported-platforms)")
    end = text.index("[//]: # (END supported-platforms)") + len("[//]: # (END supported-platforms)")
    tree.write("AGENTS.md", text[:start] + replacement + text[end:])


def test_an_empty_list_is_refused(tree: Callable[[], Tree]) -> None:
    """A repository that supports nothing has no matrix to derive."""
    broken = tree()
    _replace_block(broken, EMPTY_BLOCK)

    findings = platforms(broken.repo)

    refused(findings, "is empty")


def test_a_list_omitting_the_install_paths_platform_is_refused(
    tree: Callable[[], Tree],
) -> None:
    """A list with no systemd platform cannot carry the unit the install path enables."""
    broken = tree()
    text = broken.read("AGENTS.md")
    _replace_block(
        broken,
        "[//]: # (BEGIN supported-platforms)\n"
        "- `linux-x86_64` — runner `ubuntu-24.04`, Rust target "
        "`x86_64-unknown-linux-gnu`, service manager `launchd`, install path: no\n"
        "[//]: # (END supported-platforms)",
    )
    truth(
        text != broken.read("AGENTS.md"),
        describing="the platform block to have been replaced by the fixture",
    )

    findings = platforms(broken.repo)

    refused(findings, "install path targets")


def test_a_matrix_naming_an_undeclared_platform_is_refused(
    tree: Callable[[], Tree],
) -> None:
    """A matrix cannot widen past the list it is derived from."""
    broken = tree()
    broken.edit(
        ".github/workflows/ci.yml",
        "          - id: linux-aarch64\n            runner: ubuntu-24.04-arm\n",
        "          - id: linux-aarch64\n            runner: ubuntu-24.04-arm\n"
        "          - id: windows-x86_64\n            runner: windows-2022\n",
    )

    findings = platforms(broken.repo)

    refused(findings, "windows-x86_64")


def test_a_matrix_omitting_a_declared_platform_is_refused(
    tree: Callable[[], Tree],
) -> None:
    """Neither matrix can be narrowed independently of the list."""
    broken = tree()
    broken.edit(
        ".github/workflows/ci.yml",
        "          - id: linux-aarch64\n            runner: ubuntu-24.04-arm\n",
        "",
    )

    findings = platforms(broken.repo)

    refused_naming(findings, "job `gate`", "omits platform `linux-aarch64`")


def test_a_list_gaining_a_platform_refuses_the_unchanged_matrices(
    tree: Callable[[], Tree],
) -> None:
    """The list is the source: widening it refuses matrices that did not follow."""
    broken = tree()
    broken.edit(
        "AGENTS.md",
        AARCH64,
        AARCH64
        + (
            "- `linux-riscv64` — runner `ubuntu-24.04-riscv`, Rust target "
            "`riscv64gc-unknown-linux-gnu`, service manager `systemd`, install path: yes\n"
        ),
    )

    findings = platforms(broken.repo)

    refused_naming(findings, "job `gate`", "omits platform `linux-riscv64`")
    refused_naming(findings, "job `install-route-pypi`", "omits platform `linux-riscv64`")


def test_a_platform_dependent_job_that_dropped_its_matrix_is_refused(
    tree: Callable[[], Tree],
) -> None:
    """Narrowing a build to one platform by deleting its matrix is still narrowing."""
    broken = tree()
    broken.edit(
        ".github/workflows/ci.yml",
        MATRIX + "    runs-on: ${{ matrix.platform.runner }}\n",
        "    runs-on: ubuntu-24.04\n",
    )

    findings = platforms(broken.repo)

    refused_naming(findings, "job `gate`", "declares no platform matrix")


def test_an_install_jobs_matrix_is_held_to_the_list_too(
    tree: Callable[[], Tree],
) -> None:
    """`install` is platform-dependent as well: a route proves nothing it never ran on."""
    broken = tree()
    # Anchored on the step that follows it, because that workflow's proof jobs
    # carry the same matrix: what this narrows is the install job's own.
    broken.edit(
        ".github/workflows/install-path.yml",
        "          - id: linux-aarch64\n            runner: ubuntu-24.04-arm\n"
        "    runs-on: ${{ matrix.platform.runner }}\n    steps:\n"
        "      - uses: actions/setup-python@v5\n",
        "    runs-on: ${{ matrix.platform.runner }}\n    steps:\n"
        "      - uses: actions/setup-python@v5\n",
    )

    findings = platforms(broken.repo)

    refused_naming(findings, "job `install-route-pypi`", "omits platform `linux-aarch64`")
