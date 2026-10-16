"""The stand-in machine the world serves, driven over the wire the supervisor uses.

The supervisor keeps its connections to the machine open and is stopped from
outside, so its connections end however the host ends them. What a proof's
output carries afterwards is what these hold: one line per proof, and nothing
the machine's server said about a peer that went away.
"""

from __future__ import annotations

import socket
import struct
import time

import pytest
from release_artifacts.world import Machine
from repo_checks.expect import contains, equal, truth


def _connect(machine: Machine) -> socket.socket:
    """One open connection to the machine, as the supervisor keeps one."""
    host, _, port = machine.url.rpartition("://")[2].partition(":")
    connection = socket.create_connection((host, int(port)), timeout=10)
    connection.sendall(b"GET /api/version HTTP/1.1\r\nHost: machine\r\n\r\n")
    answered = connection.recv(65536)
    contains(answered.decode("utf-8", "replace"), "HTTP/1.1 ", describing="the machine's answer")
    return connection


def test_a_peer_that_resets_its_connection_leaves_nothing_on_standard_error(
    capfd: pytest.CaptureFixture[str],
) -> None:
    """A stopped supervisor's connections reset, and the machine says nothing about it.

    The connection is closed the way Windows closes a stopped process's — with
    a reset rather than an orderly end — which is what reaches the handler as
    `ConnectionResetError`. The default server prints a traceback per such
    connection to standard error, three per proof, which is how a tier whose
    pass is one line per proof came to say seventy-five.
    """
    machine = Machine()
    try:
        connections = [_connect(machine) for _ in range(3)]
        for connection in connections:
            # Linger of zero: closing sends a reset rather than a FIN.
            connection.setsockopt(socket.SOL_SOCKET, socket.SO_LINGER, struct.pack("ii", 1, 0))
            connection.close()
        # The handler threads meet the reset on their next read; give them the
        # moment that takes before the server is stopped underneath them.
        time.sleep(0.5)
    finally:
        machine.stop()

    captured = capfd.readouterr()
    equal(captured.err, "", describing="what the machine's server said about the resets")
    truth(captured.out == "", describing=f"nothing on standard output either: {captured.out!r}")


def test_a_peer_gone_before_its_answers_are_written_leaves_nothing_on_standard_error(
    capfd: pytest.CaptureFixture[str],
) -> None:
    """A peer that leaves before its answers are written is nothing the machine reports.

    On macOS a peer's reset reaches the handler mid-answer as a broken pipe on
    write rather than as `ConnectionResetError`. Here the peer asks a run of
    questions and closes without reading one answer: the first answer written
    draws a reset from it, and a write after that reset fails with
    `BrokenPipeError` — the same error, from the same write, on Linux. On
    Windows that write fails with `ConnectionAbortedError` instead.
    """
    machine = Machine()
    try:
        host, _, port = machine.url.rpartition("://")[2].partition(":")
        for _ in range(3):
            with socket.create_connection((host, int(port)), timeout=10) as connection:
                connection.sendall(b"GET /api/version HTTP/1.1\r\nHost: machine\r\n\r\n" * 64)
        # The handler threads write into the closed connections; give them the
        # moment that takes before the server is stopped underneath them.
        time.sleep(0.5)
    finally:
        machine.stop()

    captured = capfd.readouterr()
    equal(captured.err, "", describing="what the machine's server said about the broken pipes")
    truth(captured.out == "", describing=f"nothing on standard output either: {captured.out!r}")


def test_a_request_the_machine_fails_on_still_reaches_standard_error(
    capfd: pytest.CaptureFixture[str],
) -> None:
    """Only a peer going away is kept quiet: any other failure is still reported."""
    machine = Machine()
    try:
        host, _, port = machine.url.rpartition("://")[2].partition(":")
        with socket.create_connection((host, int(port)), timeout=10) as connection:
            connection.sendall(
                b"POST /api/job HTTP/1.1\r\nHost: machine\r\nContent-Length: many\r\n\r\n"
            )
            # The handler fails on the length and closes without answering.
            equal(
                connection.recv(65536), b"", describing="the machine's answer to a malformed post"
            )
    finally:
        machine.stop()

    captured = capfd.readouterr()
    contains(
        captured.err,
        "ValueError",
        describing="what the machine's server said about the malformed post",
    )


def test_the_machine_answers_without_looking_its_own_address_up(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    """A machine starts and answers with no name lookup of the address it bound.

    The standard server names itself by looking its address up, which on a
    hosted macOS runner waits out a resolver timeout of about 35 seconds — once
    per world, so every client journey's world start paid it. A lookup here
    fails the start, so the machine coming up and answering is the proof it
    made none.
    """
    looked_up: list[str] = []

    def lookup(name: str = "") -> str:
        looked_up.append(name)
        msg = f"the machine looked its own address up: {name!r}"
        raise AssertionError(msg)

    monkeypatch.setattr(socket, "getfqdn", lookup)
    machine = Machine()
    try:
        connection = _connect(machine)
        connection.close()
    finally:
        machine.stop()

    equal(looked_up, [], describing="the name lookups the machine made of its own address")
