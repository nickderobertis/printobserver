"""The supported-platform list is the one source every CI matrix is derived from."""

# `assert` is how pytest states an assertion and how it produces the failure
# message a reader acts on; suppressions.toml carries the reason.

from __future__ import annotations

from collections.abc import Callable

from repo_checks.checks_ci import platforms, platforms_of
from repo_checks.expect import accepted, refused, truth
from repo_checks.model import Repo
from treecopy import Tree

EMPTY_BLOCK = "[//]: # (BEGIN supported-platforms)\n[//]: # (END supported-platforms)"
AARCH64 = (
    "- `linux-aarch64` — runner `ubuntu-24.04-arm`, Rust target "
    "`aarch64-unknown-linux-gnu`, service manager `systemd`, install path: yes\n"
)


def test_the_committed_tree_is_accepted(committed: Repo) -> None:
    """Both committed matrices agree with the list they are derived from."""
    accepted(platforms(committed))


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

    refused(findings, "omits platform `linux-aarch64`")


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

    refused(findings, "omits platform `linux-riscv64`")
