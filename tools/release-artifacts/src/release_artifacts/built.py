"""Proving the artifacts a release run built, before they are published.

`just build-artifacts` writes what one platform's release cell publishes into
`dist/`: the three clients and the three routes, every route carrying the one
`printobserver` program `cargo build --release` produced on that cell. This
module proves THOSE files — never a rebuild of them — on the host that built
them:

* the release contract: every artifact this platform owes is in `dist/` under
  the name its builder gives it, at the workspace's version; the wheel's tag,
  the per-platform package's `os`/`cpu`, and the archive's name are this
  platform's; the checksum file vouches for the archive; and the program inside
  the archive, the wheel and the per-platform package is byte for byte the
  program that was built, whose own header names this platform's processor;
* each route installed from its file the way its consumer installs it — the
  wheel with an installer, the two packages with a global install, the archive
  through the committed install script against a release staged from `dist/`
  itself — and the program it put on a path run;
* each client installed from its file and its own committed smoke check run
  against a supervisor that is the built program.

`prove_service` is the macOS half of the same lane: the committed installer
puts the built program in place as launchd's daemon, and the activation command
`AGENTS.md` states loads it, read back from launchd and booted out again.

What this cannot prove is another platform's program running, and it says so
rather than pretending: a Linux host does not execute a Darwin binary. Every
platform's release cell runs the same contract over its own `dist/`.
"""

from __future__ import annotations

import hashlib
import shlex
import shutil
import struct
import tarfile
import zipfile
from pathlib import Path

from repo_checks import install_path, platforms
from repo_checks.model import Repo
from repo_checks.shell import run

from release_artifacts import installing, packages, targets, wheels
from release_artifacts.build import (
    CHECKSUMS,
    CLIENT_REQUIRES_PYTHON,
    TYPED_CLASSIFIER,
    Built,
    assembled,
    distribution,
    manifest_of,
    node_package,
)
from release_artifacts.installing import InstallError

#: The registry each target is published to, by its kind of artifact.
ROUTES = ("pypi:printobserver-cli", "npm:printobserver-cli", "release:printobserver")
CLIENTS = ("crate:printobserver-sdk", "pypi:printobserver-sdk", "npm:@printobserver/sdk")


def expected(
    repo: Repo, platform: platforms.Platform, baseline: tuple[int, int] | None
) -> dict[str, tuple[str, ...]]:
    """The file names one platform's release cell writes for each assembled target.

    Composed from the same descriptions the builders write them under, so a
    builder that renames an artifact renames what is expected of it. `baseline`
    is the floor the program states, which the route wheel's tag carries where
    the platform's tag carries one.
    """
    version = targets.workspace(repo.root)["version"]
    named: dict[str, tuple[str, ...]] = {}
    for target in assembled(repo):
        match target.id:
            case "pypi:printobserver-cli":
                tag = f"{wheels.INTERPRETER}-{platform.wheel_tag(baseline)}"
                named[target.id] = (wheels.Wheel(distribution(repo, target), tag).file_name,)
            case "npm:printobserver-cli":
                named[target.id] = (
                    node_package(repo, target, platform.npm_package).file_name,
                    node_package(repo, target).file_name,
                )
            case "release:printobserver":
                named[target.id] = (platform.asset, CHECKSUMS)
            case "pypi:printobserver-sdk":
                client = distribution(repo, target, CLIENT_REQUIRES_PYTHON, (TYPED_CLASSIFIER,))
                named[target.id] = (wheels.Wheel(client, wheels.PURE_TAG).file_name,)
            case "npm:@printobserver/sdk":
                named[target.id] = (node_package(repo, target).file_name,)
            case "crate:printobserver-sdk":
                named[target.id] = (f"{target.name}-{version}.crate",)
            case _:
                msg = f"nothing here says what `{target.id}` is written as"
                raise InstallError(msg)
    return named


def baseline_of(platform: platforms.Platform, binary: Path) -> tuple[int, int] | None:
    """The floor a wheel built on this host carries for `binary`, where the tag has one."""
    if not platform.naming.wheel_versioned:
        return None
    return platforms.host_baseline(binary)


#: The processor each executable format's header names, by what the platform
#: identifier calls it: ELF's `e_machine`, Mach-O's `cputype`, PE's `Machine`.
ELF_MACHINES = {0x3E: "x86_64", 0xB7: "aarch64"}
MACHO_CPUS = {0x01000007: "x86_64", 0x0100000C: "aarch64"}
PE_MACHINES = {0x8664: "x86_64", 0xAA64: "aarch64"}
MACHO_MAGIC = b"\xcf\xfa\xed\xfe"
PE_HEADER_OFFSET = 0x3C


def processor_of(program: bytes) -> tuple[str, str]:
    """The family of executable and the processor one program's own header names.

    Raises:
        InstallError: If the bytes are no executable this can read a processor off.
    """
    if program[:4] == b"\x7fELF":
        return "linux", ELF_MACHINES.get(struct.unpack_from("<H", program, 18)[0], "unknown")
    if program[:4] == MACHO_MAGIC:
        return "macos", MACHO_CPUS.get(struct.unpack_from("<I", program, 4)[0], "unknown")
    if program[:2] == b"MZ" and len(program) > PE_HEADER_OFFSET + 4:
        at = struct.unpack_from("<I", program, PE_HEADER_OFFSET)[0]
        if program[at : at + 4] == b"PE\0\0":
            machine = struct.unpack_from("<H", program, at + 4)[0]
            return "windows", PE_MACHINES.get(machine, "unknown")
    msg = "the program carries no ELF, Mach-O or PE header to read its processor off"
    raise InstallError(msg)


def _carried(path: Path, member: str) -> bytes:
    """One member of a built archive — a tarball or a wheel — by its path inside it.

    Raises:
        InstallError: If the archive carries no such member.
    """
    if path.name.endswith(".whl"):
        with zipfile.ZipFile(path) as wheel:
            try:
                return wheel.read(member)
            except KeyError as absent:
                msg = f"{path.name} carries no {member}"
                raise InstallError(msg) from absent
    with tarfile.open(path, "r:gz") as archive:
        try:
            handle = archive.extractfile(member)
        except KeyError as absent:
            msg = f"{path.name} carries no {member}"
            raise InstallError(msg) from absent
        if handle is None:
            msg = f"{path.name} carries {member} as something other than a file"
            raise InstallError(msg)
        return handle.read()


def contract(repo: Repo, dist: Path, binary: Path) -> list[str]:
    """Hold one platform's `dist/` to the release contract, saying what held.

    Raises:
        InstallError: Naming the first thing that does not hold.
    """
    platform = platforms.host(repo)
    version = targets.workspace(repo.root)["version"]
    names = expected(repo, platform, baseline_of(platform, binary))
    missing = [
        f"{target}: {name}"
        for target, owed in names.items()
        for name in owed
        if not (dist / name).is_file()
    ]
    if missing:
        msg = f"{dist} lacks what {platform.id} publishes: {', '.join(missing)}"
        raise InstallError(msg)
    built = binary.read_bytes()
    digest = hashlib.sha256(built).hexdigest()
    family, processor = processor_of(built)
    owed_family, owed_processor = platform.id.split("-", 1)
    if (family, processor) != (owed_family, owed_processor):
        msg = (
            f"{binary} is a {family} {processor} program, and this cell publishes "
            f"{platform.id}'s artifacts"
        )
        raise InstallError(msg)
    said = [f"{platform.id}: program {digest[:16]} is a {family} {processor} executable"]

    wheel = dist / names["pypi:printobserver-cli"][0]
    route = distribution(repo, targets.named(repo.root, "pypi:printobserver-cli"))
    script = f"{route.normalized}-{version}.data/scripts/{platform.program}"
    per_platform, _launcher = (dist / name for name in names["npm:printobserver-cli"])
    archive = dist / platform.asset
    carried = {
        archive.name: _carried(archive, platform.program),
        wheel.name: _carried(wheel, script),
        per_platform.name: _carried(
            per_platform, f"{packages.PACKAGE_ROOT}/bin/{platform.program}"
        ),
    }
    for name, content in carried.items():
        if hashlib.sha256(content).hexdigest() != digest:
            msg = f"{name} carries a program other than the one this cell built ({digest[:16]})"
            raise InstallError(msg)
    said.append(f"{platform.id}: {', '.join(carried)} each carry program {digest[:16]}")

    manifest = manifest_of(per_platform)
    owed_os, owed_cpu = platform.npm
    if (
        manifest.get("os") != [owed_os]
        or manifest.get("cpu") != [owed_cpu]
        or manifest.get("version") != version
        or manifest.get("printobserverPlatform") != platform.id
    ):
        msg = (
            f"{per_platform.name} declares os {manifest.get('os')}, cpu {manifest.get('cpu')}, "
            f"version {manifest.get('version')} and platform "
            f"{manifest.get('printobserverPlatform')}; {platform.id} at {version} owes "
            f"[{owed_os}], [{owed_cpu}], {version} and {platform.id}"
        )
        raise InstallError(msg)
    sums = (dist / CHECKSUMS).read_text(encoding="utf-8").split()
    vouched = dict(zip(sums[1::2], sums[0::2], strict=True))
    if vouched.get(archive.name) != packages.digest_of(archive):
        msg = f"{CHECKSUMS} does not vouch for {archive.name} as it was built"
        raise InstallError(msg)
    said.append(
        f"{platform.id}: {wheel.name}, {per_platform.name} and {archive.name} are tagged for "
        f"{platform.id} at {version}, and {CHECKSUMS} vouches for {archive.name}"
    )
    return said


def _taken(repo: Repo, target: str, paths: tuple[Path, ...], into: Path) -> installing.Installed:
    """One target's own files, taken the way its consumer takes them."""
    built = Built(target, paths)
    into.mkdir(parents=True, exist_ok=True)
    if target != "release:printobserver":
        return installing.INSTALLERS[target](repo, built, into)
    # The install script fetches a release in the shape the forge serves; this
    # stages one out of `dist/` itself, so what it downloads and verifies is
    # the archive and checksum file this cell is about to publish.
    version = targets.workspace(repo.root)["version"]
    staged = into / "release"
    for directory in (staged / "download" / f"v{version}", staged / "latest" / "download"):
        directory.mkdir(parents=True, exist_ok=True)
        for path in paths:
            shutil.copy2(path, directory / path.name)
    directory = into / "env" / installing.SCRIPT_DIRECTORY
    installing.ran(
        installing.install_script_argv(repo, version="", into=directory),
        cwd=into,
        env=installing.without_rust({"PRINTOBSERVER_RELEASE_BASE": str(staged)}),
        describing="running the committed install script",
    )
    return installing.Installed(target, into / "env", directory / platforms.host(repo).program, "")


def prove_built(repo: Repo, dist: Path, into: Path, binary: Path) -> list[str]:
    """Hold `dist/` to the contract, then install and run every artifact in it.

    Raises:
        InstallError: Naming the first artifact that does not install or run.
    """
    said = contract(repo, dist, binary)
    host = platforms.host(repo)
    names = expected(repo, host, baseline_of(host, binary))
    for target in (*ROUTES, *CLIENTS):
        paths = tuple(dist / name for name in names[target])
        slug = target.replace(":", "-").replace("@", "").replace("/", "-")
        taken = _taken(repo, target, paths, into / slug)
        if taken.program is not None:
            proven = installing.prove_route(repo, taken)
        else:
            proven = installing.prove_client(repo, taken, binary)
        said.append(f"{target}: {' / '.join(line for line in proven.splitlines() if line)}")
    return said


#: The service manager `prove_service` drives, and the one the host must be.
LAUNCHD = "launchd"


def activation(repo: Repo) -> list[str]:
    """The launchd activation command `AGENTS.md`'s install path states, as words.

    Raises:
        InstallError: If the section states no command for launchd's pair.
    """
    path = install_path.parse(repo.agents_md)
    pair = path.commands_for(LAUNCHD)
    if len(pair) == len(("install", "activate")):
        return shlex.split(pair[1])
    msg = "AGENTS.md's install path states no launchd activation command"
    raise InstallError(msg)


def label_of(words: list[str]) -> str:
    """The launchd label a `launchctl bootstrap system <plist>` command loads."""
    return Path(words[-1]).name.removesuffix(".plist")


def prove_service(repo: Repo, binary: Path) -> list[str]:
    """Install the built program as launchd's daemon, load it, read it back, unload it.

    Every command runs through a password-free `sudo`, as the hosted macOS
    runner gives one; a `sudo` that would ask is a refusal rather than a hang.

    Raises:
        InstallError: If this host is not a launchd host, or any step fails.
    """
    platform = platforms.host(repo)
    if platform.service_manager != LAUNCHD:
        msg = (
            f"this proof installs the service under launchd, and {platform.id} runs "
            f"{platform.service_manager}"
        )
        raise InstallError(msg)
    words = activation(repo)
    if words[:1] != ["sudo"]:
        msg = f"the launchd activation command is `{shlex.join(words)}`, which runs as no root"
        raise InstallError(msg)
    label = label_of(words)
    sudo = ["sudo", "-n"]
    said = [
        installing.ran(
            [*sudo, "sh", str(repo.path("scripts/install-service.sh")), "--binary", str(binary)],
            cwd=repo.root,
            describing="the committed service installer",
        ).strip()
    ]
    try:
        installing.ran([*sudo, *words[1:]], cwd=repo.root, describing=f"`{shlex.join(words)}`")
        printed = installing.ran(
            [*sudo, "launchctl", "print", f"system/{label}"],
            cwd=repo.root,
            describing=f"`launchctl print system/{label}`",
        )
        said.append(f"{label}: loaded by `{shlex.join(words)}`; launchd prints it")
        said.extend(line.strip() for line in printed.splitlines() if "state =" in line)
    finally:
        run([*sudo, "launchctl", "bootout", f"system/{label}"], cwd=repo.root)
    return said
