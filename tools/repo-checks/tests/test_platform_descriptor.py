"""One descriptor hands a consumer every fact this repository knows about a platform.

The table below is the plan's own frozen contract, restated here so that the
module is asserted against something other than itself. Each of the six
identifiers is made supported in turn — by writing its entry into a real copy of
`AGENTS.md`'s supported-platform list — and all eight facts are then read back
through the module every consumer reads them through. A descriptor complete for
the Linux identifiers and thin for the macOS or the Windows ones fails here,
which is the failure this node exists to make impossible.
"""

from __future__ import annotations

import os
import platform as host_platform
import re
import struct
import sys
from collections.abc import Callable
from dataclasses import dataclass
from pathlib import Path

import pytest
from repo_checks.expect import contains, equal, truth
from repo_checks.model import Repo
from repo_checks.platforms import (
    HOSTS,
    NAMING,
    PLATFORM_ID,
    PlatformError,
    ServiceManager,
    descriptor,
    host,
    host_baseline,
    install_platforms,
    supported,
)
from treecopy import Tree

BEGIN = "[//]: # (BEGIN supported-platforms)"
END = "[//]: # (END supported-platforms)"


@dataclass(frozen=True, slots=True)
class Row:
    """One row of the plan's frozen platform table."""

    id: str
    runner: str
    target: str
    service_manager: str
    npm: tuple[str, str]
    program: str
    asset: str
    #: The baseline a host of this platform reports — the oldest host a build
    #: there claims to run on, in that platform's own terms — and the wheel
    #: platform tag that host's build then carries. The Windows tags carry no
    #: version component at all, which is why the baseline is unused there.
    baseline: tuple[int, int]
    wheel_tag: str


TABLE = (
    Row(
        "linux-x86_64",
        "ubuntu-24.04",
        "x86_64-unknown-linux-gnu",
        "systemd",
        ("linux", "x64"),
        "printobserver",
        "printobserver-linux-x86_64.tar.gz",
        (2, 39),
        "manylinux_2_39_x86_64",
    ),
    Row(
        "linux-aarch64",
        "ubuntu-24.04-arm",
        "aarch64-unknown-linux-gnu",
        "systemd",
        ("linux", "arm64"),
        "printobserver",
        "printobserver-linux-aarch64.tar.gz",
        (2, 39),
        "manylinux_2_39_aarch64",
    ),
    Row(
        "macos-aarch64",
        "macos-15",
        "aarch64-apple-darwin",
        "launchd",
        ("darwin", "arm64"),
        "printobserver",
        "printobserver-macos-aarch64.tar.gz",
        (15, 0),
        "macosx_15_0_arm64",
    ),
    Row(
        "macos-x86_64",
        "macos-15-intel",
        "x86_64-apple-darwin",
        "launchd",
        ("darwin", "x64"),
        "printobserver",
        "printobserver-macos-x86_64.tar.gz",
        (15, 0),
        "macosx_15_0_x86_64",
    ),
    Row(
        "windows-x86_64",
        "windows-2025",
        "x86_64-pc-windows-msvc",
        "windows-service",
        ("win32", "x64"),
        "printobserver.exe",
        "printobserver-windows-x86_64.tar.gz",
        (0, 0),
        "win_amd64",
    ),
    Row(
        "windows-aarch64",
        "windows-11-arm",
        "aarch64-pc-windows-msvc",
        "windows-service",
        ("win32", "arm64"),
        "printobserver.exe",
        "printobserver-windows-aarch64.tar.gz",
        (0, 0),
        "win_arm64",
    ),
)

#: The six the supported-platform list names today, which is the set every
#: other assertion here has to leave exactly as it found. The four macOS and
#: Windows entries answer `install path: no` while they are brought up.
CARRIED_TODAY = tuple(row.id for row in TABLE)

#: Identifiers shaped like platforms that the list does not carry.
NOT_CARRIED = ("linux-riscv64", "freebsd-x86_64")


def entry(row: Row, *, install_path: str = "yes") -> str:
    """One line of the supported-platform list, as that list spells one."""
    return (
        f"- `{row.id}` — runner `{row.runner}`, Rust target `{row.target}`, "
        f"service manager `{row.service_manager}`, install path: {install_path}\n"
    )


def supporting(copy: Tree, *rows: Row, install_path: str = "yes") -> Repo:
    """Make exactly `rows` the supported platforms of a real copy of this tree."""
    text = copy.read("AGENTS.md")
    start, end = text.index(BEGIN), text.index(END)
    listed = "".join(entry(row, install_path=install_path) for row in rows)
    copy.write("AGENTS.md", f"{text[:start]}{BEGIN}\n{listed}{text[end:]}")
    return copy.repo


@pytest.mark.parametrize("row", TABLE, ids=[row.id for row in TABLE])
def test_every_fact_of_every_identifier_reads_back_through_the_module(
    tree: Callable[[], Tree], row: Row
) -> None:
    """All eight facts, for each of the six, from the one object a consumer reads."""
    repo = supporting(tree(), row)

    found = descriptor(repo, row.id)

    equal(found.runner, row.runner, describing=f"{row.id}'s runner")
    equal(found.target, row.target, describing=f"{row.id}'s Rust target")
    equal(found.service_manager, row.service_manager, describing=f"{row.id}'s service manager")
    equal(found.npm, row.npm, describing=f"{row.id}'s npm selectors")
    equal(found.program, row.program, describing=f"{row.id}'s program file name")
    equal(found.asset, row.asset, describing=f"{row.id}'s release asset")
    equal(
        found.wheel_tag(row.baseline),
        row.wheel_tag,
        describing=f"{row.id}'s wheel platform tag",
    )
    equal(found.install_path, True, describing=f"{row.id}'s install-path answer")


def test_the_committed_list_states_the_table_for_the_platforms_it_carries(
    committed: Repo,
) -> None:
    """The three list-borne facts are the table's own, on the tree as it stands.

    Without this the parametrized walk above would prove only that the module
    hands back whatever the fixture wrote; this is what ties the two identifiers
    this repository actually supports to the same frozen contract.
    """
    by_id = {platform.id: platform for platform in supported(committed)}

    equal(sorted(by_id), sorted(CARRIED_TODAY), describing="the platforms the list carries")
    for row in TABLE:
        if row.id not in by_id:
            continue
        equal(by_id[row.id].runner, row.runner, describing=f"{row.id}'s runner")
        equal(by_id[row.id].target, row.target, describing=f"{row.id}'s Rust target")
        equal(
            by_id[row.id].service_manager,
            row.service_manager,
            describing=f"{row.id}'s service manager",
        )


@pytest.mark.parametrize(
    "identifier",
    NOT_CARRIED,
)
def test_an_identifier_the_list_does_not_carry_is_refused_by_name(
    committed: Repo, identifier: str
) -> None:
    """Naming a platform is not supporting it: the list is what decides."""
    with pytest.raises(PlatformError) as refusal:
        descriptor(committed, identifier)

    contains(str(refusal.value), identifier, describing="the refusal")
    contains(str(refusal.value), "supported-platform list", describing="the refusal")


def test_a_platform_the_list_carries_and_nothing_names_is_refused_by_name(
    tree: Callable[[], Tree],
) -> None:
    """A list that gained a platform no artifact can name says so, rather than defaulting."""
    repo = supporting(
        tree(),
        Row(
            "linux-riscv64",
            "ubuntu-24.04-riscv",
            "riscv64gc-unknown-linux-gnu",
            "systemd",
            ("linux", "riscv64"),
            "printobserver",
            "printobserver-linux-riscv64.tar.gz",
            (2, 39),
            "manylinux_2_39_riscv64",
        ),
    )

    found = descriptor(repo, "linux-riscv64")

    equal(found.runner, "ubuntu-24.04-riscv", describing="the list-borne runner")
    with pytest.raises(PlatformError, match="linux-riscv64"):
        _ = found.npm
    with pytest.raises(PlatformError, match="NAMING"):
        _ = found.asset


def test_the_install_path_answer_selects_the_platforms_every_install_tier_carries(
    tree: Callable[[], Tree],
) -> None:
    """`install path: no` takes a platform out of the set the install tiers are held to."""
    copy = tree()
    text = copy.read("AGENTS.md")
    start, end = text.index(BEGIN), text.index(END)
    listed = entry(TABLE[0]) + entry(TABLE[2], install_path="no — its runner has no OctoPrint yet")
    copy.write("AGENTS.md", f"{text[:start]}{BEGIN}\n{listed}{text[end:]}")

    equal(
        [platform.id for platform in install_platforms(copy.repo)],
        ["linux-x86_64"],
        describing="the platforms the end-user install path targets",
    )
    equal(
        descriptor(copy.repo, "macos-aarch64").install_path_reason,
        "its runner has no OctoPrint yet",
        describing="the reason recorded for the opt-out",
    )


def test_the_host_this_dispatch_runs_on_is_answered(committed: Repo) -> None:
    """The real host, through the real module, on the tree as it stands."""
    found = host(committed)

    contains(CARRIED_TODAY, found.id, describing="the platforms this host could be")
    equal(found.runner, descriptor(committed, found.id).runner, describing="the host's runner")


def test_a_windows_host_is_answered_with_no_posix_only_interface_present(
    tree: Callable[[], Tree], monkeypatch: pytest.MonkeyPatch
) -> None:
    """A host on a machine with no `os.uname` is answered rather than crashed on.

    `os.uname` does not exist on Windows, and a tool that cannot say which
    platform it is on cannot build for it. Proven by taking that interface away
    and asking on a host the list carries: code that reached for it would raise
    `AttributeError` here instead of answering.
    """
    repo = supporting(tree(), TABLE[5])
    monkeypatch.delattr(os, "uname", raising=False)
    monkeypatch.setattr(host_platform, "system", lambda: "Windows")
    monkeypatch.setattr(host_platform, "machine", lambda: "ARM64")

    found = host(repo)

    truth(not hasattr(os, "uname"), describing="the fixture to have taken `os.uname` away")
    equal(found.id, "windows-aarch64", describing="the platform this host is")
    equal(host_baseline(), None, describing="the baseline a Windows wheel tag carries")
    equal(found.wheel_tag(host_baseline()), "win_arm64", describing="its wheel platform tag")


def test_a_host_the_list_does_not_name_is_refused_by_name(
    committed: Repo, monkeypatch: pytest.MonkeyPatch
) -> None:
    """A host of a family the list names nothing of is told so, not guessed at."""
    monkeypatch.setattr(host_platform, "system", lambda: "FreeBSD")
    monkeypatch.setattr(host_platform, "machine", lambda: "amd64")

    with pytest.raises(PlatformError) as refusal:
        host(committed)

    contains(str(refusal.value), "FreeBSD/amd64", describing="the refusal")


#: The Mach-O processor types a synthesized program is written for, spelled as
#: the format's own header spells them rather than read from the module.
ARM64 = 0x0100000C
X86_64 = 0x01000007


def build_version(major: int, minor: int, *, platform: int = 1) -> bytes:
    """An `LC_BUILD_VERSION` load command (0x32) recording `major.minor` as `minos`."""
    return struct.pack("<IIIIII", 0x32, 24, platform, major << 16 | minor << 8, 15 << 16, 0)


def version_min(major: int, minor: int) -> bytes:
    """An `LC_VERSION_MIN_MACOSX` load command (0x24), as older linkers wrote."""
    return struct.pack("<IIII", 0x24, 16, major << 16 | minor << 8, 0)


#: An `LC_UUID` load command, which says nothing about a release: a reader has
#: to walk past it to find the one that does.
UUID = struct.pack("<II", 0x1B, 24) + bytes(16)


def thin(cpu: int, *commands: bytes) -> bytes:
    """A little-endian 64-bit Mach-O executable header followed by `commands`."""
    body = b"".join(commands)
    return struct.pack("<IiiIIIII", 0xFEEDFACF, cpu, 0, 2, len(commands), len(body), 0, 0) + body


def universal(*slices: tuple[int, bytes], wide: bool = False) -> bytes:
    """A universal file carrying each `(cpu, image)` slice, as `lipo` writes one."""
    header = struct.pack(">II", 0xCAFEBABF if wide else 0xCAFEBABE, len(slices))
    entry_size = 32 if wide else 20
    offset = len(header) + entry_size * len(slices)
    entries, images = b"", b""
    for cpu, image in slices:
        if wide:
            entries += struct.pack(">iiQQII", cpu, 0, offset + len(images), len(image), 0, 0)
        else:
            entries += struct.pack(">iiIII", cpu, 0, offset + len(images), len(image), 0)
        images += image
    return header + entries + images


def macos_host(monkeypatch: pytest.MonkeyPatch, machine: str) -> None:
    """Make this interpreter report a macOS host of `machine`, on a newer release.

    The release is deliberately far newer than any program below was built for:
    a tag read off the host rather than off the program would carry it.
    """
    monkeypatch.setattr(host_platform, "system", lambda: "Darwin")
    monkeypatch.setattr(host_platform, "machine", lambda: machine)
    monkeypatch.setattr(host_platform, "mac_ver", lambda: ("15.7", ("", "", ""), machine))


@pytest.mark.parametrize(
    ("row", "machine", "image", "baseline", "tag"),
    [
        (TABLE[2], "arm64", thin(ARM64, UUID, build_version(11, 0)), (11, 0), "macosx_11_0_arm64"),
        (
            TABLE[3],
            "x86_64",
            thin(X86_64, UUID, build_version(10, 12)),
            (10, 12),
            "macosx_10_12_x86_64",
        ),
        (TABLE[2], "arm64", thin(ARM64, build_version(14, 2)), (14, 2), "macosx_15_0_arm64"),
        (TABLE[3], "x86_64", thin(X86_64, version_min(10, 9)), (10, 9), "macosx_10_9_x86_64"),
        (
            TABLE[2],
            "arm64",
            universal(
                (X86_64, thin(X86_64, build_version(10, 12))),
                (ARM64, thin(ARM64, UUID, build_version(12, 0))),
            ),
            (12, 0),
            "macosx_12_0_arm64",
        ),
        (
            TABLE[3],
            "x86_64",
            universal(
                (ARM64, thin(ARM64, build_version(11, 0))),
                (X86_64, thin(X86_64, build_version(10, 13))),
                wide=True,
            ),
            (10, 13),
            "macosx_10_13_x86_64",
        ),
    ],
    ids=["arm64", "x86_64", "a-minor-rounds-up", "version-min", "universal", "universal-64"],
)
def test_a_macos_host_takes_its_wheel_tag_baseline_from_the_program(
    tree: Callable[[], Tree],
    monkeypatch: pytest.MonkeyPatch,
    tmp_path: Path,
    row: Row,
    machine: str,
    image: bytes,
    baseline: tuple[int, int],
    tag: str,
) -> None:
    """The tag states the floor the program was linked for, spelled as `pip` matches it.

    Not the host's release: the fixture's host reports 15.7, which is a tag no
    installer generates and a floor the program does not have. A minor release
    from macOS 11 on rounds up, so the wheel never claims an older host than
    the program needs.
    """
    repo = supporting(tree(), row)
    macos_host(monkeypatch, machine)
    program = tmp_path / "printobserver"
    program.write_bytes(image)

    found = host(repo)

    equal(host_baseline(program), baseline, describing="the minimum release the program records")
    equal(found.wheel_tag(host_baseline(program)), tag, describing="its wheel tag")


@pytest.mark.parametrize(
    ("image", "naming"),
    [
        (thin(ARM64, UUID), "records no minimum macOS release"),
        (thin(ARM64, build_version(17, 0, platform=2)), "records no minimum macOS release"),
        (universal((X86_64, thin(X86_64, build_version(10, 12)))), "records no minimum"),
        (b"#!/bin/sh\necho printobserver\n", "records no minimum macOS release"),
        (thin(ARM64, UUID)[:36], "is not a macOS program"),
        (b"", "is not a macOS program"),
    ],
    ids=["no-command", "not-macos", "no-slice-for-machine", "a-script", "truncated", "empty"],
)
def test_a_macos_program_recording_no_minimum_release_is_refused_naming_it(
    monkeypatch: pytest.MonkeyPatch, tmp_path: Path, image: bytes, naming: str
) -> None:
    """A tag stating a floor nothing recorded is a wheel that installs and cannot run."""
    macos_host(monkeypatch, "arm64")
    program = tmp_path / "printobserver"
    program.write_bytes(image)

    with pytest.raises(PlatformError) as refusal:
        host_baseline(program)

    contains(str(refusal.value), str(program), describing="the refusal")
    contains(str(refusal.value), naming, describing="the refusal")


def test_a_macos_baseline_asked_for_with_no_program_is_refused(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    """The host's own release is not a fallback: it is not what the program needs."""
    macos_host(monkeypatch, "arm64")

    with pytest.raises(PlatformError, match="no program was given"):
        host_baseline()


def test_a_linux_baseline_is_the_c_library_whatever_program_is_given(
    monkeypatch: pytest.MonkeyPatch, tmp_path: Path
) -> None:
    """A Linux wheel states the C library it was built against, as it always has."""
    monkeypatch.setattr(host_platform, "system", lambda: "Linux")
    monkeypatch.setattr(os, "confstr", lambda _name: "glibc 2.39")
    program = tmp_path / "printobserver"
    program.write_bytes(thin(ARM64, build_version(11, 0)))

    equal(host_baseline(program), (2, 39), describing="the C library this host reports")
    equal(host_baseline(), (2, 39), describing="the same, with no program given")


def test_a_host_reporting_no_baseline_is_refused(monkeypatch: pytest.MonkeyPatch) -> None:
    """A tag stating a version it did not read is a wheel that installs and cannot run."""
    monkeypatch.setattr(host_platform, "system", lambda: "Linux")
    monkeypatch.setattr(os, "confstr", lambda _name: None)

    with pytest.raises(PlatformError, match="C library"):
        host_baseline()


def test_a_versioned_tag_asked_for_without_a_baseline_is_refused(committed: Repo) -> None:
    """Linux and macOS tags state a baseline; asking for one with none is a refusal."""
    with pytest.raises(PlatformError, match="linux-x86_64"):
        descriptor(committed, "linux-x86_64").wheel_tag(None)


def test_a_line_in_the_list_that_is_not_an_entry_is_passed_over(
    tree: Callable[[], Tree],
) -> None:
    """A block carrying prose beside its entries still reads as the entries it has."""
    copy = tree()
    text = copy.read("AGENTS.md")
    start, end = text.index(BEGIN), text.index(END)
    listed = f"{entry(TABLE[0])}<!-- the second platform lands in a later change -->\n"
    copy.write("AGENTS.md", f"{text[:start]}{BEGIN}\n{listed}{text[end:]}")

    equal(
        [platform.id for platform in supported(copy.repo)],
        ["linux-x86_64"],
        describing="the platforms read out of a block carrying a comment",
    )


def test_a_host_no_identifier_is_known_for_is_refused_by_name(
    committed: Repo, monkeypatch: pytest.MonkeyPatch
) -> None:
    """A machine this repository has never heard of is named back, not guessed at."""
    monkeypatch.setattr(host_platform, "system", lambda: "SunOS")
    monkeypatch.setattr(host_platform, "machine", lambda: "sparc")

    with pytest.raises(PlatformError) as refusal:
        host(committed)

    contains(str(refusal.value), "SunOS/sparc", describing="the refusal")


def test_this_hosts_own_baseline_is_read_off_the_host(committed: Repo) -> None:
    """The rule is that the tag states what the build was actually made against.

    The program read is the interpreter running this suite, which is a real
    program of this host whatever the host is: on macOS it is what the baseline
    is read off, and on every other platform it is not read at all. The tag is
    held to this host's own descriptor rather than to one family's spelling.
    """
    found = descriptor(committed, host(committed).id)
    baseline = host_baseline(Path(sys.executable).resolve())

    equal(
        baseline is not None,
        found.naming.wheel_versioned,
        describing="this host to report a baseline exactly where its wheel tags state one",
    )
    tag = found.wheel_tag(baseline)
    truth(
        tag.startswith(f"{found.naming.wheel_family}_")
        and tag.endswith(f"_{found.naming.wheel_machine}"),
        describing=f"this host's wheel platform tag {tag!r} to be of `{found.id}`'s family",
    )


def test_a_host_reporting_a_baseline_that_is_not_a_version_is_refused(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    """`manylinux_` and a word is a tag no installer can compare; it is refused instead."""
    monkeypatch.setattr(host_platform, "system", lambda: "Linux")
    monkeypatch.setattr(os, "confstr", lambda _name: "glibc two")

    with pytest.raises(PlatformError, match="C library"):
        host_baseline()


def test_the_three_key_sets_of_the_module_cannot_drift_apart(committed: Repo) -> None:
    """Every identifier is named, reachable from a host, and of the documented shape.

    Three declarations in this module are about the same six identifiers, and
    each is written out rather than derived: what a platform is called, what a
    running interpreter reports for it, and what a platform identifier looks like
    to a document. This is what holds them to one another, and the parametrized
    walk above is what holds all three to the plan's table.
    """
    equal(
        sorted(set(HOSTS.values())),
        sorted(NAMING),
        describing="the identifiers a host is answered with, against the identifiers named",
    )
    for identifier in NAMING:
        truth(
            PLATFORM_ID.match(identifier) is not None,
            describing=f"`{identifier}` to be of the shape a document is read against",
        )
    for platform in supported(committed):
        truth(
            platform.service_manager in ServiceManager,
            describing=(
                f"`{platform.service_manager}` to be a service manager this repository "
                f"has a name for"
            ),
        )


def test_a_host_reporting_a_minor_that_is_not_a_number_is_refused(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    """Reading it as zero would put a version on a wheel that nothing reported."""
    monkeypatch.setattr(host_platform, "system", lambda: "Linux")
    monkeypatch.setattr(os, "confstr", lambda _name: "glibc 2.beta")

    with pytest.raises(PlatformError, match=re.escape("2.beta")):
        host_baseline()


def test_a_baseline_written_with_no_minor_reads_it_as_zero(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    """A platform that writes `3` means `3.0`, which is a version and not a defect."""
    monkeypatch.setattr(host_platform, "system", lambda: "Linux")
    monkeypatch.setattr(os, "confstr", lambda _name: "glibc 3")

    equal(host_baseline(), (3, 0), describing="a version written with no minor component")
