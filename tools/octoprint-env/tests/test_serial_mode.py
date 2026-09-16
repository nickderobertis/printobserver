"""The serial mode composes the same configuration as the virtual one, but for its connection.

That it also *opens* the device it names is `test_serial_device.py`'s, which
starts an instance against a pseudo-terminal rather than describing one.
"""

from __future__ import annotations

import subprocess
from collections.abc import Callable
from pathlib import Path
from typing import Any

import yaml
from environment import answer, script
from repo_checks.expect import equal

BAUDRATE = 57600

# The connection, and the only thing the two modes' configurations may differ in.
CONNECTION_PATHS = frozenset(
    {
        ("serial", "port"),
        ("serial", "baudrate"),
        ("serial", "additionalPorts"),
        ("plugins", "virtual_printer", "enabled"),
    }
)


def test_the_two_modes_differ_in_the_connection_and_in_nothing_else(
    state_dir: Callable[[str], str],
) -> None:
    """One script, one configuration, one flag apart."""
    virtual = _configuration(script("install", "--state-dir", state_dir("as-virtual")))
    serial = _configuration(
        script(
            "install",
            "--state-dir",
            state_dir("as-serial"),
            "--mode",
            "serial",
            "--device",
            "/dev/ttyACM0",
            "--baudrate",
            str(BAUDRATE),
        )
    )

    differing = _differing_paths(virtual, serial)

    equal(
        differing,
        CONNECTION_PATHS,
        describing="the configuration keys the two modes differ in",
    )


def _configuration(result: subprocess.CompletedProcess[str]) -> dict[str, Any]:
    """The configuration one install composed."""
    path = Path(str(answer(result)["config_file"]))
    loaded = yaml.safe_load(path.read_text(encoding="utf-8"))
    if not isinstance(loaded, dict):
        message = f"{path} does not hold a mapping of settings"
        raise AssertionError(message)
    return loaded


def _differing_paths(
    one: dict[str, Any], other: dict[str, Any], prefix: tuple[str, ...] = ()
) -> frozenset[tuple[str, ...]]:
    """Every key path at which two configurations disagree."""
    differing: set[tuple[str, ...]] = set()
    for key in sorted(set(one) | set(other)):
        here = (*prefix, str(key))
        mine, theirs = one.get(key), other.get(key)
        if isinstance(mine, dict) and isinstance(theirs, dict):
            differing |= _differing_paths(mine, theirs, here)
        elif mine != theirs:
            differing.add(here)
    return frozenset(differing)
