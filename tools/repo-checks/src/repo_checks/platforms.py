"""Every fact this repository knows about a platform, in one place.

`AGENTS.md`'s supported-platform list is the one source of *which* platforms
this repository supports, and of the three facts a reader of that list needs:
the runner a job runs on, the Rust target the program is built for, and the
service manager the install path's own commands are written for. It is prose,
because those three are what a person reads when they ask what is supported.

Four more facts about a platform are not prose at all — what the JavaScript
registry selects a package by, what the program's file is called there, and what
the release asset is named — and a fifth, the wheel's platform tag, is a rule
rather than a value. Before this module those lived as four separate literals
scattered across two languages, two of which nothing reconciled: a platform
could be named in one and missed in another, and what a user met was an install
that resolved nothing.

So this module declares those four, for every identifier this repository's plan
uses, and hands them back **beside** the list's own three as one object. A
consumer asks it about a platform rather than reading the list and a second
table; asking about an identifier the supported-platform list does not name is
refused naming it, rather than answered with a default, because a default there
is a program built for one machine shipped under another machine's name.
"""

from __future__ import annotations

import os
import platform as host_platform
import re
from dataclasses import dataclass

from repo_checks.model import Repo
from repo_checks.parsing import marker_block

#: One entry of `AGENTS.md`'s supported-platform list.
#:
#: The install-path answer is a lever rather than a note: a platform answered
#: `no` is kept out of every install-route, registry-proof and artifact-route
#: matrix, so it carries the reason for the opt-out on the line itself.
PLATFORM_LINE = re.compile(
    r"^- `(?P<id>[a-z0-9_-]+)` — runner `(?P<runner>[^`]+)`, Rust target `(?P<target>[^`]+)`, "
    r"service manager `(?P<service_manager>[^`]+)`, install path: (?P<install>yes|no)"
    r"(?: — (?P<reason>.+))?$"
)

#: What one of those entries has to look like, quoted back at whoever writes one
#: that does not. A line of that block beginning `- ` and not matching
#: `PLATFORM_LINE` is refused rather than passed over: an entry with a typo in it
#: would otherwise be a platform silently unsupported.
PLATFORM_SHAPE = (
    "- `<platform>` — runner `<runner>`, Rust target `<target>`, "
    "service manager `<manager>`, install path: yes|no[ — <reason>]"
)

#: A token shaped like one of these identifiers: an operating-system family this
#: list names one of, and a processor. A shape rather than a fixed set, so a
#: document naming `linux-riscv64` is found by being a platform identifier rather
#: than by being on a list of wrong ones — and the processor half is closed on
#: purpose, which is what keeps a runner label (`macos-15`, `windows-11-arm`) and
#: a registry's own selector (`linux-x64`, `darwin-arm64`) out of it.
#:
#: `NAMING`'s own keys are held to it, so the two cannot drift.
#: The one spelling of that shape, used anchored below and embedded in
#: `PLATFORM_ID_IN_TEXT`, so a document and an entry are read against one rule.
PLATFORM_ID_PATTERN = (
    r"(?:linux|macos|windows)-(?:x86_64|aarch64|riscv64|armv7l?|i686|ppc64le|s390x)"
)

PLATFORM_ID = re.compile(rf"^{PLATFORM_ID_PATTERN}$")

#: The same shape as a document writes one, backticked or not. A claim about a
#: platform is a claim whether or not somebody quoted it.
PLATFORM_ID_IN_TEXT = re.compile(rf"(?<![\w-]){PLATFORM_ID_PATTERN}(?![\w-])")

#: Every service manager this repository has a name for, which is the closed set
#: a supported-platform entry's own column may state and the set a document is
#: read against. One per operating-system family, and each is the manager a host
#: of that family runs a service under with nothing installed.
SERVICE_MANAGERS = ("systemd", "launchd", "windows-service")

#: The block `AGENTS.md` records each cell that does not run in.
EXCLUSIONS_BLOCK = "platform-exclusions"

#: One recorded exclusion: a platform, the job whose matrix omits it, and why.
EXCLUSION_LINE = re.compile(r"^- `(?P<id>[^`]*)` on `(?P<job>[^`]*)`(?: — (?P<reason>.*))?$")

#: What one of those lines has to look like, quoted back at whoever wrote one
#: that does not.
EXCLUSION_SHAPE = "- `<platform>` on `<job>` — <why that cell does not run>"

#: The block `AGENTS.md` records each job that carries no platform matrix in.
UNMATRIXED_BLOCK = "unmatrixed-jobs"

#: One of those: the job, and why one change gets one cell of it rather than one
#: per platform.
UNMATRIXED_LINE = re.compile(r"^- `(?P<job>[^`]*)`(?: — (?P<reason>.*))?$")

#: What one of those lines has to look like.
UNMATRIXED_SHAPE = "- `<job>` — <why it carries no platform matrix>"


class PlatformError(ValueError):
    """A platform this repository has no complete answer for."""


@dataclass(frozen=True, slots=True)
class Naming:
    """What one platform is called by the registries and by a release.

    The four facts here and the two `wheel_` fields are this repository's own
    declaration; the runner, the Rust target and the service manager beside
    them on `Platform` come from `AGENTS.md`'s list, which is where a person
    reads them.
    """

    #: The `os` value the JavaScript registry selects a package by.
    npm_os: str
    #: The `cpu` value it selects one by.
    npm_cpu: str
    #: What the `printobserver` program's own file is called on this platform.
    program: str
    #: What the release asset carrying that program is called.
    asset: str
    #: The family a wheel's platform tag names for this platform.
    wheel_family: str
    #: The processor part of that tag.
    wheel_machine: str
    #: Whether that tag carries the minimum operating-system version the
    #: program was built against. Every tag that can carry one does — a wheel
    #: claiming an older one installs on a machine it cannot run on, and that
    #: is a failure the user meets at the printer. The Windows tags carry no
    #: version component at all, which is the platform's own rule rather than
    #: this repository choosing not to state one.
    wheel_versioned: bool


#: What every platform identifier this repository's plan uses is called.
#:
#: Six rather than the two the supported-platform list names today, and
#: deliberately: a platform joins that list only once every cell derived from
#: it is green, and it cannot be brought up in stages if nothing can name it
#: until it is already supported. Nothing here makes a platform supported —
#: `descriptor` refuses every identifier the list does not carry.
NAMING: dict[str, Naming] = {
    "linux-x86_64": Naming(
        npm_os="linux",
        npm_cpu="x64",
        program="printobserver",
        asset="printobserver-linux-x86_64.tar.gz",
        wheel_family="manylinux",
        wheel_machine="x86_64",
        wheel_versioned=True,
    ),
    "linux-aarch64": Naming(
        npm_os="linux",
        npm_cpu="arm64",
        program="printobserver",
        asset="printobserver-linux-aarch64.tar.gz",
        wheel_family="manylinux",
        wheel_machine="aarch64",
        wheel_versioned=True,
    ),
    "macos-aarch64": Naming(
        npm_os="darwin",
        npm_cpu="arm64",
        program="printobserver",
        asset="printobserver-macos-aarch64.tar.gz",
        wheel_family="macosx",
        wheel_machine="arm64",
        wheel_versioned=True,
    ),
    "macos-x86_64": Naming(
        npm_os="darwin",
        npm_cpu="x64",
        program="printobserver",
        asset="printobserver-macos-x86_64.tar.gz",
        wheel_family="macosx",
        wheel_machine="x86_64",
        wheel_versioned=True,
    ),
    "windows-x86_64": Naming(
        npm_os="win32",
        npm_cpu="x64",
        program="printobserver.exe",
        asset="printobserver-windows-x86_64.tar.gz",
        wheel_family="win",
        wheel_machine="amd64",
        wheel_versioned=False,
    ),
    "windows-aarch64": Naming(
        npm_os="win32",
        npm_cpu="arm64",
        program="printobserver.exe",
        asset="printobserver-windows-aarch64.tar.gz",
        wheel_family="win",
        wheel_machine="arm64",
        wheel_versioned=False,
    ),
}

#: What the running interpreter reports, and which identifier that is.
#:
#: Read through `platform.system()` and `platform.machine()` rather than through
#: `os.uname`, which does not exist on Windows: a tool that cannot say which
#: platform it is on cannot build for it, and raising `AttributeError` on the
#: way to finding out is not saying so.
HOSTS = {
    ("Linux", "x86_64"): "linux-x86_64",
    ("Linux", "aarch64"): "linux-aarch64",
    ("Linux", "arm64"): "linux-aarch64",
    ("Darwin", "arm64"): "macos-aarch64",
    ("Darwin", "x86_64"): "macos-x86_64",
    ("Windows", "AMD64"): "windows-x86_64",
    ("Windows", "x86_64"): "windows-x86_64",
    ("Windows", "ARM64"): "windows-aarch64",
    ("Windows", "aarch64"): "windows-aarch64",
}


@dataclass(frozen=True, slots=True)
class Platform:
    """One supported platform: every fact this repository knows about it."""

    #: Its own identifier, as `AGENTS.md` spells it.
    id: str
    #: The runner a job of this platform runs on.
    runner: str
    #: The Rust target the program is built for.
    target: str
    #: The service manager the install path's own commands are written for.
    service_manager: str
    #: Whether the end-user install path targets this platform.
    install_path: bool
    #: Why it does not, where it does not.
    install_path_reason: str = ""

    @property
    def naming(self) -> Naming:
        """What the registries and a release call this platform.

        Raises:
            PlatformError: If nothing here names this platform, which is a
                platform the list carries and no artifact of this repository
                knows how to name.
        """
        found = NAMING.get(self.id)
        if found is None:
            msg = (
                f"`{self.id}` is a supported platform that nothing here names for the "
                f"registries or a release; add it to `NAMING` beside the platform list "
                f"that gained it"
            )
            raise PlatformError(msg)
        return found

    @property
    def npm(self) -> tuple[str, str]:
        """The operating system and processor the JavaScript registry selects by.

        Raises:
            PlatformError: If nothing here names this platform.
        """
        return self.naming.npm_os, self.naming.npm_cpu

    @property
    def npm_selector(self) -> str:
        """How the launcher's own map keys this platform, as Node reports it.

        Raises:
            PlatformError: If nothing here names this platform.
        """
        system, processor = self.npm
        return f"{system}-{processor}"

    @property
    def npm_package(self) -> str:
        """The per-platform package npm resolves the program through.

        Raises:
            PlatformError: If nothing here names this platform.
        """
        return f"@printobserver/cli-{self.npm_selector}"

    @property
    def program(self) -> str:
        """What the program's own file is called on this platform.

        Raises:
            PlatformError: If nothing here names this platform.
        """
        return self.naming.program

    @property
    def asset(self) -> str:
        """What the release asset carrying that program is called.

        Raises:
            PlatformError: If nothing here names this platform.
        """
        return self.naming.asset

    def wheel_tag(self, os_version: tuple[int, int] | None) -> str:
        """The platform tag a wheel built for this platform carries.

        The tag states the minimum operating-system version the program was
        **actually** built against — the C library on Linux, the system release
        on macOS — read off the host that built it rather than assumed: a wheel
        claiming an older one installs on a machine it cannot run on, and that
        is a failure the user meets at the printer rather than at the install.
        The two Windows tags carry no version component, so a version is not
        asked of a host that cannot state one.

        Raises:
            PlatformError: If nothing here names this platform, or if the tag
                needs a version this host did not report.
        """
        naming = self.naming
        if not naming.wheel_versioned:
            return f"{naming.wheel_family}_{naming.wheel_machine}"
        if os_version is None:
            msg = (
                f"a wheel for `{self.id}` states the operating-system version it was "
                f"built against, and this host reported none"
            )
            raise PlatformError(msg)
        major, minor = os_version
        return f"{naming.wheel_family}_{major}_{minor}_{naming.wheel_machine}"


def supported(repo: Repo) -> list[Platform]:
    """Every platform `AGENTS.md`'s own supported-platform list names.

    Raises:
        MarkerBlockMissingError: If `AGENTS.md` carries no such list.
    """
    found: list[Platform] = []
    for line in marker_block(repo.agents_md, "supported-platforms"):
        match = PLATFORM_LINE.match(line)
        if match:
            found.append(
                Platform(
                    match["id"],
                    match["runner"],
                    match["target"],
                    match["service_manager"],
                    match["install"] == "yes",
                    (match["reason"] or "").strip(),
                )
            )
    return found


def install_platforms(repo: Repo) -> list[Platform]:
    """Every platform the end-user install path targets.

    The `install path` answer is the first of the two levers a platform is
    brought up in stages by: a platform answered `no` is carried by no
    install-route, registry-proof or artifact-route matrix, and one answered
    `yes` must be carried by every one of them.

    Raises:
        MarkerBlockMissingError: If `AGENTS.md` carries no such list.
    """
    return [platform for platform in supported(repo) if platform.install_path]


def descriptor(repo: Repo, identifier: str) -> Platform:
    """Every fact this repository knows about one supported platform.

    Raises:
        PlatformError: If `AGENTS.md`'s supported-platform list does not name
            `identifier`. Answering with a default there would hand a consumer
            a program built for one machine under another machine's name.
        MarkerBlockMissingError: If `AGENTS.md` carries no such list.
    """
    named = supported(repo)
    for platform in named:
        if platform.id == identifier:
            return platform
    carried = ", ".join(f"`{one.id}`" for one in named) or "nothing"
    msg = (
        f"`{identifier}` is not a platform AGENTS.md's supported-platform list names; "
        f"it names {carried}"
    )
    raise PlatformError(msg)


def host_os_version() -> tuple[int, int] | None:
    """The minimum operating-system version this host's programs are built against.

    The C library on Linux and the system release on macOS. On Windows it is
    `None`, because the platform tags this repository's wheels carry there state
    no version at all.

    Raises:
        PlatformError: If a host that states one reports none, which is a host
            no wheel of this repository is built on.
    """
    match host_platform.system():
        case "Windows":
            return None
        case "Darwin":
            return _version(host_platform.mac_ver()[0], "this host reports its system release as")
        case _:
            return _glibc()


def _glibc() -> tuple[int, int]:
    """The C library version this host's programs are built against.

    Raises:
        PlatformError: If this host reports none.
    """
    reported = os.confstr("CS_GNU_LIBC_VERSION") if hasattr(os, "confstr") else None
    parts = (reported or "").split()
    if len(parts) != 2 or parts[0] != "glibc":
        msg = (
            f"this host reports its C library as {reported!r}, and a wheel's platform "
            f"tag has to state one it was built against"
        )
        raise PlatformError(msg)
    return _version(parts[1], "this host reports its C library as")


def _version(reported: str, why: str) -> tuple[int, int]:
    """A reported `major.minor[...]` as the pair a platform tag carries.

    Raises:
        PlatformError: If it is not one.
    """
    numbers = reported.split(".")
    # A version with no minor component is that platform's own way of writing
    # `.0`; one whose minor is not a number is malformed, and reading it as zero
    # would put a tag on a wheel stating a version nothing reported.
    malformed = not numbers[0].isdigit() or (len(numbers) > 1 and not numbers[1].isdigit())
    if malformed:
        msg = (
            f"{why} {reported!r}, and a wheel's platform tag has to state one it was built against"
        )
        raise PlatformError(msg)
    return int(numbers[0]), int(numbers[1]) if len(numbers) > 1 else 0


def host(repo: Repo) -> Platform:
    """The supported platform this host is one of.

    Raises:
        PlatformError: If this host is not one the supported-platform list
            names, which is a host this repository builds nothing on.
        MarkerBlockMissingError: If `AGENTS.md` carries no such list.
    """
    system, machine = host_platform.system(), host_platform.machine()
    identifier = HOSTS.get((system, machine))
    if identifier is not None:
        for platform in supported(repo):
            if platform.id == identifier:
                return platform
    msg = (
        f"this host is `{system}/{machine}`, which AGENTS.md's supported-platform list "
        f"does not name"
    )
    raise PlatformError(msg)
