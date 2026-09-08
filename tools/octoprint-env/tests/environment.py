"""What a journey drives the scripted environment with.

Nothing here mocks the layer under test. `octoprint_env.py` is run as a
subprocess, exactly as `just octoprint-up` runs it, and every observation is
made over OctoPrint's own HTTP API with a client of this suite's own — so a
journey never proves the script right by asking the script.
"""

from __future__ import annotations

import http.client
import json
import os
import subprocess
import sys
from pathlib import Path
from typing import Any

from repo_checks.shell import run as shell_run

SCRIPT = Path(__file__).resolve().parents[1] / "octoprint_env.py"


def script(*arguments: str, timeout: int = 900) -> subprocess.CompletedProcess[str]:
    """Run the real script, the way a recipe does."""
    return shell_run([sys.executable, str(SCRIPT), *arguments], timeout=timeout)


def answer(result: subprocess.CompletedProcess[str]) -> dict[str, Any]:
    """The JSON document the script answers with on stdout."""
    parsed = json.loads(result.stdout)
    if not isinstance(parsed, dict):
        message = f"expected a JSON object on stdout; got {result.stdout!r}"
        raise AssertionError(message)
    return parsed


def said(result: subprocess.CompletedProcess[str]) -> str:
    """Everything a run said, on either stream."""
    return result.stdout + result.stderr


def api(
    url: str, path: str, key: str | None, *, timeout: float = 15.0
) -> tuple[int, dict[str, Any] | str]:
    """One GET against an instance, with or without an API key."""
    _, _, host_port = url.partition("//")
    host, _, port = host_port.partition(":")
    connection = http.client.HTTPConnection(host, int(port), timeout=timeout)
    try:
        connection.request("GET", path, headers={"X-Api-Key": key} if key else {})
        response = connection.getresponse()
        body = response.read().decode("utf-8", errors="replace")
        try:
            return response.status, json.loads(body)
        except json.JSONDecodeError:
            return response.status, body
    finally:
        connection.close()


def state_of(url: str, path: str, key: str, field: str) -> str:
    """One string field of one API answer, or `unknown` when it is not there."""
    status, body = api(url, path, key)
    if status != 200 or not isinstance(body, dict):
        return "unknown"
    value = body.get(field, "unknown")
    return str(value) if not isinstance(value, dict) else str(value.get("state", "unknown"))


def printer_state(url: str, key: str) -> str:
    """What the instance says about the printer it is connected to."""
    status, body = api(url, "/api/connection", key)
    if status != 200 or not isinstance(body, dict):
        return "unknown"
    current = body.get("current")
    return str(current.get("state", "unknown")) if isinstance(current, dict) else "unknown"


def job_state(url: str, key: str) -> str:
    """What the instance says about the print it is running."""
    return state_of(url, "/api/job", key, "state")


def key_from(api_key_file: str) -> str:
    """The API key the script wrote, read from the path it named."""
    return Path(api_key_file).read_text(encoding="utf-8").strip()


def running(pid: int) -> bool:
    """Whether a process a journey started is still there."""
    try:
        os.kill(pid, 0)
    except OSError:
        return False
    return True
