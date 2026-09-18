r"""A serial device this host cannot open is refused before anything is provisioned.

Four names — one in each platform's own shape, `COM3`, `/dev/cu.usbmodem1101`
and `/dev/ttyACM0`, and the Windows port again in the device namespace every
port opens under, `\\.\COM3` — and two journeys over them. The first runs the
committed script in `--mode serial` on whichever host this suite is on, so each
gate cell proves its own platform's naming and its own way of opening a device:
the name that host's serial devices have is opened, by that host's means, and
refused as unopenable because nothing is plugged in there; a name that host
cannot have is refused by naming what its devices are called instead. The
second fixes the script's answer to `sys.platform` to each of the three in turn
and runs its own entry point, so every platform's naming — and, where this host
can perform it, every platform's way of opening a device — is proven on every
host rather than only on the one the suite happens to be on.

Both refusals come before anything is provisioned or started: a device that
cannot be opened is no reason to install OctoPrint first, and every journey
here asserts the state directory was never made.

This module is the one in this suite that needs no OctoPrint, so it is the
project's `test` target and runs in every gate cell; the rest of the suite is
the integration tier's.
"""

from __future__ import annotations

import re
import sys
from collections.abc import Callable, Iterator
from dataclasses import dataclass
from pathlib import Path

import octoprint_env
import pytest
from environment import said, script
from repo_checks.expect import failing, refused_naming, truth


@dataclass(frozen=True, slots=True)
class PlatformCase:
    """One platform as this suite knows it, in its own words rather than the script's.

    A name in that platform's own shape; what the platform calls itself and
    something a person there would recognise in a next action; and whether it
    names devices the Windows way. A Unix names its devices under `/dev`, and a
    `udev` rule may link a printer under any name there, so a Unix refuses a
    `COM` port and nothing else.

    What a Windows port is called is this suite's own statement, `WINDOWS_PORT`,
    held against the script's by feeding every platform every name in `NAMES`:
    a name the two disagree over is refused as the wrong class and fails here.
    """

    key: str
    name: str
    device: str
    recognisable: str
    windows: bool

    def can_have(self, device: str) -> bool:
        """Whether `device` is a name this platform's serial devices can have."""
        return (WINDOWS_PORT.fullmatch(device) is not None) == self.windows


#: A Windows port, in this suite's own words: `COM` and a number, bare or
#: under the `\\.\` device namespace, which is the spelling every port opens
#: under and the one a person who read the script's own hint would type.
WINDOWS_PORT = re.compile(r"(\\\\\.\\)?COM[0-9]+", re.IGNORECASE)

#: The Windows port again, in the device namespace.
NAMESPACED_WINDOWS_DEVICE = "\\\\.\\COM3"


#: The three platforms, by what `sys.platform` answers on each.
PLATFORMS: dict[str, PlatformCase] = {
    case.key: case
    for case in (
        PlatformCase("darwin", "macOS", "/dev/cu.usbmodem1101", "ls /dev/cu.*", windows=False),
        PlatformCase("linux", "Linux", "/dev/ttyACM0", "dialout", windows=False),
        PlatformCase("win32", "Windows", "COM3", "Ports (COM & LPT)", windows=True),
    )
}

#: Every name every platform is fed: each platform's own, and the Windows port
#: in the device namespace, which Windows can have and a Unix cannot.
NAMES: list[str] = [*(case.device for case in PLATFORMS.values()), NAMESPACED_WINDOWS_DEVICE]

#: The platform this host is, as the script will read it.
HERE: PlatformCase = PLATFORMS.get(sys.platform, PLATFORMS["linux"])

#: The platforms whose way of opening a device this host can perform. A Unix
#: opens a Windows port by name exactly as Windows does — it is `os.open` on a
#: device-namespace path either way — but Windows has none of the flags a Unix
#: opens a device node with.
OPENABLE_HERE: frozenset[str] = (
    frozenset({"win32"}) if sys.platform == "win32" else frozenset(PLATFORMS)
)


def _refuses(platform: PlatformCase, device: str, lines: list[str], *, state: Path) -> None:
    """The refusal `platform` owes `device`, by class, by name and by next action.

    Raises:
        AssertionError: If it is not the refusal, or if anything was provisioned.
    """
    if platform.can_have(device):
        refused_naming(lines, "octoprint-env: failed: serial-device-unopenable")
        refused_naming(lines, "what happened:", device, "could not be opened")
        refused_naming(lines, "next action:", platform.recognisable)
    else:
        refused_naming(lines, "octoprint-env: failed: serial-device-misnamed")
        refused_naming(
            lines, "what happened:", device, f"not how {platform.name} names a serial device"
        )
        refused_naming(lines, "next action:", platform.name, platform.device)
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


@pytest.fixture(params=NAMES)
def device(request: pytest.FixtureRequest) -> Iterator[str]:
    """One device name in one platform's own shape, which nothing on this host answers.

    Yields:
        The device name.
    """
    named = str(request.param)
    _declining_a_present_device(named)
    yield named


def test_a_device_this_host_cannot_open_is_refused_before_anything_is_provisioned(
    state_dir: Callable[[str], str], device: str
) -> None:
    """The committed script, in serial mode, on this host, given each platform's name."""
    state = Path(state_dir("refused"))

    result = script("up", "--state-dir", str(state), "--mode", "serial", "--device", device)

    failing(result, naming="octoprint-env: failed")
    _refuses(HERE, device, said(result).splitlines(), state=state)


#: Every platform answering every name, less the pairs this host cannot perform:
#: a name the platform cannot have is never opened and so is answered anywhere,
#: while a name it can have is opened by that platform's own means.
ANSWERED_HERE: list[tuple[PlatformCase, str]] = [
    (platform, named)
    for platform in PLATFORMS.values()
    for named in NAMES
    if not platform.can_have(named) or platform.key in OPENABLE_HERE
]


@pytest.mark.parametrize(
    ("platform", "named"),
    ANSWERED_HERE,
    ids=[f"{platform.name.lower()}-given-{named}" for platform, named in ANSWERED_HERE],
)
def test_each_platform_answers_each_name_in_its_own_words(
    tmp_path: Path,
    monkeypatch: pytest.MonkeyPatch,
    capsys: pytest.CaptureFixture[str],
    platform: PlatformCase,
    named: str,
) -> None:
    """The script's entry point, answering as each platform, given each platform's name."""
    _declining_a_present_device(named)
    state = tmp_path / "state"
    monkeypatch.setattr(sys, "platform", platform.key)

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
    refused_naming(lines, "what happened:", f"--device {PLATFORMS[platform].device}", assignment)
    truth(not (tmp_path / "state").exists(), describing="nothing provisioned for no device")
