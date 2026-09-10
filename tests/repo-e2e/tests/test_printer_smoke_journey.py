"""The real-printer smoke test, selected by the real recipe and by nothing else.

Two things are driven here and both are real. The recipe is run four times —
once for each combination of the two inputs that select the smoke — and what it
says is asserted. And Nx is asked, for every tier the gate fans out through,
which projects and which commands that tier selects, so that "nothing selects
it automatically" is what the selector itself says rather than what this
repository claims about itself.

Neither of those runs the smoke against a machine: three of the four
combinations do not select it at all, and the fourth reaches its precondition
checks, which refuse a host with no printer on the device the test made.
"""

from __future__ import annotations

import json
import os
import subprocess
from collections.abc import Iterator
from typing import Any

import pytest
from journey import REPO_ROOT, capture, clean_environment
from repo_checks.expect import absent, contains, equal, passing, truth
from repo_checks.model import Repo
from repo_checks.parsing import recipes

RECIPE = "test-printer-smoke"
FLAG = "--run"
DEVICE_ENV = "PRINTOBSERVER_SMOKE_DEVICE"
SCRIPT = "tools/printer-smoke/printer_smoke.py"

#: How the recipe answers when the two inputs did not select it.
UNSELECTED = "not selected"


@pytest.fixture
def a_serial_device() -> Iterator[str]:
    """A character device this test creates, which is what the variable names."""
    controller, device = os.openpty()
    try:
        yield os.ttyname(device)
    finally:
        os.close(controller)
        os.close(device)


def the_recipe(*arguments: str, device: str | None = None) -> subprocess.CompletedProcess[str]:
    """Run this repository's own recipe, with or without each of the two inputs."""
    environment = clean_environment()
    environment.pop(DEVICE_ENV, None)
    if device is not None:
        environment[DEVICE_ENV] = device
    return capture(["just", RECIPE, *arguments], REPO_ROOT, timeout=600, env=environment)


def test_neither_input_leaves_it_unselected() -> None:
    """Absent both, it says so and names both, rather than skipping silently."""
    run = the_recipe()

    passing(run, describing=f"`just {RECIPE}` with neither input")
    contains(run.stdout, UNSELECTED, describing="what it said")
    contains(run.stdout, f"the `{FLAG}` flag was not given", describing="what it said")
    contains(run.stdout, f"{DEVICE_ENV} names no serial device", describing="what it said")


def test_the_flag_alone_leaves_it_unselected(a_serial_device: str) -> None:
    """One input alone does not select it, and it names the one that was missing."""
    run = the_recipe(FLAG)

    passing(run, describing=f"`just {RECIPE} {FLAG}` with no device named")
    contains(run.stdout, UNSELECTED, describing="what it said")
    contains(run.stdout, f"{DEVICE_ENV} names no serial device", describing="what it said")
    absent(run.stdout, f"the `{FLAG}` flag was not given", describing="what it said")


def test_the_device_alone_leaves_it_unselected(a_serial_device: str) -> None:
    """The other input alone does not select it either."""
    run = the_recipe(device=a_serial_device)

    passing(run, describing=f"`just {RECIPE}` with a device named and no flag")
    contains(run.stdout, UNSELECTED, describing="what it said")
    contains(run.stdout, f"the `{FLAG}` flag was not given", describing="what it said")
    absent(run.stdout, f"{DEVICE_ENV} names no serial device", describing="what it said")


def test_both_inputs_select_it_and_it_reaches_its_preconditions(a_serial_device: str) -> None:
    """Together they select it, and what it does next is check what it requires."""
    run = the_recipe(FLAG, device=a_serial_device)

    passing(run, describing=f"`just {RECIPE} {FLAG}` with a device this test created")
    absent(run.stdout, UNSELECTED, describing="what it said")
    contains(run.stdout, "precondition", describing="what it said")
    truth(
        "is met" in run.stdout or "refused: the precondition" in run.stdout,
        describing=f"the run to have reached its precondition checks; it said:\n{run.stdout}",
    )


@pytest.fixture(scope="module")
def nx_is_reachable() -> None:
    """Install what `bunx nx` needs, the way every recipe reaching Nx does.

    `bunx nx` fails outright in a clone whose JavaScript dependencies have never
    been installed, so a journey asking Nx what it selects heals that state
    first — through this repository's own recipe, which is the locked install.
    """
    passing(
        capture(["just", "node-modules"], REPO_ROOT, timeout=900, env=clean_environment()),
        describing="`just node-modules`",
    )


def _projects() -> dict[str, dict[str, Any]]:
    """Every project of the graph, by name, as the committed graph declares it."""
    found: dict[str, dict[str, Any]] = {}
    for path in Repo(REPO_ROOT).project_paths:
        data = json.loads(path.read_text(encoding="utf-8"))
        found[str(data.get("name", path.parent.name))] = data
    return found


def _fanned_out_tiers() -> list[str]:
    """Every Nx target the gate's own tiers fan out through, read off the recipes."""
    parsed = recipes((REPO_ROOT / "justfile").read_text(encoding="utf-8"))
    targets: list[str] = []
    for tier in Repo(REPO_ROOT).policy["gate"]["tiers"]:
        for line in parsed[tier].body if tier in parsed else ():
            words = line.split()
            if words[:4] == ["bunx", "nx", "run-many", "-t"]:
                targets.append(words[4])
    return targets


@pytest.mark.usefixtures("nx_is_reachable")
def test_no_tier_the_gate_fans_out_through_selects_the_smoke() -> None:
    """Nx itself is asked what each gate tier selects, and the smoke is in none of it."""
    projects = _projects()
    tiers = _fanned_out_tiers()

    truth(tiers, describing="the gate to fan out through at least one Nx target")
    for target in tiers:
        listed = capture(
            ["bunx", "nx", "show", "projects", "--withTarget", target, "--json"],
            REPO_ROOT,
            timeout=600,
            env=clean_environment(),
        )
        passing(listed, describing=f"asking Nx which projects carry `{target}`")
        for name in json.loads(listed.stdout):
            command = str(projects[name]["targets"][target].get("command", ""))
            truth(
                SCRIPT not in command and f"just {RECIPE}" not in command,
                describing=f"`{name}:{target}`, which the gate selects, to run no smoke "
                f"test; it runs `{command}`",
            )


def test_no_workflow_runs_the_smoke_on_a_change_or_on_a_schedule() -> None:
    """Continuous integration and every scheduled tier run beside no printer."""
    workflows = Repo(REPO_ROOT).workflow_paths

    truth(workflows, describing="the repository to commit at least one workflow")
    for path in workflows:
        text = path.read_text(encoding="utf-8")
        absent(text, SCRIPT, describing=f"{path.name}")
        absent(text, f"just {RECIPE}", describing=f"{path.name}")


def test_the_smoke_is_reached_by_one_recipe_and_no_graph_target() -> None:
    """The one thing that runs it is the recipe a person types beside the machine."""
    reaching = [
        f"{name}:{target}"
        for name, data in _projects().items()
        for target, spec in (data.get("targets") or {}).items()
        if SCRIPT in str(spec.get("command", ""))
    ]

    equal(reaching, [], describing="the graph targets that reach the smoke")
    contains(
        recipes((REPO_ROOT / "justfile").read_text(encoding="utf-8")),
        RECIPE,
        describing="the recipes this repository declares",
    )
