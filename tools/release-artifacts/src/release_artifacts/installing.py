"""Taking each artifact the way its own consumer takes it, and proving it there.

Six artifacts, six ways a consumer gets them, and one thing asked of each: that
what was **built from the committed tree** is then installed into an
environment holding no copy of these sources, and works there.

For the three clients that means resolvable and usable: each is installed and
its committed smoke check runs against a real supervisor, which is what makes
the check about the artifact rather than about this tree.

For the three end-user routes it means more than that. Each carries the
`printobserver` program **already built for the platform**, so each is taken
with a PATH holding no Rust toolchain at all: a route that needed one on the
installing host would fail here, and that is the whole reason these rather than
a source install are what the end user gets.
"""

from __future__ import annotations

import json
import os
import shlex
import shutil
import sys
import tarfile
from dataclasses import dataclass
from pathlib import Path, PurePath

from repo_checks import platforms
from repo_checks.model import Repo
from repo_checks.shell import run

from release_artifacts import targets
from release_artifacts.build import PROGRAM, Built, build, program, staged_release
from release_artifacts.world import World

#: Where an install script route is told to put it.
SCRIPT_DIRECTORY = "bin"

#: The committed scripts that route drives: the shell form, for every platform
#: a shell reaches, and the PowerShell form, for Windows. One route, and the
#: one of the two this host's own platform is reached by.
INSTALL_SCRIPT = "scripts/install.sh"
INSTALL_SCRIPT_WINDOWS = "scripts/install.ps1"

#: The PowerShells a Windows host may carry, in the order one is taken: the
#: cross-platform one where it is installed, and Windows PowerShell otherwise.
POWERSHELLS = ("pwsh", "powershell")

#: How long any one install is given.
INSTALL_TIMEOUT_SECONDS = 900

#: Every program a Rust toolchain puts on a path.
TOOLCHAIN = ("cargo", "rustc", "rustup")

#: What the JavaScript registry's launcher reaches for: its first line is
#: `#!/usr/bin/env node`, so the program runs only where `node` is on the path.
NODE_RUNTIME = ("node",)

#: How a route's own proof reports what the path it was installed under carried,
#: and what that report says where it carried nothing. Every route ships a
#: program already built for the platform, so `none` is the answer on every
#: host — and a proof saying anything else names a route that reached a
#: toolchain the machine beside the printer would have had to carry.
TOOLCHAIN_REPORT = "Rust toolchain on the install path: {}"
NO_TOOLCHAIN = TOOLCHAIN_REPORT.format("none")


def executable(name: str) -> str:
    """What a program called `name` is called on disk on this host.

    Windows finds a program by its suffix, and every program a Rust build or a
    virtual environment puts there carries `.exe`; everywhere else the name is
    the whole of it. The `printobserver` program itself is not asked about
    here: `repo_checks.platforms` declares what it is called on every platform,
    and a caller reads that instead.
    """
    return f"{name}.exe" if sys.platform == "win32" else name


def release_program(target_directory: Path, name: str) -> Path:
    """Where a release build into `target_directory` leaves the program called `name`."""
    return target_directory / "release" / executable(name)


def consumer_program(consumer: Path, name: str, env: dict[str, str] | None = None) -> Path:
    """Where `cargo build --release` of the consumer crate at `consumer` left `name`.

    Asked of `cargo` rather than assumed at `consumer/target`, because that is
    not where a consumer inside this clone builds: `.cargo/config.toml` at the
    root sends every build under it into `<clone>/target`, the proofs write
    their consumers under `dist/`, and only a consumer written outside the clone
    builds beside its own manifest. `--no-deps` so the answer resolves nothing.

    Raises:
        InstallError: If `cargo` would not answer where it builds, or answered
            something that names no target directory.
    """
    asking = f"asking where the consumer at {consumer} builds"
    asked = run(
        [
            "cargo",
            "metadata",
            "--format-version",
            "1",
            "--no-deps",
            "--manifest-path",
            str(consumer / "Cargo.toml"),
        ],
        cwd=consumer,
        env=env,
        timeout=INSTALL_TIMEOUT_SECONDS,
    )
    if asked.returncode != 0:
        msg = f"{asking} failed ({asked.returncode}):\n{asked.stderr}"
        raise InstallError(msg)
    try:
        metadata = json.loads(asked.stdout)
    except json.JSONDecodeError as error:
        msg = f"{asking} answered something other than JSON ({error}):\n{asked.stdout}"
        raise InstallError(msg) from error
    directory = metadata.get("target_directory") if isinstance(metadata, dict) else None
    if not isinstance(directory, str) or not directory:
        msg = f"{asking} answered no `target_directory`:\n{asked.stdout}"
        raise InstallError(msg)
    return release_program(Path(directory), name)


def programs_in(environment: Path) -> Path:
    """Where a virtual environment made on this host keeps its programs.

    The layout is the interpreter's own rather than this repository's choice:
    `Scripts` on Windows and `bin` everywhere else. A route proven against `bin`
    on a Windows host would report a program missing that the install put one
    directory over.
    """
    return environment / ("Scripts" if sys.platform == "win32" else "bin")


def interpreter_in(environment: Path) -> Path:
    """The Python interpreter of a virtual environment made on this host."""
    return programs_in(environment) / executable("python")


def powershell() -> str:
    """The PowerShell this host runs a script with.

    Raises:
        InstallError: If it carries none, naming what to install.
    """
    for candidate in POWERSHELLS:
        found = shutil.which(candidate)
        if found:
            return found
    msg = (
        "the PowerShell form of the install script is driven under PowerShell, and this host "
        "has neither `pwsh` nor `powershell` on PATH; install PowerShell 7 "
        "(https://github.com/PowerShell/PowerShell/releases) and put `pwsh` on PATH"
    )
    raise InstallError(msg)


def install_script_argv(repo: Repo, *, version: str, into: Path) -> list[str]:
    """How this host runs the committed install script that reaches its own platform.

    Exactly as the install-path section's own one-line command runs it: with
    `sh` where that command pipes into `sh`, and under PowerShell where it
    pipes into `iex`. `version` pins a release and is empty for the newest;
    `into` is the directory the program is put in.

    Raises:
        InstallError: If this is a Windows host with no PowerShell on it.
    """
    if sys.platform == "win32":
        argv = [powershell(), "-NoProfile", "-File", str(repo.path(INSTALL_SCRIPT_WINDOWS))]
        if version:
            argv += ["-Version", version]
        return [*argv, "-To", str(into)]
    argv = ["sh", str(repo.path(INSTALL_SCRIPT))]
    if version:
        argv += ["--version", version]
    return [*argv, "--to", str(into)]


def npm_global_program(prefix: Path, name: str) -> Path:
    """What `npm install --global --prefix <prefix>` puts on a path for `name`.

    On Windows npm writes a package's programs straight into the prefix, and the
    one a process can be started from is the `.cmd` shim beside the POSIX and
    PowerShell ones. Everywhere else it links them into `<prefix>/bin`.
    """
    if sys.platform == "win32":
        return prefix / f"{name}.cmd"
    return prefix / "bin" / name


class InstallError(RuntimeError):
    """An artifact could not be taken the way its own consumer takes it."""


@dataclass(frozen=True, slots=True)
class Installed:
    """One artifact, taken."""

    target: str
    #: The environment it was installed into.
    environment: Path
    #: The program it put on a path, for a route.
    program: Path | None
    #: What its own consumer runs to reach it, for a client.
    said: str
    #: The programs that route's own program reaches for on the path it is run
    #: under — the runtime a launcher's `#!/usr/bin/env` line names — which the
    #: proof keeps when it takes the toolchain off. Empty for a program that
    #: runs on its own.
    runtime: tuple[str, ...] = ()


def without_rust(
    extra: dict[str, str] | None = None,
    *,
    preserve: tuple[str, ...] = (),
    preserved_at: Path | None = None,
) -> dict[str, str]:
    """The caller's environment, minus every place a Rust toolchain lives.

    Not a claim in a comment: the directories carrying `cargo` and `rustc` are
    taken off PATH, so an install that needed one to compile something fails
    here rather than passing on this host's toolchain.
    """
    environment = dict(os.environ)
    preserved = {
        program: shutil.which(program, path=environment.get("PATH")) for program in preserve
    }
    kept = [
        directory
        for directory in environment.get("PATH", "").split(os.pathsep)
        if directory
        and not any(Path(directory, executable(program)).exists() for program in TOOLCHAIN)
    ]
    if preserve:
        if preserved_at is None:
            msg = "a directory is required when preserving programs on the toolchain-free path"
            raise InstallError(msg)
        preserved_at.mkdir(parents=True, exist_ok=True)
        for name, source in preserved.items():
            if source is None:
                raise InstallError(f"cannot preserve `{name}` because it is not on PATH")
            destination = preserved_at / name
            destination.write_text(
                f'#!/bin/sh\nexec {shlex.quote(source)} "$@"\n', encoding="utf-8"
            )
            destination.chmod(0o755)
        kept.insert(0, str(preserved_at))
    environment["PATH"] = os.pathsep.join(kept)
    environment.pop("CARGO_HOME", None)
    environment.pop("RUSTUP_HOME", None)
    if extra:
        environment.update(extra)
    return environment


def ran(
    argv: list[str],
    *,
    cwd: Path,
    env: dict[str, str] | None = None,
    describing: str,
) -> str:
    """Run one command, or stop saying what it said.

    Raises:
        InstallError: If it failed, printing everything it said.
    """
    result = run(argv, cwd=cwd, env=env, timeout=INSTALL_TIMEOUT_SECONDS)
    if result.returncode != 0:
        msg = f"{describing} failed ({result.returncode}):\n{result.stdout}{result.stderr}"
        raise InstallError(msg)
    return result.stdout + result.stderr


def _only(paths: tuple[Path, ...], suffix: str, describing: str) -> Path:
    """The one built file of a kind.

    Raises:
        InstallError: If the build wrote none, or more than one.
    """
    found = [path for path in paths if path.name.endswith(suffix)]
    if len(found) != 1:
        msg = f"{describing}: expected one `{suffix}`, and the build wrote {len(found)}"
        raise InstallError(msg)
    return found[0]


def python_client(repo: Repo, built: Built, into: Path) -> Installed:
    """The Python client, installed the way an application takes it."""
    environment = into / "env"
    ran(
        ["uv", "venv", "--clear", str(environment)],
        cwd=into,
        describing="making a Python environment",
    )
    wheel = _only(built.paths, ".whl", built.target)
    ran(
        ["uv", "pip", "install", "--python", str(interpreter_in(environment)), str(wheel)],
        cwd=into,
        describing=f"installing {wheel.name}",
    )
    return Installed(built.target, environment, None, str(interpreter_in(environment)))


def node_client(repo: Repo, built: Built, into: Path) -> Installed:
    """The Node client, installed the way an application takes it."""
    environment = into / "env"
    environment.mkdir(parents=True, exist_ok=True)
    (environment / "package.json").write_text(
        '{ "name": "a-consumer", "private": true, "type": "module" }\n', encoding="utf-8"
    )
    tarball = _only(built.paths, ".tgz", built.target)
    ran(
        ["npm", "install", "--no-audit", "--no-fund", str(tarball)],
        cwd=environment,
        describing=f"installing {tarball.name}",
    )
    return Installed(built.target, environment, None, "node")


def consumer_manifest(inside: PurePath) -> str:
    """The manifest of a consumer depending on the packaged crate unpacked at `inside`.

    The path is written with forward slashes whatever the host. It sits inside a
    TOML basic string, where a Windows path's own separators are read as escapes
    — so written as it stands it is not a path there but a manifest `cargo`
    refuses to parse — and `cargo` reads a forward-slashed path on Windows as the
    same directory.
    """
    return "\n".join(
        [
            "# Written by the install journey, not committed: no manifest in that",
            "# tree carries a version, and this one has to name the packaged",
            "# crate's own.",
            "[package]",
            'name = "printobserver-sdk-smoke"',
            'version = "0.0.0"',
            'edition = "2024"',
            "",
            "[dependencies]",
            f'printobserver-sdk = {{ path = "{inside.as_posix()}" }}',
            "",
            "[workspace]",
            "",
        ]
    )


def rust_client(repo: Repo, built: Built, into: Path) -> Installed:
    """The Rust client, taken the way a dependent takes a published crate.

    The package is unpacked and depended on by path, which is what `cargo`
    itself does with a crate it downloaded: what is built against is the
    **packaged** bytes rather than this tree's sources.
    """
    environment = into / "env"
    unpacked = environment / "vendor"
    unpacked.mkdir(parents=True, exist_ok=True)
    package = _only(built.paths, ".crate", built.target)
    with tarfile.open(package, "r:gz") as archive:
        archive.extractall(unpacked, filter="data")
    inside = next(unpacked.iterdir())

    consumer = environment / "consumer"
    (consumer / "src").mkdir(parents=True, exist_ok=True)
    shutil.copy2(repo.path("crates/printobserver-sdk/smoke/main.rs"), consumer / "src/main.rs")
    (consumer / "Cargo.toml").write_text(consumer_manifest(inside), encoding="utf-8")
    ran(
        ["cargo", "build", "--release", "--manifest-path", str(consumer / "Cargo.toml")],
        cwd=consumer,
        describing="building a consumer of the packaged crate",
    )
    return Installed(
        built.target,
        environment,
        None,
        str(consumer_program(consumer, "printobserver-sdk-smoke")),
    )


def python_route(repo: Repo, built: Built, into: Path) -> Installed:
    """The Python-registry route, taken with no Rust toolchain on the path."""
    environment = into / "env"
    ran(
        ["uv", "venv", "--clear", str(environment)],
        cwd=into,
        describing="making a Python environment",
    )
    wheel = _only(built.paths, ".whl", built.target)
    ran(
        ["uv", "pip", "install", "--python", str(interpreter_in(environment)), str(wheel)],
        cwd=into,
        env=without_rust(preserve=("node",), preserved_at=environment / ".path"),
        describing=f"installing {wheel.name}",
    )
    installed = programs_in(environment) / platforms.host(repo).program
    return Installed(built.target, environment, installed, "")


def node_route(repo: Repo, built: Built, into: Path) -> Installed:
    """The JavaScript-registry route, taken with no Rust toolchain on the path.

    Both packages are given to the install: the launcher and the per-platform
    package it resolves. A registry install resolves that second one for the
    caller's own operating system and processor; here it is handed over
    directly, because this journey stands up no registry — and what is being
    proven is that what was built installs and runs, not that npm can fetch.
    """
    environment = into / "env"
    environment.mkdir(parents=True, exist_ok=True)
    tarballs = [str(path) for path in built.paths if path.name.endswith(".tgz")]
    if len(tarballs) != 2:
        msg = f"{built.target}: expected a launcher and a per-platform package"
        raise InstallError(msg)
    ran(
        [
            "npm",
            "install",
            "--global",
            "--prefix",
            str(environment),
            "--no-audit",
            "--no-fund",
            *tarballs,
        ],
        cwd=into,
        env=without_rust(preserve=NODE_RUNTIME, preserved_at=environment / ".path"),
        describing="installing the launcher and the program beside it",
    )
    return Installed(
        built.target,
        environment,
        npm_global_program(environment, PROGRAM),
        "",
        runtime=NODE_RUNTIME,
    )


def script_route(repo: Repo, built: Built, into: Path) -> Installed:
    """The install-script route, driven against a release staged here.

    The script this host's platform is reached by is driven exactly as the
    install-path section's own one-line command drives it — with `sh`, or
    under PowerShell on Windows — against a release directory this stages in
    the shape release automation publishes. There is no network and no
    published release: a route provable only against a release that does not
    exist yet is one nothing could prove at all.
    """
    environment = into / "env"
    directory = environment / SCRIPT_DIRECTORY
    staged = staged_release(repo, into / "release", _built_program(repo, built))
    ran(
        install_script_argv(repo, version="", into=directory),
        cwd=into,
        env=without_rust({"PRINTOBSERVER_RELEASE_BASE": str(staged)}),
        describing="running the committed install script",
    )
    return Installed(built.target, environment, directory / platforms.host(repo).program, "")


def _built_program(repo: Repo, built: Built) -> Path:
    """The program one route's own artifact carries, unpacked from it."""
    archive = _only(built.paths, ".tar.gz", built.target)
    into = archive.parent / "unpacked"
    into.mkdir(parents=True, exist_ok=True)
    with tarfile.open(archive, "r:gz") as opened:
        opened.extractall(into, filter="data")
    name = platforms.host(repo).program
    program = into / name
    if not program.is_file():
        msg = f"{archive} carries no {name}"
        raise InstallError(msg)
    program.chmod(0o755)
    return program


#: How each artifact is taken, by the target it is.
INSTALLERS = {
    "crate:printobserver-sdk": rust_client,
    "pypi:printobserver-sdk": python_client,
    "npm:@printobserver/sdk": node_client,
    "pypi:printobserver-cli": python_route,
    "npm:printobserver-cli": node_route,
    "release:printobserver": script_route,
}

#: What each client's committed smoke check is, and how it is run from the
#: environment the client was installed into. `{}` is where the installed
#: client's own entry point goes.
SMOKE = {
    "crate:printobserver-sdk": ["{}"],
    "pypi:printobserver-sdk": ["{}", "python/printobserver-sdk/smoke/smoke.py"],
    "npm:@printobserver/sdk": ["{}", "npm/printobserver-sdk/smoke/smoke.mjs"],
}


def install(repo: Repo, identifier: str, into: Path, binary: Path | None = None) -> Installed:
    """Build one artifact and take it the way its own consumer takes it.

    Raises:
        InstallError: If nothing here takes that artifact.
    """
    installer = INSTALLERS.get(identifier)
    if installer is None:
        msg = f"nothing here takes `{identifier}` the way a consumer takes it"
        raise InstallError(msg)
    into.mkdir(parents=True, exist_ok=True)
    built = build(repo, identifier, into / "dist", program(repo, binary))
    return installer(repo, built, into)


def prove(repo: Repo, identifier: str, into: Path, binary: Path | None = None) -> str:
    """Take one artifact, and prove it where it was taken.

    A client is proven by its own committed smoke check, run against a real
    supervisor. A route is proven by the program it put on a path reporting its
    own version — which is what a caller who has just run one of the three
    commands does first.

    Raises:
        InstallError: If what was installed does not work where it was
            installed.
    """
    taken = install(repo, identifier, into, binary)
    if taken.program is not None:
        return _prove_route(repo, taken)
    return prove_client(repo, taken, binary)


def _prove_route(repo: Repo, taken: Installed) -> str:
    """The program a route installed runs, says its version, and says what it was taken with.

    The second line is read off the environment the install and this run were
    actually given rather than claimed about it, so a route taken where a
    toolchain was still reachable says which programs those were, in the answer
    its own recipe prints. That is what makes "no route needs a Rust toolchain"
    something a reader of a proof observes rather than something this module
    says about itself.

    Raises:
        InstallError: If the route put no program on the path it was given, or
            what it put there is not this tree's version.
    """
    installed = taken.program
    if installed is None or not installed.exists():
        msg = f"{taken.target} put no {PROGRAM} on the path it was given"
        raise InstallError(msg)
    # The runtime the program reaches for is kept where the toolchain is taken
    # off: on the hosted macOS images `node` shares its directory with `cargo`,
    # and a proof that took that directory off would fail the launcher for a
    # runtime the machine beside the printer has.
    environment = without_rust(
        preserve=taken.runtime, preserved_at=taken.environment / ".path" if taken.runtime else None
    )
    version = ran(
        [str(installed), "--version"],
        cwd=taken.environment,
        env=environment,
        describing=str(installed),
    ).strip()
    expected = targets.workspace(repo.root)["version"]
    if expected not in version:
        msg = f"{installed} reports `{version}`, and this tree's version is {expected}"
        raise InstallError(msg)
    reached = [name for name in TOOLCHAIN if shutil.which(name, path=environment["PATH"])]
    return f"{version}\n{taken.target}: {TOOLCHAIN_REPORT.format(', '.join(reached) or 'none')}"


def smoke_check(repo: Repo, taken: Installed) -> list[str]:
    """The command that runs one installed client's own smoke check where it was installed.

    Raises:
        InstallError: If that client has no committed smoke check.
    """
    smoke = SMOKE.get(taken.target)
    if smoke is None:
        msg = f"{taken.target} has no committed smoke check"
        raise InstallError(msg)
    # The smoke check is copied into the environment before it is run, so that
    # what it resolves is the installed package rather than whatever this
    # repository's own tree has beside it. That is the whole difference between
    # a check about the artifact and a check about these sources.
    argv: list[str] = []
    for piece in smoke:
        if "{}" in piece:
            argv.append(piece.format(taken.said))
            continue
        beside = taken.environment / Path(piece).name
        shutil.copy2(repo.path(piece), beside)
        argv.append(str(beside))
    return argv


def prove_client(repo: Repo, taken: Installed, binary: Path | None) -> str:
    """One installed client's own smoke check, against a real supervisor.

    Raises:
        InstallError: If that client has no committed smoke check.
    """
    argv = smoke_check(repo, taken)
    world = World(program(repo, binary), taken.environment / "world")
    try:
        running = world.start()
        return ran(
            [
                *argv,
                "--server",
                running.server,
                "--credential",
                running.credential,
                "--print-id",
                running.print_id,
                "--image-id",
                running.image_id,
            ],
            cwd=taken.environment,
            describing=f"the smoke check of {taken.target}",
        ).strip()
    finally:
        world.stop()


def host_platform(repo: Repo) -> str:
    """The platform this host's artifacts are built for."""
    return platforms.host(repo).id
