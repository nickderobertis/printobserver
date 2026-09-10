"""A smoke run against a printer the test controls.

Nothing here stands in for the thing under test. `printer_smoke.py` runs as a
subprocess exactly as its recipe runs it, and it drives the `printobserver`
program this workspace builds; what is controlled is the far side of that
command — the supervisor and the machine — which is `machine.Machine`.

The serial device is one the test creates: a pseudo-terminal, which is a real
character device this user can open, so the device precondition is met by a
device rather than by a fixture pretending to be one.
"""

from __future__ import annotations

import json
import os
import subprocess
import sys
from dataclasses import dataclass
from pathlib import Path
from typing import Any

from machine import Machine
from printer_smoke import CONSERVATIVE_ENVELOPE, FILE_NAME, address_of
from repo_checks.shell import run

REPO_ROOT = Path(__file__).resolve().parents[3]
SMOKE = REPO_ROOT / "tools" / "printer-smoke" / "printer_smoke.py"
PROGRAM = REPO_ROOT / "target" / "debug" / "printobserver"

#: What the smoke asks a machine for, and how long this harness lets it wait.
#: A socket answers at once, so a run against one needs no minute of patience.
SETTLE_S = "4"
DURATION_S = "1"

#: The manifest the print carries, which is one for the smoke's own payload.
MANIFEST: dict[str, Any] = {
    "file_name": FILE_NAME,
    "material": "PLA",
    "nozzle_diameter_mm": 0.4,
    "slicer_profile": "the profile the smoke's own payload was sliced with",
    "allowed": {},
    "metadata": {},
}

CREDENTIAL = "a-credential-this-harness-configures"


def build_the_program() -> Path:
    """Build the command the smoke drives, and answer where it is.

    Returns:
        The built `printobserver` binary.

    Raises:
        RuntimeError: If it could not be built, with everything cargo said.
    """
    built = run(["cargo", "build", "-p", "printobserver", "--locked"], cwd=REPO_ROOT, timeout=1800)
    if built.returncode != 0 or not PROGRAM.is_file():
        message = f"the smoke drives a program this workspace could not build:\n{built.stderr}"
        raise RuntimeError(message)
    return PROGRAM


@dataclass
class World:
    """One machine the test controls, and the smoke run pointed at it."""

    root: Path
    substitute: Machine
    device: str
    config: Path
    print_id: str
    program: Path
    state_dir: Path

    def environment(self, extra: dict[str, str] | None = None) -> dict[str, str]:
        """The environment a smoke run is started under.

        Args:
            extra: What this run changes about it, including a variable set to
                the empty string to take one away.

        Returns:
            The environment, with everything the smoke reads in it.
        """
        environment = dict(os.environ)
        environment.update(
            {
                "PRINTOBSERVER_SMOKE_DEVICE": self.device,
                "PRINTOBSERVER_SMOKE_CONFIG": str(self.config),
                "PRINTOBSERVER_SMOKE_PRINT_ID": self.print_id,
                "PRINTOBSERVER_SMOKE_PROGRAM": str(self.program),
                "PRINTOBSERVER_SMOKE_SETTLE_S": SETTLE_S,
                "PRINTOBSERVER_SMOKE_DURATION_S": DURATION_S,
                "OCTOPRINT_ENV_STATE_DIR": str(self.state_dir),
                "PRINTOBSERVER_SERVER": self.substitute.url,
                "PRINTOBSERVER_CREDENTIAL": CREDENTIAL,
                "PYTHONPATH": ":".join(
                    str(REPO_ROOT / part)
                    for part in ("tools/repo-checks/src", "tools/octoprint-env")
                ),
            }
        )
        for name, value in (extra or {}).items():
            if value:
                environment[name] = value
            else:
                environment.pop(name, None)
        return environment

    def smoke(
        self,
        *arguments: str,
        environment: dict[str, str] | None = None,
        timeout: float = 300,
    ) -> subprocess.CompletedProcess[str]:
        """Run the smoke the way its recipe runs it, and hand back what it said.

        Args:
            *arguments: What the recipe passed on, usually `--run`.
            environment: What this run changes about the environment.
            timeout: Seconds to let it take.

        Returns:
            The completed run.
        """
        return run(
            [sys.executable, str(SMOKE), *arguments],
            cwd=REPO_ROOT,
            env=self.environment(environment),
            timeout=timeout,
        )

    def interrupt_the_smoke(
        self, *, after: float = 8.0, duration_s: str = "30"
    ) -> subprocess.CompletedProcess[str]:
        """Run the smoke and interrupt it part-way, as a person at the machine would.

        The interrupt is a real `SIGINT` from `timeout`, and the bounded
        intervention is given long enough that the run is waiting it out when
        the signal lands — so what is interrupted is a run that has already
        started a print and already moved the machine.

        Args:
            after: How long to let it run before interrupting it, in seconds.
            duration_s: How long its bounded intervention stands for.

        Returns:
            The completed run, including everything it said while cleaning up.
        """
        return run(
            ["timeout", "--signal=INT", str(after), sys.executable, str(SMOKE), "--run"],
            cwd=REPO_ROOT,
            env=self.environment({"PRINTOBSERVER_SMOKE_DURATION_S": duration_s}),
            timeout=300,
        )


def open_a_serial_device() -> tuple[str, int, int]:
    """A character device this user can open, and the two handles holding it there.

    Returns:
        The device's path, and the two file descriptors that must stay open.
    """
    controller, device = os.openpty()
    return os.ttyname(device), controller, device


def write_the_record(state_dir: Path, url: str, device: str, *, mode: str = "serial") -> None:
    """Write what a scripted OctoPrint bring-up records about itself.

    Args:
        state_dir: The directory `octoprint-env` keeps its state under.
        url: Where that instance answers.
        device: The serial device it connected to.
        mode: The mode it was brought up in.
    """
    state_dir.mkdir(parents=True, exist_ok=True)
    key_file = state_dir / "api-key"
    key_file.write_text("a-provisioned-key\n", encoding="utf-8")
    (state_dir / "instance.json").write_text(
        json.dumps(
            {
                "state_dir": str(state_dir),
                "url": url,
                "pid": os.getpid(),
                "mode": mode,
                "device": device,
                "api_key_file": str(key_file),
            },
            indent=2,
        ),
        encoding="utf-8",
    )


def write_the_configuration(
    path: Path,
    url: str,
    envelope: dict[str, tuple[float, float]] | None = None,
    *,
    octoprint_url: str | None = None,
    client_server: str | None = None,
) -> None:
    """Write the supervisor's own configuration file, which the smoke also reads.

    The server's own file rather than a client's: it says where the supervisor
    was told to listen and which OctoPrint that supervisor drives, which is the
    pair the smoke's binding precondition holds to the instance it verified.

    Args:
        path: Where to write it.
        url: Where the supervisor answers.
        envelope: The safety envelope it configures, or the conservative one.
        octoprint_url: The OctoPrint it says the supervisor drives, or `url`.
        client_server: A `[client] server` to write beside `listen`, when one
            is wanted — a configuration pointing a client somewhere else.
    """
    allowed = CONSERVATIVE_ENVELOPE if envelope is None else envelope
    lines = [
        f'listen = "{address_of(url)}"',
        "",
        "[octoprint]",
        f'url = "{url if octoprint_url is None else octoprint_url}"',
        "",
    ]
    if client_server is not None:
        lines.extend(["[client]", f'server = "{client_server}"', ""])
    lines.append("[safety.allowed]")
    lines.extend(
        f'"{name}" = {{ min = {low}, max = {high} }}' for name, (low, high) in allowed.items()
    )
    path.write_text("\n".join(lines) + "\n", encoding="utf-8")


def a_world(root: Path, *, device: str) -> World:
    """One world: a substitute, a device, a recorded environment and a configuration.

    Args:
        root: A directory of this test's own.
        device: The serial device the smoke is pointed at.

    Returns:
        The world, answering.
    """
    print_id = "01860d5a-4a8f-7c3d-9a4e-4d2f6b1c8e90"
    substitute = Machine(
        device=device, envelope=CONSERVATIVE_ENVELOPE, manifest=MANIFEST, print_id=print_id
    )
    state_dir = root / "octoprint-env"
    write_the_record(state_dir, substitute.url, device)
    config = root / "config.toml"
    write_the_configuration(config, substitute.url)
    return World(
        root=root,
        substitute=substitute,
        device=device,
        config=config,
        print_id=print_id,
        program=PROGRAM,
        state_dir=state_dir,
    )
