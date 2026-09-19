"""The one supervisor all three clients' printer-integration journeys drive.

It is stood up by this repository's own tool rather than by each client in its
own language: a world built three times would be three worlds, and what the
three journeys are for is that the same nine steps against the same supervisor
come out the same in all three.
"""

from __future__ import annotations

import json
import os
import subprocess
from dataclasses import dataclass
from pathlib import Path
from types import TracebackType
from typing import NewType

from repo_checks.shell import start

REPO_ROOT = Path(__file__).resolve().parents[3]

#: How long the shared world is given to come up, read from the same committed
#: value as the Node journey rather than copied between two clients.
_STARTUP_TIMEOUT_TEXT = (
    REPO_ROOT / "tools/release-artifacts/world-startup-timeout-seconds"
).read_text(encoding="utf-8")
STARTUP_TIMEOUT_SECONDS = int(_STARTUP_TIMEOUT_TEXT)
if STARTUP_TIMEOUT_SECONDS <= 0:
    message = "the shared world startup timeout must be a positive whole number of seconds"
    raise ValueError(message)


#: The API credential a supervisor serves under, as a request presents it.
Credential = NewType("Credential", str)


@dataclass(frozen=True, slots=True)
class Supervisor:
    """Where the supervisor is, and what a journey acts on."""

    server: str
    #: The credential it serves under, as the client configuration it wrote
    #: carries it.
    credential: Credential
    print_id: str
    image_id: str
    event_id: str
    file_name: str


def _pythonpath() -> str:
    """The packages this repository's own tools live in, read from the justfile.

    Joined with this host's own separator, as the justfile's export joins them:
    a Windows interpreter reads a `:`-joined search path as one entry.

    Raises:
        AssertionError: If the justfile lists none, which is a tree these
            tools are not reachable in.
    """
    for line in (REPO_ROOT / "justfile").read_text(encoding="utf-8").splitlines():
        if line.startswith("TOOL_PACKAGES :="):
            return os.pathsep.join(line.partition(":=")[2].strip().strip('"').split(":"))
    message = "the justfile lists no TOOL_PACKAGES, and this tier's world lives on it"
    raise AssertionError(message)


class Standing:
    """A supervisor held up for as long as this is entered."""

    def __init__(self, into: Path) -> None:
        """Bring one up under `into`, over the scripted `OctoPrint`."""
        self.into = into
        self._holding: subprocess.Popen[str] | None = None

    def __enter__(self) -> Supervisor:
        """Start it, and say where it is.

        Returns:
            Where the supervisor answers and what a journey acts on.

        Raises:
            AssertionError: If it did not come up, naming the recipe that
                brings the printer environment up: a tier that quietly passed
                against no printer would prove nothing.
        """
        self._holding = start(
            [
                "uv",
                "run",
                "-q",
                "python",
                "-m",
                "release_artifacts",
                "world",
                "--octoprint",
                "--into",
                str(self.into),
            ],
            cwd=REPO_ROOT,
            env={**os.environ, "PYTHONPATH": _pythonpath()},
        )
        said = self._holding.stdout.readline() if self._holding.stdout else ""
        if not said.strip():
            why = self._holding.stderr.read() if self._holding.stderr else ""
            self._holding.kill()
            message = (
                "the world did not come up. Run `just octoprint-up` first; this tier "
                f"drives a real OctoPrint and has no fixture to fall back to.\n{why}"
            )
            raise AssertionError(message)
        described = json.loads(said)
        return Supervisor(
            server=described["server"],
            credential=Credential(described["credential"]),
            print_id=described["print_id"],
            image_id=described["image_id"],
            event_id=described["event_id"],
            file_name=described["file_name"],
        )

    def __exit__(
        self,
        kind: type[BaseException] | None,
        value: BaseException | None,
        traceback: TracebackType | None,
    ) -> None:
        """Close the input the world waits on, which is how it is asked to stop."""
        if self._holding is None:
            return
        if self._holding.stdin is not None:
            self._holding.stdin.close()
        try:
            self._holding.wait(timeout=120)
        except subprocess.TimeoutExpired:
            self._holding.kill()
            self._holding.wait(timeout=120)
        self._holding = None
