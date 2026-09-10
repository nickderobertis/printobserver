"""The suppression allowlist is the only way to suppress a diagnostic here.

The directive spellings below are assembled from pieces rather than written
whole, because a test file that carried a real directive would itself be a
suppression the check would then demand an allowlist entry for.
"""

# `assert` is how pytest states an assertion and how it produces the failure
# message a reader acts on; suppressions.toml carries the reason.

from __future__ import annotations

from collections.abc import Callable
from pathlib import Path

from repo_checks.checks_suppressions import scan, suppressions
from repo_checks.expect import accepted, contains, refused
from repo_checks.model import Repo
from repo_checks.shell import run
from treecopy import REPO_ROOT, Tree

ALLOW = "#[" + "allow(dead_code)]"
ENTRY = """
[[suppression]]
rule = "dead_code"
file = "crates/printobserver-types/src/lib.rs"
site = "pub fn placeholder"
reason = "A stated reason."
"""


def _plant_directive(tree: Tree) -> None:
    """Put one suppression directive into a crate, with the site the entry anchors to."""
    source = tree.read("crates/printobserver-types/src/lib.rs")
    tree.write(
        "crates/printobserver-types/src/lib.rs",
        source + f"\n{ALLOW}\npub fn placeholder() {{}}\n",
    )


def test_the_committed_tree_is_accepted(committed: Repo) -> None:
    """Every directive standing in this repository carries its entry."""
    accepted(suppressions(committed))


def test_the_scanner_finds_the_directives_this_repository_carries(
    committed: Repo,
) -> None:
    """The check reads real directives out of the tree rather than a list beside it."""
    found = {(d.file, d.rule) for d in scan(REPO_ROOT)}

    contains(found, ("scripts/session-setup.sh", "tool_output_is_signal"))
    contains(found, ("AGENTS.md", "instruction_layer_localized"))


def test_a_directive_with_no_entry_is_refused(tree: Callable[[], Tree]) -> None:
    """A suppression nobody wrote down is a check switched off in silence."""
    broken = tree()
    _plant_directive(broken)

    findings = suppressions(broken.repo)

    refused(findings, "with no entry in suppressions.toml")


def test_an_entry_with_no_reason_is_refused(tree: Callable[[], Tree]) -> None:
    """An entry without a reason records that a rule was silenced, not why."""
    broken = tree()
    _plant_directive(broken)
    broken.write(
        "suppressions.toml",
        broken.read("suppressions.toml")
        + ENTRY.replace('reason = "A stated reason."', 'reason = ""'),
    )

    findings = suppressions(broken.repo)

    refused(findings, "carries no reason")


def test_an_entry_matching_nothing_is_refused(tree: Callable[[], Tree]) -> None:
    """A stale entry is an allowlist that has stopped describing the tree."""
    broken = tree()
    broken.write("suppressions.toml", broken.read("suppressions.toml") + ENTRY)

    findings = suppressions(broken.repo)

    refused(findings, "matches no suppression directive")


def test_a_directive_and_its_entry_together_are_accepted(
    tree: Callable[[], Tree],
) -> None:
    """The allowlist is a way through, not a wall."""
    allowed = tree()
    _plant_directive(allowed)
    allowed.write("suppressions.toml", allowed.read("suppressions.toml") + ENTRY)

    accepted(suppressions(allowed.repo))


def _git(root: Path, *args: str) -> None:
    run(["git", *args], cwd=root, check=True)


def test_a_change_adding_a_directive_without_its_entry_is_refused(
    tree: Callable[[], Tree],
) -> None:
    """Against a base revision, the entry lands in the same change as the directive."""
    broken = tree()
    _git(broken.root, "init", "-q", "-b", "main")
    _git(broken.root, "config", "user.email", "test@example.com")
    _git(broken.root, "config", "user.name", "test")
    _git(broken.root, "add", "-A")
    _git(broken.root, "commit", "-q", "-m", "chore: the base")
    base = run(["git", "rev-parse", "HEAD"], cwd=broken.root, check=True).stdout.strip()

    _plant_directive(broken)
    _git(broken.root, "add", "-A")
    _git(broken.root, "commit", "-q", "-m", "feat: silence a rule")

    findings = suppressions(broken.repo, base=base)

    refused(findings, "without adding its entry")


def test_a_change_adding_both_together_is_accepted(tree: Callable[[], Tree]) -> None:
    """The same change carrying both is exactly what the rule asks for."""
    allowed = tree()
    _git(allowed.root, "init", "-q", "-b", "main")
    _git(allowed.root, "config", "user.email", "test@example.com")
    _git(allowed.root, "config", "user.name", "test")
    _git(allowed.root, "add", "-A")
    _git(allowed.root, "commit", "-q", "-m", "chore: the base")
    base = run(["git", "rev-parse", "HEAD"], cwd=allowed.root, check=True).stdout.strip()

    _plant_directive(allowed)
    allowed.write("suppressions.toml", allowed.read("suppressions.toml") + ENTRY)
    _git(allowed.root, "add", "-A")
    _git(allowed.root, "commit", "-q", "-m", "feat: silence a rule, with its reason")

    accepted(suppressions(allowed.repo, base=base))
