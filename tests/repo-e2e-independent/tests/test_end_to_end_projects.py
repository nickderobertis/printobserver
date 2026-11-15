"""Every end-to-end test file is collected by exactly the one project it is assigned to.

The end-to-end tier is two Nx projects. `repo-e2e` holds the journeys whose
subject behaves differently by platform — installers, service managers, paths,
the shipped program, the gate as each platform's own cell runs it — and runs on
every platform. `repo-e2e-independent` holds the journeys that drive only this
repository's own tooling, workflows and records, and carries the tag
`repo-policy.toml`'s `[gate] platform_independent_tag` declares, so CI may run
it once. `ASSIGNED` is the one record of which file is whose.

Each project's real `test-e2e` target is run here in pytest's collection mode,
through Nx as the gate runs it, and what each collected is held to that record:
a file on disk that neither project collects, one both collect, or one the
other project collects is refused naming it.
"""

from __future__ import annotations

import json
import re
from pathlib import Path

from journey import REPO_ROOT, capture, clean_environment, output
from repo_checks import gate_shards
from repo_checks.checks_graph import platform_independent_tag
from repo_checks.expect import equal, truth
from repo_checks.model import Repo

#: The two projects, by name, and the directory each collects its tests from.
DEPENDENT = "repo-e2e"
INDEPENDENT = "repo-e2e-independent"
DIRECTORIES = {
    DEPENDENT: REPO_ROOT / "tests" / "repo-e2e" / "tests",
    INDEPENDENT: REPO_ROOT / "tests" / "repo-e2e-independent" / "tests",
}

#: Which project collects each of the tier's test files.
ASSIGNED = {
    DEPENDENT: frozenset(
        {
            "test_bootstrap_journey.py",
            "test_cargo_build_contract_journey.py",
            "test_commit_msg_journey.py",
            "test_fresh_clone_journey.py",
            "test_gate_copy_journey.py",
            "test_gate_tier_journey.py",
            "test_host_portability_journey.py",
            "test_install_powershell_script_journey.py",
            "test_install_script_journey.py",
            "test_isolation_copies.py",
            "test_octoprint_tier.py",
            "test_per_user_unix_install_journey.py",
            "test_per_user_windows_install_journey.py",
            "test_printer_smoke_journey.py",
            "test_prove_service_journey.py",
            "test_registry_proof_journey.py",
            "test_release_probe_journey.py",
            "test_sdk_project_targets.py",
            "test_server_shutdown_journey.py",
            "test_service_installer_journey.py",
            "test_service_manager_journey.py",
            "test_service_report_journey.py",
            "test_shipped_artifacts_journey.py",
            "test_windows_lint_journey.py",
            "test_windows_service_installer_journey.py",
            "test_workers.py",
            "test_world_journey.py",
        }
    ),
    INDEPENDENT: frozenset(
        {
            "test_actions_runner.py",
            "test_end_to_end_projects.py",
            "test_llmlint_harness_journey.py",
            "test_platform_dispatch_journey.py",
            "test_platform_matrix_journey.py",
            "test_release_gating_journey.py",
            "test_release_path_journey.py",
            "test_release_program.py",
            "test_release_tag_journey.py",
            "test_release_workflow_journey.py",
            "test_required_checks_journey.py",
            "test_workflow_validation.py",
        }
    ),
}

#: One collected test as pytest lists it: its file's path from the root, then
#: `::`. Anchored at the tier's own directories, because Nx's stream prefixes
#: each line with the project's name and can split a long one.
COLLECTED = re.compile(r"(?P<path>tests/repo-e2e(?:-independent)?/tests/[\w/]*test_\w+\.py)::")


def collected_by(project: str) -> set[str]:
    """The test files `project`'s real `test-e2e` target collects, by their path from the root.

    Nx runs the target as the gate does, minus the build it depends on, which
    collection does not need; `PYTEST_ADDOPTS` puts every pytest invocation the
    target's command makes into collection mode.
    """
    environment = clean_environment(PYTEST_ADDOPTS="--collect-only")
    for name in gate_shards.PARTS:
        environment.pop(name, None)
    result = capture(
        [
            "bunx",
            "nx",
            "run",
            f"{project}:test-e2e",
            "--excludeTaskDependencies",
            "--skipNxCache",
            "--output-style=stream",
        ],
        REPO_ROOT,
        timeout=600,
        env=environment,
    )
    equal(result.returncode, 0, describing=f"`{project}:test-e2e` collecting:\n{output(result)}")
    return {match["path"] for match in COLLECTED.finditer(result.stdout)}


def on_disk(directory: Path) -> set[str]:
    """Every test file under `directory`, by its path from the root."""
    return {path.relative_to(REPO_ROOT).as_posix() for path in directory.rglob("test_*.py")}


def test_each_project_collects_exactly_the_files_assigned_to_it() -> None:
    """No file is left to neither project, run by both, or run by the one it is not assigned to."""
    collected = {project: collected_by(project) for project in DIRECTORIES}
    every_file = set().union(*(on_disk(directory) for directory in DIRECTORIES.values()))

    equal(
        sorted(every_file - collected[DEPENDENT] - collected[INDEPENDENT]),
        [],
        describing="test files neither project collects",
    )
    equal(
        sorted(collected[DEPENDENT] & collected[INDEPENDENT]),
        [],
        describing="test files both projects collect",
    )
    for project, directory in DIRECTORIES.items():
        expected = {
            (directory / name).relative_to(REPO_ROOT).as_posix() for name in ASSIGNED[project]
        }
        equal(sorted(collected[project]), sorted(expected), describing=f"what `{project}` collects")


def test_the_independent_project_carries_the_declared_tag_and_the_other_does_not() -> None:
    """The tag is the one `repo-policy.toml` spells, on the one project whose tests are once-run."""
    declared = platform_independent_tag(Repo(REPO_ROOT))
    tags = {
        project: json.loads((directory.parent / "project.json").read_text(encoding="utf-8"))["tags"]
        for project, directory in DIRECTORIES.items()
    }
    truth(declared in tags[INDEPENDENT], describing=f"`{declared}` among {INDEPENDENT}'s {tags}")
    truth(declared not in tags[DEPENDENT], describing=f"`{declared}` absent from {DEPENDENT}'s")
