"""A printer on a pseudo-terminal, for the serial mode to be exercised rather than described.

A script that wrote plausible serial configuration and never reached a
connection is the failure a person standing next to the Prusa would find. So
the serial journey creates a pseudo-terminal, speaks enough Marlin over it for
OctoPrint to call the connection established, and then reads back — from the
terminal's own attributes, not from the script — which device was opened and
at what baud rate.
"""

from __future__ import annotations

import contextlib
import os
import pty
import re
import select
import termios
import threading
import time
import tty

# What a printer answers: a G-, M- or T-code, with or without a line number and
# a checksum around it. Anything else on the wire is not a command.
COMMAND = re.compile(r"^[GMT]\d+")


def code_of(line: str) -> str:
    """The G-code or M-code one sent line carries, without line number or checksum."""
    body = line.split("*")[0].strip()
    if body.startswith("N") and " " in body:
        body = body.split(" ", 1)[1]
    words = body.split()
    return words[0] if words else ""


class FakePrinter:
    """A Marlin-speaking device on a pseudo-terminal a test owns."""

    def __init__(self) -> None:
        """Open the terminal pair and note the device the other end will open."""
        self.controller, self.peripheral = pty.openpty()
        # Raw, so the terminal does not echo what is written to it back at the
        # writer: an echoing terminal answers its own answers forever, and
        # nothing the other end sent would ever be read.
        tty.setraw(self.peripheral)
        self.device = os.ttyname(self.peripheral)
        self.commands: list[str] = []
        self._stop = threading.Event()
        self._thread = threading.Thread(target=self._serve, daemon=True)

    def __enter__(self) -> FakePrinter:
        """Start answering."""
        self._thread.start()
        return self

    def __exit__(self, *_: object) -> None:
        """Stop answering and close the terminal."""
        self.close()

    @property
    def baudrate(self) -> int:
        """The line speed whoever opened the device set, as a `termios` constant.

        A pseudo-terminal's two ends share their attributes, so this is the baud
        rate the other end really asked for rather than one it reported.
        """
        return int(termios.tcgetattr(self.controller)[5])

    def saw(self, command: str) -> bool:
        """Whether this exact code has been sent to the device."""
        return any(code_of(seen) == command for seen in self.commands)

    def close(self) -> None:
        """Stop answering and close both ends."""
        self._stop.set()
        self._thread.join(timeout=5)
        for handle in (self.controller, self.peripheral):
            with contextlib.suppress(OSError):
                os.close(handle)

    def _serve(self) -> None:
        """Answer whatever is sent, the way a printer's firmware does."""
        self._say("start")
        pending = b""
        while not self._stop.is_set():
            readable, _, _ = select.select([self.controller], [], [], 0.2)
            if not readable:
                continue
            try:
                pending += os.read(self.controller, 4096)
            except OSError:
                return
            while b"\n" in pending:
                line, pending = pending.split(b"\n", 1)
                command = line.decode("utf-8", errors="replace").strip()
                if COMMAND.match(code_of(command)):
                    self.commands.append(command)
                    self._answer(command)

    def _say(self, line: str) -> None:
        """Send one line back over the terminal."""
        try:
            os.write(self.controller, f"{line}\n".encode())
        except OSError:
            self._stop.set()

    def _answer(self, command: str) -> None:
        """One firmware answer, enough of Marlin for a connection to be established."""
        body = command.split("*")[0].strip()
        if body.startswith("N") and " " in body:
            body = body.split(" ", 1)[1]
        match code_of(command):
            case "M105":
                self._say("ok T:21.3 /0.0 B:20.9 /0.0")
            case "M115":
                self._say(
                    "FIRMWARE_NAME:FakeMarlin 1.0 PROTOCOL_VERSION:1.0 "
                    "MACHINE_TYPE:printobserver-fake EXTRUDER_COUNT:1"
                )
                self._say("ok")
            case "M114":
                self._say("X:0.00 Y:0.00 Z:0.00 E:0.00 Count X:0 Y:0 Z:0")
                self._say("ok")
            case "G4":
                self._dwell(body)
                self._say("ok")
            case _:
                self._say("ok")

    def _dwell(self, body: str) -> None:
        """Wait out a dwell, the way the machine would."""
        seconds = 0.0
        for part in body.split()[1:]:
            if part.startswith("S"):
                seconds = float(part[1:])
            elif part.startswith("P"):
                seconds = float(part[1:]) / 1000.0
        deadline = time.monotonic() + seconds
        while time.monotonic() < deadline and not self._stop.is_set():
            time.sleep(0.2)
