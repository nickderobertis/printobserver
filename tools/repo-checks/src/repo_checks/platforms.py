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
import struct
from dataclasses import dataclass
from enum import StrEnum
from pathlib import Path
from typing import BinaryIO

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

#: A token shaped like a platform identifier: an operating-system family and a
#: processor. A shape rather than a fixed set, so a document naming
#: `linux-riscv64` or `freebsd-x86_64` is found by being a platform identifier
#: rather than by being on a list of wrong ones — which is what lets it catch a
#: platform nothing here supports as readily as one it might.
#:
#: The **processor** half is closed and the family half is not, and that is the
#: whole of what keeps the false positives out: a runner label (`macos-15`,
#: `windows-11-arm`, `ubuntu-24.04-arm`), a Rust target (`aarch64-apple-darwin`)
#: and a registry's own selector (`linux-x64`, `darwin-arm64`) each fail on
#: their second half rather than on their first.
#:
#: `NAMING`'s own keys are held to it, so the two cannot drift. It is the one
#: spelling of that shape, used anchored below and embedded in
#: `PLATFORM_ID_IN_TEXT`, so a document and an entry are read against one rule.
PLATFORM_ID_PATTERN = (
    r"(?:[a-z][a-z0-9]*)-(?:x86_64|aarch64|riscv64|loongarch64|armv7l?|i686|ppc64le|s390x)"
)

PLATFORM_ID = re.compile(rf"^{PLATFORM_ID_PATTERN}$")

#: The same shape as a document writes one, backticked or not. A claim about a
#: platform is a claim whether or not somebody quoted it.
PLATFORM_ID_IN_TEXT = re.compile(rf"(?<![\w-]){PLATFORM_ID_PATTERN}(?![\w-])")


class ServiceManager(StrEnum):
    """Every service manager this repository has a name for.

    A closed set: it is what a supported-platform entry's own column may state,
    what the install path states one pair of commands per, and what a document
    is read against. One per operating-system family, and each is the manager a
    host of that family runs a service under with nothing installed.

    A `StrEnum` so that a member compares equal to the spelling `AGENTS.md`
    carries, which is how a value read out of prose is held to this set without
    a conversion that could fail on the way.
    """

    SYSTEMD = "systemd"
    LAUNCHD = "launchd"
    WINDOWS_SERVICE = "windows-service"


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
    #: Whether that tag carries the baseline the program was built against —
    #: the oldest host the wheel claims to run on, in whichever version that
    #: platform states its floor as. Every tag that can carry one does: a wheel
    #: claiming an older baseline installs on a machine it cannot run on, and
    #: that is a failure the user meets at the printer. The Windows tags carry
    #: no version component at all, which is that platform's own rule rather
    #: than this repository choosing not to state one.
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

    def wheel_tag(self, baseline: tuple[int, int] | None) -> str:
        """The platform tag a wheel built for this platform carries.

        The tag states the oldest host the wheel claims to run on, in whichever
        version that platform states its own floor as — the C library on Linux,
        the minimum system release the program was linked for on macOS — and it
        is what the program was **actually** built against, read off the host
        and the program that built it rather than assumed: a wheel claiming an
        older baseline installs on a machine it cannot run on, and that is a
        failure the user meets at the printer rather than at the install. The
        two Windows tags carry no version component, so a baseline is not asked
        of a host that states none.

        A macOS baseline is spelled the way an installer can match it, which
        `macos_tag_version` states the rule for.

        Raises:
            PlatformError: If nothing here names this platform, or if the tag
                needs a baseline this host did not report.
        """
        naming = self.naming
        if not naming.wheel_versioned:
            return f"{naming.wheel_family}_{naming.wheel_machine}"
        if baseline is None:
            msg = (
                f"a wheel for `{self.id}` states the baseline it was built against, and "
                f"this host reported none"
            )
            raise PlatformError(msg)
        major, minor = macos_tag_version(baseline) if naming.wheel_family == MACOS else baseline
        return f"{naming.wheel_family}_{major}_{minor}_{naming.wheel_machine}"


#: The wheel family a macOS platform tag names.
MACOS = "macosx"

#: The first macOS release whose platform tags an installer generates by major
#: version alone.
MACOS_MAJOR_ONLY_FROM = 11


def macos_tag_version(minimum: tuple[int, int]) -> tuple[int, int]:
    """The version a macOS wheel tag carries for a program needing `minimum`.

    From macOS 11 on, `pip` (through `packaging`) generates the tags a host
    accepts as `macosx_<major>_0` alone — a `macosx_15_7` wheel is one no
    installer on any host would ever take. So a floor of `<major>.0` is spelled
    as itself, and a floor with a minor release is rounded **up** to the next
    major: rounding down would claim the program runs on a release older than
    the one it was linked for, which is a wheel that installs and cannot run.
    Before 11 the minor release is part of what the host generates, so it is
    kept as it is.
    """
    major, minor = minimum
    if major < MACOS_MAJOR_ONLY_FROM or minor == 0:
        return major, minor
    return major + 1, 0


def supported(repo: Repo) -> list[Platform]:
    """Every platform `AGENTS.md`'s own supported-platform list names.

    A line of that block this cannot read is passed over here and **refused** by
    `checks_ci.platforms`, which reads the same block and reports one beginning
    `- ` that is not of `PLATFORM_SHAPE`. The split is deliberate: this answers a
    list of platforms and a check answers a list of findings, so raising here
    would abort every check that reads the list rather than report the one line
    that is wrong with it — and every consumer of this function reaches it
    through a gate that has already run that check.

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


def host_baseline(program: Path | None = None) -> tuple[int, int] | None:
    """The baseline a wheel built on this host states in its platform tag.

    Every platform states the oldest host a build runs on in its own terms. On
    Linux that is the C library of the host that built it, because a program is
    linked against that library's symbols and runs on no older one. On Windows
    it is `None`, because the platform tags this repository's wheels carry there
    state no version at all.

    On macOS it is the minimum system release **the program itself** was built
    for, read off `program` — and not the release of the host that built it.
    The linker records that floor in the program (it is the deployment target
    the build used), and it is routinely far older than the builder: a program
    linked on macOS 15.7 runs on macOS 11. Tagging the wheel with the host's
    release would refuse every older host the program runs on, and a host's
    release is not a fact the program carries at all.

    Args:
        program: The program the wheel carries. Required on macOS, where the
            baseline is the program's own; read on no other platform.

    Raises:
        PlatformError: If a host that states a baseline reports none — which on
            macOS includes no program given, or one that is not a macOS program
            or records no minimum release.
    """
    match host_platform.system():
        case "Windows":
            return None
        case "Darwin":
            if program is None:
                msg = (
                    "a macOS wheel states the minimum release its program was built for, "
                    "and no program was given to read it off"
                )
                raise PlatformError(msg)
            return macos_minimum(program, host_platform.machine())
        case _:
            return _glibc()


#: The first four bytes of a thin 64-bit and a thin 32-bit Mach-O file, read
#: little-endian — the byte order of every processor macOS runs on.
MH_MAGIC_64 = 0xFEEDFACF
MH_MAGIC = 0xFEEDFACE

#: The first four bytes of a universal file, read big-endian as that header
#: always is, with 32-bit and with 64-bit slice offsets.
FAT_MAGIC = 0xCAFEBABE
FAT_MAGIC_64 = 0xCAFEBABF

#: The load command recording the platform and minimum release a program was
#: built for, and the older one recording the minimum macOS release alone.
LC_BUILD_VERSION = 0x32
LC_VERSION_MIN_MACOSX = 0x24

#: The bytes after the load-command header this reader takes from each command
#: it reads a release out of: a platform and a `minos` for the first, a version
#: for the second.
PAYLOAD_SIZES = {LC_BUILD_VERSION: 8, LC_VERSION_MIN_MACOSX: 4}

#: `LC_BUILD_VERSION`'s platform value for macOS.
PLATFORM_MACOS = 1

#: The smallest a load command can be: its own type and size.
LOAD_COMMAND_HEADER = 8

#: The Mach-O processor type each machine `platform.machine()` reports on macOS
#: selects out of a universal file.
MACHO_CPU_TYPES = {"arm64": 0x0100000C, "x86_64": 0x01000007}


def macos_minimum(program: Path, machine: str) -> tuple[int, int]:
    """The minimum macOS release `program` was built for, as `(major, minor)`.

    Read out of the program's own load commands: `LC_BUILD_VERSION`'s `minos`,
    which every current linker writes, or `LC_VERSION_MIN_MACOSX`'s `version`,
    which older ones wrote instead. A universal file is read through the slice
    for `machine`, because that slice is the program this host runs and its
    floor is the one a wheel tagged for this machine states.

    Raises:
        PlatformError: If `program` cannot be read as a Mach-O file, carries no
            slice for `machine`, or records neither load command.
    """
    try:
        with program.open("rb") as opened:
            found = _macho_minimum(opened, machine)
    except (OSError, struct.error) as unreadable:
        msg = f"{program} is not a macOS program a minimum release can be read off: {unreadable}"
        raise PlatformError(msg) from unreadable
    if found is None:
        msg = (
            f"{program} records no minimum macOS release for `{machine}` (no LC_BUILD_VERSION "
            f"or LC_VERSION_MIN_MACOSX load command), and a wheel's platform tag has to state "
            f"the one it was built for"
        )
        raise PlatformError(msg)
    return found


def _macho_minimum(opened: BinaryIO, machine: str) -> tuple[int, int] | None:
    """The minimum release the Mach-O file `opened` records, or `None` where none.

    Raises:
        struct.error: If the file ends inside a header it declares.
    """
    (magic,) = struct.unpack(">I", _exactly(opened, 4))
    if magic not in (FAT_MAGIC, FAT_MAGIC_64):
        opened.seek(0)
        return _thin_minimum(opened)
    (count,) = struct.unpack(">I", _exactly(opened, 4))
    wanted = MACHO_CPU_TYPES.get(machine)
    for _ in range(count):
        if magic == FAT_MAGIC_64:
            cpu, _sub, offset, _size, _align, _reserved = struct.unpack(
                ">iiQQII", _exactly(opened, 32)
            )
        else:
            cpu, _sub, offset, _size, _align = struct.unpack(">iiIII", _exactly(opened, 20))
        if cpu == wanted:
            opened.seek(offset)
            return _thin_minimum(opened)
    return None


def _thin_minimum(opened: BinaryIO) -> tuple[int, int] | None:
    """The minimum release the thin Mach-O image at `opened`'s position records.

    Raises:
        struct.error: If the image ends inside a header it declares.
    """
    start = opened.tell()
    (magic,) = struct.unpack("<I", _exactly(opened, 4))
    if magic not in (MH_MAGIC_64, MH_MAGIC):
        return None
    _cpu, _sub, _filetype, count, _size, _flags = struct.unpack("<iiIIII", _exactly(opened, 24))
    at = start + (32 if magic == MH_MAGIC_64 else 28)
    for _ in range(count):
        opened.seek(at)
        command, size = struct.unpack("<II", _exactly(opened, LOAD_COMMAND_HEADER))
        # The payload a command is read for has to lie inside the size it
        # declares: read past it, the fields would be the next command's bytes.
        payload = PAYLOAD_SIZES.get(command, 0)
        if size < LOAD_COMMAND_HEADER + payload:
            msg = (
                f"a load command {command:#x} declares {size} bytes, fewer than the "
                f"{LOAD_COMMAND_HEADER + payload} its own fields take"
            )
            raise struct.error(msg)
        if command == LC_BUILD_VERSION:
            platform, minimum = struct.unpack("<II", _exactly(opened, payload))
            if platform == PLATFORM_MACOS:
                return _packed_version(minimum)
        elif command == LC_VERSION_MIN_MACOSX:
            (minimum,) = struct.unpack("<I", _exactly(opened, payload))
            return _packed_version(minimum)
        at += size
    return None


def _exactly(opened: BinaryIO, count: int) -> bytes:
    """`count` bytes of `opened`.

    Raises:
        struct.error: If the file ends before that many, which is a header it
            declares and does not carry.
    """
    read = opened.read(count)
    if len(read) != count:
        msg = f"the file ends {count - len(read)} bytes inside a header it declares"
        raise struct.error(msg)
    return read


def _packed_version(packed: int) -> tuple[int, int]:
    """A Mach-O `xxxx.yy.zz` version, packed into nibbles, as `(major, minor)`."""
    return packed >> 16, (packed >> 8) & 0xFF


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
