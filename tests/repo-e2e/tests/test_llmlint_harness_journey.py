"""The judged tier gets an agent to judge through, wherever it runs.

`scripts/setup-llmlint.sh` installs `llmlint` and `oneharness`, and oneharness
drives a *separate* agent binary that neither of those carries. On a host with
none of them — a continuous-integration runner, where nothing else installs an
agent — every candidate in `oneharness.toml`'s fallback chain is skipped as
uninstalled and the tier errors with "all harnesses in the fallback chain
failed", having judged nothing.

That is not hypothetical. It is how the `llmlint` required check failed on this
repository's first branch, and it read as a broken toolchain rather than as a
missing agent. It had looked green on `main` only because a push to `main`
diffs against `origin/main` and finds nothing, so no rule ran to need one.

Each journey runs the real recipe over a real copy, on a host of its own: a PATH
carrying exactly the programs the script may find, and a HOME it installs into.
The npm registry is the one thing stood in for, by a recorder that writes the
binary a real install would write — so these prove the script's own decision and
its wiring, driven end to end, without a package download inside the gate.
"""

from __future__ import annotations

import shutil
import subprocess
from collections.abc import Callable
from pathlib import Path

import pytest
from journey import HERE, REPO_ROOT, GateCopy, capture, clean_environment, output
from repo_checks import platforms
from repo_checks.expect import absent, contains, passing, truth
from repo_checks.model import Repo
from repo_checks.parsing import jobs_of, load_workflow, marker_block

HARNESS_PACKAGE = "@anthropic-ai/claude-code"
#: The judged tier's job, which is the one that runs the installer these
#: journeys drive, and the workflow that declares it.
LLMLINT_JOB = "llmlint"
CI_WORKFLOW = ".github/workflows/ci.yml"


def _installer_platforms() -> list[str]:
    """The platforms the `llmlint` job runs on, read off its own `runs-on`.

    Resolved through the supported-platform table rather than compared as a
    runner name, so this journey follows that job wherever the table says its
    runner is — and so a job that grew a matrix would grow this journey's
    hosts with it, with nobody editing a test.
    """
    job = jobs_of(load_workflow(REPO_ROOT / CI_WORKFLOW))[LLMLINT_JOB]
    runners = job["runs-on"] if isinstance(job["runs-on"], list) else [job["runs-on"]]
    return [
        platform.id
        for platform in platforms.supported(Repo(REPO_ROOT))
        if platform.runner in runners
    ]


def _unmatrixed_record() -> str:
    """AGENTS.md's own line on why the `llmlint` job carries no platform matrix."""
    for line in marker_block(Repo(REPO_ROOT).agents_md, platforms.UNMATRIXED_BLOCK):
        match = platforms.UNMATRIXED_LINE.match(line)
        if match is not None and match["job"] == LLMLINT_JOB:
            return line
    message = (
        f"AGENTS.md records no unmatrixed `{LLMLINT_JOB}` job, and this journey is keyed to it"
    )
    raise AssertionError(message)


#: These journeys run where the judged tier's job runs, and nowhere else. The
#: installer they drive is a Unix-shaped shell script — it installs under
#: `$HOME/.local/bin`, which is where `uv tool` and `npm --prefix` put programs
#: on Unix and not on Windows — run by that single-platform job and by agent
#: sessions, and PyPI ships no `win_arm64` wheel of `llmlint-cli` or
#: `oneharness-cli` at all, so on a Windows cell these journeys could only prove
#: something this repository never does.
pytestmark = pytest.mark.skipif(
    HERE.id not in _installer_platforms(),
    reason=(
        f"the judged tier's installer is proven where its job runs, and `{LLMLINT_JOB}` runs "
        f"on {_installer_platforms()} rather than on `{HERE.id}`; AGENTS.md's record of why "
        f"that job carries no platform matrix: {_unmatrixed_record()!r}. The installer is a "
        f"Unix-shaped shell script run only by that job and by agent sessions, and PyPI "
        f"ships no win_arm64 wheel of llmlint-cli or oneharness-cli"
    ),
)

# `uv` is what step 1 of the script installs through, and `bash` and `python3` —
# which `just` and the chain lookup reach for — live in the system directories.
# The platforms this runs on are Linux, which is where the `llmlint` job runs.
UV_DIRECTORY = str(Path(shutil.which("uv") or "uv").parent)
SYSTEM_DIRECTORIES = "/usr/bin:/bin"
# `oneharness detect` probes a harness by running it, so one that is installed
# has to answer for a version. Both the agent this host starts out carrying and
# the one its npm "installs" are this same program, defined once here and handed
# to the recorder through the environment rather than quoted into it.
HARNESS_BINARY = '#!/bin/sh\necho "2.1.263 (Claude Code)"\n'
# Stands in for the npm registry: records the arguments it was called with, and
# writes the binary a real `npm install -g --prefix <dir>` leaves behind, so a
# journey can assert on where that binary landed.
NPM_RECORDER = """#!/usr/bin/env bash
printf '%s\\n' "$*" >> "$NPM_CALLS"
prefix=""
for ((index = 1; index <= $#; index++)); do
  if [ "${!index}" = "--prefix" ]; then
    next=$((index + 1))
    prefix="${!next}"
  fi
done
mkdir -p "$prefix/bin"
printf '%s' "$HARNESS_SOURCE" > "$prefix/bin/claude"
chmod +x "$prefix/bin/claude"
"""


class HarnessHost:
    """A host carrying exactly the programs a journey wants the script to find."""

    def __init__(self, root: Path, cache: Path, *, npm: bool = True, harness: bool = False) -> None:
        """Lay out a host: a HOME to install into and a PATH to install from."""
        self.home = root / "home"
        self.programs = root / "programs"
        self.npm_calls = root / "npm-calls"
        self.github_path = root / "github-path"
        self.cache = cache
        self.home.mkdir(parents=True)
        self.programs.mkdir(parents=True)
        self.github_path.write_text("", encoding="utf-8")
        if npm:
            self._program("npm", NPM_RECORDER)
        if harness:
            self._program("claude", HARNESS_BINARY)

    def _program(self, name: str, source: str) -> None:
        path = self.programs / name
        path.write_text(source, encoding="utf-8")
        path.chmod(0o755)

    @property
    def harness_binary(self) -> Path:
        """Where an installed harness lands: the directory the script puts on PATH."""
        return self.home / ".local" / "bin" / "claude"

    def npm_installs(self) -> str:
        """Every argument list this host's npm was called with, or nothing."""
        return self.npm_calls.read_text(encoding="utf-8") if self.npm_calls.exists() else ""

    def environment(self) -> dict[str, str]:
        """The environment the recipe runs under on this host.

        `CLAUDE_ENV_FILE` and `GITHUB_PATH` are redirected rather than inherited:
        the script appends to both, and a journey that inherited them would write
        into the session or the workflow job running it. Every `ONEHARNESS_*` and
        `LLMLINT_*` variable is dropped, because those layer over the copy's own
        `oneharness.toml` and it is the committed file these journeys are about.
        """
        environment = {
            name: value
            for name, value in clean_environment().items()
            if not name.startswith(("ONEHARNESS_", "LLMLINT_"))
        }
        environment.pop("CLAUDE_ENV_FILE", None)
        environment.update(
            HOME=str(self.home),
            PATH=f"{self.programs}:{UV_DIRECTORY}:{SYSTEM_DIRECTORIES}",
            NPM_CALLS=str(self.npm_calls),
            HARNESS_SOURCE=HARNESS_BINARY,
            GITHUB_PATH=str(self.github_path),
            # One toolchain download for the whole session rather than one per
            # journey: a HOME of its own would put uv's cache somewhere cold.
            UV_CACHE_DIR=str(self.cache),
        )
        return environment

    def setup_llmlint(self, copy: GateCopy) -> subprocess.CompletedProcess[str]:
        """Run the copy's real `setup-llmlint` recipe against this host."""
        return capture(["just", "setup-llmlint"], copy.root, timeout=600, env=self.environment())


@pytest.fixture(scope="session")
def uv_cache(tmp_path_factory: pytest.TempPathFactory) -> Path:
    """One uv cache every host shares, so the toolchain is fetched once."""
    return tmp_path_factory.mktemp("uv-cache")


@pytest.fixture
def harness_host(tmp_path: Path, uv_cache: Path) -> Callable[..., HarnessHost]:
    """A factory for hosts carrying a chosen set of programs."""
    counter = {"n": 0}

    def make(*, npm: bool = True, harness: bool = False) -> HarnessHost:
        counter["n"] += 1
        return HarnessHost(tmp_path / f"host{counter['n']}", uv_cache, npm=npm, harness=harness)

    return make


def test_a_host_carrying_no_agent_gets_one(
    gate_copy: Callable[..., GateCopy], harness_host: Callable[..., HarnessHost]
) -> None:
    """The runner case: nothing else installs an agent, so this step has to."""
    copy = gate_copy()
    host = harness_host()

    result = host.setup_llmlint(copy)

    passing(result)
    contains(output(result), "no harness oneharness can spawn")
    contains(host.npm_installs(), HARNESS_PACKAGE)
    truth(
        host.harness_binary.is_file(),
        describing="the agent to land in the directory the script puts on PATH",
    )


def test_a_host_that_already_carries_an_agent_installs_none(
    gate_copy: Callable[..., GateCopy], harness_host: Callable[..., HarnessHost]
) -> None:
    """A contributor's own authenticated agent is the one the tier judges through."""
    copy = gate_copy()
    host = harness_host(harness=True)

    result = host.setup_llmlint(copy)

    passing(result)
    # The no-op itself first, so a detection that stopped working fails on the
    # install it made rather than on the line it did not log.
    absent(host.npm_installs(), HARNESS_PACKAGE)
    contains(output(result), "harness `claude-code` is installed")


def test_the_installed_agent_reaches_the_step_that_runs_the_tier(
    gate_copy: Callable[..., GateCopy], harness_host: Callable[..., HarnessHost]
) -> None:
    """Each step of a workflow job is a new shell, which an export does not reach."""
    copy = gate_copy()
    host = harness_host()

    result = host.setup_llmlint(copy)

    passing(result)
    contains(
        host.github_path.read_text(encoding="utf-8"),
        str(host.harness_binary.parent),
        describing="the directory the next step has to find the agent in",
    )


def test_a_host_with_no_way_to_install_says_so_rather_than_failing(
    gate_copy: Callable[..., GateCopy], harness_host: Callable[..., HarnessHost]
) -> None:
    """A setup this script cannot complete must never break session startup."""
    copy = gate_copy()
    host = harness_host(npm=False)

    result = host.setup_llmlint(copy)

    passing(result)
    contains(output(result), "npm absent")
    truth(
        not host.harness_binary.exists(),
        describing="no agent, rather than a broken one, where none could be installed",
    )
