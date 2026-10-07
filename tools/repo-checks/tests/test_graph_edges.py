"""The graph's edges are the code's: imports, Cargo manifests and task dependencies.

Each refusal is driven over a real copy of the committed tree carrying the one
defect, and the selection itself is read off the real `nx` over the committed
graph — which is the thing the declarations exist for.
"""

from __future__ import annotations

import json
import os
from collections.abc import Callable

from repo_checks.checks_graph import graph_edges
from repo_checks.expect import accepted, contains, equal, refused, refused_naming, truth
from repo_checks.model import Repo
from repo_checks.shell import run
from treecopy import REPO_ROOT, Tree

OBICO_PROJECT = "tools/obico-env/project.json"


def affected_by(*files: str) -> set[str]:
    """The projects real `nx` selects for a change to `files`, off the committed graph."""
    shown = run(
        ["bunx", "nx", "show", "projects", "--affected", f"--files={','.join(files)}", "--json"],
        cwd=REPO_ROOT,
        env={**os.environ, "NX_DAEMON": "false", "NX_NO_CLOUD": "true"},
        timeout=600,
    )
    equal(shown.returncode, 0, describing=f"`nx show projects`:\n{shown.stdout}{shown.stderr}")
    selected = json.loads(shown.stdout)
    truth(isinstance(selected, list), describing="what `nx show projects --json` printed")
    return {str(name) for name in selected}


def without_edge(tree: Tree, relative: str, edge: str) -> None:
    """Drop one declared edge from one project.json, leaving the rest as written."""
    data = json.loads(tree.read(relative))
    data["implicitDependencies"] = [
        name for name in data.get("implicitDependencies", []) if name != edge
    ]
    tree.write(relative, json.dumps(data, indent=2) + "\n")


def test_the_committed_graph_declares_every_edge_the_code_draws(committed: Repo) -> None:
    """Every import, manifest edge and task dependency is one the graph knows."""
    accepted(graph_edges(committed))


def test_an_import_the_graph_does_not_declare_is_refused(tree: Callable[[], Tree]) -> None:
    """obico-env importing the release tooling is an edge a change there must follow."""
    copy = tree()
    copy.append("tools/obico-env/obico_env.py", "\nimport release_artifacts\n")

    refused_naming(
        graph_edges(copy.repo), "`obico-env` imports `release-artifacts`", "obico_env.py"
    )


def test_an_import_from_a_test_module_counts_like_one_from_the_package(
    tree: Callable[[], Tree],
) -> None:
    """A suite's own import is what decides whether a change must select that suite."""
    copy = tree()
    without_edge(copy, "tests/skilltest-wiring/project.json", "repo-checks")

    refused_naming(
        graph_edges(copy.repo),
        "`skilltest-wiring` imports `repo-checks`",
        "tests/skilltest-wiring/",
    )


def test_a_cargo_dependency_the_graph_does_not_declare_is_refused(
    tree: Callable[[], Tree],
) -> None:
    """An adapter built from the printer port is selected by a change to that port."""
    copy = tree()
    without_edge(copy, "crates/printobserver-octoprint/project.json", "printobserver-printer-api")

    refused_naming(
        graph_edges(copy.repo),
        "`printobserver-octoprint` depends on crate `printobserver-printer-api`",
        "would not select `printobserver-octoprint`",
    )


def test_a_crate_edge_its_manifest_does_not_draw_is_refused(tree: Callable[[], Tree]) -> None:
    """Rust edges are the manifest's exactly, so one left behind is a stale claim."""
    copy = tree()
    relative = "crates/printobserver-types/project.json"
    data = json.loads(copy.read(relative))
    data["implicitDependencies"] = ["printobserver-core"]
    copy.write(relative, json.dumps(data, indent=2) + "\n")

    refused(
        graph_edges(copy.repo),
        "`printobserver-types` declares an edge to crate `printobserver-core`, which its "
        "Cargo manifest does not depend on",
    )


def test_a_task_dependency_without_an_edge_is_refused(tree: Callable[[], Tree]) -> None:
    """`dependsOn` orders a run; it does not make a change select the project."""
    copy = tree()
    without_edge(copy, "tools/printer-smoke/project.json", "printobserver")

    refused(graph_edges(copy.repo), "`printer-smoke:test` depends on a target of `printobserver`")


def test_an_edge_to_no_project_is_refused(tree: Callable[[], Tree]) -> None:
    """A misspelt edge would select nothing, and say nothing about it."""
    copy = tree()
    data = json.loads(copy.read(OBICO_PROJECT))
    data["implicitDependencies"] = [*data.get("implicitDependencies", []), "no-such-project"]
    copy.write(OBICO_PROJECT, json.dumps(data, indent=2) + "\n")

    refused(graph_edges(copy.repo), "declares an edge to `no-such-project`, which is no project")


def test_a_change_to_an_imported_tool_package_selects_every_suite_importing_it() -> None:
    """The release tooling's importers are exactly the suites a change there must run."""
    selected = affected_by("tools/release-artifacts/src/release_artifacts/wheels.py")

    for importer in ("release-artifacts", "repo-checks", "contract-codegen", "repo-e2e"):
        contains(selected, importer, describing="what a release-tooling change selects")
    for unrelated in ("printobserver-core", "printobserver-sdk-node"):
        truth(unrelated not in selected, describing=f"{unrelated} left out of {sorted(selected)}")


def test_a_change_to_repo_checks_selects_the_suites_that_import_it() -> None:
    """The checks package is imported by suites in every corner of the tree."""
    selected = affected_by("tools/repo-checks/src/repo_checks/expect.py")

    for importer in ("repo-e2e", "printobserver-sdk-python", "skill-install", "obico-env"):
        contains(selected, importer, describing="what a repo-checks change selects")


def test_a_crate_change_selects_the_crates_built_from_it_and_not_its_siblings() -> None:
    """The Cargo edges reach the composition root and stop at the other adapters."""
    selected = affected_by("crates/printobserver-octoprint/src/lib.rs")

    for dependent in ("printobserver-octoprint", "printobserver-server", "printobserver"):
        contains(selected, dependent, describing="what an adapter change selects")
    for sibling in ("printobserver-obico", "printobserver-types", "printobserver-core"):
        truth(sibling not in selected, describing=f"{sibling} left out of {sorted(selected)}")


def test_a_leaf_suite_change_selects_that_suite_alone() -> None:
    """Nothing imports or runs the Obico environment, so nothing else is selected."""
    equal(affected_by("tools/obico-env/obico_env.py"), {"obico-env"})
