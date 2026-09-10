"""The one supervisor all three clients' printer-integration journeys drive.

It is stood up by this repository's own tool rather than by each client in its
own language: a world built three times would be three worlds, and what the
three journeys are for is that the same nine steps against the same supervisor
come out the same in all three.
"""

from __future__ import annotations

import json
import subprocess
from dataclasses import dataclass
from pathlib import Path
from types import TracebackType

from repo_checks.shell import start

REPO_ROOT = Path(__file__).resolve().parents[3]

#: How long the world is given to come up, which includes a release build of
#: the program it runs the first time this tier is driven.
STARTUP_TIMEOUT_SECONDS = 2400


@dataclass(frozen=True, slots=True)
class Supervisor:
    """Where the supervisor is, and what a journey acts on."""

    server: str
    print_id: str
    image_id: str
    file_name: str


def _pythonpath() -> str:
    """The packages this repository's own tools live in, read from the justfile.

    Raises:
        AssertionError: If the justfile exports none, which is a tree these
            tools are not reachable in.
    """
    for line in (REPO_ROOT / "justfile").read_text(encoding="utf-8").splitlines():
        if line.startswith("export PYTHONPATH :="):
            return line.partition(":=")[2].strip().strip('"')
    message = "the justfile exports no PYTHONPATH, and this tier's world lives on it"
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
        import os

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
            print_id=described["print_id"],
            image_id=described["image_id"],
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
