"""Build one artifact this repository publishes, or every one of them."""

from __future__ import annotations

import argparse
import json
import os
import sys
from pathlib import Path

from repo_checks.model import Repo

from release_artifacts.build import BuildError, build, build_all, staged_release
from release_artifacts.installing import InstallError, prove
from release_artifacts.publishing import PublishError, publish
from release_artifacts.registries import (
    UNREADABLE,
    VERSION_FIELD,
    RegistryError,
    cut_at,
    supported_version,
)
from release_artifacts.registries import prove as prove_registry
from release_artifacts.standin import Registries, StandinError
from release_artifacts.targets import TargetError, declared
from release_artifacts.world import World, WorldError, scripted_printer


def main(argv: list[str] | None = None) -> int:
    """Run one of this tool's commands, answering a process exit status."""
    parser = argparse.ArgumentParser(prog="release-artifacts", description=__doc__)
    parser.add_argument(
        "command",
        choices=[
            "build",
            "build-all",
            "stage-release",
            "prove",
            "publish",
            "world",
            "standin",
            "released",
            "list",
        ],
    )
    parser.add_argument("--target", default="", help="which declared target to build")
    parser.add_argument(
        "--registry",
        action="store_true",
        help="prove what the target's own registry serves, rather than a local build",
    )
    parser.add_argument(
        "--serves",
        action="append",
        default=[],
        metavar="VERSION",
        help="a version the stand-in registries serve, whose program runs and reports it",
    )
    parser.add_argument(
        "--broken",
        action="append",
        default=[],
        metavar="VERSION",
        help="a version they serve whose program installs and does not run",
    )
    parser.add_argument(
        "--mislabelled",
        action="append",
        default=[],
        metavar="SERVED=REPORTED",
        help="a version they serve whose program reports a different one",
    )
    parser.add_argument(
        "--release",
        action="append",
        default=[],
        metavar="TAG",
        help="a release the stand-in forge lists, whether or not anything serves it",
    )
    parser.add_argument(
        "--octoprint",
        action="store_true",
        help="drive the OctoPrint `just octoprint-up` started, rather than a stand-in",
    )
    parser.add_argument(
        "--commit",
        default="",
        metavar="SHA",
        help="the commit a release-time run ran at, whose release is the one to prove",
    )
    parser.add_argument("--into", type=Path, default=Path("dist"))
    parser.add_argument("--root", type=Path, default=Path.cwd())
    parser.add_argument(
        "--binary",
        type=Path,
        default=None,
        help="the printobserver program to carry, rather than building one",
    )
    arguments = parser.parse_args(argv)
    # Resolved here and nowhere else: everything below runs programs in
    # directories of its own, and a relative path handed to one of those would
    # name a different place in each.
    arguments.into = arguments.into.resolve()
    if arguments.binary is not None:
        arguments.binary = arguments.binary.resolve()
    repo = Repo(arguments.root)

    try:
        match arguments.command:
            case "list":
                for target in declared(repo.root):
                    print(f"{target.id}\t{target.description}")
            case "stage-release":
                staged = staged_release(repo, arguments.into, arguments.binary)
                print(f"staged {staged}", file=sys.stderr)
            case "world":
                return _world(repo, arguments)
            case "standin":
                return _standin(repo, arguments)
            case "released":
                return _released(repo, arguments)
            case "publish":
                for line in publish(repo, arguments.into, dict(os.environ)):
                    print(line)
            case "prove":
                return _prove(repo, arguments)
            case "build-all":
                for built in build_all(repo, arguments.into, arguments.binary):
                    for path in built.paths:
                        print(f"{built.target}\t{path}")
            case _:
                return _build(repo, arguments)
    except (
        BuildError,
        InstallError,
        PublishError,
        RegistryError,
        StandinError,
        TargetError,
        WorldError,
    ) as refused:
        print(f"release-artifacts: {refused}", file=sys.stderr)
        return 1
    return 0


def _prove(repo: Repo, arguments: argparse.Namespace) -> int:
    """Prove one artifact: what this tree built, or what its registry serves.

    A proof's own answer is the exit status, and the report goes wherever a
    reader of that answer looks: a pass to standard output, and the two
    failures to standard error beside every other diagnostic this program
    writes.
    """
    if not arguments.target:
        print("prove takes --target <id>; `list` names them", file=sys.stderr)
        return 2
    if arguments.registry:
        try:
            proof = prove_registry(repo, arguments.target, arguments.into, dict(os.environ))
        except RegistryError as unreadable:
            # An exit of its own, and not one of the three outcomes: a registry
            # nothing could read is a network rather than a release or a build.
            print(f"release-artifacts: {unreadable}", file=sys.stderr)
            return UNREADABLE
        print(proof.report, file=sys.stdout if proof.exit_status == 0 else sys.stderr)
        return proof.exit_status
    print(prove(repo, arguments.target, arguments.into, arguments.binary))
    return 0


def _released(repo: Repo, arguments: argparse.Namespace) -> int:
    """Say which release the release-time run at one commit cut.

    Answered as `version=<version>`, which is the one line a job publishes an
    output from: the release-time run resolves this once and hands that one
    concrete version to every route proof, so the three cannot resolve three
    different releases between them.

    A run that cut no release answers an empty field rather than failing, which
    is every push that found nothing unreleased — and what the jobs proving a
    route are gated on, so an ordinary push proves nothing rather than proving
    somebody else's release.
    """
    if not arguments.commit:
        print(
            "released takes --commit <sha>: the commit a release-time run ran at",
            file=sys.stderr,
        )
        return 2
    print(f"{VERSION_FIELD}={cut_at(repo.root, arguments.commit)}")
    return 0


def _build(repo: Repo, arguments: argparse.Namespace) -> int:
    """Build one declared target, and say where each file it wrote went."""
    if not arguments.target:
        print("build takes --target <id>; `list` names them", file=sys.stderr)
        return 2
    built = build(repo, arguments.target, arguments.into, arguments.binary)
    for path in built.paths:
        print(f"{built.target}\t{path}")
    return 0


def _version(given: str, *, option: str) -> str:
    """One version a caller named, as the registries serve it.

    What a version is, is `registries.supported`'s answer rather than a second
    pattern here: a stand-in that served a version the proof reading it could
    not select would be a stand-in for nothing.

    Raises:
        StandinError: If it is not a version release automation would have
            written. Everything a caller names here reaches a package
            manifest, a wheel's own file name and a path on disk, so it is
            validated where it arrives rather than where it lands.
    """
    version = supported_version(given)
    if not version:
        msg = (
            f"`{option} {given}` is not a version to serve: it must be three numbers, "
            f"as `0.1.0` or `v0.1.0`"
        )
        raise StandinError(msg)
    return version


def _mislabelled(given: str) -> tuple[str, str]:
    """The version a package is served as, and the one its program reports.

    Raises:
        StandinError: If either side is missing or is not a version.
    """
    served, separator, reported = given.partition("=")
    if not separator:
        msg = f"`--mislabelled {given}` names no reported version: write it as `0.1.0=0.2.0`"
        raise StandinError(msg)
    return _version(served, option="--mislabelled"), _version(reported, option="--mislabelled")


def _standin(repo: Repo, arguments: argparse.Namespace) -> int:
    """Stand every registry up, say where they answer, and hold them up.

    One implementation, two consumers: this repository's own suites drive the
    registry proof against these, and a developer runs the same proof against
    them by hand. Held up until whoever started it closes this program's input,
    which is what a journey does when it is done — and what the operating system
    does for it if that journey is killed.
    """
    serving = [_version(str(version), option="--serves") for version in arguments.serves]
    broken = [_version(str(version), option="--broken") for version in arguments.broken]
    mislabelled = [_mislabelled(str(pair)) for pair in arguments.mislabelled]
    released = [_version(str(tag), option="--release") for tag in arguments.release]

    registries = Registries(repo, arguments.into)
    try:
        for version in serving:
            registries.serve(version)
        for version in broken:
            registries.serve(version, broken=True)
        for served, reported in mislabelled:
            registries.serve(served, reported=reported)
        for tag in released:
            registries.release(f"v{tag}")
        print(json.dumps({"base": registries.base}), flush=True)
        sys.stdin.read()
    finally:
        registries.stop()
    return 0


def _world(repo: Repo, arguments: argparse.Namespace) -> int:
    """Bring a supervisor up, say where it is, and hold it up until stdin closes.

    One implementation, three consumers: each client's own printer-integration
    journey spawns this, reads the one line it prints, drives its nine steps
    against that supervisor, and closes this program's input to bring it down.
    A world stood up three times in three languages would be three worlds.
    """
    from release_artifacts.build import program

    printer = scripted_printer(repo.root) if arguments.octoprint else None
    world = World(program(repo, arguments.binary), arguments.into, printer)
    try:
        running = world.start()
        print(
            json.dumps(
                {
                    "server": running.server,
                    "print_id": running.print_id,
                    "image_id": running.image_id,
                    "event_id": running.event_id,
                    "state": str(running.state),
                    "file_name": running.file_name,
                }
            ),
            flush=True,
        )
        # Held up until whoever started it closes this program's input, which
        # is what a journey does when it is done — and what the operating
        # system does for it if that journey is killed.
        sys.stdin.read()
    finally:
        world.stop()
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
