"""The platforms an artifact is built for, read from the one list that names them.

`AGENTS.md`'s supported-platform list is the source, exactly as it is for every
continuous-integration matrix, and this reads it rather than restating it: an
artifact set narrowed independently of that list is the failure the list exists
to prevent.
"""

from __future__ import annotations

import os
from dataclasses import dataclass

from repo_checks.checks_ci import platforms_of
from repo_checks.model import Repo

#: What each supported platform is called by the two registries that select on
#: it. npm reads `os` and `cpu` out of a package's own manifest, and there is
#: no list of these anywhere else in the tree.
NPM_NAMES = {
    "linux-x86_64": ("linux", "x64"),
    "linux-aarch64": ("linux", "arm64"),
}

#: The processor part of a Python wheel's platform tag, per supported platform.
WHEEL_MACHINES = {"linux-x86_64": "x86_64", "linux-aarch64": "aarch64"}


class PlatformError(ValueError):
    """A supported platform no artifact of this repository knows how to name."""


@dataclass(frozen=True, slots=True)
class Platform:
    """One supported platform, as the artifacts name it."""

    #: Its own identifier, as `AGENTS.md` spells it.
    id: str
    #: The runner it is built on.
    runner: str
    #: The Rust target the program is built for.
    target: str

    @property
    def npm(self) -> tuple[str, str]:
        """The operating system and processor npm selects a package by.

        Raises:
            PlatformError: If nothing here names this platform for npm.
        """
        named = NPM_NAMES.get(self.id)
        if named is None:
            msg = (
                f"`{self.id}` is a supported platform that nothing here names for npm; "
                f"add it to `NPM_NAMES` beside the platform list that gained it"
            )
            raise PlatformError(msg)
        return named

    @property
    def npm_package(self) -> str:
        """The per-platform package npm resolves the program through."""
        system, processor = self.npm
        return f"@printobserver/cli-{system}-{processor}"

    def wheel_tag(self, glibc: tuple[int, int]) -> str:
        """The platform tag a wheel built for this platform carries.

        The tag states the C library the program was **actually** built
        against, read off the host rather than assumed: a wheel claiming an
        older one would install on a machine it cannot run on, and that is a
        failure the user meets at the printer rather than at the install.

        Raises:
            PlatformError: If nothing here names this platform for a wheel.
        """
        machine = WHEEL_MACHINES.get(self.id)
        if machine is None:
            msg = (
                f"`{self.id}` is a supported platform that nothing here names for a "
                f"wheel; add it to `WHEEL_MACHINES` beside the platform list that "
                f"gained it"
            )
            raise PlatformError(msg)
        major, minor = glibc
        return f"manylinux_{major}_{minor}_{machine}"


def supported(repo: Repo) -> list[Platform]:
    """Every platform `AGENTS.md`'s own list names."""
    return [
        Platform(id=found.id, runner=found.runner, target=found.target)
        for found in platforms_of(repo)
    ]


def host_glibc() -> tuple[int, int]:
    """The C library version this host's programs are built against.

    Raises:
        PlatformError: If this host reports none, which is a host no wheel of
            this repository is built on.
    """
    reported = os.confstr("CS_GNU_LIBC_VERSION") if hasattr(os, "confstr") else None
    parts = (reported or "").split()
    if len(parts) != 2 or parts[0] != "glibc":
        msg = (
            f"this host reports its C library as {reported!r}, and a wheel's platform "
            f"tag has to state one it was built against"
        )
        raise PlatformError(msg)
    numbers = parts[1].split(".")
    return int(numbers[0]), int(numbers[1] if len(numbers) > 1 else 0)


def host(repo: Repo) -> Platform:
    """The supported platform this host is one of.

    Raises:
        PlatformError: If this host is not one the supported-platform list
            names, which is a host this repository builds nothing on.
    """
    machine = os.uname().machine
    wanted = {"x86_64": "linux-x86_64", "aarch64": "linux-aarch64", "arm64": "linux-aarch64"}
    named = wanted.get(machine)
    for platform in supported(repo):
        if platform.id == named:
            return platform
    msg = (
        f"this host is `{os.uname().sysname}/{machine}`, which AGENTS.md's "
        f"supported-platform list does not name"
    )
    raise PlatformError(msg)
