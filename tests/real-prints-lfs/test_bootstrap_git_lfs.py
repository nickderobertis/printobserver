"""`just bootstrap`'s git-lfs step, on a host carrying no git-lfs, as an ordinary user.

The step is the repository's own — `just install-tools git-lfs`, then the
`install-hooks` that configures a clone's LFS filter — run with `HOME`, and so
the user install directory `~/.local/bin`, in a sandbox, and a PATH on which no
git-lfs is found. It downloads the real release `repo-policy.toml` holds from
the forge and verifies it against that release's own checksums, so this is the
one test here that reaches the network. Nothing it does needs, or asks for,
administrator rights: everything it writes is under the sandbox's home.
"""

from __future__ import annotations

import os
import re
import shutil
import tomllib
from pathlib import Path

import pytest
from lfs_world import REPO_ROOT, path_without
from repo_checks.expect import contains, equal
from repo_checks.fixture_repository import init_repository
from repo_checks.shell import run

#: The packages the justfile puts on the path for `python -m repo_checks`.
TOOL_PACKAGES = os.pathsep.join(
    str(REPO_ROOT / "tools" / name / "src")
    for name in ("repo-checks", "contract-codegen", "release-artifacts")
)


def held_release() -> str:
    """The release `repo-policy.toml` holds git-lfs at."""
    policy = tomllib.loads((REPO_ROOT / "repo-policy.toml").read_text(encoding="utf-8"))
    return next(t["version"] for t in policy["toolchain"]["tool"] if t["command"] == "git-lfs")


def test_bootstrap_installs_the_held_git_lfs_per_user_and_configures_the_clone(
    tmp_path: Path,
) -> None:
    """The held release ends up in the user's own bin, and a clone's filter in its local config."""
    home = tmp_path / "home"
    home.mkdir()
    user_bin = home / ".local" / "bin"
    uv_cache = run(["uv", "cache", "dir"], check=True).stdout.strip()
    path = os.pathsep.join(
        [str(user_bin), path_without("git-lfs", ("just", "uv", "git"), tmp_path / "shims")]
    )
    environment = {
        **os.environ,
        "HOME": str(home),
        "USERPROFILE": str(home),
        "XDG_CONFIG_HOME": str(home / ".config"),
        "GIT_CONFIG_NOSYSTEM": "1",
        "PATH": path,
        "UV_CACHE_DIR": uv_cache,
    }
    equal(shutil.which("git-lfs", path=path), None, describing="a git-lfs on the sandbox's PATH")

    installed = run(
        ["just", "install-tools", "git-lfs"], cwd=REPO_ROOT, env=environment, check=False
    )
    equal(installed.returncode, 0, describing=f"the step's exit, having said:\n{installed.stderr}")

    found = shutil.which("git-lfs", path=path)
    if found is None:
        pytest.fail(f"no git-lfs on PATH after the step, which said:\n{installed.stderr}")
    equal(Path(found).parent, user_bin, describing="the directory git-lfs was installed into")
    answered = run([found, "--version"], env=environment, check=True).stdout
    release = re.match(r"git-lfs/(\S+) ", answered)
    equal(release and release.group(1), held_release(), describing=f"the release in {answered!r}")

    clone = init_repository(tmp_path / "clone", env=environment)
    configured = run(
        ["uv", "run", "-q", "python", "-m", "repo_checks", "install-hooks", "--root", str(clone)],
        cwd=REPO_ROOT,
        env={**environment, "PYTHONPATH": TOOL_PACKAGES},
    )
    equal(configured.returncode, 0, describing=f"install-hooks, having said:\n{configured.stderr}")
    local = run(["git", "config", "--local", "--list"], cwd=clone, env=environment, check=True)
    contains(local.stdout, "filter.lfs.process=git-lfs filter-process")
    contains(local.stdout, "filter.lfs.required=true")
    contains(local.stdout, "core.hookspath=.githooks")
    equal(
        sorted(path.name for path in clone.iterdir() if path.name != ".git"),
        [],
        describing="files the step left in the clone's tree",
    )


def test_the_docs_name_the_release_the_bootstrap_installs() -> None:
    """`AGENTS.md` and the cases' README state the held release, read from the policy."""
    release = held_release()
    for document in ("AGENTS.md", "tests/real-prints/README.md"):
        contains(
            (REPO_ROOT / document).read_text(encoding="utf-8"),
            f"Git LFS {release}",
        )
