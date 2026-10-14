"""A client world is ready only after its alert has observed the printer, and asks no agent."""

from __future__ import annotations

import json
import os
import sqlite3
import sys
import threading
from concurrent.futures import ThreadPoolExecutor
from pathlib import Path

import pytest
from journey import RELEASE_BUILD, REPO_ROOT
from release_artifacts.build import program
from release_artifacts.stand_in import stand_in_program
from release_artifacts.world import STORE, Machine, Printer, World
from repo_checks.expect import contains, equal, truth
from repo_checks.model import Repo

pytestmark = RELEASE_BUILD


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


def test_world_runs_its_supervision_turn_over_no_harness_the_host_has(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    """The alert's turn finds no harness, even with one on the path the world runs with.

    A host with the supervising agent's harness installed would otherwise hold
    every client world's start for one real conversation with a real agent.
    The world puts nothing on its supervisor's path, so the turn is recorded the
    way a missing harness is, against the event, and the world is ready then.
    """
    harnesses = tmp_path / "harnesses"
    name = "claude.exe" if sys.platform == "win32" else "claude"
    stand_in_program(Repo(REPO_ROOT), harnesses / name, "claude 0.0.0 (a stand-in)")
    monkeypatch.setenv("PATH", f"{harnesses}{os.pathsep}{os.environ['PATH']}")
    binary = program(Repo(REPO_ROOT), None)
    printer = Machine()
    world = World(binary, tmp_path / "world", Printer(printer.url, "test-key", scripted=False))
    try:
        running = world.start()
    finally:
        world.stop()
        printer.stop()

    truth(running.print_id, describing="the ready world's print")
    store = sqlite3.connect(f"file:{tmp_path / 'world' / 'state' / STORE}?mode=ro", uri=True)
    try:
        failures = [
            json.loads(payload)["payload"]
            for (payload,) in store.execute(
                "SELECT payload FROM events WHERE kind = 'port_failure'"
            ).fetchall()
        ]
    finally:
        store.close()
    turns = [failure for failure in failures if failure.get("site") == "supervision_turn"]
    equal(len(turns), 1, describing=f"the alert's supervision turn, recorded once: {failures}")
    contains(
        turns[0]["detail"],
        "`claude` not found on PATH",
        describing="the turn's failure, which is a missing harness rather than one that ran",
    )
