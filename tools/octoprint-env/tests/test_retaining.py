"""What `retain` keeps of an instance that never wrote some of its logs.

Each journey below leaves an instance short of a log in the way a real start
does — refused before anything started, or started and dead before OctoPrint
set its logging up — and runs the real script's `retain` over it. Exactly the
logs the instance never wrote are reported absent, by name, and every one it
did write is kept as it wrote it. The case where it wrote all three is the
failed tier's own, driven from `tests/repo-e2e/tests/test_octoprint_tier.py`.
"""

from __future__ import annotations

import json
import shutil
from collections.abc import Callable
from pathlib import Path

from environment import answer, script
from repo_checks.expect import equal, failing, passing, truth

#: Every log a failed tier keeps, in the order `retain` reports them.
REQUIRED = ["server.log", "logs/octoprint.log", "logs/serial.log"]


def _retained(state: str, into: Path) -> dict[str, object]:
    kept = script("retain", "--state-dir", state, "--into", str(into))
    passing(kept, describing="keeping the instance's logs")
    record = answer(kept)
    equal(
        json.loads((into / "retained.json").read_text(encoding="utf-8")),
        record,
        describing="the record kept beside the logs, against what `retain` answered",
    )
    equal(
        record["instance"],
        {"state_dir": str(Path(state).resolve()), "record": None},
        describing="the instance the kept logs name",
    )
    return record


def test_an_instance_refused_before_it_started_has_every_log_reported_absent(
    state_dir: Callable[[str], str], tmp_path: Path
) -> None:
    """Refused before anything was provisioned, it wrote no log, and each is named."""
    state = state_dir("refused")
    into = tmp_path / "kept"
    refused = script(
        "up", "--state-dir", state, "--mode", "serial", "--device", str(tmp_path / "no-device")
    )
    failing(refused, naming="device")

    record = _retained(state, into)

    equal(record["retained"], [], describing="the logs kept")
    equal(record["absent"], REQUIRED, describing="the logs reported absent")
    for name in REQUIRED:
        truth(not (into / name).exists(), describing=f"no kept {name}")


def test_an_instance_that_died_before_logging_keeps_what_it_said_and_names_the_rest(
    state_dir: Callable[[str], str], tmp_path: Path
) -> None:
    """Started and dead before its logging was set up: its own output is kept, the rest named.

    OctoPrint's log directory is made a file before the start, so the server
    process says why it cannot go on and exits before it writes either of
    OctoPrint's own logs — and a copy an earlier run left of one of those is
    removed rather than passed off as this run's.
    """
    state = state_dir("died")
    into = tmp_path / "kept"
    passing(script("install", "--state-dir", state), describing="provisioning the instance")
    logs = Path(state) / "instance" / "logs"
    shutil.rmtree(logs, ignore_errors=True)
    logs.write_text("not a directory\n", encoding="utf-8")
    (into / "logs").mkdir(parents=True)
    (into / "logs" / "serial.log").write_text("an earlier run's serial log\n", encoding="utf-8")
    failing(
        script("up", "--state-dir", state, "--start-timeout", "60"),
        naming="the server exited before it answered",
    )

    record = _retained(state, into)

    equal(record["retained"], ["server.log"], describing="the logs kept")
    equal(record["absent"], REQUIRED[1:], describing="the logs reported absent")
    said = (Path(state) / "server.log").read_text(encoding="utf-8")
    truth(bool(said), describing="the server to have said something before it died")
    equal(
        (into / "server.log").read_text(encoding="utf-8"),
        said,
        describing="the kept server.log, against what the server wrote",
    )
    for name in REQUIRED[1:]:
        truth(not (into / name).exists(), describing=f"no kept {name}")
