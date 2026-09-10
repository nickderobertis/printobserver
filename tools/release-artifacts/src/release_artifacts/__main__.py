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
from release_artifacts.registries import RegistryError
from release_artifacts.registries import prove as prove_registry
from release_artifacts.standin import Registries
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
        if arguments.command == "list":
            for target in declared(repo.root):
                print(f"{target.id}\t{target.description}")
            return 0
        if arguments.command == "stage-release":
            staged = staged_release(repo, arguments.into, arguments.binary)
            print(f"staged {staged}", file=sys.stderr)
            return 0
        if arguments.command == "world":
            return _world(repo, arguments)
        if arguments.command == "standin":
            return _standin(repo, arguments)
        if arguments.command == "publish":
            for line in publish(repo, arguments.into, dict(os.environ)):
                print(line)
            return 0
        if arguments.command == "prove":
            if not arguments.target:
                print("prove takes --target <id>; `list` names them", file=sys.stderr)
                return 2
            if arguments.registry:
                proof = prove_registry(repo, arguments.target, arguments.into, dict(os.environ))
                print(proof.report)
                return proof.exit_status
            print(prove(repo, arguments.target, arguments.into, arguments.binary))
            return 0
        if arguments.command == "build-all":
            for built in build_all(repo, arguments.into, arguments.binary):
                for path in built.paths:
                    print(f"{built.target}\t{path}")
            return 0
        if not arguments.target:
            print("build takes --target <id>; `list` names them", file=sys.stderr)
            return 2
        built = build(repo, arguments.target, arguments.into, arguments.binary)
        for path in built.paths:
            print(f"{built.target}\t{path}")
    except (
        BuildError,
        InstallError,
        PublishError,
        RegistryError,
        TargetError,
        WorldError,
    ) as refused:
        print(f"release-artifacts: {refused}", file=sys.stderr)
        return 1
    return 0


def _standin(repo: Repo, arguments: argparse.Namespace) -> int:
    """Stand every registry up, say where they answer, and hold them up.

    One implementation, two consumers: this repository's own suites drive the
    registry proof against these, and a developer runs the same proof against
    them by hand. Held up until whoever started it closes this program's input,
    which is what a journey does when it is done — and what the operating system
    does for it if that journey is killed.
    """
    registries = Registries(repo, arguments.into)
    try:
        for version in arguments.serves:
            registries.serve(str(version))
        for version in arguments.broken:
            registries.serve(str(version), broken=True)
        for pair in arguments.mislabelled:
            served, _, reported = str(pair).partition("=")
            registries.serve(served, reported=reported)
        for tag in arguments.release:
            registries.release(str(tag))
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
