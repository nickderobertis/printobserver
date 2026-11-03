"""The sandboxed host the real-print LFS tests drive real git and git-lfs on.

A host here is a home directory of its own — its global git configuration is
the "host configuration" a real machine's would be — and the environment every
command runs under, so nothing a test does reads or writes the caller's own
git configuration. Its LFS remote is a bare repository reached at a file URL,
served by `counting_remote.py`, which records every object it hands out.
"""

from __future__ import annotations

import hashlib
import os
import shutil
import sys
from dataclasses import dataclass, field
from pathlib import Path

from counting_remote import Oid
from repo_checks.fixture_repository import QUIESCENT
from repo_checks.shell import run

HERE = Path(__file__).resolve().parent
REPO_ROOT = HERE.parents[1]
JUSTFILE = REPO_ROOT / "justfile"
COUNTING_REMOTE = HERE / "counting_remote.py"

#: The first line of every Git LFS pointer.
POINTER_VERSION = b"version https://git-lfs.github.com/spec/v1\n"


def git_lfs_or_fail() -> str:
    """The `git-lfs` on PATH, or a failure naming how to get one."""
    found = shutil.which("git-lfs")
    if found is None:
        msg = "git-lfs is not on PATH; `just bootstrap` installs it"
        raise AssertionError(msg)
    return found


def path_without(program: str, shims: Path) -> str:
    """The caller's PATH, every directory of it kept, on which no `program` is found.

    A directory that also holds `program` — `/usr/bin` carries git, bash and
    git-lfs together on many hosts — is reached through a directory of links
    to its every other entry instead, so a step finds every tool it would have
    found but that one.
    """
    names = {program, f"{program}.exe"}
    directories: list[str] = []
    for index, entry in enumerate(filter(None, os.environ.get("PATH", "").split(os.pathsep))):
        directory = Path(entry)
        if not any((directory / name).is_file() for name in names):
            directories.append(entry)
            continue
        linked = shims / str(index)
        linked.mkdir(parents=True, exist_ok=True)
        for item in directory.iterdir():
            if item.name not in names and not (linked / item.name).exists():
                (linked / item.name).symlink_to(item)
        directories.append(str(linked))
    found = shutil.which(program, path=os.pathsep.join(directories))
    if found is not None:
        msg = f"{program} is still found at {found}"
        raise AssertionError(msg)
    return os.pathsep.join(directories)


@dataclass(frozen=True, slots=True)
class Served:
    """One object the fixture's remote served: which, and how many bytes."""

    oid: Oid
    size: int


@dataclass
class Host:
    """One sandboxed machine: its home, its environment, and its record of served objects."""

    root: Path
    remote: Path
    env: dict[str, str] = field(default_factory=dict)

    @property
    def served_log(self) -> Path:
        """Where the remote records each object it serves this host."""
        return self.root / "served"

    def git(self, *argv: str, cwd: Path, check: bool = True) -> str:
        """Run git as this host, answering what it printed."""
        return run(["git", *argv], cwd=cwd, env=self.env, check=check).stdout

    def just(self, recipe: str, *, cwd: Path) -> str:
        """Run one of printobserver's own recipes in the repository at `cwd`."""
        environment = {
            **self.env,
            "JUST_JUSTFILE": str(JUSTFILE),
            "JUST_WORKING_DIRECTORY": str(cwd),
        }
        return run(["just", recipe], cwd=cwd, env=environment, check=True).stdout

    def served(self) -> list[Served]:
        """Every object the remote has served this host, in order."""
        if not self.served_log.exists():
            return []
        lines = self.served_log.read_text(encoding="utf-8").splitlines()
        return [Served(Oid(oid), int(size)) for oid, size in (line.split() for line in lines)]

    def served_bytes(self) -> int:
        """The bytes the remote has served this host."""
        return sum(served.size for served in self.served())

    def forget_served(self) -> None:
        """Start the record of served objects afresh."""
        self.served_log.write_text("", encoding="utf-8")

    def clone(self, name: str, *options: str) -> Path:
        """A fresh clone of the remote, its LFS filter configured as bootstrap configures it."""
        target = self.root / name
        self.git("clone", "-q", *options, self.remote.as_uri(), str(target), cwd=self.root)
        self.git("lfs", "install", "--local", "--skip-repo", cwd=target)
        return target


def make_host(root: Path, remote: Path, **config: str) -> Host:
    """A host with git-lfs installed, whose LFS file URLs reach the counting remote.

    `config` is more of the host's own git configuration, such as `lfs.storage`.
    """
    home = root / "home"
    scratch = root / "scratch"
    for directory in (home, scratch, home / ".config"):
        directory.mkdir(parents=True, exist_ok=True)
    host = Host(root, remote)
    host.env = {
        **os.environ,
        "HOME": str(home),
        "USERPROFILE": str(home),
        "XDG_CONFIG_HOME": str(home / ".config"),
        "GIT_CONFIG_NOSYSTEM": "1",
        "LFS_FIXTURE_REMOTE": str(remote),
        "LFS_FIXTURE_SERVED": str(host.served_log),
        "LFS_FIXTURE_SCRATCH": str(scratch),
    }
    settings = {
        "user.name": "fixture",
        "user.email": "fixture@example.invalid",
        "init.defaultBranch": "main",
        "lfs.standalonetransferagent": "counting",
        "lfs.customtransfer.counting.path": sys.executable,
        "lfs.customtransfer.counting.args": COUNTING_REMOTE.as_posix(),
        "lfs.customtransfer.counting.concurrent": "false",
        **dict(QUIESCENT),
        **config,
    }
    for name, value in settings.items():
        host.git("config", "--global", name, value, cwd=root)
    # The filter in the host's own configuration, as `git lfs install` puts it
    # there and hosted runners and Git for Windows ship it: a clone then runs it
    # from its first checkout, which is what `.lfsconfig` has to hold back.
    host.git("lfs", "install", "--skip-repo", cwd=root)
    host.forget_served()
    return host


def payload(name: str, size: int) -> bytes:
    """Deterministic bytes standing for one fixture file."""
    out = bytearray()
    counter = 0
    while len(out) < size:
        out += hashlib.sha256(f"{name}:{counter}".encode()).digest()
        counter += 1
    return bytes(out[:size])


#: The fixture's files: four distinct LFS objects, one of them at two paths,
#: and two plain files. A few kilobytes in all.
LFS_FILES: dict[str, bytes] = {
    "tests/real-prints/case-a/01-frame.jpg": payload("frame-a", 3000),
    "tests/real-prints/case-a/print.gcode": payload("gcode-a", 2000),
    "tests/real-prints/case-a/crop.png": payload("crop-a", 1500),
    "tests/real-prints/case-b/01-frame.jpeg": payload("frame-b", 2500),
    "tests/real-prints/case-b/same-as-case-a.jpg": payload("frame-a", 3000),
}
PLAIN_FILES: dict[str, bytes] = {
    "tests/real-prints/case-a/case.json": b'{"name": "case-a"}\n',
    "README.md": b"A fixture repository.\n",
}

#: Every distinct object's bytes: what a fetch over an empty store downloads.
FIXTURE_OBJECT_BYTES = sum(
    {hashlib.sha256(data).hexdigest(): len(data) for data in LFS_FILES.values()}.values()
)
FIXTURE_OBJECTS = len({hashlib.sha256(data).hexdigest() for data in LFS_FILES.values()})


def is_pointer(path: Path) -> bool:
    """Whether a working-tree file is a Git LFS pointer rather than its content."""
    return path.read_bytes().startswith(POINTER_VERSION)


def differing_from_originals(checkout: Path) -> list[str]:
    """The fixture files whose working-tree bytes are not their originals': none when usable."""
    return [
        relative
        for relative, data in LFS_FILES.items()
        if (checkout / relative).read_bytes() != data
    ]


def publish_fixture(root: Path) -> tuple[Host, Path]:
    """The fixture's remote, holding one commit of the fixture and its LFS objects.

    The source repository carries the tree's own `.gitattributes` and
    `.lfsconfig`, and the committed git-lfs hooks; its push goes through the
    committed `pre-push`, which is what uploads the objects.
    """
    remote = root / "remote.git"
    run(["git", "init", "-q", "--bare", "-b", "main", str(remote)], cwd=root, check=True)
    publisher = make_host(root / "publisher", remote)
    source = publisher.root / "source"
    source.mkdir()
    publisher.git("init", "-q", cwd=source)
    for name in (".gitattributes", ".lfsconfig"):
        shutil.copy2(REPO_ROOT / name, source / name)
    hooks = source / ".githooks"
    hooks.mkdir()
    for hook in ("pre-push", "post-checkout", "post-commit", "post-merge"):
        shutil.copy2(REPO_ROOT / ".githooks" / hook, hooks / hook)
    publisher.git("config", "core.hooksPath", ".githooks", cwd=source)
    publisher.git("lfs", "install", "--local", "--skip-repo", cwd=source)
    for relative, data in {**LFS_FILES, **PLAIN_FILES}.items():
        target = source / relative
        target.parent.mkdir(parents=True, exist_ok=True)
        target.write_bytes(data)
    publisher.git("add", "-A", cwd=source)
    publisher.git("commit", "-q", "-m", "the fixture's cases", cwd=source)
    publisher.git("remote", "add", "origin", remote.as_uri(), cwd=source)
    publisher.git("push", "-q", "origin", "main", cwd=source)
    return publisher, source
