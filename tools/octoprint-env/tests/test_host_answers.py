"""What the scripted environment answers on each host it is run on.

Two of the script's answers depend on the host: where a virtual environment
keeps its programs, and how a serial device is opened. Each journey here runs
the script's own entry point with the host's answer to `sys.platform` fixed —
Windows', and this suite's own for contrast — over a state directory laid out
the way that host lays one out, and reads back what the script said. So both
answers are proven on whichever host the suite runs, rather than only on the one
the tier happens to be on.

The last journey is the one no fixture can reach: stopping a real process, which
on Windows goes through the process table and `taskkill` rather than signals. It
runs against the host it is on, so each platform's cell proves its own answer.
"""

from __future__ import annotations

import json
import sys
from pathlib import Path

import octoprint_env
import pytest
import yaml
from environment import running
from repo_checks.expect import contains, equal, truth
from repo_checks.shell import run as shell_run


def _provisioned_over(state: Path, program: Path) -> None:
    """A state directory an earlier install left, its OctoPrint at `program`.

    The account and its key are there too, so an install over it has nothing
    left to do but say so — which is what makes its answer readable.
    """
    (state / "venv" / program).parent.mkdir(parents=True)
    (state / "venv" / program).write_text("an installed program\n", encoding="utf-8")
    (state / "instance").mkdir(parents=True)
    (state / "instance" / "users.yaml").write_text(
        yaml.safe_dump({octoprint_env.ACCOUNT: {"apikey": "a-provisioned-key"}}),
        encoding="utf-8",
    )


@pytest.mark.parametrize(
    ("platform", "program"),
    [
        pytest.param("win32", Path("Scripts") / "octoprint.exe", id="windows"),
        pytest.param("linux", Path("bin") / "octoprint", id="linux"),
    ],
)
def test_an_installed_instance_is_found_where_the_host_keeps_its_programs(
    tmp_path: Path,
    monkeypatch: pytest.MonkeyPatch,
    capsys: pytest.CaptureFixture[str],
    platform: str,
    program: Path,
) -> None:
    """Windows keeps an environment's programs under `Scripts`, with `.exe`.

    Looked for under `bin` there, an installed OctoPrint is never found: every
    run installs it again, and nothing is ever started from where it is.
    """
    state = tmp_path / "state"
    _provisioned_over(state, program)
    monkeypatch.setattr(sys, "platform", platform)

    code = octoprint_env.main(["install", "--state-dir", str(state)])

    said = capsys.readouterr()
    equal(code, 0, describing=f"installing over an instance already there: {said.err}")
    contains(
        said.err,
        f"is already installed in {state.resolve() / 'venv'}",
        describing=f"what the install said on a `{platform}` host",
    )
    equal(
        json.loads(said.out)["api_key_file"],
        str(state.resolve() / "api-key"),
        describing="the key file the install named",
    )


def test_a_windows_serial_device_is_opened_through_the_device_namespace(
    tmp_path: Path,
    monkeypatch: pytest.MonkeyPatch,
    capsys: pytest.CaptureFixture[str],
) -> None:
    r"""`COM250` is opened as `\\.\COM250`, the only name every COM port opens under."""
    monkeypatch.setattr(sys, "platform", "win32")

    code = octoprint_env.main(
        ["up", "--state-dir", str(tmp_path / "state"), "--mode", "serial", "--device", "COM250"]
    )

    said = capsys.readouterr().err
    equal(code, 1, describing=f"starting against a port nothing is plugged into: {said}")
    contains(said, "serial-device-unopenable", describing="the failure class reported")
    contains(
        said,
        repr("\\\\.\\COM250")[1:-1],
        describing="the device-namespace path the open was refused on",
    )
    truth(
        not (tmp_path / "state").exists(),
        describing="nothing provisioned for a device that cannot be opened",
    )


# A process that outlives the one that started it, as the server `up` starts
# does: started in a group of its own by a launcher that exits at once, so what
# is left is nobody's child and a stopped one is gone rather than unreaped.
LAUNCHER = """
import subprocess, sys
detached = (
    {"creationflags": subprocess.CREATE_NEW_PROCESS_GROUP | subprocess.DETACHED_PROCESS}
    if sys.platform == "win32"
    else {"start_new_session": True}
)
child = subprocess.Popen(
    [sys.executable, "-c", "import time; time.sleep(300)"],
    stdin=subprocess.DEVNULL, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL, **detached
)
print(child.pid)
"""


def test_stopping_a_recorded_instance_leaves_no_process_behind(tmp_path: Path) -> None:
    """`down` over a running process ends it, by this host's own means."""
    state = tmp_path / "state"
    state.mkdir()
    launched = shell_run([sys.executable, "-c", LAUNCHER], timeout=60)
    pid = int((launched.stdout or "").strip())
    (state / "instance.json").write_text(json.dumps({"pid": pid}), encoding="utf-8")
    truth(running(pid), describing=f"process {pid} before it is stopped")

    code = octoprint_env.main(["down", "--state-dir", str(state)])

    equal(code, 0, describing="stopping the recorded instance")
    truth(not running(pid), describing=f"process {pid} to be gone once the instance is stopped")
    truth(
        not (state / "instance.json").exists(),
        describing="the record of a stopped instance to be gone",
    )
