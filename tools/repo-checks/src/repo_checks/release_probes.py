"""Every declared target, asked of the registry that serves it, through the committed probe.

The probe restates four producers' contracts — crates.io's crate document,
PyPI's project JSON, npm's `latest` document and github.com's release
redirects — and its journey proves it against doubles of them. Those doubles
cannot notice a producer changing its answer, so this is the reconciliation:
the committed probe, run the way onevcs runs it, over every target the
declaration names, against the real registries. Every one of them is
published, so each must answer a version; a target answering "no release" is
a probe misreading a registry, and one answering nothing is a registry it can no
longer read.

`just probe-registries` runs it, and the registry install-path proof's script
route calls that recipe, so it runs on that proof's schedule and after every
release. It reads the real registries, so it is no tier of the gate.
"""

from __future__ import annotations

import os
import re
import sys
from pathlib import Path
from typing import NamedTuple

from repo_checks import release_declaration
from repo_checks.model import Repo
from repo_checks.shell import run

#: How long one run of the probe is given: onevcs's own bound on a probe.
PROBE_TIMEOUT_SECONDS = 60

#: A semantic version (semver.org 2.0.0), the one answer a released target has.
_IDENTIFIER = r"(?:0|[1-9][0-9]*|[0-9]*[A-Za-z-][0-9A-Za-z-]*)"
SEMVER = re.compile(
    r"(?:0|[1-9][0-9]*)\.(?:0|[1-9][0-9]*)\.(?:0|[1-9][0-9]*)"
    rf"(?:-{_IDENTIFIER}(?:\.{_IDENTIFIER})*)?(?:\+[0-9A-Za-z-]+(?:\.[0-9A-Za-z-]+)*)?"
)


class Asked(NamedTuple):
    """One target and what the probe answered for it."""

    target: release_declaration.Target
    code: int
    stdout: str
    stderr: str


def _probe(repo: Repo) -> Path | None:
    """The probe `release-targets.toml` names, or `None` where it names none."""
    named = repo.read_toml(release_declaration.DECLARATION).get("probe")
    return repo.path(named) if isinstance(named, str) and named.strip() else None


def _environment() -> dict[str, str]:
    """What onevcs hands a probe: `PATH` and `HOME`, and nothing else."""
    return {"PATH": os.environ.get("PATH", ""), "HOME": os.environ.get("HOME", str(Path.home()))}


def _ask(repo: Repo, probe: Path, target: release_declaration.Target) -> Asked:
    said = run(
        [str(probe), target.id],
        cwd=repo.root,
        env=_environment(),
        timeout=PROBE_TIMEOUT_SECONDS,
    )
    return Asked(target, said.returncode, said.stdout or "", said.stderr or "")


def _finding(asked: Asked) -> str | None:
    """What is wrong with one answer, or `None` for a version."""
    identifier = asked.target.id
    if asked.code == 0 and SEMVER.fullmatch(asked.stdout.removesuffix("\n")):
        return None
    if asked.code == 0 and asked.stdout:
        return (
            f"{identifier}: the probe answered {asked.stdout!r}, which is not one line holding "
            f"one semantic version"
        )
    if asked.code == 0:
        return (
            f"{identifier}: the probe answered no release, and this repository publishes it: "
            f"the probe is misreading what its registry serves"
        )
    reason = " ".join(asked.stderr.split()) or f"exit {asked.code} and nothing said"
    return f"{identifier}: the probe did not answer: {reason}"


def probe_registries(repo: Repo) -> int:
    """Ask the real registries for every declared target; fail unless each answers a version.

    One line per target on stdout, `<name> <id> <version>`, so a reader of a
    scheduled run sees what each registry serves; every finding on stderr.
    """
    probe = _probe(repo)
    if probe is None:
        print(
            f"probe-registries: {release_declaration.DECLARATION} names no `probe`, so "
            f"nothing answers what a registry serves",
            file=sys.stderr,
        )
        return 1
    if sys.platform == "win32":
        print(
            "probe-registries: not run on Windows: the probe is a bash script a Unix host "
            "spawns directly, and the Unix cells of this run ask the same registries",
            file=sys.stderr,
        )
        return 0
    findings: list[str] = []
    for target in release_declaration.joined(repo.root):
        asked = _ask(repo, probe, target)
        finding = _finding(asked)
        if finding is None:
            print(f"{target.name} {target.id} {asked.stdout.strip()}")
        else:
            findings.append(finding)
    for finding in findings:
        print(f"probe-registries: {finding}", file=sys.stderr)
    return 1 if findings else 0
