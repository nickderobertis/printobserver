"""The end-to-end tier: journeys that drive this repository's own gate.

Journeys copy the committed tree and run its real recipes and checks. Nothing
is mocked — `just` runs, `cargo` runs, `nx` runs, and assertions describe what
those public interfaces report and leave behind.

Every run below is captured through `capture`, which pins the colouring on and
then strips it back out. Both halves of that are load-bearing, and the reason is
in its docstring.
"""

from __future__ import annotations

import functools
import os
import re
import shutil
import subprocess
import tomllib
from pathlib import Path
from typing import Protocol

import pytest
from repo_checks import platforms
from repo_checks.fixture_repository import init_repository
from repo_checks.model import Repo
from repo_checks.shell import run as shell_run

REPO_ROOT = Path(__file__).resolve().parents[3]

#: This host's own entry in AGENTS.md's supported-platform list.
HERE = platforms.host(Repo(REPO_ROOT))

#: A journey over an end-user install route, run where the install path targets
#: this host's platform and skipped where it does not, for the reason the
#: platform's own descriptor gives — the one the artifact tool's route proofs
#: read too.
NO_ROUTE_HERE: str | None = HERE.no_route_proof
ROUTE_JOURNEY = pytest.mark.skipif(NO_ROUTE_HERE is not None, reason=NO_ROUTE_HERE or "")
#: The journeys that build, or run, what one checkout keeps at fixed paths of its
#: own tree: the release program under `target/release`, the client crate's
#: smoke consumer under `target/release`, and
#: every proof and the built Node client under `dist/` — and, on Windows, the
#: user's own `Path`, which the PowerShell install script writes. Two of these at
#: once would build into, clear or read one another's directories, so xdist runs
#: the whole group on one worker, one journey after another.
RELEASE_BUILD = pytest.mark.xdist_group("repo-root-release-build")

#: The journeys that build the debug program in the repository root and run it,
#: or build into that root's `target` from a copy, and the service-manager
#: journey, which also takes the host's one launchd label or service control
#: manager and its machine-wide paths. Each builds with the program's default
#: features after the tier's own `printobserver:build`, so a build here waits on
#: cargo's lock and finds nothing to do; the group keeps them on one worker so a
#: host without that graph target — a run of pytest by hand — never has one
#: journey replace a program another is running, nor two service journeys
#: register the same service.
DEBUG_BUILD = pytest.mark.xdist_group("repo-root-debug-build")

#: The journeys that draft or publish a release with `release-plz` over a copy of
#: the tree. Drafting runs `cargo-semver-checks`, which builds the rustdoc of
#: every crate the release changes; two of those beside each other and the
#: gate's own test tier push one past the bound its journey sets on the
#: program, so the group runs them one at a time.
RELEASE_DRAFTING = pytest.mark.xdist_group("release-plz-drafting")

# What every other journey shares with another running beside it, it only reads
# or syncs: the repository's `node_modules`, which a copy links to and a frozen
# `bun install` over an unchanged lockfile leaves alone; the repository's `.venv`
# and the tool caches, which `uv`, `cargo` and `bun` each lock themselves; and
# the bootstrap's tools, which it installs only where a host lacks one. A copy's
# own build, a stand-in bound to port 0, and each worker's `shared_venv` are
# that journey's or that worker's alone.

# Every escape sequence a terminal-aware program writes here: a CSI sequence —
# introducer, numeric and private parameters, intermediates, final byte — and an
# OSC sequence, which `nx` uses for hyperlinks and which ends at BEL or at ST.
#: The author and committer every commit a journey writes is made under, as
#: `git` options: a runner holds no configured identity, and `git commit` with
#: none exits 128 saying nothing a test would show.
COMMIT_IDENTITY = ("-c", "user.email=e2e@printobserver.test", "-c", "user.name=e2e")

ANSI_ESCAPE = re.compile(r"\x1b\[[0-9;:?]*[ -/]*[@-~]|\x1b\][^\x07]*(?:\x07|\x1b\\)")


def pythonpath() -> str:
    """The packages this repository's own tools live in, as a search path.

    Read out of the justfile's own list rather than repeated here, so a package
    this repository grows is one a journey's subprocesses find without being
    told — and joined with this host's own separator, as the justfile's export
    joins it, because a Windows interpreter reads a `:`-joined path as one entry.
    """
    for line in (REPO_ROOT / "justfile").read_text(encoding="utf-8").splitlines():
        if line.startswith("TOOL_PACKAGES :="):
            return os.pathsep.join(line.partition(":=")[2].strip().strip('"').split(":"))
    message = "the justfile lists no TOOL_PACKAGES, and these tools live on it"
    raise AssertionError(message)


#: The one Agent Skill this repository publishes, which `gh skill install`
#: puts on a host: the skill's own directory, alone.
SKILL_DIRECTORY = REPO_ROOT / "skills" / "printobserver"


def install_the_skill(configuration: Path) -> Path:
    """Put the agent's skill where one server configuration names it.

    The program carries no skill: the install path installs it with `gh skill
    install nickderobertis/printobserver printobserver --dir <state>/skills`, and
    the installer's configuration names `<state>/skills/printobserver/SKILL.md`.
    This lays down what that install does — the committed skill's directory
    alone, as regular files — at the directory the configuration's own
    `supervisor.skill_path` names, and answers that directory.
    """
    with configuration.open("rb") as handle:
        document = tomllib.load(handle)
    supervisor = document.get("supervisor")
    named = supervisor.get("skill_path") if isinstance(supervisor, dict) else None
    if not isinstance(named, str) or not named:
        message = f"{configuration} names no `supervisor.skill_path` to install the skill at"
        raise AssertionError(message)
    installed = Path(named).parent
    # llmlint: ignore[e2e_not_mocked, tests_mirror_real_usage] suppressions.toml has the reason.
    shutil.copytree(SKILL_DIRECTORY, installed, dirs_exist_ok=True)
    return installed


def tracked_files(root: Path) -> list[str]:
    """Every file a clone would carry once this change lands.

    `--others --exclude-standard` includes files this change has added but not
    yet committed, and excludes everything `.gitignore` covers. Reading only the
    index would copy a tree missing exactly the files the change is about.
    """
    listing = shell_run(
        ["git", "ls-files", "-z", "--cached", "--others", "--exclude-standard"],
        cwd=root,
        check=True,
    ).stdout
    return [name for name in listing.split("\0") if name]


@functools.cache
def listed(root: Path) -> tuple[str, ...]:
    """`tracked_files`, asked of git once per process for each tree.

    Every copy a worker takes in a session is of the same listing, and asking
    git for it once a copy was a child process each journey's copy paid for.
    What a file holds is read when it is copied; only which files there are is
    kept.
    """
    return tuple(tracked_files(root))


def copy_tracked(destination: Path, *, root: Path = REPO_ROOT) -> Path:
    """Copy the committed tree, symlinks and all, carrying no build products."""
    destination.mkdir(parents=True, exist_ok=True)
    for name in listed(root):
        source = root / name
        if not source.exists() and not source.is_symlink():
            # A file the index still lists and the working tree no longer
            # has: a deletion nobody has staged yet. A clone would not carry it.
            continue
        target = destination / name
        target.parent.mkdir(parents=True, exist_ok=True)
        if source.is_symlink():
            # Windows keeps file and directory links apart: a link to a
            # directory made as a file link is one nothing can open there, so
            # the kind is read off what the link reaches. Elsewhere the flag
            # is ignored.
            target.symlink_to(source.readlink(), target_is_directory=source.is_dir())
        else:
            shutil.copy2(source, target)
    return destination


#: Every journey that runs Nx in a copy nobody will clean up after runs it
#: without the daemon a local Nx would otherwise start for that copy and leave
#: running once the journey is over.
QUIET = {"NX_DAEMON": "false", "NX_NO_CLOUD": "true"}

#: The `uv` settings that stop a copy's own commands from syncing the copy's own
#: lock into the environment they run in: skipping the sync, discovering no
#: project or another one, and leaving a dependency group out of it. A host may
#: export any of them for its own work — one orchestrating agents exports
#: `UV_NO_SYNC` — and inherited, the first `uv run` of a fresh environment finds
#: none of the copy's dependencies and fails on a missing module.
DEPENDENCY_SYNC_SUPPRESSION = (
    "UV_NO_SYNC",
    "UV_NO_PROJECT",
    "UV_PROJECT",
    "UV_WORKING_DIR",
    "UV_NO_DEV",
    "UV_NO_GROUP",
    "UV_NO_DEFAULT_GROUPS",
)


def clean_environment(**extra: str) -> dict[str, str]:
    """The environment a journey's subprocesses run under.

    The caller's, minus this checkout's own activated virtualenv and every
    setting in `DEPENDENCY_SYNC_SUPPRESSION`, and with colouring pinned on.
    `FORCE_COLOR` is not decoration: the publication harness runs the gate under
    a terminal that colours it, so `bun` and `nx` there emit exactly the escapes
    `capture` strips — while a run started by hand from a plain pipe emits none.
    Pinning it makes the suite prove the harder of the two outputs here as well
    as there, rather than passing on this machine and failing on the one that
    pushes.
    """
    environment = dict(os.environ)
    for name in ("VIRTUAL_ENV", *DEPENDENCY_SYNC_SUPPRESSION):
        environment.pop(name, None)
    environment["FORCE_COLOR"] = "1"
    environment.update(extra)
    return environment


def plain(text: str) -> str:
    """A stream as a reader sees it, with the terminal's control sequences gone."""
    return ANSI_ESCAPE.sub("", text)


def capture(
    command: list[str], cwd: Path, *, timeout: int, env: dict[str, str]
) -> subprocess.CompletedProcess[str]:
    """Run a real command and hand back what it said rather than how it was drawn.

    A colouring `bun` writes `250`, ESC-bracket-0-m, ` packages`, another of
    those, then ` installed`; a colouring `nx` writes `Successfully ran target `
    and then one before `format-check`. Both say what a reader reads and neither
    *contains* the phrase a journey asserts on, so `contains` fails against
    output that is exactly right — and, the way round that costs more, `absent`
    passes against output saying precisely what it forbids. Stripping here is
    what makes every assertion in this tier about the text a run produced rather
    than about the escapes a terminal was going to draw it with.
    """
    result = shell_run(command, cwd=cwd, timeout=timeout, env=env)
    return subprocess.CompletedProcess(
        result.args,
        result.returncode,
        plain(result.stdout or ""),
        plain(result.stderr or ""),
    )


def run(command: list[str], cwd: Path, *, timeout: int = 900) -> subprocess.CompletedProcess[str]:
    """Run a real command in a real directory and hand back everything it said."""
    return capture(command, cwd, timeout=timeout, env=clean_environment())


def output(result: subprocess.CompletedProcess[str]) -> str:
    """Everything a run said, on either stream."""
    return result.stdout + result.stderr


class GateCopy:
    """A copy of the committed tree the gate can be run over."""

    def __init__(self, root: Path, shared: Path, *, node_modules: bool = True) -> None:
        """Prepare a copy of the tree, with or without its dependencies installed.

        `node_modules=False` is the clone a publication is cut from: one that has
        never run `just bootstrap`, and in which the gate has to heal that state
        for itself.
        """
        self.root = copy_tracked(root)
        init_repository(self.root)
        for args in (
            ["add", "-A"],
            [*COMMIT_IDENTITY, "commit", "-q", "-m", "chore: the committed tree, copied"],
        ):
            shell_run(["git", *args], cwd=self.root, check=True)

        if node_modules:
            (self.root / "node_modules").symlink_to(REPO_ROOT / "node_modules")
        self.shared_venv = shared

    def read(self, relative: str) -> str:
        """Read a file of the copy."""
        return (self.root / relative).read_text(encoding="utf-8")

    def write(self, relative: str, text: str) -> None:
        """Replace a file of the copy."""
        path = self.root / relative
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_text(text, encoding="utf-8")

    def edit(self, relative: str, old: str, new: str) -> None:
        """Replace one exact fragment, refusing a no-op edit."""
        text = self.read(relative)
        if old not in text:
            msg = f"{relative} does not contain {old!r}; the fixture is stale"
            raise AssertionError(msg)
        self.write(relative, text.replace(old, new, 1))

    def hook(self, name: str, *, timeout: int = 1800) -> subprocess.CompletedProcess[str]:
        """Run a committed git hook the way git runs it, `GIT_DIR` and all.

        git exports `GIT_DIR` into every hook it runs — so that variable
        reaches every check and suite a hook starts, and each of them works on
        the repository the hook was invoked for unless something drops it.
        `env` puts it back on the way in, because `repo_checks.shell.run` is
        the thing that drops it.
        """
        return capture(
            [
                "env",
                f"GIT_DIR={self.root / '.git'}",
                str(self.root / ".githooks" / name),
            ],
            self.root,
            timeout=timeout,
            env=clean_environment(UV_PROJECT_ENVIRONMENT=str(self.shared_venv)),
        )

    def just(
        self,
        recipe: str,
        *arguments: str,
        timeout: int = 900,
        environment: dict[str, str] | None = None,
    ) -> subprocess.CompletedProcess[str]:
        """Run one recipe of the copy's own command surface, `environment` added to its own."""
        return capture(
            ["just", recipe, *arguments],
            self.root,
            timeout=timeout,
            env=clean_environment(
                UV_PROJECT_ENVIRONMENT=str(self.shared_venv), **(environment or {})
            ),
        )

    def git(self, *arguments: str) -> str:
        """Run git in the copy, as the identity its first commit was made under."""
        return shell_run(
            ["git", *COMMIT_IDENTITY, *arguments], cwd=self.root, check=True
        ).stdout.strip()


class CopiesTheTree(Protocol):
    """What the `gate_copy` fixture is: a factory for copies of the committed tree.

    `node_modules=False` is the copy a publication is cut from — one that has
    never run `just bootstrap` — and the one a journey that copies the whole
    tree aside, or refuses a symbolic link out of it, is driven over.
    """

    def __call__(self, *, node_modules: bool = True) -> GateCopy:
        """Make one more copy, with or without the JavaScript dependencies linked in."""
        ...
