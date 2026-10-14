"""Only one crate knows what an OctoPrint is.

Everything above `printobserver-octoprint` is written as though printers were
normal. The moment a second crate constructs an OctoPrint request there are two
places a vendor's own vocabulary has to be kept right, which is the coupling the
port exists to remove — so this is a check rather than a convention, and it is
driven against real copies of the committed tree with the defect introduced.
"""

# `assert` is how pytest states an assertion and how it produces the failure
# message a reader acts on; suppressions.toml carries the reason.

from __future__ import annotations

from collections.abc import Callable

from repo_checks.checks_repo import octoprint_client
from repo_checks.expect import accepted, refused, refused_naming
from repo_checks.model import Repo
from treecopy import Tree

# A crate that has no business knowing what an OctoPrint is.
INNOCENT = "crates/printobserver-core/src/lib.rs"


def test_the_committed_tree_is_accepted(committed: Repo) -> None:
    """The adapter constructs OctoPrint requests, and nothing else does."""
    accepted(octoprint_client(committed))


def test_another_crate_constructing_a_request_is_refused(
    tree: Callable[[], Tree],
) -> None:
    """A second place a vendor's vocabulary would have to be kept right."""
    broken = tree()
    broken.append(
        INNOCENT,
        '\nconst WHERE_THE_JOB_IS: &str = "/api/job";\n',
    )

    findings = octoprint_client(broken.repo)

    refused_naming(findings, "printobserver-core", "/api/job")


def test_another_crate_naming_the_authentication_header_is_refused(
    tree: Callable[[], Tree],
) -> None:
    """The header is as much of a request as the path is."""
    broken = tree()
    broken.append(
        "crates/printobserver-server/src/lib.rs",
        '\nconst HEADER: &str = "X-Api-Key";\n',
    )

    findings = octoprint_client(broken.repo)

    refused_naming(findings, "printobserver-server", "X-Api-Key")


def test_another_crate_may_say_what_it_may_not_build(tree: Callable[[], Tree]) -> None:
    """A comment naming an endpoint is prose, not a request."""
    allowed = tree()
    allowed.append(
        INNOCENT,
        "\n//! The printer port is what reads `/api/job`, and this crate never does.\n",
    )

    accepted(octoprint_client(allowed.repo))


def test_a_tree_whose_adapter_constructs_nothing_is_refused(
    tree: Callable[[], Tree],
) -> None:
    """A rule that guards a boundary nothing is on has stopped being a rule."""
    broken = tree()
    broken.write(
        "crates/printobserver-octoprint/src/lib.rs",
        "//! Owns: nothing yet.\n//!\n//! May depend on: nothing.\n",
    )
    for name in ("client.rs", "config.rs", "convert.rs", "http.rs", "wire.rs"):
        broken.remove(f"crates/printobserver-octoprint/src/{name}")
    for name in (
        "tests/journeys",
        "tests/live",
        "tests/instance",
        "tests/support",
        "tests/octoprint.rs",
        "tests/integration.rs",
    ):
        broken.remove(f"crates/printobserver-octoprint/{name}")

    findings = octoprint_client(broken.repo)

    refused(findings, "constructs no OctoPrint request")


def test_a_policy_naming_a_crate_the_workspace_lacks_is_refused(
    tree: Callable[[], Tree],
) -> None:
    """The permitted crate is one this workspace holds."""
    broken = tree()
    broken.edit(
        "repo-policy.toml",
        "[octoprint]\n# The one crate",
        '[octoprint]\ncrate = "printobserver-gone"\n# The one crate',
    )
    broken.edit("repo-policy.toml", '\ncrate = "printobserver-octoprint"\n', "\n")

    findings = octoprint_client(broken.repo)

    refused(findings, "printobserver-gone")


def test_a_tree_with_no_octoprint_policy_is_refused(tree: Callable[[], Tree]) -> None:
    """The rule is declared in the policy file or it is not declared."""
    broken = tree()
    text = broken.read("repo-policy.toml")
    start = text.index("[octoprint]")
    end = text.index("[manifests]")
    broken.write("repo-policy.toml", text[:start] + text[end:])

    findings = octoprint_client(broken.repo)

    refused(findings, "no `[octoprint]` section")


def test_a_policy_naming_no_marker_is_refused(tree: Callable[[], Tree]) -> None:
    """A marker set nothing is in matches nothing, which is not a rule."""
    broken = tree()
    text = broken.read("repo-policy.toml")
    start = text.index("request_markers = [")
    end = text.index("]", start) + 1
    broken.write("repo-policy.toml", text[:start] + "request_markers = []" + text[end:])

    findings = octoprint_client(broken.repo)

    refused(findings, "no marker of an OctoPrint request")
