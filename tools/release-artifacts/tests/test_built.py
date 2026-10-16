"""A finished build, proven before it is published: what the release's macOS cell runs.

The release workflow's macOS cell runs `just build-artifacts` and then `just
prove-built-artifacts` over the `dist/` it is about to upload. These journeys
drive the same two commands of the same tool — `build-all` carrying a real
supervisor, then `prove-built` over what it wrote — with nothing published and
no registry reached: every route and client is installed from its own file in
`dist/` and run, a client by its own smoke check against that supervisor.

What a Linux host cannot do is execute another platform's program, so the other
platforms are held at the contract instead: the names every platform's cell
owes, and the processor a program's own header declares.
"""

from __future__ import annotations

import io
import json
import shutil
import struct
import tarfile
from pathlib import Path

import pytest
from release_artifacts import targets
from release_artifacts.__main__ import main
from release_artifacts.build import CHECKSUMS, release_route
from release_artifacts.built import (
    CLIENTS,
    ROUTES,
    activation,
    expected,
    label_of,
    processor_of,
)
from release_artifacts.installing import InstallError
from repo_checks import platforms
from repo_checks.expect import contains, equal, passing, truth
from repo_checks.model import Repo
from repo_checks.shell import run

REPO_ROOT = Path(__file__).resolve().parents[3]

#: How long the one program build these share is given.
BUILD_TIMEOUT_SECONDS = 2400


@pytest.fixture(scope="module")
def supervisor() -> Path:
    """The real program, built once: what the finished build carries and the clients reach."""
    repo = Repo(REPO_ROOT)
    built = repo.root / "target" / "debug" / platforms.host(repo).program
    if not built.is_file():
        passing(
            run(
                ["cargo", "build", "--locked", "-p", "printobserver"],
                cwd=repo.root,
                timeout=BUILD_TIMEOUT_SECONDS,
            ),
            describing="building the program the finished build carries",
        )
    return built


@pytest.fixture(scope="module")
def finished(tmp_path_factory: pytest.TempPathFactory, supervisor: Path) -> Path:
    """What `build-all` writes into `dist/` carrying the real program, built once."""
    dist = tmp_path_factory.mktemp("finished") / "dist"
    equal(
        main(
            [
                "build-all",
                "--root",
                str(REPO_ROOT),
                "--into",
                str(dist),
                "--binary",
                str(supervisor),
            ]
        ),
        0,
        describing="building every artifact carrying the real program",
    )
    return dist


def _prove(dist: Path, into: Path, binary: Path) -> int:
    return main(
        [
            "prove-built",
            "--root",
            str(REPO_ROOT),
            "--dist",
            str(dist),
            "--into",
            str(into),
            "--binary",
            str(binary),
        ]
    )


def test_a_finished_build_is_held_to_its_contract_then_installed_and_run(
    finished: Path,
    supervisor: Path,
    tmp_path: Path,
    capsys: pytest.CaptureFixture[str],
) -> None:
    """Every route and client in `dist/` installs from its own file and runs.

    The routes report the workspace's version from where they were installed;
    each client's smoke check reaches the supervisor; and `dist/` holds
    afterwards exactly what it held before, because it is what is uploaded.
    """
    repo = Repo(REPO_ROOT)
    host = platforms.host(repo)
    if host.no_route_proof:
        pytest.skip(host.no_route_proof)
    before = sorted(path.name for path in finished.iterdir())
    capsys.readouterr()

    equal(_prove(finished, tmp_path / "proof", supervisor), 0, describing="prove-built's exit")

    printed = capsys.readouterr().out
    equal(len(printed.splitlines()), 1, describing="what a passing proof prints, in lines")
    said = printed.removeprefix("prove-built: ").replace("; ", "\n")
    version = targets.workspace(REPO_ROOT)["version"]
    family, processor = host.id.split("-", 1)
    contains(said, f"{host.id}: program ", describing="the contract's first line")
    contains(said, f"is a {family} {processor} executable", describing="the processor it read")
    contains(said, "each carry program", describing="the binary reuse across the route forms")
    contains(said, f"{CHECKSUMS} vouches for {host.asset}", describing="the checksum line")
    for target in ROUTES:
        contains(said, f"{target}: printobserver {version}", describing=f"what {target} ran")
    for target in CLIENTS:
        line = next((one for one in said.splitlines() if one.startswith(f"{target}: ")), "")
        contains(line, f"smoke: contract {version}", describing=f"what {target}'s check said")
    equal(
        sorted(path.name for path in finished.iterdir()),
        before,
        describing="what dist/ holds after the proof",
    )


def test_a_route_carrying_another_program_than_the_build_is_refused_naming_it(
    finished: Path,
    supervisor: Path,
    tmp_path: Path,
    capsys: pytest.CaptureFixture[str],
    stand_in: Path,
) -> None:
    """An archive rebuilt around a different program fails before anything is installed."""
    repo = Repo(REPO_ROOT)
    dist = Path(shutil.copytree(finished, tmp_path / "dist"))
    release_route(repo, targets.named(REPO_ROOT, "release:printobserver"), dist, stand_in)
    capsys.readouterr()

    equal(_prove(dist, tmp_path / "proof", supervisor), 1, describing="prove-built's exit")

    contains(
        capsys.readouterr().err,
        f"{platforms.host(repo).asset} carries a program other than the one this cell built",
        describing="the refusal",
    )
    truth(not (tmp_path / "proof").exists(), describing="nothing was installed")


def test_a_build_missing_an_artifact_is_refused_naming_what_is_owed(
    finished: Path,
    supervisor: Path,
    tmp_path: Path,
    capsys: pytest.CaptureFixture[str],
) -> None:
    """A `dist/` lacking a route's wheel names the target and the file it owes."""
    dist = Path(shutil.copytree(finished, tmp_path / "dist"))
    wheel = next(path for path in dist.iterdir() if path.name.startswith("printobserver_cli-"))
    wheel.unlink()
    capsys.readouterr()

    equal(_prove(dist, tmp_path / "proof", supervisor), 1, describing="prove-built's exit")

    contains(
        capsys.readouterr().err,
        f"pypi:printobserver-cli: {wheel.name}",
        describing="the refusal",
    )


def test_a_program_for_another_platform_is_refused_before_anything_is_installed(
    finished: Path,
    tmp_path: Path,
    capsys: pytest.CaptureFixture[str],
) -> None:
    """A cell handed another platform's program says whose it is and installs nothing."""
    other = tmp_path / "printobserver"
    other.write_bytes(MACHO_ARM64)
    capsys.readouterr()

    equal(_prove(finished, tmp_path / "proof", other), 1, describing="prove-built's exit")

    contains(
        capsys.readouterr().err,
        f"is a macos aarch64 program, and this cell publishes {platforms.host(Repo(REPO_ROOT)).id}",
        describing="the refusal",
    )
    truth(not (tmp_path / "proof").exists(), describing="nothing was installed")


@pytest.mark.parametrize(
    ("field", "value"),
    [("cpu", ["mips"]), ("os", ["aix"]), ("version", "0.0.0"), ("printobserverPlatform", "none")],
)
def test_a_per_platform_package_declaring_another_platform_is_refused(
    finished: Path,
    supervisor: Path,
    tmp_path: Path,
    capsys: pytest.CaptureFixture[str],
    field: str,
    value: object,
) -> None:
    """The package npm selects by `os` and `cpu` must declare this platform, at this version."""
    dist = Path(shutil.copytree(finished, tmp_path / "dist"))
    package = dist / Path(f"printobserver-cli-{platforms.host(Repo(REPO_ROOT)).npm_selector}-")
    package = next(path for path in dist.iterdir() if path.name.startswith(package.name))
    rewritten = tmp_path / "rewritten.tgz"
    with tarfile.open(package, "r:gz") as source, tarfile.open(rewritten, "w:gz") as target:
        for member in source.getmembers():
            handle = source.extractfile(member)
            content = handle.read() if handle is not None else b""
            if member.name == "package/package.json":
                manifest = json.loads(content)
                manifest[field] = value
                content = json.dumps(manifest).encode()
                member.size = len(content)
            target.addfile(member, io.BytesIO(content))
    shutil.move(rewritten, package)
    capsys.readouterr()

    equal(_prove(dist, tmp_path / "proof", supervisor), 1, describing="prove-built's exit")

    contains(capsys.readouterr().err, f"{package.name} declares os", describing="the refusal")
    truth(not (tmp_path / "proof").exists(), describing="nothing was installed")


def test_a_checksum_file_not_vouching_for_the_archive_is_refused(
    finished: Path,
    supervisor: Path,
    tmp_path: Path,
    capsys: pytest.CaptureFixture[str],
) -> None:
    """The checksum file the install script verifies by must state the archive's digest."""
    dist = Path(shutil.copytree(finished, tmp_path / "dist"))
    asset = platforms.host(Repo(REPO_ROOT)).asset
    (dist / CHECKSUMS).write_text(f"{'0' * 64}  {asset}\n", encoding="utf-8")
    capsys.readouterr()

    equal(_prove(dist, tmp_path / "proof", supervisor), 1, describing="prove-built's exit")

    contains(capsys.readouterr().err, f"does not vouch for {asset}", describing="the refusal")


def test_every_install_platform_owes_its_own_named_artifacts() -> None:
    """The contract names each platform's route files apart and its clients alike.

    A release is five cells' `dist/` merged, so two platforms owing one name
    would be one platform's artifact replacing another's.
    """
    repo = Repo(REPO_ROOT)
    owed = {
        platform.id: expected(repo, platform, (11, 0) if platform.naming.wheel_versioned else None)
        for platform in platforms.install_platforms(repo)
    }
    equal(
        sorted(owed),
        sorted(platform.id for platform in platforms.install_platforms(repo)),
        describing="the platforms the contract names",
    )
    for platform_id, names in owed.items():
        equal(
            sorted(names),
            sorted([*ROUTES, *CLIENTS]),
            describing=f"the targets {platform_id} owes",
        )
    per_platform = [
        (names["pypi:printobserver-cli"][0], names["npm:printobserver-cli"][0])
        for names in owed.values()
    ]
    equal(
        len({name for pair in per_platform for name in pair}),
        2 * len(owed),
        describing="every platform's route wheel and package named apart",
    )
    equal(
        len({names["release:printobserver"][0] for names in owed.values()}),
        len(owed),
        describing="every platform's archive named apart",
    )
    for target in (*CLIENTS, "npm:printobserver-cli"):
        shared = {names[target][-1] for names in owed.values()}
        equal(len(shared), 1, describing=f"{target}'s platform-independent file")


#: Hand-written executable headers: the first bytes each format's loader reads
#: the processor off, which is all the contract asks of a program it cannot run.
MACHO_ARM64 = b"\xcf\xfa\xed\xfe" + struct.pack("<I", 0x0100000C) + bytes(24)
PE_ARM64 = b"MZ" + bytes(0x3A) + struct.pack("<I", 0x40) + b"PE\0\0" + struct.pack("<H", 0xAA64)


@pytest.mark.parametrize(
    ("header", "family", "processor"),
    [(MACHO_ARM64, "macos", "aarch64"), (PE_ARM64, "windows", "aarch64")],
)
def test_a_program_for_another_platform_is_read_off_its_own_header(
    header: bytes, family: str, processor: str
) -> None:
    """What a Mach-O or PE program says it runs on, read without running it."""
    equal(processor_of(header), (family, processor), describing="the header's answer")


def test_the_built_program_names_this_hosts_processor(supervisor: Path) -> None:
    """The program cargo built for this host is read as this host's platform."""
    host = platforms.host(Repo(REPO_ROOT))
    equal(
        processor_of(supervisor.read_bytes()),
        tuple(host.id.split("-", 1)),
        describing="the built program's header",
    )


def test_bytes_that_are_no_program_are_refused() -> None:
    """A file with no executable header is not read as any platform's."""
    with pytest.raises(InstallError, match="no ELF, Mach-O or PE header"):
        processor_of(b"#!/bin/sh\n")


@pytest.mark.parametrize(
    "truncated",
    [b"\x7fELF\x02\x01", MACHO_ARM64[:6], PE_ARM64[:0x3E], b"MZ" + bytes(0x3A) + b"\xff\xff"],
)
def test_a_header_that_ends_before_its_processor_is_refused(truncated: bytes) -> None:
    """A truncated program is a refusal naming where it ends, not an unpacking error."""
    with pytest.raises(InstallError, match="header ends before offset"):
        processor_of(truncated)


def test_the_service_proof_runs_the_launchd_command_agents_md_states() -> None:
    """The activation it runs is the install path's own, as root, naming the daemon's label."""
    words = activation(Repo(REPO_ROOT))
    equal(words[:2], ["sudo", "launchctl"], describing="the activation command's program")
    equal(
        label_of(words), "io.github.nickderobertis.printobserver", describing="the label it loads"
    )


def test_the_service_proof_refuses_a_host_launchd_does_not_run(
    supervisor: Path, capsys: pytest.CaptureFixture[str]
) -> None:
    """Off macOS it installs nothing, and says what the host answered in launchd's place."""
    host = platforms.host(Repo(REPO_ROOT))
    if host.service_manager == "launchd":
        pytest.skip("this host is a launchd host, where the proof runs for real")
    capsys.readouterr()
    equal(
        main(["prove-service", "--root", str(REPO_ROOT), "--binary", str(supervisor)]),
        1,
        describing="prove-service's exit",
    )
    contains(
        capsys.readouterr().err,
        "the installer writes launchd's property list on `Darwin` alone",
        describing="the refusal",
    )
