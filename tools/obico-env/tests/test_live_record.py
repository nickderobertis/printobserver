"""The recording command against the real self-hosted Obico: the recording's drift gate.

The server journeys' REST double serves a recording of Obico's printer answer.
This re-records it from the live stack — a snapshot posted through Obico's
printer API, a bearer minted through its OAuth2 models, the read made with that
bearer — and fails when the shape Obico now answers differs from the committed
recording's, naming every field that moved. A field that moved is a change to
the recording to make deliberately, by re-running the command into place.
"""

from __future__ import annotations

import json
import sys
from pathlib import Path

from harness import TIER_CHOOSES, TOOL, environment, said
from repo_checks.expect import absent, equal, truth
from repo_checks.shell import run as shell_run


def test_the_live_printer_answer_has_the_committed_recordings_shape(
    state_dir: str, tmp_path: Path
) -> None:
    """Recorded under the bearer, every credential the sentinel, and no field moved."""
    brought_up = environment("up", "--state-dir", state_dir, "--webhook-url", TIER_CHOOSES)
    truth(brought_up.returncode == 0, describing=f"the bring-up to succeed:\n{said(brought_up)}")
    printer_token = json.loads(brought_up.stdout)["printer_token"]

    out = tmp_path / "obico-printer.json"
    run = shell_run(
        [
            sys.executable,
            str(TOOL / "obico_record.py"),
            "--state-dir",
            state_dir,
            "--out",
            str(out),
        ],
        timeout=900,
    )
    truth(run.returncode in {0, 1}, describing=f"a recording rather than an error:\n{said(run)}")
    recorded = json.loads(run.stdout)
    equal(recorded["status"], 200, describing="Obico's answer under the bearer")
    absent(out.read_text(encoding="utf-8"), printer_token, describing="the recording")
    absent(said(run), printer_token, describing="what the command said")
    equal(recorded["differences"], [], describing="fields moved since the committed recording")
