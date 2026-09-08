"""The crate set, the module comments, and the dependency rule the design rests on."""

# `assert` is how pytest states an assertion and how it produces the failure
# message a reader acts on; suppressions.toml carries the reason.
# ruff: noqa: S101

from __future__ import annotations

from collections.abc import Callable

from repo_checks.checks_repo import workspace
from repo_checks.model import Repo
from treecopy import Tree

# The crates this repository is required to hold, named here rather than read
# from the tree, so that a tree that lost one is refused by the list rather than
# by agreeing with itself.
REQUIRED_CRATES = (
    "printobserver-types",
    "printobserver-printer-api",
    "printobserver-vision-api",
    "printobserver-supervisor-api",
    "printobserver-store-api",
    "printobserver-octoprint",
    "printobserver-obico",
    "printobserver-oneharness",
    "printobserver-store-sqlite",
    "printobserver-core",
    "printobserver-server",
    "printobserver-sdk",
    "printobserver",
)

DEPENDENCY = '\n[dependencies]\n{name} = {{ path = "../{name}" }}\n'


def test_the_workspace_holds_exactly_the_required_crates(committed: Repo) -> None:
    """Every crate the design names, and no crate it does not."""
    assert sorted(committed.crate_names) == sorted(REQUIRED_CRATES)


def test_the_committed_workspace_is_accepted(committed: Repo) -> None:
    """The workspace this repository ships satisfies its own dependency rule."""
    assert workspace(committed) == []


def test_every_crate_states_what_it_owns_and_what_it_may_depend_on(
    committed: Repo,
) -> None:
    """The layering is readable at the top of every crate, not only in a check."""
    for directory in committed.crate_dirs:
        source = directory / "src" / "lib.rs"
        if not source.is_file():
            source = directory / "src" / "main.rs"
        comment = source.read_text(encoding="utf-8")
        assert "Owns:" in comment, directory.name
        assert "May depend on:" in comment, directory.name


def test_a_missing_crate_is_refused(tree: Callable[[], Tree]) -> None:
    """A workspace that lost a crate the design names."""
    broken = tree()
    broken.remove("crates/printobserver-obico")

    findings = workspace(broken.repo)

    assert any("printobserver-obico" in finding for finding in findings), findings


def test_an_undeclared_crate_is_refused(tree: Callable[[], Tree]) -> None:
    """A crate nobody declared is a layer nobody agreed to."""
    broken = tree()
    broken.write(
        "crates/printobserver-surprise/Cargo.toml",
        '[package]\nname = "printobserver-surprise"\nversion.workspace = true\n',
    )
    broken.write("crates/printobserver-surprise/src/lib.rs", "//! Owns: nothing.\n")

    findings = workspace(broken.repo)

    assert any("printobserver-surprise" in finding for finding in findings), findings


def test_a_crate_whose_comment_omits_what_it_owns_is_refused(
    tree: Callable[[], Tree],
) -> None:
    """A module comment that does not say what the crate owns."""
    broken = tree()
    broken.write("crates/printobserver-core/src/lib.rs", "//! May depend on: nothing.\n")

    findings = workspace(broken.repo)

    assert any("does not say what it owns" in finding for finding in findings), findings


def test_a_crate_whose_comment_omits_its_dependencies_is_refused(
    tree: Callable[[], Tree],
) -> None:
    """A module comment that does not say what the crate may depend on."""
    broken = tree()
    broken.write("crates/printobserver-core/src/lib.rs", "//! Owns: the supervision logic.\n")

    findings = workspace(broken.repo)

    assert any("does not say what it may depend on" in finding for finding in findings), findings


def test_core_depending_on_an_implementation_is_refused(
    tree: Callable[[], Tree],
) -> None:
    """The edge the whole design exists to forbid."""
    broken = tree()
    broken.write(
        "crates/printobserver-core/Cargo.toml",
        broken.read("crates/printobserver-core/Cargo.toml")
        + DEPENDENCY.format(name="printobserver-octoprint"),
    )

    findings = workspace(broken.repo)

    assert any(
        "printobserver-core` depends on `printobserver-octoprint" in finding for finding in findings
    ), findings


def test_core_depending_outside_its_layer_is_refused(tree: Callable[[], Tree]) -> None:
    """Core may name the type crate and the four ports, and nothing else."""
    broken = tree()
    broken.write(
        "crates/printobserver-core/Cargo.toml",
        broken.read("crates/printobserver-core/Cargo.toml")
        + DEPENDENCY.format(name="printobserver-server"),
    )

    findings = workspace(broken.repo)

    assert any("outside its layer" in finding for finding in findings), findings


def test_an_implementation_depending_on_another_is_refused(
    tree: Callable[[], Tree],
) -> None:
    """An edge between two adapters is a hidden coupling between two vendors."""
    broken = tree()
    broken.write(
        "crates/printobserver-obico/Cargo.toml",
        broken.read("crates/printobserver-obico/Cargo.toml")
        + DEPENDENCY.format(name="printobserver-octoprint"),
    )

    findings = workspace(broken.repo)

    assert any(
        "no implementation crate may depend on another" in finding for finding in findings
    ), findings


def test_core_may_depend_on_a_port(tree: Callable[[], Tree]) -> None:
    """The rule forbids the edges it names and permits the ones it allows."""
    allowed = tree()
    allowed.write(
        "crates/printobserver-core/Cargo.toml",
        allowed.read("crates/printobserver-core/Cargo.toml")
        + DEPENDENCY.format(name="printobserver-printer-api"),
    )

    assert workspace(allowed.repo) == []
