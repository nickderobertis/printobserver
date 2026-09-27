"""An artifact is proven on an interpreter its own tag and its own floor both admit.

And found where the machine that made its environment put it. Each of those is a
question about a machine this suite may not be running on — a Windows layout, an
arm64 host whose default interpreter is x86_64 — so each is asked by stating
that machine's layout or offer rather than by being run on it, the way
`tools/octoprint-env/tests/test_host_answers.py` asks for both of its layouts
from one host. One case then makes a real environment on this host through the
same selection, and reads back what it holds.
"""

from __future__ import annotations

import platform as host_platform
import re
from pathlib import Path

import pytest
from release_artifacts.build import CLIENT_REQUIRES_PYTHON, REQUIRES_PYTHON
from release_artifacts.installing import (
    ANY_PLATFORM,
    InstallError,
    Interpreter,
    choose,
    interpreter_in,
    platform_of,
    program_in,
    python_environment,
    wheel_tag,
)
from repo_checks import platforms
from repo_checks.expect import contains, equal, passing
from repo_checks.model import Repo
from repo_checks.shell import run

#: Where each host family's environments keep the two programs a proof reaches
#: for, relative to the environment: the file an installer is handed and looks
#: for, suffix included.
LAYOUTS = {
    "win32": {"python": Path("Scripts") / "python.exe", "pip": Path("Scripts") / "pip.exe"},
    "linux": {"python": Path("bin") / "python", "pip": Path("bin") / "pip"},
}

#: The command-line wheel an arm64 Windows host builds, as the build names it.
ARM64_WHEEL = "printobserver_cli-9.9.9-py3-none-win_arm64.whl"


def _offer(key: str, *, installed: bool = True) -> Interpreter:
    """One interpreter as `uv` reports it, from its own key alone."""
    implementation, version, system, processor, libc = key.split("-")
    major, minor, _ = version.split(".")
    path = f"/offered/{key}/python" if installed else None
    del implementation
    return Interpreter(key, (int(major), int(minor)), system, processor, libc, path)


@pytest.mark.parametrize("host", sorted(LAYOUTS))
def test_an_environment_is_read_where_its_own_hosts_layout_put_it(
    host: str, tmp_path: Path
) -> None:
    """A Windows environment's interpreter is `Scripts/python.exe`, never `Scripts/python`.

    The path without the suffix is one a process spawner forgives, because it
    appends `.exe` itself, and one an installer handed it does not: it looks for
    that file and finds nothing.
    """
    environment = tmp_path / "env"
    equal(
        interpreter_in(environment, host),
        environment / LAYOUTS[host]["python"],
        describing=f"the interpreter of an environment made on `{host}`",
    )
    equal(
        program_in(environment, "pip", host),
        environment / LAYOUTS[host]["pip"],
        describing=f"the `pip` of an environment made on `{host}`",
    )


def test_an_interpreter_of_another_processor_is_refused_naming_tag_floor_and_offer() -> None:
    """An arm64 wheel is not proven on the x86_64 interpreter an arm64 host offers first.

    That interpreter satisfies the floor, and a request by version alone takes
    it; the wheel's own tag excludes it, and the installer then refuses the
    wheel. Where nothing satisfies both, the refusal says what was asked and
    what was there.
    """
    offered = [_offer("cpython-3.11.16-windows-x86_64-none")]

    with pytest.raises(InstallError) as refused:
        choose(offered, REQUIRES_PYTHON, platform_of(wheel_tag(Path(ARM64_WHEEL))), ARM64_WHEEL)

    said = str(refused.value)
    for named in ("win_arm64", REQUIRES_PYTHON, "cpython-3.11.16-windows-x86_64-none"):
        contains(said, named, describing="the refusal of an interpreter the tag excludes")


def test_the_interpreter_chosen_is_the_one_both_tag_and_floor_admit() -> None:
    """Of what a machine offers, the one of the wheel's processor at or above its floor.

    Installed before downloaded, so a proof does not fetch an interpreter the
    machine already has; one below the floor is not taken for being the right
    processor, nor a musl one for a wheel linked against glibc.
    """
    arm64 = platform_of("win_arm64")
    offered = [
        _offer("cpython-3.13.15-windows-x86_64-none"),
        _offer("cpython-3.13.15-windows-aarch64-none", installed=False),
        _offer("cpython-3.12.14-windows-aarch64-none"),
    ]
    equal(
        choose(offered, REQUIRES_PYTHON, arm64, ARM64_WHEEL).key,
        "cpython-3.12.14-windows-aarch64-none",
        describing="the installed interpreter the arm64 wheel admits",
    )
    equal(
        choose(offered[:2], REQUIRES_PYTHON, arm64, ARM64_WHEEL).key,
        "cpython-3.13.15-windows-aarch64-none",
        describing="the downloadable one, where none installed is admitted",
    )
    with pytest.raises(InstallError, match=re.escape("cpython-3.12.14-windows-aarch64-none")):
        choose(offered[2:], ">=3.13", arm64, ARM64_WHEEL)
    with pytest.raises(InstallError, match="manylinux_2_17_x86_64"):
        choose(
            [_offer("cpython-3.13.15-linux-x86_64-musl")],
            REQUIRES_PYTHON,
            platform_of("manylinux_2_17_x86_64"),
            "a wheel tagged manylinux_2_17_x86_64",
        )
    equal(
        choose(offered[:1], CLIENT_REQUIRES_PYTHON, platform_of(ANY_PLATFORM), "any").key,
        "cpython-3.13.15-windows-x86_64-none",
        describing="the interpreter a wheel tagged `any` admits",
    )


@pytest.mark.parametrize("identifier", sorted(platforms.NAMING))
def test_every_platform_a_wheel_is_built_for_is_read_back_off_its_tag(identifier: str) -> None:
    """Each tag the build writes names the platform it was built for, baseline or none."""
    tag = platforms.Platform(identifier, "", "", "", install_path=True).wheel_tag((2, 17))
    equal(platform_of(tag), identifier, describing=f"the platform `{tag}` names")


def test_a_tag_naming_no_platform_is_refused() -> None:
    """A tag no platform here builds is not read as whichever one it resembles."""
    with pytest.raises(InstallError, match="linux_riscv64"):
        platform_of("linux_riscv64")


def test_an_environment_made_here_holds_an_interpreter_of_this_hosts_processor(
    repo: Repo, tmp_path: Path
) -> None:
    """The real selection over what this machine offers, read back from what it made."""
    host = platforms.host(repo).id
    environment = tmp_path / "env"

    python_environment(environment, CLIENT_REQUIRES_PYTHON, platform=host, artifact=host)

    asked = run(
        [str(interpreter_in(environment)), "-c", "import platform; print(platform.machine())"],
        cwd=tmp_path,
    )
    passing(asked, describing="asking the environment's interpreter its processor")
    equal(
        platforms.HOSTS[(host_platform.system(), asked.stdout.strip())],
        host,
        describing="the platform of the interpreter the environment holds",
    )
