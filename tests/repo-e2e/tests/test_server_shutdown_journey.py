"""The supervisor stops the way its own platform stops a process, and stops cleanly.

`printobserver server` runs until the operating system asks it to stop. On Unix
that is `SIGTERM`, which a service manager stops a unit with; on Windows, which
has no signals, it is a console control event, and `CTRL_BREAK_EVENT` is the one
another process can address to a process group of its own. Each journey here
builds the real program, starts the real server over a state directory and a
stand-in OctoPrint that answers, confirms it is listening, and then asks it to
stop by this host's own means.

What is asserted is what a service manager reads: the program exited with
status zero, which a process killed outright by either platform never does, and
the address it served on no longer answers. Both mechanisms enter the one
shutdown the server has, so the same journey proves each on its own platform.
"""

from __future__ import annotations

import json
import os
import queue
import signal
import socket
import sys
import threading
from collections.abc import Iterator
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from pathlib import Path

import pytest
from journey import REPO_ROOT, SKILL_DIRECTORY, clean_environment, run
from repo_checks.expect import equal, passing, truth
from repo_checks.model import Repo
from repo_checks.platforms import host
from repo_checks.shell import start

CREDENTIAL = "a-credential-this-journey-configures-4f1c7d2e9b"
SERVING = "printobserver is serving on "
STOPPED_WITHIN_S = 60


class _AnswersEverything(BaseHTTPRequestHandler):
    """An OctoPrint that answers every request with an empty document.

    The server asks the machine one question before it listens, and refuses to
    start when nothing answers or the key is refused; anything else will do.
    """

    def do_GET(self) -> None:
        """Answer an empty document."""
        body = b"{}"
        self.send_response(200)
        self.send_header("Content-Type", "application/json")
        self.send_header("Content-Length", str(len(body)))
        self.end_headers()
        self.wfile.write(body)

    def log_message(self, format: str, *args: object) -> None:
        """Say nothing: the journey's own output is what a reader reads."""


@pytest.fixture
def octoprint() -> Iterator[str]:
    """Where a stand-in OctoPrint answers, for as long as the journey runs."""
    server = ThreadingHTTPServer(("127.0.0.1", 0), _AnswersEverything)
    thread = threading.Thread(target=server.serve_forever, daemon=True)
    thread.start()
    try:
        yield f"http://127.0.0.1:{server.server_address[1]}"
    finally:
        server.shutdown()
        server.server_close()


def _program() -> Path:
    """The `printobserver` program, built from this tree for this host."""
    built = run(["cargo", "build", "--locked", "-p", "printobserver"], cwd=REPO_ROOT, timeout=1800)
    passing(built, describing="building the printobserver program")
    return REPO_ROOT / "target" / "debug" / host(Repo(REPO_ROOT)).program


def _configuration(root: Path, octoprint: str) -> Path:
    """The configuration the installer writes, with its three blanks filled in."""
    path = root / "config.toml"
    lines = [
        f"state_dir = {json.dumps(str(root / 'state'))}",
        'listen = "127.0.0.1:0"',
        "[octoprint]",
        f"url = {json.dumps(octoprint)}",
        'api_key = "a-provisioned-key"',
        'fan = "commandable"',
        "[supervisor]",
        'harness = "claude-code"',
        f"skill_path = {json.dumps(str(SKILL_DIRECTORY / 'SKILL.md'))}",
        "[ingress]",
        'shared_secret = "a-shared-secret"',
        "[api]",
        f"credential = {json.dumps(CREDENTIAL)}",
        "[safety]",
        "agent_min_interval_s = 30",
        "[safety.allowed]",
        "feedrate = { min = 0.5, max = 1.5 }",
        "[safety.actions]",
        'operator = ["pause"]',
        "agent = []",
        "system = []",
    ]
    path.write_text("\n".join(lines) + "\n", encoding="utf-8")
    return path


def _serving_on(lines: queue.Queue[str | None]) -> str:
    """The address the server said it is serving on.

    Raises:
        AssertionError: If it never says so, carrying everything it did say.
    """
    said: list[str] = []
    while True:
        try:
            line = lines.get(timeout=120)
        except queue.Empty:
            break
        if line is None:
            break
        said.append(line)
        if line.startswith(SERVING):
            return line.removeprefix(SERVING).strip()
    message = f"the server never said where it is serving; it said:\n{''.join(said)}"
    raise AssertionError(message)


def _still_listening(address: str) -> bool:
    """Whether anything still accepts a connection at the address."""
    hostname, _, port = address.rpartition(":")
    try:
        with socket.create_connection((hostname, int(port)), timeout=5):
            return True
    except OSError:
        return False


def _ask_to_stop(pid: int) -> str:
    """Ask the process to stop the way this platform asks one, and say how."""
    if sys.platform == "win32":
        os.kill(pid, signal.CTRL_BREAK_EVENT)
        return "CTRL_BREAK_EVENT"
    os.kill(pid, signal.SIGTERM)
    return "SIGTERM"


def test_the_server_stops_cleanly_when_its_platform_asks_it_to(
    tmp_path: Path, octoprint: str
) -> None:
    """Serving, asked to stop by this host's own means, and gone with status zero."""
    program = _program()
    configuration = _configuration(tmp_path, octoprint)
    server = start(
        [str(program), "server", "--config", str(configuration)],
        cwd=tmp_path,
        env=clean_environment(),
        own_group=True,
    )
    lines: queue.Queue[str | None] = queue.Queue()

    def read_stderr() -> None:
        stream = server.stderr
        if stream is not None:
            for line in stream:
                lines.put(line)
        lines.put(None)

    reader = threading.Thread(target=read_stderr, daemon=True)
    reader.start()
    try:
        address = _serving_on(lines)
        truth(_still_listening(address), describing=f"the running server to accept at {address}")

        mechanism = _ask_to_stop(server.pid)
        code = server.wait(timeout=STOPPED_WITHIN_S)

        equal(
            code,
            0,
            describing=f"the status the server exited with once sent {mechanism}",
        )
        truth(
            not _still_listening(address),
            describing=f"{address} to have stopped answering once the server stopped",
        )
    finally:
        if server.poll() is None:
            server.kill()
            server.wait()
