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
from collections.abc import Callable
from dataclasses import dataclass

import pytest
from repo_checks.expect import contains, equal, truth
from repo_checks.model import Repo
from repo_checks.platforms import (
    HOSTS,
    NAMING,
    PLATFORM_ID,
    SERVICE_MANAGERS,
    PlatformError,
    descriptor,
    host,
    host_os_version,
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
    #: The operating-system version a host of this platform reports, and the
    #: wheel platform tag that host's build then carries. The Windows tags carry
    #: no version component at all, which is why the version is unused there.
    os_version: tuple[int, int]
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

#: The two the supported-platform list names today, which is the pair every
#: other assertion here has to leave exactly as it found.
CARRIED_TODAY = ("linux-x86_64", "linux-aarch64")


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
        found.wheel_tag(row.os_version),
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
    [row.id for row in TABLE if row.id not in CARRIED_TODAY],
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
    equal(host_os_version(), None, describing="the version a Windows wheel tag carries")
    equal(found.wheel_tag(host_os_version()), "win_arm64", describing="its wheel platform tag")


def test_a_host_the_list_does_not_name_is_refused_by_name(
    committed: Repo, monkeypatch: pytest.MonkeyPatch
) -> None:
    """A Windows host of a repository that supports none is told so, not guessed at."""
    monkeypatch.delattr(os, "uname", raising=False)
    monkeypatch.setattr(host_platform, "system", lambda: "Windows")
    monkeypatch.setattr(host_platform, "machine", lambda: "ARM64")

    with pytest.raises(PlatformError) as refusal:
        host(committed)

    contains(str(refusal.value), "Windows/ARM64", describing="the refusal")


def test_a_macos_host_takes_its_wheel_tag_version_from_the_host(
    tree: Callable[[], Tree], monkeypatch: pytest.MonkeyPatch
) -> None:
    """The rule is the same one the glibc tag follows: state what it was built against."""
    repo = supporting(tree(), TABLE[2])
    monkeypatch.setattr(host_platform, "system", lambda: "Darwin")
    monkeypatch.setattr(host_platform, "machine", lambda: "arm64")
    monkeypatch.setattr(host_platform, "mac_ver", lambda: ("15.4", ("", "", ""), "arm64"))

    found = host(repo)

    equal(host_os_version(), (15, 4), describing="the system release this host reports")
    equal(found.wheel_tag(host_os_version()), "macosx_15_4_arm64", describing="its wheel tag")


def test_a_host_reporting_no_version_is_refused(monkeypatch: pytest.MonkeyPatch) -> None:
    """A tag stating a version it did not read is a wheel that installs and cannot run."""
    monkeypatch.setattr(host_platform, "system", lambda: "Linux")
    monkeypatch.setattr(os, "confstr", lambda _name: None)

    with pytest.raises(PlatformError, match="C library"):
        host_os_version()


def test_a_versioned_tag_asked_for_without_a_version_is_refused(committed: Repo) -> None:
    """Linux and macOS tags state a version; asking for one with none is a refusal."""
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


def test_this_hosts_own_version_is_read_off_the_host(committed: Repo) -> None:
    """The rule is that the tag states what the build was actually made against."""
    version = host_os_version()

    truth(version is not None, describing="this host to report the version its wheels state")
    contains(
        descriptor(committed, host(committed).id).wheel_tag(version),
        "manylinux_",
        describing="this Linux host's wheel platform tag",
    )


def test_a_host_reporting_a_version_that_is_not_one_is_refused(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    """`macos-` and a word is a tag no installer can compare; it is refused instead."""
    monkeypatch.setattr(host_platform, "system", lambda: "Darwin")
    monkeypatch.setattr(host_platform, "mac_ver", lambda: ("", ("", "", ""), "arm64"))

    with pytest.raises(PlatformError, match="system release"):
        host_os_version()


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
        contains(
            SERVICE_MANAGERS,
            platform.service_manager,
            describing="the service managers this repository has a name for",
        )


def test_a_host_reporting_a_minor_version_that_is_not_a_number_is_refused(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    """Reading it as zero would put a version on a wheel that nothing reported."""
    monkeypatch.setattr(host_platform, "system", lambda: "Darwin")
    monkeypatch.setattr(host_platform, "mac_ver", lambda: ("15.beta", ("", "", ""), "arm64"))

    with pytest.raises(PlatformError, match=re.escape("15.beta")):
        host_os_version()


def test_a_host_reporting_a_version_with_no_minor_reads_it_as_zero(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    """A platform that writes `15` means `15.0`, which is a version and not a defect."""
    monkeypatch.setattr(host_platform, "system", lambda: "Darwin")
    monkeypatch.setattr(host_platform, "mac_ver", lambda: ("15", ("", "", ""), "arm64"))

    equal(host_os_version(), (15, 0), describing="a release written with no minor component")
