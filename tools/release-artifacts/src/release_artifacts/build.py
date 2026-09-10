"""Building each artifact this repository publishes, from the committed tree.

Six artifacts and one place they are built. Three are the clients a dependent
takes as a dependency; three are the alternative routes an end user gets the
`printobserver` program by, and each of those carries the program **already
built for the platform** — which is the whole reason they exist, because the
host they are installed on is the small machine beside the printer and the
worst place to compile a Rust workspace.
"""

from __future__ import annotations

import json
import shutil
from dataclasses import dataclass
from pathlib import Path

from repo_checks.model import Repo
from repo_checks.shell import run

from release_artifacts import packages, platforms, targets, wheels

#: The program every one of the three end-user routes delivers.
PROGRAM = "printobserver"

#: The crate that program is built from.
PROGRAM_CRATE = "printobserver"

#: The profile a published program is built under.
PROFILE = "release"

#: Where the release artifacts an install script downloads are assembled.
RELEASE_DIRECTORY = "release"

#: The file a release publishes its artifacts' digests in, in the format
#: `sha256sum` itself writes and reads.
CHECKSUMS = "SHA256SUMS"

#: The interpreter versions the Python distributions declare support for.
REQUIRES_PYTHON = ">=3.9"

#: What `built_by` says about a target this tool assembles, rather than one
#: release automation publishes straight from the workspace.
ASSEMBLED_HERE = "release-artifacts"

#: The file a Python client's own metadata records the server contract in. A
#: consumer reads it off the installed distribution rather than off this tree.
CONTRACT_FILE = "printobserver-contract-version"

#: The field a Node client's manifest records the same thing in.
CONTRACT_FIELD = "printobserverContractVersion"

#: What the generated modules declare it as.
CONTRACT_CONSTANT = "CONTRACT_VERSION"


class BuildError(RuntimeError):
    """An artifact could not be built from this tree."""


@dataclass(frozen=True, slots=True)
class Built:
    """What one build produced."""

    target: str
    #: Every file it wrote, in the order it wrote them.
    paths: tuple[Path, ...]


def program(repo: Repo, given: Path | None = None) -> Path:
    """The `printobserver` program, built for this host.

    Raises:
        BuildError: If the program could not be built or is not where the
            build put it.
    """
    if given is not None:
        if not given.is_file():
            msg = f"{given} is not a program this build can carry"
            raise BuildError(msg)
        return given
    built = run(
        ["cargo", "build", f"--{PROFILE}", "--locked", "-p", PROGRAM_CRATE],
        cwd=repo.root,
        timeout=1800,
    )
    if built.returncode != 0:
        msg = f"`cargo build --{PROFILE}` failed:\n{built.stderr}"
        raise BuildError(msg)
    path = repo.root / "target" / PROFILE / PROGRAM
    if not path.is_file():
        msg = f"`cargo build --{PROFILE}` left no program at {path}"
        raise BuildError(msg)
    return path


def contract_version(repo: Repo, generated: str) -> str:
    """The server contract one generated client was written from.

    Read out of the client's own generated module rather than out of the
    workspace: what a consumer wants to know is which contract the package they
    installed was generated against, and that is what the module records.

    Raises:
        BuildError: If the generated module records none.
    """
    import re

    found = re.search(rf'{CONTRACT_CONSTANT}[^"\n]*"(?P<version>[^"]+)"', repo.read(generated))
    if found is None:
        msg = f"{generated} records no `{CONTRACT_CONSTANT}` for its package to carry"
        raise BuildError(msg)
    return found["version"]


def _distribution(repo: Repo, target: targets.Target) -> wheels.Distribution:
    """What one Python distribution of this repository says about itself."""
    inherited = targets.workspace(repo.root)
    return wheels.Distribution(
        name=target.name,
        version=inherited["version"],
        summary=target.description,
        requires_python=REQUIRES_PYTHON,
        license=inherited["license"],
        homepage=inherited["repository"],
    )


def _node(repo: Repo, target: targets.Target, name: str = "") -> packages.NodePackage:
    """What one package of the JavaScript registry says about itself."""
    inherited = targets.workspace(repo.root)
    return packages.NodePackage(
        name=name or target.name,
        version=inherited["version"],
        description=target.description,
        license=inherited["license"],
        repository=inherited["repository"],
    )


def python_client(repo: Repo, target: targets.Target, into: Path) -> Built:
    """The Python client, as the wheel an ordinary install takes.

    Its metadata records the server contract it was generated from, so a
    consumer can read that off the installed distribution rather than off this
    tree.
    """
    distribution = _distribution(repo, target)
    wheel = wheels.Wheel(distribution, wheels.PURE_TAG)
    wheel.add_tree(repo.path("python/printobserver-sdk/src/printobserver_sdk"), "printobserver_sdk")
    recorded = contract_version(repo, "python/printobserver-sdk/src/printobserver_sdk/contract.py")
    wheel.add(f"{distribution.dist_info}/{CONTRACT_FILE}", f"{recorded}\n".encode())
    return Built(target.id, (wheel.write(into),))


#: Where the Node client's sources are compiled to before they are packed. An
#: installed package is JavaScript with types beside it: a consumer's own
#: toolchain is theirs to choose, and one that shipped TypeScript would only
#: run under the toolchains that read it.
NODE_BUILD = "npm/printobserver-sdk/tsconfig.build.json"

#: Where that compilation leaves what it wrote.
NODE_OUTPUT = "dist/npm-sdk"


def node_client(repo: Repo, target: targets.Target, into: Path) -> Built:
    """The Node client, as the package an ordinary install takes.

    Raises:
        BuildError: If the client would not compile.
    """
    compiled = run(
        ["bunx", "tsc", "-p", NODE_BUILD],
        cwd=repo.root,
        timeout=900,
    )
    if compiled.returncode != 0:
        msg = f"the Node client would not compile:\n{compiled.stdout}{compiled.stderr}"
        raise BuildError(msg)

    package = _node(repo, target)
    archive = packages.Archive()
    root = repo.path(NODE_OUTPUT)
    written = sorted(root.rglob("*"))
    if not any(path.name == "index.js" for path in written):
        msg = f"the Node client compiled to {root} with no entry point in it"
        raise BuildError(msg)
    for path in written:
        if path.is_file():
            archive.add(
                f"{packages.PACKAGE_ROOT}/dist/{path.relative_to(root).as_posix()}",
                path.read_bytes(),
            )
    manifest = package.manifest(
        type="module",
        main="dist/index.js",
        types="dist/index.d.ts",
        exports={".": {"types": "./dist/index.d.ts", "default": "./dist/index.js"}},
        files=["dist"],
        **{CONTRACT_FIELD: contract_version(repo, "npm/printobserver-sdk/src/contract.ts")},
    )
    return Built(target.id, (packages.packed(package, manifest, archive, into),))


def rust_client(repo: Repo, target: targets.Target, into: Path) -> Built:
    """The Rust client, as the crate package a dependent takes.

    Raises:
        BuildError: If `cargo` would not package the crate.
    """
    #  because a journey builds this from a working tree it has
    # just written into: what is packaged is the tree as it stands, which is
    # what the journey is about. Release automation packages a commit.
    packaged = run(
        ["cargo", "package", "--locked", "--no-verify", "--allow-dirty", "-p", target.name],
        cwd=repo.root,
        timeout=900,
    )
    if packaged.returncode != 0:
        msg = f"`cargo package -p {target.name}` failed:\n{packaged.stderr}"
        raise BuildError(msg)
    version = targets.workspace(repo.root)["version"]
    built = repo.root / "target" / "package" / f"{target.name}-{version}.crate"
    if not built.is_file():
        msg = f"`cargo package -p {target.name}` left no package at {built}"
        raise BuildError(msg)
    into.mkdir(parents=True, exist_ok=True)
    return Built(target.id, (Path(shutil.copy2(built, into / built.name)),))


def python_route(repo: Repo, target: targets.Target, into: Path, binary: Path) -> Built:
    """The Python-registry route: a wheel carrying the program for this platform."""
    platform = platforms.host(repo)
    tag = f"{wheels.INTERPRETER}-{platform.wheel_tag(platforms.host_glibc())}"
    wheel = wheels.Wheel(_distribution(repo, target), tag)
    wheel.add_script(PROGRAM, binary)
    return Built(target.id, (wheel.write(into),))


def node_route(repo: Repo, target: targets.Target, into: Path, binary: Path) -> Built:
    """The JavaScript-registry route: a launcher and the package it resolves.

    The launcher declares one package per supported platform as an optional
    dependency and runs whichever one the install resolved. That is what makes
    a global install put a program **already built for the platform** on the
    path: nothing is compiled on the way in, and a host with no Rust toolchain
    is exactly the host this route is for.
    """
    inherited = targets.workspace(repo.root)
    platform = platforms.host(repo)
    system, processor = platform.npm

    per_platform = _node(repo, target, platform.npm_package)
    carried = packages.Archive()
    carried.add(f"{packages.PACKAGE_ROOT}/bin/{PROGRAM}", binary.read_bytes(), executable=True)
    platform_manifest = per_platform.manifest(
        description=f"{target.description} ({platform.id}).",
        os=[system],
        cpu=[processor],
        files=["bin"],
        printobserverPlatform=platform.id,
    )
    written = [packages.packed(per_platform, platform_manifest, carried, into)]

    launcher = _node(repo, target)
    beside = packages.Archive()
    beside.add(
        f"{packages.PACKAGE_ROOT}/bin/{PROGRAM}.mjs", _launcher(repo).encode(), executable=True
    )
    launcher_manifest = launcher.manifest(
        type="module",
        bin={PROGRAM: f"bin/{PROGRAM}.mjs"},
        files=["bin"],
        optionalDependencies={
            supported.npm_package: inherited["version"] for supported in platforms.supported(repo)
        },
    )
    written.append(packages.packed(launcher, launcher_manifest, beside, into))
    return Built(target.id, tuple(written))


def _launcher(repo: Repo) -> str:
    """The program the launcher package puts on the path."""
    return repo.read("npm/printobserver-cli/bin/printobserver.mjs")


def release_route(repo: Repo, target: targets.Target, into: Path, binary: Path) -> Built:
    """The install-script route: the prebuilt artifact that script downloads."""
    platform = platforms.host(repo)
    archive = packages.Archive()
    archive.add(PROGRAM, binary.read_bytes(), executable=True)
    written = archive.write(into / f"{PROGRAM}-{platform.id}.tar.gz")
    digests = into / CHECKSUMS
    digests.write_bytes(packages.checksums([written]))
    return Built(target.id, (written, digests))


#: What builds each declared target, by the registry that serves it. A target
#: whose registry is not here is one nothing builds, which `just check-repo`'s
#: release checks refuse.
BUILDERS = {
    "crate": rust_client,
    "pypi": python_client,
    "npm": node_client,
}

#: What builds each target that backs one of the end-user routes. Read by
#: identifier rather than by registry, because two of the three share a
#: registry with a client.
ROUTE_BUILDERS = {
    "pypi:printobserver-cli": python_route,
    "npm:printobserver-cli": node_route,
    "release:printobserver": release_route,
}


def build(repo: Repo, identifier: str, into: Path, binary: Path | None = None) -> Built:
    """Build one declared artifact into `into`.

    Raises:
        BuildError: If nothing here builds that target.
    """
    target = targets.named(repo.root, identifier)
    route = ROUTE_BUILDERS.get(target.id)
    if route is not None:
        return route(repo, target, into, program(repo, binary))
    builder = BUILDERS.get(target.registry)
    if builder is None or target.built_by != ASSEMBLED_HERE:
        msg = (
            f"nothing here builds `{identifier}`: the declaration says it is published "
            f"by `{target.built_by}`"
        )
        raise BuildError(msg)
    return builder(repo, target, into)


def assembled(repo: Repo) -> list[targets.Target]:
    """Every declared target this tool assembles.

    Not every declared target: release automation publishes each crate straight
    from the workspace, and a tool that packed those as well would be a second
    way one artifact is made.
    """
    return [target for target in targets.declared(repo.root) if target.built_by == ASSEMBLED_HERE]


def build_all(repo: Repo, into: Path, binary: Path | None = None) -> list[Built]:
    """Build every artifact this tool assembles, sharing the one program build."""
    program_path = program(repo, binary)
    return [build(repo, target.id, into, program_path) for target in assembled(repo)]


def staged_release(repo: Repo, into: Path, binary: Path | None = None) -> Path:
    """Stage a release directory in the shape release automation publishes.

    `<into>/latest/download/` and `<into>/download/<tag>/` are the two shapes
    the install script fetches from, and they are the shapes the forge itself
    serves — so a script proven against this directory is proven against the
    layout it will meet.
    """
    version = targets.workspace(repo.root)["version"]
    target = targets.named(repo.root, "release:printobserver")
    tagged = into / "download" / f"v{version}"
    release_route(repo, target, tagged, program(repo, binary))
    latest = into / "latest" / "download"
    latest.mkdir(parents=True, exist_ok=True)
    for path in sorted(tagged.iterdir()):
        shutil.copy2(path, latest / path.name)
    return into


def manifest_of(path: Path) -> dict[str, object]:
    """The `package.json` inside one packed package of the JavaScript registry."""
    import tarfile

    with tarfile.open(path, "r:gz") as archive:
        try:
            handle = archive.extractfile(f"{packages.PACKAGE_ROOT}/package.json")
        except KeyError as absent:
            msg = f"{path} carries no package manifest"
            raise BuildError(msg) from absent
        if handle is None:
            msg = f"{path} carries no package manifest"
            raise BuildError(msg)
        parsed = json.loads(handle.read().decode("utf-8"))
    if not isinstance(parsed, dict):
        msg = f"{path} carries a manifest that is not an object"
        raise BuildError(msg)
    return parsed
