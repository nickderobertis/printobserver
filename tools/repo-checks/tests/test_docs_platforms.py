"""No document names a platform, or gives one a service manager, the list does not.

A document is the one place a claim about a platform goes on reading true after
the list moved under it: nothing installs a paragraph, so nobody meets the
disagreement until they follow it. This is the mechanical half of keeping every
document derived from that one list, and every refusal below is driven by
putting the claim into a real document of a real copy of this tree.
"""

from __future__ import annotations

from collections.abc import Callable

from repo_checks.checks_docs import platform_names
from repo_checks.expect import accepted, refused_naming
from repo_checks.model import Repo
from treecopy import Tree

TESTING = "docs/reference/testing.md"


def test_every_committed_document_agrees_with_the_list(committed: Repo) -> None:
    """No document of this tree names a platform the supported-platform list does not."""
    accepted(platform_names(committed))


def test_a_document_naming_a_platform_the_list_does_not_carry_is_refused(
    tree: Callable[[], Tree],
) -> None:
    """A document that promises a platform nothing builds for promises an install that fails."""
    broken = tree()
    broken.append(TESTING, "\nThe gate also runs on `macos-aarch64`.\n")

    findings = platform_names(broken.repo)

    refused_naming(findings, TESTING, "`macos-aarch64`", "does not carry")


def test_a_document_giving_a_platform_another_service_manager_is_refused(
    tree: Callable[[], Tree],
) -> None:
    """The service manager is the list's column, and a second answer to it is a second source."""
    broken = tree()
    broken.append(TESTING, "\nOn `linux-aarch64` the service runs under launchd.\n")

    findings = platform_names(broken.repo)

    refused_naming(findings, TESTING, "`linux-aarch64`", "`launchd`", "`systemd`")


def test_a_document_stating_the_service_manager_the_list_gives_is_accepted(
    tree: Callable[[], Tree],
) -> None:
    """Saying the right thing about a platform is what the check is for."""
    agreeing = tree()
    agreeing.append(TESTING, "\nOn `linux-aarch64` the service runs under systemd.\n")

    accepted(platform_names(agreeing.repo), describing="a document that agrees with the list")


def test_a_document_naming_a_platform_the_list_gained_is_accepted(
    tree: Callable[[], Tree],
) -> None:
    """The list is the source: a platform it names is one a document may name."""
    grown = tree()
    grown.edit(
        "AGENTS.md",
        "- `linux-aarch64` — runner `ubuntu-24.04-arm`, Rust target "
        "`aarch64-unknown-linux-gnu`, service manager `systemd`, install path: yes\n",
        "- `linux-aarch64` — runner `ubuntu-24.04-arm`, Rust target "
        "`aarch64-unknown-linux-gnu`, service manager `systemd`, install path: yes\n"
        "- `macos-aarch64` — runner `macos-15`, Rust target `aarch64-apple-darwin`, "
        "service manager `launchd`, install path: no — brought up in a later change\n",
    )
    grown.append(TESTING, "\nOn `macos-aarch64` the service runs under launchd.\n")

    accepted(platform_names(grown.repo), describing="a document naming a platform the list gained")


def test_a_platform_named_without_backticks_is_refused(tree: Callable[[], Tree]) -> None:
    """A claim about a platform is a claim whether or not somebody quoted the name."""
    broken = tree()
    broken.append(TESTING, "\nThe gate also runs on macos-x86_64 hosts.\n")

    findings = platform_names(broken.repo)

    refused_naming(findings, TESTING, "`macos-x86_64`", "does not carry")


def test_a_line_naming_several_managers_one_of_which_is_the_platforms_is_accepted(
    tree: Callable[[], Tree],
) -> None:
    """A sentence contrasting two managers is right about the platform it names."""
    agreeing = tree()
    agreeing.append(
        TESTING,
        "\nWhere launchd would be the manager on a Mac, `linux-aarch64` runs the service "
        "under systemd.\n",
    )

    accepted(platform_names(agreeing.repo), describing="a sentence contrasting two managers")


def test_a_line_naming_several_managers_and_not_the_platforms_own_is_refused(
    tree: Callable[[], Tree],
) -> None:
    """A set of managers a platform's own is not in is a claim about it that is wrong."""
    broken = tree()
    broken.append(
        TESTING,
        "\nOn `linux-aarch64` the service runs under launchd, as it does under "
        "windows-service elsewhere.\n",
    )

    findings = platform_names(broken.repo)

    refused_naming(findings, TESTING, "`linux-aarch64`", "`launchd`", "`systemd`")


def test_a_document_nothing_can_read_is_reported_rather_than_raised(
    tree: Callable[[], Tree],
) -> None:
    """A check that died on one document would say nothing about the rest of them."""
    broken = tree()
    (broken.root / "docs" / "reference" / "not-utf8.md").write_bytes(b"# \xff\xfe not text\n")

    findings = platform_names(broken.repo)

    refused_naming(findings, "docs/reference/not-utf8.md", "nothing here can read")


def test_a_document_naming_a_platform_of_a_family_nothing_here_supports_is_refused(
    tree: Callable[[], Tree],
) -> None:
    """A platform identifier the list does not carry is one whatever family it names."""
    broken = tree()
    broken.append(TESTING, "\nThe clients are also built for `freebsd-x86_64`.\n")

    findings = platform_names(broken.repo)

    refused_naming(findings, TESTING, "`freebsd-x86_64`", "does not carry")


def test_a_runner_label_and_a_registry_selector_are_not_platform_names(
    tree: Callable[[], Tree],
) -> None:
    """Neither is an identifier.

    Refusing a document for naming one would be refusing it for being right.
    """
    agreeing = tree()
    agreeing.append(
        TESTING,
        "\nThe gate's cells run on `ubuntu-24.04` and `ubuntu-24.04-arm`; the JavaScript "
        "registry selects them as `linux-x64` and `linux-arm64`, built for "
        "`aarch64-unknown-linux-gnu` among others.\n",
    )

    accepted(platform_names(agreeing.repo), describing="runner labels and registry selectors")
