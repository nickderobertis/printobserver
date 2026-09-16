r"""The serial-device precondition, answered the way each host family keeps devices.

On a POSIX host a serial device is a file at a path with a mode, and the walk in
`test_preconditions.py` meets and refuses it with a real pseudo-terminal. A
Windows serial port is a name in the system's device table — `COM3`, opened as
`\\.\COM3` — and the smoke asks that table rather than opening the port,
because opening one can reset the printer on the other end.

That table exists only on Windows, so off Windows the lookup is the one thing
answered from a table this suite writes; everything around it — the platform
branch, the namespace a device may be written in, what is refused and how it is
said — is the smoke's own code. On a Windows host the first test here and the
whole precondition walk ask the real table.
"""

from __future__ import annotations

import platform as host_platform
import sys
from collections.abc import Callable

import printer_smoke
import pytest
import world
from printer_smoke import PRECONDITIONS, Smoke, smoke_of
from repo_checks.expect import contains, equal
from world import WINDOWS_DEVICE, a_serial_device

#: The precondition every test here is about, as the smoke declares it.
DEVICE_PRECONDITION: Callable[[Smoke], str | None] = dict(PRECONDITIONS)["serial-device"]

#: What a Windows host's device table maps each name it carries to.
DEVICE_TABLE = {"COM3": "\\Device\\USBSER000", "NUL": "\\Device\\Null"}


def test_the_device_this_harness_names_meets_the_precondition_on_this_host() -> None:
    """Whatever host this is, the device every smoke journey is pointed at is one.

    Nothing is substituted: on POSIX this is a pseudo-terminal's path and its
    mode, and on Windows it is the system's own device table.
    """
    with a_serial_device() as device:
        equal(DEVICE_PRECONDITION(smoke_of({}, device)), None, describing=f"the device {device}")


@pytest.fixture
def windows(monkeypatch: pytest.MonkeyPatch) -> list[str]:
    """This host answering as Windows, with a device table of this suite's own.

    Returns:
        Every name the smoke looked up, in order.
    """
    looked_up: list[str] = []

    def query(name: str) -> str | None:
        looked_up.append(name)
        return DEVICE_TABLE.get(name)

    monkeypatch.setattr(sys, "platform", "win32")
    monkeypatch.setattr(printer_smoke, "_query_dos_device", query)
    return looked_up


def test_on_windows_a_port_the_device_table_carries_is_met_under_either_spelling(
    windows: list[str],
) -> None:
    r"""`COM3` and `\\.\COM3` name one port, and that port is looked up by its name."""
    for device in ("COM3", "\\\\.\\COM3"):
        equal(DEVICE_PRECONDITION(smoke_of({}, device)), None, describing=f"the device {device}")

    equal(windows, ["COM3", "COM3"], describing="the names looked up in the device table")


def test_on_windows_a_port_the_device_table_does_not_carry_is_refused(
    windows: list[str],
) -> None:
    """A port nothing is plugged into is not there, and the run says which it named."""
    for device in ("COM9", "\\\\.\\COM9", "C:\\Users\\runner\\not-a-device"):
        refusal = DEVICE_PRECONDITION(smoke_of({}, device))

        contains(str(refusal), f"names {device}, which is not there", describing="the refusal")


def test_on_windows_an_empty_device_name_is_refused_without_asking_the_table(
    windows: list[str],
) -> None:
    """Asked for no name, the table lists every device — which is not a device being there."""
    refusal = DEVICE_PRECONDITION(smoke_of({}, "\\\\.\\"))

    contains(str(refusal), "which is not there", describing="the refusal")
    equal(windows, [], describing="the names looked up in the device table")


def test_on_windows_the_harness_names_a_device_the_table_carries(windows: list[str]) -> None:
    """Windows has no pseudo-terminal, so the journeys point the smoke at its null device."""
    with a_serial_device() as device:
        equal(device, WINDOWS_DEVICE, describing="the device a Windows journey names")
        equal(DEVICE_PRECONDITION(smoke_of({}, device)), None, describing=f"the device {device}")


def test_on_windows_the_program_the_smoke_drives_is_the_built_exe(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    """A Rust build on Windows leaves `printobserver.exe`, and that is what is driven."""
    monkeypatch.setattr(host_platform, "system", lambda: "Windows")
    monkeypatch.setattr(host_platform, "machine", lambda: "AMD64")

    equal(world.built_program().name, "printobserver.exe", describing="the program on Windows")
