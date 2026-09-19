"""The committed launcher of route 2, driven under Node as each platform's install runs it.

The launcher is the one program every `npm install -g printobserver-cli`
puts on the path, and what it does is resolve the per-platform package the
caller's own install selected and run the program inside it. Which package
that is, and what the program inside it is called, are the two facts the
platform declaration states — so this drives the real launcher for the
platforms whose package cannot be installed here, with Node's own answers
about the host reached through a preloaded fixture, and reads what it ran.

Nothing about the launcher is stood in for. The fixture is two properties of
`process`, the package it resolves is laid out as npm lays one out, and the
program in it is a stand-in of this suite's own that says which one it is.
"""

from __future__ import annotations

import json
import shutil
from pathlib import Path

import pytest
from release_artifacts.build import LAUNCHER, PROGRAM
from release_artifacts.stand_in import stand_in_program
from repo_checks import platforms
from repo_checks.expect import contains, equal, failing, passing
from repo_checks.model import Repo
from repo_checks.shell import run

#: What each stand-in program the launcher resolves answers, naming the
#: package it was found in so that the answer says which one ran.
ANSWER = "printobserver 0.0.0 from {package}"


def _fixture(root: Path, system: str, processor: str) -> Path:
    """A module Node preloads that makes `process` report another platform."""
    path = root / "fixture.mjs"
    path.write_text(
        f'Object.defineProperty(process, "platform", {{ value: {json.dumps(system)} }});\n'
        f'Object.defineProperty(process, "arch", {{ value: {json.dumps(processor)} }});\n',
        encoding="utf-8",
    )
    return path


def _installed(repo: Repo, root: Path, platform: platforms.Platform | None) -> Path:
    """The launcher, laid out where a global install puts it, beside one platform's package.

    `platform` is the one whose package is installed beside it, or none — the
    state an install with optional dependencies disabled leaves.
    """
    modules = root / "node_modules"
    launcher = modules / "printobserver-cli" / "bin" / f"{PROGRAM}.mjs"
    launcher.parent.mkdir(parents=True)
    shutil.copy2(repo.path(LAUNCHER), launcher)
    if platform is not None:
        package = modules / platform.npm_package
        (package / "bin").mkdir(parents=True)
        (package / "package.json").write_text(
            json.dumps({"name": platform.npm_package, "version": "0.0.0"}), encoding="utf-8"
        )
        stand_in_program(
            repo,
            package / "bin" / platform.program,
            ANSWER.format(package=platform.npm_package),
        )
    return launcher


def _run(launcher: Path, fixture: Path, *arguments: str) -> tuple[int, str]:
    """Run the launcher under Node with the fixture preloaded, as an install runs it."""
    result = run(
        ["node", "--import", fixture.as_uri(), str(launcher), *arguments],
        cwd=launcher.parent,
        timeout=120,
    )
    return result.returncode, (result.stdout or "") + (result.stderr or "")


@pytest.mark.parametrize("identifier", ["windows-x86_64", "windows-aarch64"])
def test_a_windows_install_resolves_the_package_the_declaration_names(
    identifier: str, repo: Repo, tmp_path: Path
) -> None:
    """On `win32`, either processor, the launcher runs the `.exe` in that platform's package."""
    platform = platforms.descriptor(repo, identifier)
    system, processor = platform.npm
    launcher = _installed(repo, tmp_path, platform)

    code, said = _run(launcher, _fixture(tmp_path, system, processor), "--version")

    passing((code, said), describing=f"the launcher as a {identifier} install runs it")
    contains(
        said,
        ANSWER.format(package=platform.npm_package),
        describing=f"the program the launcher ran for {identifier}",
    )


def test_this_hosts_own_install_resolves_its_own_package(repo: Repo, tmp_path: Path) -> None:
    """With nothing stood in for, the launcher runs the program for the host it is on."""
    platform = platforms.host(repo)
    launcher = _installed(repo, tmp_path, platform)
    result = run(["node", str(launcher), "--version"], cwd=launcher.parent, timeout=120)

    passing((result.returncode, result.stdout + result.stderr), describing="the launcher here")
    contains(result.stdout, ANSWER.format(package=platform.npm_package), describing="what ran")


def test_a_platform_no_package_is_published_for_is_refused_naming_the_others(
    repo: Repo, tmp_path: Path
) -> None:
    """A host outside the map is a stop naming what the map carries, not a guess."""
    launcher = _installed(repo, tmp_path, None)

    code, said = _run(launcher, _fixture(tmp_path, "freebsd", "x64"), "--version")

    failing((code, said), naming="ships no program for freebsd-x64")
    for platform in platforms.install_platforms(repo):
        contains(said, platform.npm_selector, describing="the platforms the launcher names")


def test_a_platform_package_the_install_left_out_is_said_to_be_missing(
    repo: Repo, tmp_path: Path
) -> None:
    """An install run with optional dependencies disabled leaves the program out."""
    platform = platforms.descriptor(repo, "windows-aarch64")
    system, processor = platform.npm
    launcher = _installed(repo, tmp_path, None)

    code, said = _run(launcher, _fixture(tmp_path, system, processor), "--version")

    failing((code, said), naming="is not installed")
    contains(said, platform.npm_package, describing="the package the launcher named")


def test_the_launcher_resolves_exactly_the_platforms_the_install_path_targets(
    repo: Repo,
) -> None:
    """The map the fixture above drives is the one `just check-repo` holds to the list."""
    text = repo.read(LAUNCHER)
    for platform in platforms.install_platforms(repo):
        contains(text, f'"{platform.npm_selector}": "{platform.npm_package}"', describing=LAUNCHER)
    equal(
        text.count("@printobserver/cli-"),
        len(platforms.install_platforms(repo)),
        describing="one package per platform the install path targets, and no other",
    )
