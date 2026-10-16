"""The release's macOS service proof, run: install, load, read back, unload.

`release-plz.yml`'s macOS cell runs `just prove-built-service` before it
publishes, which installs the built program with the committed installer and
loads it with the launchd activation command `AGENTS.md` states. On macOS that
is the host's own launchd. Here it is the same command line — the artifact
tool's `prove-service` with the installer's own `--root` and `--user` — against
the launchd stand-in `test_service_manager_journey.py` drives on Linux:
`launchctl` on `PATH` as `launchctl_standin.py`, which starts the real program
the property list names, and `uname -s` answering `Darwin`, which is how the
installer and the proof both decide they are on a launchd host. Nothing is
installed on this host.
"""

from __future__ import annotations

import getpass
import json
import os
import shlex
import shutil
import subprocess
import sys
from pathlib import Path

import launchctl_standin
import pytest
from journey import DEBUG_BUILD, REPO_ROOT, clean_environment, pythonpath
from repo_checks.expect import contains, equal, passing, truth
from repo_checks.shell import run

pytestmark = [
    DEBUG_BUILD,
    pytest.mark.skipif(
        not sys.platform.startswith("linux"), reason="the launchd stand-in runs under /proc"
    ),
]

#: How long the one program build these share is given.
BUILD_TIMEOUT_SECONDS = 2400

LABEL = "io.github.nickderobertis.printobserver"
PLIST = f"/Library/LaunchDaemons/{LABEL}.plist"


@pytest.fixture(scope="module")
def program() -> Path:
    """The `printobserver` program built from this tree, which the proof installs."""
    passing(
        run(
            ["cargo", "build", "--locked", "-p", "printobserver"],
            cwd=REPO_ROOT,
            timeout=BUILD_TIMEOUT_SECONDS,
        ),
        describing="building the printobserver program",
    )
    return REPO_ROOT / "target" / "debug" / "printobserver"


def _machine(scratch: Path, *, refuse_print: bool = False) -> dict[str, str]:
    """A launchd machine on this host: shims on `PATH`, and the stand-in's root and state."""
    shims = scratch / "bin"
    shims.mkdir()
    packages = os.pathsep.join(str(REPO_ROOT / entry) for entry in pythonpath().split(os.pathsep))
    standin = Path(launchctl_standin.__file__).resolve()
    refusal = 'if [ "$1" = print ]; then echo "Could not find service" >&2; exit 113; fi\n'
    # llmlint: ignore[e2e_not_mocked] suppressions.toml has the reason.
    (shims / "launchctl").write_text(
        "#!/bin/sh\n" + (refusal if refuse_print else "") + f"PYTHONPATH={shlex.quote(packages)} "
        f'exec {shlex.quote(sys.executable)} {shlex.quote(str(standin))} "$@"\n',
        encoding="utf-8",
    )
    real_uname = shutil.which("uname")
    truth(real_uname is not None, describing="`uname` on this host's PATH")
    # llmlint: ignore[e2e_not_mocked] suppressions.toml has the reason.
    (shims / "uname").write_text(
        f'#!/bin/sh\n[ "$*" = "-s" ] && {{ echo Darwin; exit 0; }}\n'
        f'exec {shlex.quote(str(real_uname))} "$@"\n',
        encoding="utf-8",
    )
    for shim in shims.iterdir():
        shim.chmod(0o755)
    (scratch / "machine").mkdir()
    return clean_environment(
        PATH=f"{shims}{os.pathsep}{os.environ['PATH']}",
        PYTHONPATH=packages,
        **{
            launchctl_standin.ROOT: str(scratch / "machine"),
            launchctl_standin.STATE: str(scratch / "launchd"),
        },
    )


def _prove(
    program: Path, scratch: Path, environment: dict[str, str]
) -> subprocess.CompletedProcess[str]:
    return run(
        [
            "uv",
            "run",
            "-q",
            "python",
            "-m",
            "release_artifacts",
            "prove-service",
            "--binary",
            str(program),
            "--service-root",
            str(scratch / "machine"),
            "--service-user",
            getpass.getuser(),
        ],
        cwd=REPO_ROOT,
        env=environment,
        timeout=600,
    )


def _asked(scratch: Path) -> list[list[str]]:
    recording = scratch / "launchd" / launchctl_standin.RECORDING
    if not recording.is_file():
        return []
    return [json.loads(line) for line in recording.read_text(encoding="utf-8").splitlines()]


def test_the_built_program_is_installed_loaded_read_back_and_unloaded(
    program: Path, tmp_path: Path
) -> None:
    """The documented activation loads what the installer wrote, and the proof unloads it."""
    environment = _machine(tmp_path)

    proved = _prove(program, tmp_path, environment)

    passing(proved, describing="prove-service against the launchd stand-in")
    equal(len(proved.stdout.splitlines()), 1, describing="what a passing proof prints, in lines")
    contains(proved.stdout, f"prove-service: {LABEL}: installed", describing="its one line")
    contains(proved.stdout, f"loaded by `sudo launchctl bootstrap system {PLIST}`", describing="it")
    truth(
        (tmp_path / "machine" / PLIST.lstrip("/")).is_file(),
        describing="the property list the committed installer wrote",
    )
    equal(
        _asked(tmp_path),
        [
            ["bootstrap", "system", PLIST],
            ["print", f"system/{LABEL}"],
            ["bootout", f"system/{LABEL}"],
        ],
        describing="what the proof asked of launchd, in order",
    )


def test_a_service_launchd_cannot_print_fails_the_proof_and_is_still_unloaded(
    program: Path, tmp_path: Path
) -> None:
    """A load launchd does not report is a failed proof, and nothing is left loaded after it."""
    environment = _machine(tmp_path, refuse_print=True)

    proved = _prove(program, tmp_path, environment)

    equal(proved.returncode, 1, describing="prove-service's exit")
    contains(proved.stderr, f"`launchctl print system/{LABEL}` failed", describing="the refusal")
    equal(
        _asked(tmp_path),
        [["bootstrap", "system", PLIST], ["bootout", f"system/{LABEL}"]],
        describing="what reached the stand-in: the load, and the unload after the failure",
    )
