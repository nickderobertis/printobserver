"""The recording command, driven against a stand-in for Obico's REST API.

`obico_record.py` is run as a subprocess, exactly as a person re-records with
it; the stand-in serves the committed recording's own shape with a live-looking
credential put back where the sentinel is, and answers only the bearer Obico
would, so what is proven is that the command reads with that bearer and writes
nothing that carries a credential. `test_live_record.py` runs it against the
real stack.
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
from repo_checks.expect import absent, contains, equal, failing, passing, truth
from repo_checks.loopback import UNREACHABLE, LoopbackServer
from repo_checks.shell import run as shell_run

TOOL = Path(__file__).resolve().parents[1]
RECORD = TOOL / "obico_record.py"
RECORDING = TOOL.parents[1] / "crates/printobserver-server/tests/support/obico-printer.json"

BEARER = "minted-bearer-5d1c0f9e"
PRINTER_CREDENTIAL = "a1b2c3d4e5f60718293a"
SENTINEL = "SENTINEL-obico-printer-auth-token-0b6f3c"


class Obico:
    """Serves one printer's body at its path, to the bearer alone, as Obico does."""

    def __init__(self, answer: dict[str, Any]) -> None:
        """Serve `answer` as JSON until `body` is set to other bytes."""
        self.answer = answer
        self.body: bytes | None = None
        self.authorizations: list[str] = []
        self.snapshots: list[str] = []
        stand_in = self

        class Handler(BaseHTTPRequestHandler):
            def do_GET(self) -> None:
                stand_in.authorizations.append(self.headers.get("Authorization", ""))
                if self.path != "/api/v1/printers/1/":
                    self._answer(404, b"{}")
                elif self.headers.get("Authorization") != f"Bearer {BEARER}":
                    self._answer(403, b'{"detail": "not authenticated"}')
                elif stand_in.body is not None:
                    self._answer(200, stand_in.body)
                else:
                    self._answer(200, json.dumps(stand_in.answer).encode())

            def do_POST(self) -> None:
                """Take a snapshot the printer posts, as Obico's printer API does."""
                length = int(self.headers.get("Content-Length", "0"))
                self.rfile.read(length)
                stand_in.snapshots.append(self.headers.get("Authorization", ""))
                code = 200 if self.path == "/api/v1/octo/pic/" else 404
                self._answer(code, b"{}")

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


def run_record(*arguments: str, bearer: str | None = BEARER) -> subprocess.CompletedProcess[str]:
    """Run the command the way a person does, the bearer in its variable."""
    env = {key: value for key, value in os.environ.items() if key != "OBICO_RECORD_ACCESS_TOKEN"}
    if bearer is not None:
        env["OBICO_RECORD_ACCESS_TOKEN"] = bearer
    return shell_run([sys.executable, str(RECORD), *arguments], env=env, timeout=120)


def record(
    obico: Obico, out: Path, bearer: str | None = BEARER
) -> subprocess.CompletedProcess[str]:
    """Record printer 1 from the stand-in into `out`."""
    return run_record("--url", obico.url, "--printer-id", "1", "--out", str(out), bearer=bearer)


def test_it_records_the_answer_read_with_the_bearer_and_no_credential(
    obico: Obico, tmp_path: Path
) -> None:
    """Every credential is the sentinel, every other field as served, with provenance beside."""
    out = tmp_path / "not-yet-made" / "obico-printer.json"
    run = record(obico, out)
    passing(run, describing="recording the committed shape against the stand-in")
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
    equal(json.loads(run.stdout)["verdict"], "agrees", describing="the committed shape compared")


def test_credentials_are_found_inside_lists_and_under_every_credential_key(
    obico: Obico, tmp_path: Path
) -> None:
    """A token in an array, or under another credential's key, is the sentinel; a null stays."""
    obico.answer["settings"]["temp_profiles"] = [{"token": "list-held-secret"}, {"token": None}]
    obico.answer["api_key"] = "an-api-key"
    out = tmp_path / "obico-printer.json"
    run = record(obico, out)
    written = out.read_text(encoding="utf-8")
    for credential in ("list-held-secret", "an-api-key"):
        absent(written, credential, describing="the recording")
    replaced = json.loads(run.stdout)["credentials_replaced"]
    contains(replaced, "settings.temp_profiles[0].token", describing="where credentials were")
    contains(replaced, "api_key", describing="where credentials were")
    equal(
        json.loads(written)["settings"]["temp_profiles"][1],
        {"token": None},
        describing="a credential Obico left empty",
    )


def test_a_shape_that_moved_is_recorded_and_named(obico: Obico, tmp_path: Path) -> None:
    """Exit 1, the recording written, and every field that moved named."""
    obico.answer["a_field_obico_added"] = 3
    del obico.answer["settings"]
    out = tmp_path / "obico-printer.json"
    run = record(obico, out)
    equal(run.returncode, 1, describing=f"the exit for a moved shape:\n{run.stderr}")
    truth(out.is_file(), describing="the recording written anyway")
    moved = "\n".join(json.loads(run.stdout)["differences"])
    contains(moved, "unexpected field `a_field_obico_added`", describing="what moved")
    contains(moved, "missing field `settings`", describing="what moved")


@pytest.mark.parametrize(
    ("change", "naming"),
    [
        ({"id": 2}, "no id 1"),
        ({"id": True}, "no id 1"),
        ({"pic": None}, "no pic.img_url"),
        ({"pic": {"img_url": ""}}, "no pic.img_url"),
        ({"current_print": None}, "no running current_print"),
        ({"current_print": {}}, "no running current_print"),
        ({"current_print": {"id": 1, "cancelled_at": "2026-11-19T12:00:00Z"}}, "running"),
    ],
)
def test_an_answer_the_status_could_not_read_is_not_recorded(
    obico: Obico, tmp_path: Path, change: dict[str, object], naming: str
) -> None:
    """Another printer, an id that is no integer, no snapshot or no print: nothing written."""
    obico.answer.update(change)
    out = tmp_path / "obico-printer.json"
    failing(record(obico, out), naming=naming)
    truth(not out.exists(), describing="no recording")


@pytest.mark.parametrize(("body", "naming"), [(b"<html>", "is not JSON"), (b"[]", "not a JSON")])
def test_a_body_that_is_no_answer_is_not_recorded(
    obico: Obico, tmp_path: Path, body: bytes, naming: str
) -> None:
    """A body that is not one JSON object is refused, nothing written."""
    obico.body = body
    out = tmp_path / "obico-printer.json"
    failing(record(obico, out), naming=naming)
    truth(not out.exists(), describing="no recording")


def test_a_recording_that_cannot_be_written_is_an_error_rather_than_a_verdict(
    obico: Obico, tmp_path: Path
) -> None:
    """An output path under a file exits 2 naming it, never 1, the moved-shape verdict."""
    (tmp_path / "a-file").write_text("", encoding="utf-8")
    out = tmp_path / "a-file" / "obico-printer.json"
    run = record(obico, out)
    equal(run.returncode, 2, describing=f"the exit:\n{run.stderr}")
    contains(run.stderr, "could not be written", describing="the refusal")
    absent(run.stdout + run.stderr, BEARER, describing="what it said")


def test_a_provenance_that_cannot_be_written_leaves_the_recording_as_it_was(
    obico: Obico, tmp_path: Path
) -> None:
    """The recording is never replaced without its provenance, and nothing half-written stays."""
    out = tmp_path / "obico-printer.json"
    out.write_text("the recording before\n", encoding="utf-8")
    (tmp_path / "obico-printer.provenance.json").mkdir()
    run = record(obico, out)
    equal(run.returncode, 2, describing=f"the exit:\n{run.stderr}")
    contains(run.stderr, "could not be written", describing="the refusal")
    equal(out.read_text(encoding="utf-8"), "the recording before\n", describing="the recording")
    equal(
        sorted(path.name for path in tmp_path.iterdir()),
        ["obico-printer.json", "obico-printer.provenance.json"],
        describing="what is left beside it",
    )


@pytest.mark.parametrize("baseline", ["{ cut short", None])
def test_a_baseline_that_cannot_be_read_records_nothing(
    obico: Obico, tmp_path: Path, baseline: str | None
) -> None:
    """A recording to compare with that is malformed or absent is refused before any write."""
    compare = tmp_path / "baseline.json"
    if baseline is not None:
        compare.write_text(baseline, encoding="utf-8")
    out = tmp_path / "obico-printer.json"
    run = run_record(
        "--url", obico.url, "--printer-id", "1", "--out", str(out), "--compare-with", str(compare)
    )
    equal(run.returncode, 2, describing=f"the exit:\n{run.stderr}")
    contains(run.stderr, "could not be read", describing="the refusal")
    truth(not out.exists(), describing="no recording")


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


@pytest.mark.parametrize(
    ("arguments", "naming"),
    [
        ((), "pass `--state-dir`"),
        (("--url", "ftp://127.0.0.1:1", "--printer-id", "1"), "no http(s) address"),
        (("--url", "http://127.0.0.1:1", "--printer-id", "-3"), "not a positive integer"),
        (("--url", f"http://{UNREACHABLE[0]}:{UNREACHABLE[1]}", "--printer-id", "1"), "could not"),
    ],
)
def test_an_obico_it_cannot_read_is_refused_naming_why(
    tmp_path: Path, arguments: tuple[str, ...], naming: str
) -> None:
    """No address, a bad one, a bad printer or nothing listening: exit 2, nothing written."""
    out = tmp_path / "obico-printer.json"
    run = run_record(*arguments, "--out", str(out))
    equal(run.returncode, 2, describing=f"the exit:\n{run.stderr}")
    contains(run.stderr, naming, describing="the refusal")
    truth(not out.exists(), describing="no recording")


def test_a_credential_sanitizing_cannot_reach_is_refused_and_nothing_written(
    obico: Obico, tmp_path: Path
) -> None:
    """A credential where no value is — as a field's name — stops the run rather than leaking."""
    obico.answer[BEARER] = 1
    out = tmp_path / "obico-printer.json"
    run = record(obico, out)
    failing(run, naming="a credential survived")
    absent(run.stdout + run.stderr, BEARER, describing="what it said")
    truth(not out.exists(), describing="no recording")


def stack_record(state_dir: Path, url: str, **more: object) -> None:
    """The record `obico_env.py up` writes, for a stack under `state_dir` at `url`."""
    record = {
        "state_dir": str(state_dir),
        "project": "printobserver-record-absent",
        "url": url,
        "printer_token": PRINTER_CREDENTIAL,
        **more,
    }
    (state_dir / "instance.json").write_text(json.dumps(record), encoding="utf-8")


def test_a_stack_that_refuses_the_snapshot_is_refused_naming_it(tmp_path: Path) -> None:
    """The tier's snapshot refusal comes out as the recorder's, with no credential in it."""
    stack_record(tmp_path, f"http://{UNREACHABLE[0]}:{UNREACHABLE[1]}", printer_id=1)
    out = tmp_path / "obico-printer.json"
    run = run_record("--state-dir", str(tmp_path), "--out", str(out))
    equal(run.returncode, 2, describing=f"the exit:\n{run.stderr}")
    contains(run.stderr, "refused the printer's snapshot", describing="the refusal")
    absent(run.stdout + run.stderr, PRINTER_CREDENTIAL, describing="what it said")
    truth(not out.exists(), describing="no recording")


def test_a_stack_that_mints_no_bearer_is_refused_and_reads_nothing(
    obico: Obico, tmp_path: Path
) -> None:
    """The snapshot is posted with the printer's token; no bearer minted, no read made."""
    stack_record(tmp_path, obico.url, printer_id=1)
    out = tmp_path / "obico-printer.json"
    run = run_record("--state-dir", str(tmp_path), "--out", str(out), bearer=None)
    equal(run.returncode, 2, describing=f"the exit:\n{run.stderr}")
    equal(obico.snapshots, [f"Token {PRINTER_CREDENTIAL}"], describing="the snapshot posted")
    contains(run.stderr, "did not report a minted access token", describing="the refusal")
    equal(obico.authorizations, [], describing="reads made with no bearer")
    absent(run.stdout + run.stderr, PRINTER_CREDENTIAL, describing="what it said")
    truth(not out.exists(), describing="no recording")


def test_a_stack_record_naming_no_printer_is_refused_before_anything_is_posted(
    tmp_path: Path,
) -> None:
    """A state directory whose record names no printer stops before the stack is touched."""
    stack_record(tmp_path, "http://127.0.0.1:1", printer_id=0)
    out = tmp_path / "obico-printer.json"
    run = run_record("--state-dir", str(tmp_path), "--out", str(out))
    equal(run.returncode, 2, describing=f"the exit:\n{run.stderr}")
    contains(run.stderr, "names no printer", describing="the refusal")
    absent(run.stderr, PRINTER_CREDENTIAL, describing="the refusal")
