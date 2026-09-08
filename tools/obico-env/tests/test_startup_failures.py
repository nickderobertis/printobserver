"""Waiting for a condition rather than a duration, and naming the service that failed.

Two properties are proved here over observable behaviour, without naming a
mechanism.

The first: a service that does not come up is named in the failure with a
concrete next action. Each journey holds exactly one service back — by giving
its container an entrypoint that runs and answers nothing — runs the real
bring-up, and reads which service the failure names.

The second: the bring-up returns when the services are ready rather than after a
duration it chose. The slowest service is held back and released at two different
elapsed times, both inside the bring-up's own stated timeout, and the two return
times are compared. A bring-up that waited a fixed duration would return at the
same time in both runs and fail that journey.

Every journey here holds the service back by editing the *cloned* compose file
under the state directory — Obico's own file, in a directory the script cloned —
and restores it afterwards with git. Nothing in the script under test knows this
suite exists.
"""

from __future__ import annotations

import ast
import subprocess
from collections.abc import Iterator
from pathlib import Path

import pytest
import yaml
from harness import ENVIRONMENT, environment, free_port, said, timed
from obico_env import FAILURE_CLASSES, SERVICES
from repo_checks.expect import contains, refused_naming, truth
from repo_checks.shell import run as shell_run

# Short, because these journeys are about the failure rather than the wait: the
# bring-up must still name the service after this, not after a default.
HELD_BACK_TIMEOUT = "60"

# The two elapsed times the slowest service is released at, and how far apart the
# two returns may be from the difference between them. Both are inside the
# timeout above, so neither run reaches it.
RELEASES = (10, 55)
TOLERANCE = 20.0

# The service the timing journeys hold back: the one whose own startup is longest,
# so releasing it late is releasing the bring-up late.
SLOWEST = "web"


def _compose_file(state_dir: str) -> Path:
    """Obico's own composition, in the clone the script made."""
    return Path(state_dir) / "obico-server" / "docker-compose.yml"


def _hold_back(state_dir: str, service: str, *, seconds: int | None = None) -> None:
    """Give one service an entrypoint that answers nothing, or answers late.

    `seconds` is None to hold it back for good, or a number of seconds to sleep
    before handing control to whatever the composition told it to run. The
    entrypoint runs the container's own command through `$0 "$@"`, so this needs
    to know nothing about what any service actually runs.
    """
    path = _compose_file(state_dir)
    composition = yaml.safe_load(path.read_text(encoding="utf-8"))
    held = composition["services"][service]
    held["entrypoint"] = (
        ["sh", "-c", "sleep 100000"]
        if seconds is None
        else ["sh", "-c", f'sleep {seconds}; exec "$0" "$@"']
    )
    # Compose's own healthcheck is not what the bring-up waits on, and a held
    # service that never becomes healthy would only slow the journey down.
    held.pop("healthcheck", None)
    path.write_text(yaml.safe_dump(composition, sort_keys=False), encoding="utf-8")


def _release(state_dir: str) -> None:
    """Put Obico's own composition back exactly as it was cloned."""
    source = _compose_file(state_dir).parent
    shell_run(["git", "-C", str(source), "checkout", "--", "docker-compose.yml"], timeout=300)


@pytest.fixture
def held_back(state_dir: str) -> Iterator[str]:
    """A stopped stack whose composition this journey may edit, restored afterwards."""
    environment("down", "--state-dir", state_dir, timeout=1800)
    # Installed rather than started: the journeys below edit the clone, so it
    # has to be there before they do.
    environment("install", "--state-dir", state_dir)
    yield state_dir
    _release(state_dir)
    environment("down", "--state-dir", state_dir, timeout=1800)


def _bring_up(
    state_dir: str, timeout: str = HELD_BACK_TIMEOUT
) -> tuple[float, subprocess.CompletedProcess[str]]:
    """Run the real bring-up, timed, with a webhook address of this journey's own."""
    webhook = f"http://host.docker.internal:{free_port()}/alert"
    return timed(
        environment,
        "up",
        "--state-dir",
        state_dir,
        "--webhook-url",
        webhook,
        "--start-timeout",
        timeout,
    )


@pytest.mark.parametrize("service", [service.name for service in SERVICES])
def test_a_service_that_does_not_come_up_is_named_with_a_next_action(
    held_back: str, service: str
) -> None:
    """One service held back, and the failure names that one and what to do."""
    _hold_back(held_back, service)

    _, result = _bring_up(held_back)

    truth(result.returncode != 0, describing=f"the bring-up to fail:\n{said(result)}")
    lines = said(result).splitlines()
    refused_naming(lines, "what happened:", f"`{service}`")
    refused_naming(lines, "next action:", "docker compose")
    contains(said(result), "logs", describing="the next action the failure names")


def test_the_bring_up_returns_when_the_services_are_ready_rather_than_after_a_duration(
    held_back: str,
) -> None:
    """Release the slowest service at two different times, and read the two returns.

    Both releases are inside the bring-up's own stated timeout, so neither run
    ends in a failure — each ends when the stack became usable. A bring-up that
    waited a fixed duration would return at the same time in both runs, and the
    difference below would be zero rather than the difference between the two
    releases.
    """
    elapsed: list[float] = []
    for release in RELEASES:
        environment("down", "--state-dir", held_back, timeout=1800)
        _hold_back(held_back, SLOWEST, seconds=release)
        took, result = _bring_up(held_back, timeout="300")
        truth(
            result.returncode == 0,
            describing=f"the bring-up to succeed with {SLOWEST} released at {release}s:\n"
            f"{said(result)}",
        )
        truth(
            took > release,
            describing=(
                f"the bring-up to return after `{SLOWEST}` was released at {release}s; "
                f"it returned after {took:.1f}s"
            ),
        )
        elapsed.append(took)

    expected = RELEASES[1] - RELEASES[0]
    difference = elapsed[1] - elapsed[0]
    truth(
        abs(difference - expected) <= TOLERANCE,
        describing=(
            f"the two returns to differ by about {expected}s, the difference between "
            f"the two releases; they took {elapsed[0]:.1f}s and {elapsed[1]:.1f}s, a "
            f"difference of {difference:.1f}s"
        ),
    )


def _classes_raised(module: ast.Module) -> set[str]:
    """The class every `StartupError` in the script names, refusing a computed one.

    Raises:
        AssertionError: If one is raised with something other than a literal, so
            that a failure class a reader could not enumerate cannot be smuggled
            in past the declared set.
    """
    found: set[str] = set()
    for node in ast.walk(module):
        if not isinstance(node, ast.Raise) or node.exc is None:
            continue
        raised = node.exc
        if not isinstance(raised, ast.Call) or not isinstance(raised.func, ast.Name):
            continue
        if raised.func.id != "StartupError":
            continue
        first = raised.args[0] if raised.args else None
        if not isinstance(first, ast.Constant) or not isinstance(first.value, str):
            message = f"expected a literal failure class at line {node.lineno}"
            raise AssertionError(message)
        found.add(first.value)
    return found


def _handled_by_main(module: ast.Module) -> set[str]:
    """What `main` catches, which is where every failure is reported.

    Raises:
        AssertionError: If the script has no `main` at all.
    """
    for node in module.body:
        if isinstance(node, ast.FunctionDef) and node.name == "main":
            return {
                handler.type.id
                for inner in ast.walk(node)
                for handler in getattr(inner, "handlers", [])
                if isinstance(handler.type, ast.Name)
            }
    message = "expected a `main` in the script, which is where every failure is reported"
    raise AssertionError(message)


def test_the_script_can_reach_no_failure_path_the_declared_set_does_not_cover() -> None:
    """Every declared class is raised, every raise is declared, and `main` reports both.

    One diagnosed failure path and a bare timeout everywhere else is the shape
    that reads as diagnostics without being any, and this is what refuses it.
    """
    module = ast.parse(ENVIRONMENT.read_text(encoding="utf-8"))

    raised = _classes_raised(module)
    truth(FAILURE_CLASSES, describing="a declared set of failure classes in the script")
    for failure_class, next_action in FAILURE_CLASSES.items():
        truth(next_action.strip(), describing=f"a next action for `{failure_class}`")
        contains(raised, failure_class, describing="the classes the script raises, so none is dead")
    for failure_class in raised:
        contains(
            set(FAILURE_CLASSES), failure_class, describing="the declared set of failure classes"
        )

    handled = _handled_by_main(module)
    contains(handled, "StartupError", describing="what `main` handles")
    contains(handled, "Exception", describing="what `main` handles")
