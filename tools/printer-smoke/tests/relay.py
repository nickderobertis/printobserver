#!/usr/bin/env python3
"""A stand-in for the installed program that stops answering, on demand.

Everything the smoke does it does by running one command and reading what came
back, and the failure this exists to produce is the one where nothing comes
back at all: a command that never answers inside its bound. That is not
something a controlled machine can be scripted into — it answers or it does
not — so it is produced here, in front of the real program, by not answering.

This is one long-lived process, started once by a test's `World` and stopped
with it. The smoke's program is `relay_client.rs`, compiled: a thin client that
hands this process each command's arguments and input over a loopback
connection and answers with what comes back. So no command pays for an
interpreter's start-up, which is what a command's bound would otherwise have to
cover on a loaded host.

Every command is passed straight through to the program `SMOKE_RELAY_PROGRAM`
names, except the ones the environment below selects, which are never answered
— the caller's own bound stops them:

* `SMOKE_RELAY_HANG_ON` — the client command to stop answering on.
* `SMOKE_RELAY_AFTER` — how many of that command to answer normally first.
* `SMOKE_RELAY_ARMED_BY` — a command that arms the hang, so that a run can be
  driven all the way through and only then meet a machine it cannot read.
* `SMOKE_RELAY_STATE` — the file this counts in, one run's own.

It prints the address it listens on as its first line, and it stops when its
standard input ends, taking every program it started with it.
"""

from __future__ import annotations

import contextlib
import io
import json
import os
import socket
import subprocess
import sys
import threading
from collections.abc import Callable
from dataclasses import dataclass, field
from pathlib import Path
from typing import IO, BinaryIO

from repo_checks.shell import PROGRAM_NOT_FOUND, start

PROGRAM = "SMOKE_RELAY_PROGRAM"
HANG_ON = "SMOKE_RELAY_HANG_ON"
AFTER = "SMOKE_RELAY_AFTER"
ARMED_BY = "SMOKE_RELAY_ARMED_BY"
STATE = "SMOKE_RELAY_STATE"

#: What the relay sets in the environment of every program it runs: its own
#: process id, so a program can say which relay it was run by.
SERVED_BY = "SMOKE_RELAY_SERVED_BY"

#: Where the relay listens: loopback, on a port the system chooses.
LISTEN_ON = ("127.0.0.1", 0)

#: The most arguments one command may carry, and the most bytes one frame may:
#: far beyond anything the smoke sends, and short of what would exhaust a host.
MOST_ARGUMENTS = 1024
MOST_FRAME_BYTES = 16 * 1024 * 1024

#: Where a state is written before it is moved over the file: beside it, so
#: that the move is within one directory and the platform does it as one step.
STAGING_SUFFIX = ".next"


@dataclass(frozen=True)
class RelayState:
    """What the relay has counted: whether the hang is armed, and what it has seen."""

    armed: bool
    seen: int

    @classmethod
    def of(cls, document: object) -> RelayState | None:
        """The state a document is, or `None` where it is not one this wrote.

        Args:
            document: What a state file decoded to.
        """
        if not isinstance(document, dict):
            return None
        armed, seen = document.get("armed"), document.get("seen")
        if not isinstance(armed, bool) or isinstance(seen, bool) or not isinstance(seen, int):
            return None
        return cls(armed=armed, seen=seen)

    @classmethod
    def read(cls, state_file: Path, *, armed: bool) -> RelayState:
        """The state the file holds, or the starting one where there is none yet.

        Args:
            state_file: The file this relay counts in.
            armed: Whether the hang starts armed, for a run that has not
                written yet.

        Raises:
            ValueError: If the file holds anything but a state this wrote. It is
                this relay's own record and nothing else writes it, so what is
                not a state is a defect to stop on rather than to count from.
        """
        if not state_file.is_file():
            return cls(armed=armed, seen=0)
        document = json.loads(state_file.read_text(encoding="utf-8"))
        state = cls.of(document)
        if state is None:
            message = f"{state_file} holds {document!r} rather than a relay state"
            raise ValueError(message)
        return state

    def write(self, state_file: Path) -> None:
        """Write the state so that the file holds the previous one or this one, never neither.

        The relay can be stopped wherever it happens to be — killed with the
        `World` that started it, or with the test process that did. A write
        that truncated the file in place and was killed before it filled it
        again would leave an empty file that no later relay, and no test,
        could read. So the state is written beside the file and moved over it,
        which every platform does as one step: a kill before the move leaves
        the previous state and a leftover beside it, and the next write
        replaces both.

        Args:
            state_file: The file this relay counts in.
        """
        staging = state_file.with_name(state_file.name + STAGING_SUFFIX)
        staging.write_text(json.dumps({"armed": self.armed, "seen": self.seen}), encoding="utf-8")
        staging.replace(state_file)


@dataclass(frozen=True)
class Reply:
    """What the client is answered with: the program's exit status and both of its streams."""

    status: int
    output: bytes
    error: bytes

    @classmethod
    def refusing(cls, reason: object) -> Reply:
        """A reply refusing the command, saying why on its error stream."""
        return cls(status=1, output=b"", error=f"{reason}\n".encode())

    def send(self, connection: socket.socket) -> None:
        """Send this to the client."""
        connection.sendall(
            self.status.to_bytes(4, "big", signed=True)
            + len(self.output).to_bytes(4, "big")
            + self.output
            + len(self.error).to_bytes(4, "big")
            + self.error
        )


@dataclass
class Relay:
    """One relay: what it passes commands to, what it hangs, and what it has started."""

    program: str
    hang_on: str
    after: int
    armed_by: str
    state_file: Path
    stopping: threading.Event = field(default_factory=threading.Event)
    lock: threading.Lock = field(default_factory=threading.Lock)
    running: set[subprocess.Popen[str]] = field(default_factory=set)

    @classmethod
    def from_environment(cls) -> Relay:
        """The relay the environment this process was started under describes."""
        return cls(
            program=os.environ[PROGRAM],
            hang_on=os.environ.get(HANG_ON, ""),
            after=int(os.environ.get(AFTER, "0")),
            armed_by=os.environ.get(ARMED_BY, ""),
            state_file=Path(os.environ[STATE]),
        )

    def hangs(self, argv: list[str]) -> bool:
        """Count one command, and answer whether it is one never to answer.

        Args:
            argv: The arguments the smoke ran the program with.

        Raises:
            ValueError: If the state file holds anything but a state this wrote.
        """
        selected = {name for name in (self.hang_on, self.armed_by) if name}
        command = next((argument for argument in argv if argument in selected), "")
        with self.lock:
            state = RelayState.read(self.state_file, armed=not self.armed_by)
            if command == self.hang_on and state.armed:
                state = RelayState(armed=True, seen=state.seen + 1)
                state.write(self.state_file)
                return state.seen > self.after
            if command and command == self.armed_by:
                RelayState(armed=True, seen=state.seen).write(self.state_file)
        return False

    def answer(self, connection: socket.socket) -> None:
        """Answer one command as the program does, or not at all.

        Args:
            connection: The client's connection, its arguments already waiting.
        """
        with connection:
            try:
                argv = [
                    _frame(connection).decode("utf-8")
                    for _ in range(_length(connection, most=MOST_ARGUMENTS))
                ]
                hanging = self.hangs(argv)
            except ValueError as error:
                Reply.refusing(error).send(connection)
                return
            if hanging:
                # Held open, unanswered, until the caller's own bound stops the
                # client — or until this relay is stopped.
                self.stopping.wait()
                return
            self.run(argv, connection).send(connection)

    def run(self, argv: list[str], connection: socket.socket) -> Reply:
        """Run the program with `argv`, fed what the client forwards, and collect its answer.

        Args:
            argv: The arguments to run it with.
            connection: Where the client's standard input arrives from.

        Returns:
            Its exit status and both of its streams, as it wrote them.
        """
        environment = {**os.environ, SERVED_BY: str(os.getpid())}
        try:
            process = start([self.program, *argv], env=environment)
        except FileNotFoundError as error:
            return Reply(status=PROGRAM_NOT_FOUND, output=b"", error=f"{error}\n".encode())
        with self.lock:
            self.running.add(process)
        try:
            threading.Thread(target=_forward_input, args=(connection, process), daemon=True).start()
            error = _drain(process.stderr)
            output = _read(_raw(process.stdout))
            status = process.wait()
            return Reply(status=status, output=output, error=error())
        finally:
            with self.lock:
                self.running.discard(process)

    def stop(self) -> None:
        """Release every command held unanswered, and stop every program still running."""
        self.stopping.set()
        with self.lock:
            running = list(self.running)
        for process in running:
            process.kill()
            process.wait()


def _exactly(connection: socket.socket, size: int) -> bytes:
    """Read exactly `size` bytes, or fail naming a client that went away."""
    received = bytearray()
    while len(received) < size:
        chunk = connection.recv(size - len(received))
        if not chunk:
            message = "the client closed its connection mid-message"
            raise ConnectionError(message)
        received.extend(chunk)
    return bytes(received)


def _length(connection: socket.socket, *, most: int) -> int:
    """Read one length off the wire.

    Raises:
        ValueError: If it is more than `most`, which no client of this relay sends.
    """
    length = int.from_bytes(_exactly(connection, 4), "big")
    if length > most:
        message = f"the relay's client sent a length of {length}, over the most it takes ({most})"
        raise ValueError(message)
    return length


def _frame(connection: socket.socket) -> bytes:
    """Read one frame off the wire."""
    return _exactly(connection, _length(connection, most=MOST_FRAME_BYTES))


def _forward_input(connection: socket.socket, process: subprocess.Popen[str]) -> None:
    """Feed the program what the client forwards of its input, and close it at the end."""
    sink = _raw(process.stdin)
    try:
        while sink is not None and (chunk := _frame(connection)):
            sink.write(chunk)
            sink.flush()
    except OSError, ValueError:
        # The client went away, or the program stopped reading: either way
        # there is no more input for it.
        pass
    finally:
        if sink is not None:
            with contextlib.suppress(OSError):
                sink.close()


def _raw(stream: IO[str] | None) -> BinaryIO | None:
    """The bytes beneath one of a program's streams, which `start` opens as text.

    What the client writes is then what the program wrote, byte for byte.
    """
    return stream.buffer if isinstance(stream, io.TextIOWrapper) else None


def _read(stream: BinaryIO | None) -> bytes:
    """Everything `stream` holds, to its end."""
    return stream.read() if stream is not None else b""


def _drain(stream: IO[str] | None) -> Callable[[], bytes]:
    """Read `stream` to its end beside the caller, and hand back a way to collect it.

    Both of a program's streams are read at once, so that one filling while the
    other is being waited on cannot hold the program up.
    """
    collected: list[bytes] = []
    reader = threading.Thread(target=lambda: collected.append(_read(_raw(stream))), daemon=True)
    reader.start()

    def collect() -> bytes:
        reader.join()
        return b"".join(collected)

    return collect


def main() -> int:
    """Listen, answer commands until this process's input ends, and stop.

    Returns:
        Zero, once every command held and every program started has been stopped.
    """
    relay = Relay.from_environment()
    listener = socket.create_server(LISTEN_ON)
    host, port = listener.getsockname()[:2]
    sys.stdout.write(f"{host}:{port}\n")
    sys.stdout.flush()

    def serve() -> None:
        while True:
            try:
                connection, _ = listener.accept()
            except OSError:
                return
            threading.Thread(target=_answer, args=(relay, connection), daemon=True).start()

    threading.Thread(target=serve, daemon=True).start()
    sys.stdin.read()
    listener.close()
    relay.stop()
    return 0


def _answer(relay: Relay, connection: socket.socket) -> None:
    """Answer one connection; a client that went away mid-command has no answer to take."""
    with contextlib.suppress(OSError):
        relay.answer(connection)


if __name__ == "__main__":
    raise SystemExit(main())
