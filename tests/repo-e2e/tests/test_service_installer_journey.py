"""The Linux and macOS service installer's options, run against a throwaway root.

`scripts/install-service.sh` is the first command of the systemd and launchd
pairs the install path states. `test_service_manager_journey.py` activates what
it installs through each manager. This journey holds what the installer writes
into the configuration for the options it takes, the way an administrator's
automated deployment passes them. It runs the committed script as the invoking
user, under `--root` and `--user`, so it writes every file it would write
beneath `/` into a directory here, and it asks no manager anything.

The Windows installer takes the same options, and
`test_windows_service_installer_journey.py` holds them there the same way.
"""

from __future__ import annotations

import getpass
import sys
import tomllib
from collections.abc import Callable
from dataclasses import dataclass
from pathlib import Path

import pytest
from journey import DEBUG_BUILD, REPO_ROOT, clean_environment, run
from repo_checks.checks_service import installer_for
from repo_checks.expect import contains, equal, failing, passing, truth
from repo_checks.model import Repo
from repo_checks.platforms import ServiceManager
from repo_checks.shell import run as shell_run

pytestmark = [
    DEBUG_BUILD,
    pytest.mark.skipif(
        sys.platform == "win32",
        reason="the shell installer is the Linux and macOS one; Windows takes install-service.ps1",
    ),
]

#: The committed installer the systemd and launchd pairs fetch, as declared.
INSTALLER = REPO_ROOT / installer_for(Repo(REPO_ROOT), ServiceManager.SYSTEMD)

#: The option this journey is about.
INTERVAL = "--observation-interval-seconds"

#: How long the program build is given the first time this tier runs.
BUILD_TIMEOUT_SECONDS = 2400


# llmlint: ignore[expensive_tests_stay_behind_their_own_edge] suppressions.toml has the reason.
@pytest.fixture(scope="module")
def program() -> Path:
    """The `printobserver` program built from this tree, which the installer places."""
    passing(
        run(
            ["cargo", "build", "--locked", "-p", "printobserver"],
            REPO_ROOT,
            timeout=BUILD_TIMEOUT_SECONDS,
        ),
        describing="building the printobserver program",
    )
    return REPO_ROOT / "target" / "debug" / "printobserver"


@dataclass(frozen=True, slots=True)
class Ran:
    """One run of the installer against a root."""

    code: int
    said: str
    root: Path

    @property
    def configuration(self) -> Path:
        """Where the configuration goes beneath the root."""
        return self.root / "etc" / "printobserver" / "config.toml"

    def supervisor(self) -> dict[str, object]:
        """The configuration's `[supervisor]` table, as the server's TOML reader reads it."""
        written = tomllib.loads(self.configuration.read_text(encoding="utf-8"))
        table = written.get("supervisor")
        truth(isinstance(table, dict), describing="the configuration to carry `[supervisor]`")
        return table if isinstance(table, dict) else {}


@pytest.fixture
def installer(tmp_path: Path, program: Path) -> Callable[..., Ran]:
    """Run the committed installer as this user, against a root here."""
    counter = {"n": 0}

    def install(*, root: Path | None = None, interval: str | None = None) -> Ran:
        counter["n"] += 1
        root = root or tmp_path / f"root{counter['n']}"
        arguments = ["--root", str(root), "--binary", str(program), "--user", getpass.getuser()]
        if interval is not None:
            arguments += [INTERVAL, interval]
        result = shell_run(
            ["sh", str(INSTALLER), *arguments],
            cwd=tmp_path,
            env=clean_environment(),
            timeout=300,
        )
        return Ran(result.returncode, (result.stdout or "") + (result.stderr or ""), root)

    return install


def test_an_observation_interval_given_is_written_into_the_configuration(
    installer: Callable[..., Ran],
) -> None:
    """`--observation-interval-seconds 60` is the server's `supervisor.observation_interval_s`."""
    ran = installer(interval="60")

    passing((ran.code, ran.said), describing="the installer given an observation interval")
    equal(
        ran.supervisor().get("observation_interval_s"),
        60,
        describing="the observation interval the configuration names",
    )
    contains(ran.said, "started nothing", describing="what the installer said it did")


def test_with_no_observation_interval_the_configuration_keeps_the_default(
    installer: Callable[..., Ran],
) -> None:
    """Left out, the configuration names no interval, so the server takes its own default."""
    ran = installer()

    passing((ran.code, ran.said), describing="the installer given no observation interval")
    truth(
        "observation_interval_s" not in ran.supervisor(),
        describing=f"no observation interval in the configuration: {ran.supervisor()}",
    )


@pytest.mark.parametrize("interval", ["0", "-5", "060", "sixty", "9223372036854775808"])
def test_an_observation_interval_the_server_would_refuse_is_refused_naming_it(
    installer: Callable[..., Ran], interval: str
) -> None:
    """Zero, a negative, or anything no configuration holds as one is refused before placing."""
    ran = installer(interval=interval)

    failing((ran.code, ran.said), naming=INTERVAL)
    contains(ran.said, f"'{interval}' is not one", describing="the value it refused, quoted")
    truth(not ran.root.exists(), describing="nothing to have been placed")


def test_the_largest_interval_a_configuration_holds_is_written(
    installer: Callable[..., Ran],
) -> None:
    """The bound is the TOML integer's own: its largest value is accepted and written."""
    largest = "9223372036854775807"
    ran = installer(interval=largest)

    passing((ran.code, ran.said), describing="the installer given the largest interval")
    equal(
        ran.supervisor().get("observation_interval_s"),
        int(largest),
        describing="the largest observation interval, as the configuration names it",
    )


def test_a_reinstall_given_an_interval_leaves_the_configuration_alone_and_says_so(
    installer: Callable[..., Ran],
) -> None:
    """A configuration already in place is the operator's; the interval not written is named."""
    first = installer()
    passing((first.code, first.said), describing="the first install")
    before = first.configuration.read_text(encoding="utf-8")

    again = installer(root=first.root, interval="60")

    passing((again.code, again.said), describing="the reinstall given an interval")
    equal(
        first.configuration.read_text(encoding="utf-8"),
        before,
        describing="the configuration after a reinstall given an interval",
    )
    contains(
        again.said,
        f"{INTERVAL} was not written into it",
        describing="what the reinstall said of the interval it did not write",
    )


def test_an_observation_interval_option_with_no_value_is_refused_naming_it(
    tmp_path: Path, program: Path
) -> None:
    """The option given last with no number after it is a stop naming it, placing nothing."""
    root = tmp_path / "root"
    result = shell_run(
        [
            "sh",
            str(INSTALLER),
            "--root",
            str(root),
            "--binary",
            str(program),
            "--user",
            getpass.getuser(),
            INTERVAL,
        ],
        cwd=tmp_path,
        env=clean_environment(),
        timeout=300,
    )

    failing(result, naming=f"{INTERVAL} takes a number of seconds")
    truth(not root.exists(), describing="nothing to have been placed")
