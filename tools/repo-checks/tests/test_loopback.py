"""A loopback stand-in is ready without asking any name service what it is called.

Each start here runs with the name service refused on the starting thread, so
a stand-in that consulted one fails naming the lookup rather than passing on a
host whose resolver happens to be quick.
"""

from __future__ import annotations

import ast
import socket
import threading
from collections.abc import Iterator
from contextlib import contextmanager
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from pathlib import Path

import pytest
from repo_checks.expect import contains, equal, truth
from repo_checks.loopback import LoopbackServer, NameServiceRefusedError, name_service_refused
from standin_release import Release, serving
from treecopy import REPO_ROOT

#: Where loopback stand-ins live: every Python file under these.
SWEPT = ("tools", "tests", "python", "scripts")

#: The HTTP servers the standard library builds, each naming itself by a lookup.
STANDARD = frozenset({"HTTPServer", "ThreadingHTTPServer"})

#: Files that build a standard HTTP server, and why each may.
NOT_LOOPBACK = {
    # It listens on every interface for a container's webhook rather than on
    # loopback, under the address that container is configured to post to.
    "tools/obico-env/obico_tier.py": "listens on every interface for the Obico stack",
    # It is run by a bare interpreter that cannot import `repo_checks`, so it
    # binds by number the way `LoopbackServer` does, inline.
    "tools/release-artifacts/tests/unprepared_ingress.py": "binds by number inline",
    # The client distribution's own suites run where `repo_checks` is absent.
    "python/printobserver-sdk/tests/host.py": "the SDK's suite runs without repo_checks",
    "python/printobserver-sdk/integration/live.py": "the SDK's suite runs without repo_checks",
    # This module and the helper itself.
    "tools/repo-checks/tests/test_loopback.py": "builds the standard server to refuse it",
    "tools/repo-checks/src/repo_checks/loopback.py": "is the helper",
}


class _Answers(BaseHTTPRequestHandler):
    def do_GET(self) -> None:
        self.send_response(204)
        self.end_headers()

    def log_message(self, format: str, *args: object) -> None:
        """Say nothing: the assertions are the signal."""


def _status(port: int) -> bytes:
    """The status line a server on loopback `port` answers, asked by number alone."""
    with socket.socket(socket.AF_INET, socket.SOCK_STREAM) as connection:
        connection.settimeout(30)
        connection.connect(("127.0.0.1", port))
        connection.sendall(b"GET / HTTP/1.0\r\n\r\n")
        return connection.recv(1024).split(b"\r\n", 1)[0]


@contextmanager
def _serving_forever(server: ThreadingHTTPServer) -> Iterator[int]:
    thread = threading.Thread(target=server.serve_forever, daemon=True)
    thread.start()
    try:
        yield int(server.server_address[1])
    finally:
        server.shutdown()
        server.server_close()


def test_the_helper_is_ready_and_answers_with_every_lookup_refused() -> None:
    """Built, served and asked on a thread that can resolve nothing, it answers."""
    with name_service_refused():
        with pytest.raises(NameServiceRefusedError, match="getaddrinfo"):
            socket.getaddrinfo("localhost", 80)
        with _serving_forever(LoopbackServer(_Answers)) as port:
            equal(_status(port), b"HTTP/1.0 204 No Content", describing="what it answered")


def test_the_helper_names_itself_by_the_address_it_bound() -> None:
    """Its name is the number it bound, so its URL needs no resolver either."""
    server = LoopbackServer(_Answers)
    try:
        equal(server.server_name, "127.0.0.1", describing="the name it gives itself")
        equal(server.url, f"http://127.0.0.1:{server.server_address[1]}", describing="its URL")
    finally:
        server.server_close()


def test_the_standard_server_is_refused_for_its_reverse_lookup() -> None:
    """The guard catches the lookup a default-built server makes before it is ready."""
    with name_service_refused(), pytest.raises(NameServiceRefusedError) as refused:
        ThreadingHTTPServer(("127.0.0.1", 0), _Answers).server_close()

    contains(str(refused.value), "socket.getfqdn", describing="the lookup it names")


def test_a_stand_in_that_looks_itself_up_fails_its_start() -> None:
    """Binding the standard way under the helper fails the start rather than waiting."""

    class LooksItselfUp(LoopbackServer):
        def server_bind(self) -> None:
            ThreadingHTTPServer.server_bind(self)

    with pytest.raises(NameServiceRefusedError, match="getfqdn"):
        LooksItselfUp(_Answers).server_close()


def test_the_refusal_is_the_starting_thread_s_alone() -> None:
    """A lookup on another thread while a stand-in starts is answered as usual."""
    resolved: list[object] = []
    with name_service_refused():
        beside = threading.Thread(
            target=lambda: resolved.append(socket.getaddrinfo("127.0.0.1", 80)[0][4][0])
        )
        beside.start()
        beside.join(timeout=30)
    equal(resolved, ["127.0.0.1"], describing="what the other thread resolved")


def test_the_release_stand_in_is_ready_with_every_lookup_refused() -> None:
    """Started where nothing resolves, the forge's release stand-in still answers."""
    with name_service_refused(), serving("/r", Release()) as url:
        truth(
            _status(int(url.rsplit(":", 1)[1])).startswith(b"HTTP/1."),
            describing="a status line from it",
        )


def _builds_a_standard_server(source: str) -> list[int]:
    lines: list[int] = []
    for node in ast.walk(ast.parse(source)):
        if isinstance(node, ast.Call):
            called = node.func
            name = called.attr if isinstance(called, ast.Attribute) else getattr(called, "id", "")
            if name in STANDARD:
                lines.append(node.lineno)
        if isinstance(node, ast.ClassDef):
            for base in node.bases:
                name = base.attr if isinstance(base, ast.Attribute) else getattr(base, "id", "")
                if name in STANDARD:
                    lines.append(node.lineno)
    return lines


def _python_files() -> Iterator[Path]:
    for top in SWEPT:
        for path in sorted((REPO_ROOT / top).rglob("*.py")):
            parts = set(path.relative_to(REPO_ROOT).parts)
            if parts & {"node_modules", ".venv", "__pycache__"}:
                continue
            yield path


def test_every_loopback_stand_in_is_built_on_the_helper() -> None:
    """No file builds a standard HTTP server except those named, each with its reason."""
    found = {
        f"{path.relative_to(REPO_ROOT).as_posix()}:{line}"
        for path in _python_files()
        if path.relative_to(REPO_ROOT).as_posix() not in NOT_LOOPBACK
        for line in _builds_a_standard_server(path.read_text(encoding="utf-8"))
    }
    equal(sorted(found), [], describing="standard HTTP servers built outside LoopbackServer")
