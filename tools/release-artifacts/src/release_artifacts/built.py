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

The release workflow runs both on its macOS cell alone, where they stand in for
the registry proofs that no longer run on macOS. What they cannot prove is
another platform's program running, and they say so rather than pretend: a
Linux host does not execute a Darwin binary, so what another platform's build
owes is held at the contract — `expected` names every platform's files, and
`processor_of` reads a program's processor without running it.
"""

from __future__ import annotations

import hashlib
import re
import shlex
import shutil
import struct
import subprocess
import tarfile
import zipfile
from pathlib import Path, PurePosixPath
from typing import NamedTuple

from repo_checks import install_path, platforms
from repo_checks.model import Repo, policy_table
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
ELF_MAGIC = b"\x7fELF"
ELF_MACHINE_AT = 18
MACHO_MAGIC = b"\xcf\xfa\xed\xfe"
MACHO_CPU_AT = 4
PE_MAGIC = b"MZ"
PE_SIGNATURE = b"PE\0\0"
PE_HEADER_OFFSET = 0x3C


class Executable(NamedTuple):
    """What a program's own header says it runs on, in a platform identifier's words."""

    family: str
    processor: str

    @property
    def platform(self) -> str:
        """The platform identifier this executable is for, as AGENTS.md's list spells one."""
        return f"{self.family}-{self.processor}"


def _field(program: bytes, at: int, layout: str) -> int:
    """One little-endian header field, or a refusal where the header ends before it.

    Raises:
        InstallError: If the program is too short to carry the field.
    """
    if at < 0 or len(program) < at + struct.calcsize(layout):
        msg = f"the program's header ends before offset {at}, so no processor can be read off it"
        raise InstallError(msg)
    return struct.unpack_from(layout, program, at)[0]


def processor_of(program: bytes) -> Executable:
    """The family of executable and the processor one program's own header names.

    Raises:
        InstallError: If the bytes are no executable this can read a processor
            off, or a header ends before the field it is read from.
    """
    if program.startswith(ELF_MAGIC):
        machine = ELF_MACHINES.get(_field(program, ELF_MACHINE_AT, "<H"), "unknown")
        return Executable("linux", machine)
    if program.startswith(MACHO_MAGIC):
        return Executable("macos", MACHO_CPUS.get(_field(program, MACHO_CPU_AT, "<I"), "unknown"))
    if program.startswith(PE_MAGIC):
        at = _field(program, PE_HEADER_OFFSET, "<I")
        if program[at : at + len(PE_SIGNATURE)] == PE_SIGNATURE:
            machine = _field(program, at + len(PE_SIGNATURE), "<H")
            return Executable("windows", PE_MACHINES.get(machine, "unknown"))
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
    executable = processor_of(built)
    if executable.platform != platform.id:
        msg = (
            f"{binary} is a {executable.family} {executable.processor} program, and this "
            f"cell publishes {platform.id}'s artifacts"
        )
        raise InstallError(msg)
    said = [
        f"{platform.id}: program {digest[:16]} is a {executable.family} "
        f"{executable.processor} executable"
    ]

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
    vouched = _checksums(dist / CHECKSUMS)
    if vouched.get(archive.name) != packages.digest_of(archive):
        msg = f"{CHECKSUMS} does not vouch for {archive.name} as it was built"
        raise InstallError(msg)
    said.append(
        f"{platform.id}: {wheel.name}, {per_platform.name} and {archive.name} are tagged for "
        f"{platform.id} at {version}, and {CHECKSUMS} vouches for {archive.name}"
    )
    return said


#: One line of the checksum file, as `sha256sum` writes it: a digest, two
#: spaces, a file name.
CHECKSUM_LINE = re.compile(r"^(?P<digest>[0-9a-f]{64})  (?P<name>\S+)$")


def _checksums(path: Path) -> dict[str, str]:
    """Each file the checksum file vouches for, and the digest it states.

    Raises:
        InstallError: If a line is not `sha256sum`'s, or one file is vouched for twice.
    """
    vouched: dict[str, str] = {}
    for number, line in enumerate(path.read_text(encoding="utf-8").splitlines(), start=1):
        found = CHECKSUM_LINE.match(line)
        if found is None:
            msg = f"{path.name} line {number} is not a digest and a file name: {line!r}"
            raise InstallError(msg)
        if found["name"] in vouched:
            msg = f"{path.name} vouches for {found['name']} more than once"
            raise InstallError(msg)
        vouched[found["name"]] = found["digest"]
    return vouched


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


#: The shape the launchd activation command takes: load one property list from
#: the directory launchd starts daemons from into the system domain, as root.
ACTIVATION_SHAPE = ("sudo", "launchctl", "bootstrap", "system")


def activation(repo: Repo) -> list[str]:
    """The launchd activation command `AGENTS.md`'s install path states, as words.

    Held to its shape before anything runs it: the proof then reads back and
    unloads the label that command loads, so a command of another shape would
    have the proof inspect a service nothing loaded.

    Raises:
        InstallError: If the section states no command for launchd's pair, or
            one that is not `sudo launchctl bootstrap system <directory>/<label>.plist`.
    """
    path = install_path.parse(repo.agents_md)
    pair = path.commands_for(LAUNCHD)
    if len(pair) != len(("install", "activate")):
        msg = "AGENTS.md's install path states no launchd activation command"
        raise InstallError(msg)
    words = shlex.split(pair[1])
    managers = policy_table(repo, "service").get("managers", {})
    directory = (managers.get(LAUNCHD, {}) if isinstance(managers, dict) else {}).get(
        "registration_directory", ""
    )
    plist = PurePosixPath(words[-1]) if words else PurePosixPath()
    if (
        tuple(words[:-1]) != ACTIVATION_SHAPE
        or plist.suffix != ".plist"
        or str(plist.parent) != directory
    ):
        msg = (
            f"AGENTS.md's launchd activation command is `{pair[1]}`, which is not "
            f"`{shlex.join(ACTIVATION_SHAPE)} {directory or '<directory>'}/<label>.plist`"
        )
        raise InstallError(msg)
    return words


def label_of(words: list[str]) -> str:
    """The launchd label a `launchctl bootstrap system <plist>` command loads."""
    return Path(words[-1]).name.removesuffix(".plist")


#: What `launchctl bootout` exits with for a label launchd has not loaded —
#: `ESRCH`, "No such process" — which after a load that never happened is the
#: answer that nothing was left loaded.
NOT_LOADED = 3

#: What `uname -s` answers on a launchd host, which is how the installer itself
#: decides it is writing launchd's property list rather than systemd's unit.
DARWIN = "Darwin"


# llmlint: ignore[async_typed_clients_at_boundaries] suppressions.toml has the reason.
def _launchctl(argv: list[str], repo: Repo, timeout: float, describing: str) -> None:
    """One `launchctl` command, its failure and its overrunning `timeout` both a refusal.

    Raises:
        InstallError: If it failed or did not finish in time.
    """
    try:
        result = run(argv, cwd=repo.root, timeout=timeout)
    except subprocess.TimeoutExpired as late:
        msg = f"{describing} did not finish within {timeout:g}s"
        raise InstallError(msg) from late
    if result.returncode != 0:
        msg = f"{describing} failed ({result.returncode}):\n{result.stdout}{result.stderr}"
        raise InstallError(msg)


#: How long one `launchctl` command is given: loading, reading back or
#: unloading one daemon is seconds of work, so a minute past that is a hang.
LAUNCHCTL_TIMEOUT_SECONDS = 120.0


def prove_service(
    repo: Repo,
    binary: Path,
    root: Path | None = None,
    user: str | None = None,
    timeout: float = LAUNCHCTL_TIMEOUT_SECONDS,
) -> list[str]:
    """Install the built program as launchd's daemon, load it, read it back, unload it.

    On the host, every command runs through a password-free `sudo`, as the
    hosted macOS runner gives one; a `sudo` that would ask is a refusal rather
    than a hang. `root` and `user` are the installer's own `--root` and
    `--user`: a machine whose `/` is `root`, owned by `user`, which is how a
    journey drives this against a launchd stand-in with nothing installed on
    the host — and then nothing needs root. Each `launchctl` command is given
    `timeout` seconds, and one that overruns fails the proof like one that
    exits non-zero — the unload still attempted.

    Raises:
        InstallError: If this is not a launchd host, any step fails, or the
            service could not be unloaded afterwards.
    """
    if shutil.which("uname") is None:
        msg = (
            f"this host has no `uname`, so it is no launchd host: the installer writes "
            f"launchd's property list on `{DARWIN}` alone"
        )
        raise InstallError(msg)
    system = installing.ran(["uname", "-s"], cwd=repo.root, describing="`uname -s`").strip()
    if system != DARWIN:
        msg = (
            f"this proof installs the service under launchd, and `uname -s` answers "
            f"`{system}`: the installer writes launchd's property list on `{DARWIN}` alone"
        )
        raise InstallError(msg)
    words = activation(repo)
    label = label_of(words)
    sudo = [] if root is not None else ["sudo", "-n"]
    beneath = [
        *(["--root", str(root)] if root is not None else []),
        *(["--user", user] if user else []),
    ]
    installer = [*sudo, "sh", str(repo.path("scripts/install-service.sh")), "--binary", str(binary)]
    installing.ran(
        [*installer, *beneath], cwd=repo.root, describing="the committed service installer"
    )
    said = [f"{label}: installed by scripts/install-service.sh"]
    failed: InstallError | None = None
    try:
        _launchctl([*sudo, *words[1:]], repo, timeout, f"`{shlex.join(words)}`")
        _launchctl(
            [*sudo, "launchctl", "print", f"system/{label}"],
            repo,
            timeout,
            f"`launchctl print system/{label}`",
        )
        said.append(f"loaded by `{shlex.join(words)}` and printed by launchd")
    except InstallError as refused:
        failed = refused
    # Unloaded whatever happened above, and its own answer kept: a failure to
    # unload is reported beside the failure that came first, never instead of it.
    unload = f"`launchctl bootout system/{label}`"
    try:
        # llmlint: ignore[async_typed_clients_at_boundaries] suppressions.toml has the reason.
        unloaded = run(
            [*sudo, "launchctl", "bootout", f"system/{label}"], cwd=repo.root, timeout=timeout
        )
        stuck = (
            ""
            if unloaded.returncode in {0, NOT_LOADED}
            else f"{unload} failed ({unloaded.returncode}), so the service this proof loaded "
            f"may still be loaded:\n{unloaded.stdout}{unloaded.stderr}"
        )
    except subprocess.TimeoutExpired:
        stuck = f"{unload} did not finish within {timeout:g}s, so the service may still be loaded"
    if stuck:
        if failed is not None:
            msg = f"{failed}\nand then {stuck}"
            raise InstallError(msg) from failed
        raise InstallError(stuck)
    if failed is not None:
        raise failed
    said.append("unloaded")
    return said
