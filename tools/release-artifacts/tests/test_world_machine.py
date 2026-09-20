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
