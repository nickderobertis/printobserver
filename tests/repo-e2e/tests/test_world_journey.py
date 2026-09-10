"""A client world is ready only after its alert has observed the printer."""

from __future__ import annotations

import threading
from concurrent.futures import ThreadPoolExecutor
from pathlib import Path

import pytest
from journey import REPO_ROOT
from release_artifacts.build import program
from release_artifacts.world import Machine, Printer, World
from repo_checks.expect import truth
from repo_checks.model import Repo


class HeldPrinter(Machine):
    """A real HTTP printer boundary whose alert snapshot can be held back."""

    def __init__(self) -> None:
        """Start listening with the alert's snapshot held until released."""
        self.requested = threading.Event()
        self.release = threading.Event()
        self.snapshots = 0
        super().__init__()

    def answer(self, path: str) -> tuple[int, bytes]:
        """Hold the snapshot after the ingress has already stored its image."""
        if path.startswith("/api/printer"):
            self.snapshots += 1
            # The first read is the server's startup configuration probe.
            if self.snapshots > 1:
                self.requested.set()
                self.release.wait(timeout=30)
        return super().answer(path)


def test_world_waits_for_the_alert_before_a_client_can_change_the_printer(tmp_path: Path) -> None:
    """An image in the store does not mean the asynchronous alert is finished."""
    binary = program(Repo(REPO_ROOT), None)
    printer = HeldPrinter()
    world = World(binary, tmp_path, Printer(printer.url, "test-key", scripted=False))
    try:
        with ThreadPoolExecutor(max_workers=1) as workers:
            starting = workers.submit(world.start)
            try:
                truth(
                    printer.requested.wait(timeout=30),
                    describing="the real server reading the printer",
                )
                # The HTTP handler is blocked, not slow: returning here lets a
                # journey cancel before the alert samples the original print.
                with pytest.raises(TimeoutError):
                    starting.result(timeout=0.5)
            finally:
                printer.release.set()
            running = starting.result(timeout=60)
            truth(running.print_id, describing="the ready world's print")
    finally:
        printer.release.set()
        world.stop()
        printer.stop()
