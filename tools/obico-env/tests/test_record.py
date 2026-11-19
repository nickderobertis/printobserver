"""The recording command, driven against a stand-in for Obico's REST API.

`obico_record.py` is run as a subprocess, exactly as a person re-records with
it; the stand-in serves the committed recording's own shape with a live-looking
credential put back where the sentinel is, and answers only the bearer Obico
would, so what is proven is that the command reads with that bearer and writes
nothing that carries a credential. The run against the real stack is the one
`tools/obico-env` brings up, and is the comparison the recording's provenance
names rather than something every test pays for.
"""

from __future__ import annotations

import json
import os
import subprocess
import sys
import threading
from collections.abc import Iterator
from http.server import BaseHTTPRequestHandler
from pathlib import Path
from typing import Any

import pytest
from repo_checks.expect import absent, equal, failing, passing, truth
from repo_checks.loopback import LoopbackServer
from repo_checks.shell import run as shell_run

TOOL = Path(__file__).resolve().parents[1]
RECORD = TOOL / "obico_record.py"
RECORDING = TOOL.parents[1] / "crates/printobserver-server/tests/support/obico-printer.json"

BEARER = "minted-bearer-5d1c0f9e"
PRINTER_CREDENTIAL = "a1b2c3d4e5f60718293a"
SENTINEL = "SENTINEL-obico-printer-auth-token-0b6f3c"


class Obico:
    """Serves one printer's answer at its path, to the bearer alone, as Obico does."""

    def __init__(self, answer: dict[str, Any]) -> None:
        """Take the answer it serves."""
        self.answer = answer
        self.authorizations: list[str] = []
        stand_in = self

        class Handler(BaseHTTPRequestHandler):
            def do_GET(self) -> None:
                stand_in.authorizations.append(self.headers.get("Authorization", ""))
                if self.path != "/api/v1/printers/1/":
                    self._answer(404, b"{}")
                elif self.headers.get("Authorization") != f"Bearer {BEARER}":
                    self._answer(403, b'{"detail": "not authenticated"}')
                else:
                    self._answer(200, json.dumps(stand_in.answer).encode())

            def _answer(self, status: int, body: bytes) -> None:
                self.send_response(status)
                self.send_header("Content-Type", "application/json")
                self.send_header("Content-Length", str(len(body)))
                self.end_headers()
                self.wfile.write(body)

            def log_message(self, format: str, *args: object) -> None:
                """Say nothing."""

        self.server = LoopbackServer(Handler)
        self.url = f"http://127.0.0.1:{self.server.server_address[1]}"


@pytest.fixture
def obico() -> Iterator[Obico]:
    """The committed recording's shape, carrying live credentials again."""
    answer = json.loads(RECORDING.read_text(encoding="utf-8").replace(SENTINEL, PRINTER_CREDENTIAL))
    # A credential the command knows of, somewhere no credential key names it.
    answer["pic"]["img_url"] += f"&leaked={BEARER}"
    stand_in = Obico(answer)
    thread = threading.Thread(target=stand_in.server.serve_forever, daemon=True)
    thread.start()
    yield stand_in
    stand_in.server.shutdown()
    stand_in.server.server_close()
    thread.join(timeout=10)


def record(
    obico: Obico, out: Path, bearer: str | None = BEARER
) -> subprocess.CompletedProcess[str]:
    """Run the command the way a person does, the bearer in its variable."""
    env = {key: value for key, value in os.environ.items() if key != "OBICO_RECORD_ACCESS_TOKEN"}
    if bearer is not None:
        env["OBICO_RECORD_ACCESS_TOKEN"] = bearer
    return shell_run(
        [sys.executable, str(RECORD), "--url", obico.url, "--printer-id", "1", "--out", str(out)],
        env=env,
        timeout=120,
    )


def test_it_records_the_answer_read_with_the_bearer_and_no_credential(
    obico: Obico, tmp_path: Path
) -> None:
    """Every credential is the sentinel, every other field as served, with provenance beside."""
    out = tmp_path / "obico-printer.json"
    run = record(obico, out)
    passing(run, describing="recording against the stand-in")
    equal(obico.authorizations, [f"Bearer {BEARER}"], describing="what the command sent")

    written = out.read_text(encoding="utf-8")
    provenance_text = out.with_suffix(".provenance.json").read_text(encoding="utf-8")
    for credential in (BEARER, PRINTER_CREDENTIAL):
        for where, text in (
            ("the recording", written),
            ("its provenance", provenance_text),
            ("what it said", run.stdout + run.stderr),
        ):
            absent(text, credential, describing=where)

    recorded = json.loads(written)
    equal(recorded["auth_token"], SENTINEL, describing="the printer's credential")
    equal(recorded["current_print"]["printer"]["auth_token"], SENTINEL, describing="its copy")
    served = obico.answer
    for field in ("id", "normalized_p", "status", "settings", "name"):
        equal(recorded[field], served[field], describing=f"`{field}` as served")
    equal(recorded["current_print"]["id"], served["current_print"]["id"], describing="the print")
    truth(recorded["pic"]["img_url"].endswith(f"&leaked={SENTINEL}"), describing="the URL scrubbed")

    provenance = json.loads(provenance_text)
    equal(provenance["request"], "GET /api/v1/printers/1/", describing="the request")
    equal(provenance["status"], 200, describing="the status")
    equal(
        provenance["authentication"],
        "Authorization: Bearer <[obico] access_token>",
        describing="the scheme",
    )
    equal(
        sorted(provenance["credentials_replaced"]),
        ["auth_token", "current_print.printer.auth_token", "pic.img_url"],
        describing="where credentials were",
    )


def test_a_refused_bearer_records_nothing(obico: Obico, tmp_path: Path) -> None:
    """Obico's 403 is reported, and nothing is written."""
    out = tmp_path / "obico-printer.json"
    failing(record(obico, out, bearer="some-other-token"), naming="403")
    truth(not out.exists(), describing="no recording after a refusal")


def test_no_bearer_reads_nothing(obico: Obico, tmp_path: Path) -> None:
    """With no bearer to send, nothing is asked of Obico."""
    out = tmp_path / "obico-printer.json"
    failing(record(obico, out, bearer=None), naming="OBICO_RECORD_ACCESS_TOKEN")
    equal(obico.authorizations, [], describing="what reached Obico")


def test_an_answer_with_no_snapshot_is_not_recorded(obico: Obico, tmp_path: Path) -> None:
    """The double needs a snapshot to move; an answer without one is refused."""
    obico.answer["pic"] = None
    out = tmp_path / "obico-printer.json"
    failing(record(obico, out), naming="pic")
    truth(not out.exists(), describing="no recording without a snapshot")
