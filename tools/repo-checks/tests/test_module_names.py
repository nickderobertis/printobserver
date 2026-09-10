"""No two of the type checker's search roots offer one top-level module name."""

from __future__ import annotations

from collections.abc import Callable

from repo_checks.checks_repo import module_names
from repo_checks.expect import accepted, refused, refused_naming
from repo_checks.model import Repo
from treecopy import Tree

#: A module body is enough: what makes a name ambiguous is the file being there
#: under a second root, not anything it declares.
MODULE = '"""A module a fixture puts under a second search root."""\n'


def test_the_committed_tree_offers_every_name_from_one_root(committed: Repo) -> None:
    """Every bare import in this tree reaches the module beside its importer."""
    accepted(module_names(committed))


def test_a_second_root_offering_an_existing_name_is_refused(tree: Callable[[], Tree]) -> None:
    """The exact collision that cost this repository a gate, rebuilt.

    `python/printobserver-sdk/integration` is listed ahead of
    `tools/printer-smoke/tests`, so a `world` under both leaves printer-smoke's
    tests type-checking against the clients' supervisor.
    """
    broken = tree()
    broken.write("python/printobserver-sdk/integration/world.py", MODULE)

    findings = module_names(broken.repo)

    refused_naming(
        findings,
        "world",
        "python/printobserver-sdk/integration/world.py",
        "tools/printer-smoke/tests/world.py",
    )


def test_a_package_colliding_with_a_module_is_refused(tree: Callable[[], Tree]) -> None:
    """A directory is importable under its own name exactly as a file is."""
    broken = tree()
    broken.write("tools/octoprint-env/live/__init__.py", MODULE)

    findings = module_names(broken.repo)

    refused(findings, "`live` is offered by more than one search root")


def test_a_test_module_under_two_roots_is_not_ambiguous(tree: Callable[[], Tree]) -> None:
    """A test module is placed by its own directory, so nothing imports it by a bare name."""
    shared = tree()
    shared.write("tools/octoprint-env/tests/test_workspace.py", MODULE)
    shared.write("tools/octoprint-env/tests/conftest.py", MODULE)

    accepted(module_names(shared.repo), describing="a tree sharing only pytest-placed names")
