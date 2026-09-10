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

import os
import shutil
import tarfile
from dataclasses import dataclass
from pathlib import Path

from repo_checks.model import Repo
from repo_checks.shell import run

from release_artifacts import platforms, targets
from release_artifacts.build import PROGRAM, Built, build, program, staged_release
from release_artifacts.world import World

#: Where an install script route is told to put it.
SCRIPT_DIRECTORY = "bin"

#: The committed script that route drives.
INSTALL_SCRIPT = "scripts/install.sh"

#: How long any one install is given.
INSTALL_TIMEOUT_SECONDS = 900

#: Every program a Rust toolchain puts on a path.
TOOLCHAIN = ("cargo", "rustc", "rustup")

#: How a route's own proof reports what the path it was installed under carried,
#: and what that report says where it carried nothing. Every route ships a
#: program already built for the platform, so `none` is the answer on every
#: host — and a proof saying anything else names a route that reached a
#: toolchain the machine beside the printer would have had to carry.
TOOLCHAIN_REPORT = "Rust toolchain on the install path: {}"
NO_TOOLCHAIN = TOOLCHAIN_REPORT.format("none")


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


def _without_rust(extra: dict[str, str] | None = None) -> dict[str, str]:
    """The caller's environment, minus every place a Rust toolchain lives.

    Not a claim in a comment: the directories carrying `cargo` and `rustc` are
    taken off PATH, so an install that needed one to compile something fails
    here rather than passing on this host's toolchain.
    """
    environment = dict(os.environ)
    kept = [
        directory
        for directory in environment.get("PATH", "").split(os.pathsep)
        if directory and not any(Path(directory, program).exists() for program in TOOLCHAIN)
    ]
    environment["PATH"] = os.pathsep.join(kept)
    environment.pop("CARGO_HOME", None)
    environment.pop("RUSTUP_HOME", None)
    if extra:
        environment.update(extra)
    return environment


def _ran(
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
    _ran(
        ["uv", "venv", "--clear", str(environment)],
        cwd=into,
        describing="making a Python environment",
    )
    wheel = _only(built.paths, ".whl", built.target)
    _ran(
        ["uv", "pip", "install", "--python", str(environment / "bin/python"), str(wheel)],
        cwd=into,
        describing=f"installing {wheel.name}",
    )
    return Installed(built.target, environment, None, str(environment / "bin/python"))


def node_client(repo: Repo, built: Built, into: Path) -> Installed:
    """The Node client, installed the way an application takes it."""
    environment = into / "env"
    environment.mkdir(parents=True, exist_ok=True)
    (environment / "package.json").write_text(
        '{ "name": "a-consumer", "private": true, "type": "module" }\n', encoding="utf-8"
    )
    tarball = _only(built.paths, ".tgz", built.target)
    _ran(
        ["npm", "install", "--no-audit", "--no-fund", str(tarball)],
        cwd=environment,
        describing=f"installing {tarball.name}",
    )
    return Installed(built.target, environment, None, "node")


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
    (consumer / "Cargo.toml").write_text(
        "\n".join(
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
                f'printobserver-sdk = {{ path = "{inside}" }}',
                "",
                "[workspace]",
                "",
            ]
        ),
        encoding="utf-8",
    )
    _ran(
        ["cargo", "build", "--release", "--manifest-path", str(consumer / "Cargo.toml")],
        cwd=consumer,
        describing="building a consumer of the packaged crate",
    )
    return Installed(
        built.target, environment, None, str(consumer / "target/release/printobserver-sdk-smoke")
    )


def python_route(repo: Repo, built: Built, into: Path) -> Installed:
    """The Python-registry route, taken with no Rust toolchain on the path."""
    environment = into / "env"
    _ran(
        ["uv", "venv", "--clear", str(environment)],
        cwd=into,
        describing="making a Python environment",
    )
    wheel = _only(built.paths, ".whl", built.target)
    _ran(
        ["uv", "pip", "install", "--python", str(environment / "bin/python"), str(wheel)],
        cwd=into,
        env=_without_rust(),
        describing=f"installing {wheel.name}",
    )
    return Installed(built.target, environment, environment / "bin" / PROGRAM, "")


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
    _ran(
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
        env=_without_rust(),
        describing="installing the launcher and the program beside it",
    )
    return Installed(built.target, environment, environment / "bin" / PROGRAM, "")


def script_route(repo: Repo, built: Built, into: Path) -> Installed:
    """The install-script route, driven against a release staged here.

    The script is driven exactly as the install-path section's own one-line
    command drives it — with `sh` — against a release directory this stages in
    the shape release automation publishes. There is no network and no
    published release: a route provable only against a release that does not
    exist yet is one nothing could prove at all.
    """
    environment = into / "env"
    directory = environment / SCRIPT_DIRECTORY
    staged = staged_release(repo, into / "release", _built_program(built))
    _ran(
        ["sh", str(repo.path(INSTALL_SCRIPT)), "--to", str(directory)],
        cwd=into,
        env=_without_rust({"PRINTOBSERVER_RELEASE_BASE": str(staged)}),
        describing="running the committed install script",
    )
    return Installed(built.target, environment, directory / PROGRAM, "")


def _built_program(built: Built) -> Path:
    """The program one route's own artifact carries, unpacked from it."""
    archive = _only(built.paths, ".tar.gz", built.target)
    into = archive.parent / "unpacked"
    into.mkdir(parents=True, exist_ok=True)
    with tarfile.open(archive, "r:gz") as opened:
        opened.extractall(into, filter="data")
    program = into / PROGRAM
    if not program.is_file():
        msg = f"{archive} carries no {PROGRAM}"
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
    return _prove_client(repo, taken, binary)


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
    environment = _without_rust()
    version = _ran(
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


def _prove_client(repo: Repo, taken: Installed, binary: Path | None) -> str:
    """One client's own smoke check, against a real supervisor."""
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
    world = World(program(repo, binary), taken.environment / "world")
    try:
        running = world.start()
        return _ran(
            [
                *argv,
                "--server",
                running.server,
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
