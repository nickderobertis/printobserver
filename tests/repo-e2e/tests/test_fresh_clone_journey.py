"""The gate runs in a clone that has never been bootstrapped.

Every publication of this repository is made from a clone cut fresh for it, and
a tier run there — by a hook, by a person, or by the pull request's own jobs —
finds nothing installed. Nothing installs the workspace's JavaScript
dependencies in such a clone, so before this journey existed the first tier
died on `NX Could not find Nx modules` and no change could be pushed at all.

Each journey below drives the real recipe over a real copy carrying no
installed dependencies. Nothing is mocked: `just` runs, `bun` installs, `nx`
fans out, and the assertion is on what the recipe said and what it left behind.

Every one runs under the environment a host orchestrating agents hands it —
`UV_NO_SYNC` exported, `NX_DAEMON` set to nothing — and a copy is a directory
nobody cleans up after, so each runs Nx with no daemon and leaves none running.
"""

# `assert` is how pytest states an assertion and how it produces the failure
# message a reader acts on; suppressions.toml carries the reason.

from __future__ import annotations

import json
from collections.abc import Callable
from pathlib import Path

import pytest
from journey import QUIET, REPO_ROOT, GateCopy, capture, clean_environment, output
from repo_checks.expect import absent, contains, equal, failing, passing, truth

PACKAGE_JSON = "package.json"
UNDESCRIBED_DEPENDENCY = '"left-pad": "^1.3.0",\n    "@types/bun": "^1.4.1"'

#: A recipe appended to a copy's own `justfile`, and the script it runs: what
#: the copy's commands see of the two settings a host hands down, said by an
#: interpreter that imports a dependency only the copy's own synced environment
#: carries.
PROBE_RECIPE = "\nenvironment-probe:\n    uv run -q python environment_probe.py\n"
PROBE_SCRIPT = """import json
import os

import yaml

print(json.dumps({name: os.environ.get(name) for name in ("UV_NO_SYNC", "NX_DAEMON")}))
print(yaml.__name__)
"""


@pytest.fixture(autouse=True)
def a_host_that_suppresses_sync(monkeypatch: pytest.MonkeyPatch) -> None:
    """The caller's environment a host orchestrating agents runs these under."""
    monkeypatch.setenv("UV_NO_SYNC", "1")
    monkeypatch.delenv("NX_DAEMON", raising=False)


def no_daemon_left_running(copy: GateCopy) -> None:
    """Fail if an Nx daemon serves the copy, stopping it first so the failure leaks nothing."""
    asked = capture(["bunx", "nx", "daemon"], copy.root, timeout=300, env=clean_environment())
    passing(asked, describing="asking Nx whether a daemon serves the copy")
    if "Nx Daemon is not running" in asked.stdout:
        return
    capture(["bunx", "nx", "daemon", "--stop"], copy.root, timeout=300, env=clean_environment())
    truth(False, describing=f"no Nx daemon to outlive the journey, but Nx says: {asked.stdout}")


def test_a_gate_tier_runs_in_a_clone_that_never_bootstrapped(
    gate_copy: Callable[..., GateCopy],
) -> None:
    """A tier reaching Nx heals the missing dependency tree instead of dying on it."""
    fresh = gate_copy(node_modules=False)
    truth(
        not (fresh.root / "node_modules").exists(),
        describing="a fresh clone to carry no installed dependencies",
    )

    # The full sweep, so that the clone's tier fans out over every project
    # whatever its history says the change since the base branch reaches.
    result = fresh.just("format-check", "all", environment=QUIET)

    passing(result)
    truth(
        (fresh.root / "node_modules" / "nx").is_dir(),
        describing="the tier to install the Nx modules it needs",
    )
    contains(output(result), "Successfully ran target format-check")
    no_daemon_left_running(fresh)


# llmlint: ignore[expensive_tests_stay_behind_their_own_edge] suppressions.toml has the reason.
def test_a_clones_commands_sync_its_own_environment_and_start_no_daemon(tmp_path: Path) -> None:
    """The copy's commands see no inherited `UV_NO_SYNC`, and `NX_DAEMON=false`.

    The environment is a new one, so it holds the copy's dependencies only if
    the copy's own `uv run` synced them there — which an inherited `UV_NO_SYNC`
    stops, failing the import below on a missing module.
    """
    fresh = GateCopy(tmp_path / "copy", tmp_path / "environment", node_modules=False)
    fresh.write("justfile", fresh.read("justfile") + PROBE_RECIPE)
    fresh.write("environment_probe.py", PROBE_SCRIPT)

    result = fresh.just("environment-probe", environment=QUIET)

    passing(result)
    equal(
        json.loads(result.stdout.strip().splitlines()[-2]),
        {"UV_NO_SYNC": None, "NX_DAEMON": "false"},
        describing="what the copy's commands saw of the host's settings",
    )


def test_the_install_records_nothing_the_lockfile_does_not_already_describe(
    gate_copy: Callable[..., GateCopy],
) -> None:
    """The locked install cannot rewrite the lockfile the gate is meant to prove."""
    fresh = gate_copy(node_modules=False)

    passing(fresh.just("node-modules"))

    equal(
        fresh.read("bun.lock"),
        (REPO_ROOT / "bun.lock").read_text(encoding="utf-8"),
        describing="the lockfile a locked install left alone",
    )


def test_an_already_installed_tree_is_not_installed_again(
    gate_copy: Callable[..., GateCopy],
) -> None:
    """Healing a fresh clone must not cost every other run an install."""
    fresh = gate_copy(node_modules=False)
    first = fresh.just("node-modules")
    passing(first)
    contains(output(first), "packages installed")

    again = fresh.just("node-modules")

    passing(again)
    contains(output(again), "no changes")
    absent(output(again), "packages installed")


def test_the_install_refuses_a_dependency_the_lockfile_does_not_describe(
    gate_copy: Callable[..., GateCopy],
) -> None:
    """A manifest the lockfile does not cover stops the tier rather than resolving."""
    broken = gate_copy(node_modules=False)
    broken.edit(PACKAGE_JSON, '"@types/bun": "^1.4.1"', UNDESCRIBED_DEPENDENCY)

    result = broken.just("format-check", environment=QUIET)

    failing(result, naming="lockfile is frozen")
    absent(output(result), "Successfully ran target format-check")
