"""What a journey drives the self-hosted Obico environment and its tier with.

Nothing here mocks the layer under test. `obico_env.py` and `obico_tier.py` are
run as subprocesses, exactly as `just obico-up` and `just test-obico` run them,
and every observation is made over Obico's own HTTP API with a client of this
suite's own — so a journey never proves the environment right by asking it.
"""

from __future__ import annotations

import json
import socket
import subprocess
import sys
import time
import urllib.error
import urllib.request
from collections.abc import Callable
from pathlib import Path
from typing import Any

from obico_env import run_program
from repo_checks.shell import run as shell_run

TOOL = Path(__file__).resolve().parents[1]
ENVIRONMENT = TOOL / "obico_env.py"
TIER = TOOL / "obico_tier.py"
SNAPSHOT = TOOL / "snapshot" / "spaghetti.jpg"

# The sample the `contracts` node committed, which the tier reconciles against.
SAMPLE = (
    TOOL.parents[1] / "crates" / "printobserver-types" / "samples" / "obico" / "failure-alert.json"
)


def environment(*arguments: str, timeout: int = 5400) -> subprocess.CompletedProcess[str]:
    """Run the real environment script, the way a recipe does."""
    return shell_run([sys.executable, str(ENVIRONMENT), *arguments], timeout=timeout)


def tier(*arguments: str, timeout: int = 900) -> subprocess.CompletedProcess[str]:
    """Run the real tier, the way `just test-obico` runs it."""
    return shell_run([sys.executable, str(TIER), *arguments], timeout=timeout)


def timed(
    call: Callable[..., subprocess.CompletedProcess[str]], *arguments: str
) -> tuple[float, subprocess.CompletedProcess[str]]:
    """Run something and hand back how long it took beside what it answered."""
    started = time.monotonic()
    answered = call(*arguments)
    return time.monotonic() - started, answered


def answer(result: subprocess.CompletedProcess[str]) -> dict[str, Any]:
    """The JSON document a run answers with on stdout.

    Raises:
        AssertionError: If stdout is not one JSON object.
    """
    parsed = json.loads(result.stdout)
    if not isinstance(parsed, dict):
        message = f"expected a JSON object on stdout; got {result.stdout!r}"
        raise AssertionError(message)
    return parsed


def said(result: subprocess.CompletedProcess[str]) -> str:
    """Everything a run said, on either stream."""
    return result.stdout + result.stderr


def free_port() -> int:
    """A port nothing is listening on, so two journeys do not collide."""
    with socket.socket(socket.AF_INET, socket.SOCK_STREAM) as probe:
        probe.bind(("127.0.0.1", 0))
        return int(probe.getsockname()[1])


def get(
    url: str,
    *,
    timeout: float = 30.0,
    token: str | None = None,
    printer_token: str | None = None,
) -> tuple[int, Any]:
    """One GET against the running stack, with this suite's own client."""
    request = urllib.request.Request(url)  # noqa: S310
    if token:
        request.add_header("Authorization", f"Bearer {token}")
    if printer_token:
        request.add_header("Authorization", f"Token {printer_token}")
    try:
        with urllib.request.urlopen(request, timeout=timeout) as response:  # noqa: S310
            body = response.read()
            try:
                return response.status, json.loads(body.decode("utf-8"))
            except UnicodeDecodeError, json.JSONDecodeError:
                return response.status, body
    except urllib.error.HTTPError as error:
        return error.code, error.read()


def containers(project: str) -> list[str]:
    """Every running container of a compose project, by name, asked of Docker itself.

    Asked of `docker ps` rather than of the environment script: a journey that
    asked the script whether it had stopped what it started would be asking the
    thing under test.
    """
    listed = shell_run(
        [
            "docker",
            "ps",
            "--filter",
            f"label=com.docker.compose.project={project}",
            "--format",
            "{{.Names}}",
        ],
        timeout=300,
    )
    return [line.strip() for line in listed.stdout.splitlines() if line.strip()]


def printer_over_the_api(url: str, printer_token: str) -> dict[str, Any]:
    """What Obico's own printer API says about the printer a token belongs to.

    Raises:
        AssertionError: If the stack does not answer for that token.
    """
    status, body = get(f"{url}/api/v1/octo/printer/", token=None, printer_token=printer_token)
    if status != 200 or not isinstance(body, dict):
        message = (
            f"expected Obico to answer for the printer's token; it answered {status}: {body!r}"
        )
        raise AssertionError(message)
    return body


def ask_the_stack(state_dir: str, snippet: str) -> str:
    """Run one query of this suite's own against the stack's own Django models.

    Reading Obico's own database through Obico's own ORM, with a query this
    suite wrote — rather than asking the environment script whether it
    provisioned, which would be asking the thing under test.

    Raises:
        AssertionError: If the stack will not run it.
    """
    record = json.loads((Path(state_dir) / "instance.json").read_text(encoding="utf-8"))
    answered = run_program(
        [
            "docker",
            "compose",
            "-p",
            str(record["project"]),
            "-f",
            str(record["compose_file"]),
            "-f",
            str(record["override_file"]),
            "exec",
            "-T",
            "web",
            "python",
            "manage.py",
            "shell",
        ],
        timeout=900,
        stdin=snippet,
    )
    if answered.returncode != 0:
        message = (
            f"expected the stack to answer a query; it exited "
            f"{answered.returncode}:\n{answered.stdout}"
        )
        raise AssertionError(message)
    return answered.stdout
