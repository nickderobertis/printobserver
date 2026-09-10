"""Build one artifact this repository publishes, or every one of them."""

from __future__ import annotations

import argparse
import sys
from pathlib import Path

from repo_checks.model import Repo

from release_artifacts.build import BuildError, build, build_all, staged_release
from release_artifacts.installing import InstallError, prove
from release_artifacts.targets import TargetError, declared


def main(argv: list[str] | None = None) -> int:
    """Run one of this tool's commands, answering a process exit status."""
    parser = argparse.ArgumentParser(prog="release-artifacts", description=__doc__)
    parser.add_argument("command", choices=["build", "build-all", "stage-release", "prove", "list"])
    parser.add_argument("--target", default="", help="which declared target to build")
    parser.add_argument("--into", type=Path, default=Path("dist"))
    parser.add_argument("--root", type=Path, default=Path.cwd())
    parser.add_argument(
        "--binary",
        type=Path,
        default=None,
        help="the printobserver program to carry, rather than building one",
    )
    arguments = parser.parse_args(argv)
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
        if arguments.command == "prove":
            if not arguments.target:
                print("prove takes --target <id>; `list` names them", file=sys.stderr)
                return 2
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
    except (BuildError, InstallError, TargetError) as refused:
        print(f"release-artifacts: {refused}", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
