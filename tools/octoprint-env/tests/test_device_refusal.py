"""A serial device this host cannot open is refused before anything is provisioned.

Three names, one in each platform's own shape — `COM3`, `/dev/cu.usbmodem1101`,
`/dev/ttyACM0` — and two journeys over them. The first runs the committed
script in `--mode serial` on whichever host this suite is on, so each gate cell
proves its own platform's naming and its own way of opening a device: the name
that host's serial devices have is opened, by that host's means, and refused as
unopenable because nothing is plugged in there; a name that host cannot have is
refused by naming what its devices are called instead. The second fixes the
script's answer to `sys.platform` to each of the three in turn and runs its own
entry point, so every platform's naming — and, where this host can perform it,
every platform's way of opening a device — is proven on every host rather than
only on the one the suite happens to be on.

Both refusals come before anything is provisioned or started: a device that
cannot be opened is no reason to install OctoPrint first, and every journey
here asserts the state directory was never made.

This module is the one in this suite that needs no OctoPrint, so it is the
project's `test` target and runs in every gate cell; the rest of the suite is
the integration tier's.
"""

from __future__ import annotations

import sys
from collections.abc import Callable, Iterator
from pathlib import Path

import octoprint_env
import pytest
from environment import said, script
from repo_checks.expect import failing, refused_naming, truth

#: One name in each platform's own shape, by what `sys.platform` answers there.
NAMES: dict[str, str] = {
    "linux": "/dev/ttyACM0",
    "darwin": "/dev/cu.usbmodem1101",
    "win32": "COM3",
}

#: What each platform's refusals have to say, in this suite's own words rather
#: than the script's: the name it calls itself, and something a person there
#: would recognise in the next action.
WORDS: dict[str, tuple[str, str]] = {
    "linux": ("Linux", "dialout"),
    "darwin": ("macOS", "ls /dev/cu.*"),
    "win32": ("Windows", "Ports (COM & LPT)"),
}

#: Whether a Windows name — a bare `COM` port — is one a platform can have. A
#: Unix names its devices under `/dev`, and a `udev` rule may link a printer
#: under any name there, so a Unix refuses a `COM` port and nothing else.
IS_WINDOWS: dict[str, bool] = {"linux": False, "darwin": False, "win32": True}

#: The platforms whose way of opening a device this host can perform. A Unix
#: opens a Windows port by name exactly as Windows does — it is `os.open` on a
#: device-namespace path either way — but Windows has none of the flags a Unix
#: opens a device node with.
OPENABLE_HERE: frozenset[str] = (
    frozenset({"win32"}) if sys.platform == "win32" else frozenset(NAMES)
)


def _shaped_for(platform: str, device: str) -> bool:
    """Whether `device` is a name `platform`'s serial devices can have."""
    return device.upper().startswith("COM") == IS_WINDOWS[platform]


def _refuses(platform: str, device: str, lines: list[str], *, state: Path) -> None:
    """The refusal `platform` owes `device`, by class, by name and by next action.

    Raises:
        AssertionError: If it is not the refusal, or if anything was provisioned.
    """
    name, recognisable = WORDS[platform]
    if _shaped_for(platform, device):
        refused_naming(lines, "octoprint-env: failed: serial-device-unopenable")
        refused_naming(lines, "what happened:", device, "could not be opened")
        refused_naming(lines, "next action:", recognisable)
    else:
        refused_naming(lines, "octoprint-env: failed: serial-device-misnamed")
        refused_naming(lines, "what happened:", device, f"not how {name} names a serial device")
        refused_naming(lines, "next action:", name, NAMES[platform])
    truth(
        not state.exists(),
        describing=f"no state directory at {state}: nothing provisioned or started for a "
        f"device that was refused",
    )


def _declining_a_present_device(device: str) -> None:
    """Decline to open `device` where this host has one.

    A Unix host that carries a device at that path has a printer plugged in
    where a journey would open one, and a journey that opened a printer
    somebody is using would be the very thing the script exists to refuse — so
    that host is named and the journey does not run there. Nothing a gate cell
    runs on has one.
    """
    if sys.platform != "win32" and Path(device).exists():
        pytest.skip(f"{device} is a device this host has, and this journey will not open it")


@pytest.fixture(params=sorted(NAMES), ids=lambda platform: WORDS[platform][0].lower())
def device(request: pytest.FixtureRequest) -> Iterator[str]:
    """One device name in one platform's own shape, which nothing on this host answers.

    Yields:
        The device name.
    """
    named = NAMES[str(request.param)]
    _declining_a_present_device(named)
    yield named


def test_a_device_this_host_cannot_open_is_refused_before_anything_is_provisioned(
    state_dir: Callable[[str], str], device: str
) -> None:
    """The committed script, in serial mode, on this host, given each platform's name."""
    state = Path(state_dir("refused"))
    here = sys.platform if sys.platform in NAMES else "linux"

    result = script("up", "--state-dir", str(state), "--mode", "serial", "--device", device)

    failing(result, naming="octoprint-env: failed")
    _refuses(here, device, said(result).splitlines(), state=state)


#: Every platform answering every name, less the pairs this host cannot perform:
#: a name the platform cannot have is never opened and so is answered anywhere,
#: while a name it can have is opened by that platform's own means.
ANSWERED_HERE: list[tuple[str, str]] = [
    (platform, NAMES[named])
    for platform in sorted(NAMES)
    for named in sorted(NAMES)
    if not _shaped_for(platform, NAMES[named]) or platform in OPENABLE_HERE
]


@pytest.mark.parametrize(
    ("platform", "named"),
    ANSWERED_HERE,
    ids=[f"{WORDS[platform][0].lower()}-given-{named}" for platform, named in ANSWERED_HERE],
)
def test_each_platform_answers_each_name_in_its_own_words(
    tmp_path: Path,
    monkeypatch: pytest.MonkeyPatch,
    capsys: pytest.CaptureFixture[str],
    platform: str,
    named: str,
) -> None:
    """The script's entry point, answering as each platform, given each platform's name."""
    _declining_a_present_device(named)
    state = tmp_path / "state"
    monkeypatch.setattr(sys, "platform", platform)

    code = octoprint_env.main(
        ["up", "--state-dir", str(state), "--mode", "serial", "--device", named]
    )

    lines = capsys.readouterr().err.splitlines()
    truth(code == 1, describing=f"a refused start to exit 1; it said:\n{chr(10).join(lines)}")
    _refuses(platform, named, lines, state=state)


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

    lines = capsys.readouterr().err.splitlines()
    truth(code == 1, describing=f"starting against a port nothing is plugged into:\n{lines}")
    refused_naming(lines, "serial-device-unopenable")
    refused_naming(lines, "what happened:", repr("\\\\.\\COM250")[1:-1])
    truth(
        not (tmp_path / "state").exists(),
        describing="nothing provisioned for a device that cannot be opened",
    )


@pytest.mark.parametrize(
    ("platform", "assignment"),
    [
        pytest.param("linux", "OCTOPRINT_ENV_DEVICE=/dev/ttyACM0", id="linux"),
        pytest.param("darwin", "OCTOPRINT_ENV_DEVICE=/dev/cu.usbmodem1101", id="macos"),
        pytest.param("win32", "$env:OCTOPRINT_ENV_DEVICE='COM3'", id="windows"),
    ],
)
def test_serial_mode_naming_no_device_is_told_what_to_pass_on_each_platform(
    tmp_path: Path,
    monkeypatch: pytest.MonkeyPatch,
    capsys: pytest.CaptureFixture[str],
    platform: str,
    assignment: str,
) -> None:
    """`--mode serial` with no device names the platform's own example, in its own shell."""
    monkeypatch.setattr(sys, "platform", platform)

    code = octoprint_env.main(["up", "--state-dir", str(tmp_path / "state"), "--mode", "serial"])

    lines = capsys.readouterr().err.splitlines()
    truth(code == 1, describing=f"a start naming no device to exit 1 as {platform}")
    refused_naming(lines, "serial-device-misnamed")
    refused_naming(lines, "what happened:", f"--device {NAMES[platform]}", assignment)
    truth(not (tmp_path / "state").exists(), describing="nothing provisioned for no device")
