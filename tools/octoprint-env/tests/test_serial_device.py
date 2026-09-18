"""The serial mode is exercised, not just configured.

The instance is started against a pseudo-terminal this journey created, and
what it opened is read back off that terminal: the device it opened, and the
line speed it set on it. Asserting the configuration the script generated is
not what proves this — a script that wrote plausible serial configuration and
never reached a connection is exactly the failure this refuses.
"""

from __future__ import annotations

import sys
from collections.abc import Callable

import pytest

if sys.platform == "win32":
    # Windows has no pseudo-terminal to stand a printer on, and a hosted runner
    # has no serial port to plug one into. The configuration half of the serial
    # mode is `test_serial_mode.py`'s, and it runs everywhere.
    pytest.skip("Windows has no pseudo-terminal to stand a printer on", allow_module_level=True)

import termios

from environment import answer, api, key_from, printer_state, script
from fake_printer import FakePrinter
from repo_checks.expect import contains, equal, truth

BAUDRATE = 57600
# The `termios` constant for that baud rate, this journey's own rather than the
# script's: it is read off the terminal the instance opened.
EXPECTED_SPEED = termios.B57600

CONNECTED = frozenset({"Operational", "Printing"})


def test_the_serial_mode_opens_the_named_device_at_the_named_baud_rate(
    state_dir: Callable[[str], str],
) -> None:
    """A real connection, over a pseudo-terminal standing in for the USB device."""
    with FakePrinter() as printer:
        result = script(
            "up",
            "--state-dir",
            state_dir("serial"),
            "--mode",
            "serial",
            "--device",
            printer.device,
            "--baudrate",
            str(BAUDRATE),
        )

        started = answer(result)
        url = str(started["url"])
        key = key_from(str(started["api_key_file"]))

        # M115 is the firmware query OctoPrint sends over a port it has just
        # opened, so seeing it is the device being opened rather than described.
        truth(
            printer.saw("M115"),
            describing=(
                f"the instance to have opened {printer.device} and spoken to it; "
                f"it sent {printer.commands[:5]}"
            ),
        )
        equal(
            printer.baudrate,
            EXPECTED_SPEED,
            describing=f"the line speed set on {printer.device} (termios B{BAUDRATE})",
        )
        contains(CONNECTED, printer_state(url, key), describing="printer states that are connected")
        status, connection = api(url, "/api/connection", key)
        equal(status, 200, describing="the connection the instance reports")
        truth(isinstance(connection, dict), describing="a decoded /api/connection answer")
        current = connection["current"] if isinstance(connection, dict) else {}
        equal(current["port"], printer.device, describing="the device the instance opened")
        equal(current["baudrate"], BAUDRATE, describing="the baud rate it opened it at")
