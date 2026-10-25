"""Every declared target, asked of the registry that serves it, through the committed probe.

The probe restates four producers' contracts — crates.io's crate document,
PyPI's project JSON, npm's `latest` document and github.com's release
redirects — and its journey proves it against doubles of them. Those doubles
cannot notice a producer changing its answer, so this is the reconciliation:
the committed probe, run the way onevcs runs it, over every target the
declaration names, against the real registries. Each must answer one of the
two answers a consumer acts on — a version, or no release, which a target the
declaration names before its first publication honestly answers — and one that
is not answered, or answers something that is not one version, is a registry
the probe can no longer read.

`just probe-registries` runs it, and the registry install-path proof's script
route calls that recipe, so it runs on that proof's schedule and after every
release. It reads the real registries, so it is no tier of the gate.
"""

from __future__ import annotations

import os
import re
import sys
from functools import cache
from pathlib import Path
from typing import NamedTuple

from repo_checks import release_declaration
from repo_checks.model import Repo
from repo_checks.shell import run

# llmlint: ignore-block[contracts_have_one_source_or_a_drift_gate] suppressions.toml has the reason.
#: How long one run of the probe is given: onevcs's own bound on a probe.
PROBE_TIMEOUT_SECONDS = 60
# llmlint: ignore-end[contracts_have_one_source_or_a_drift_gate]

#: The committed probe, whose own grammar of a semantic version is the one
#: this reconciliation holds an answer to, so the two cannot disagree.
COMMITTED_PROBE = Path(__file__).with_name("release-probe.sh")


def semver(script: str) -> re.Pattern[str]:
    """The probe's `SEMVER`, expanded the way bash expands those two assignments."""
    identifier = re.search(r"^readonly SEMVER_IDENTIFIER='([^']*)'$", script, re.MULTILINE)
    pattern = re.search(r'^readonly SEMVER="([^"]*)"$', script, re.MULTILINE)
    if identifier is None or pattern is None:
        message = (
            f"{COMMITTED_PROBE.name} assigns no `readonly SEMVER_IDENTIFIER='...'` and "
            f'`readonly SEMVER="..."`, which is the grammar a released version is read by'
        )
        raise ValueError(message)
    expanded = pattern[1].replace("${SEMVER_IDENTIFIER}", identifier[1])
    return re.compile(expanded.replace("\\\\", "\\").replace("\\$", "$"))


@cache  # pragma: unreached on win32 - Windows spawns no probe
def _committed_semver() -> re.Pattern[str]:
    return semver(COMMITTED_PROBE.read_text(encoding="utf-8"))


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


# llmlint: ignore-block[contracts_have_one_source_or_a_drift_gate] suppressions.toml has the reason.
def _environment() -> dict[str, str]:  # pragma: unreached on win32 - Windows spawns no probe
    """What onevcs hands a probe: `PATH` and `HOME`, and nothing else."""
    return {"PATH": os.environ.get("PATH", ""), "HOME": os.environ.get("HOME", str(Path.home()))}


# llmlint: ignore-end[contracts_have_one_source_or_a_drift_gate]


def _ask(  # pragma: unreached on win32 - Windows spawns no probe
    repo: Repo, probe: Path, target: release_declaration.Target
) -> Asked:
    said = run(
        [str(probe), target.id],
        cwd=repo.root,
        env=_environment(),
        timeout=PROBE_TIMEOUT_SECONDS,
    )
    return Asked(target, said.returncode, said.stdout or "", said.stderr or "")


#: What points the registry install-path proof at stand-in registries rather
#: than the real ones: `release_artifacts.registries`' own variable.
STANDING_IN = "PRINTOBSERVER_PROOF_REGISTRIES"

#: What a reconciled target answered when its registry serves nothing yet.
NO_RELEASE = "no-release"


def _answered(asked: Asked) -> str | None:  # pragma: unreached on win32 - Windows spawns no probe
    """The version or `NO_RELEASE` one answer gave, or `None` where it gave neither."""
    match asked:
        case Asked(code=0, stdout=""):
            return NO_RELEASE
        case Asked(code=0, stdout=answer) if _committed_semver().fullmatch(
            answer.removesuffix("\n")
        ):
            return answer.strip()
        case _:
            return None


def _finding(asked: Asked) -> str:  # pragma: unreached on win32 - Windows spawns no probe
    """What is wrong with an answer that was neither a version nor no release."""
    identifier = asked.target.id
    match asked:
        case Asked(code=0, stdout=answer):
            return (
                f"{identifier}: the probe answered {answer!r}, which is not one line holding "
                f"one semantic version"
            )
        case _:
            reason = " ".join(asked.stderr.split()) or f"exit {asked.code} and nothing said"
            return f"{identifier}: the probe did not answer: {reason}"


def _asked_all(
    repo: Repo, probe: Path
) -> list[str]:  # pragma: unreached on win32 - Windows spawns no probe
    """Run the probe for every declared target, printing each answer; the findings it left."""
    findings: list[str] = []
    for target in release_declaration.joined(repo.root):
        asked = _ask(repo, probe, target)
        answered = _answered(asked)
        if answered is None:
            findings.append(_finding(asked))
        else:
            print(f"{target.name} {target.id} {answered}")
    return findings


def probe_registries(repo: Repo) -> int:
    """Ask the real registries for every declared target; fail on any not answered.

    One line per target on stdout, `<name> <id> <version>` or `<name> <id>
    no-release`, so a reader of a scheduled run sees what each registry serves;
    every finding on stderr. A run whose proof is pointed at stand-in
    registries asks nothing and says nothing.
    """
    probe = _probe(repo)
    if probe is None:
        print(
            f"probe-registries: {release_declaration.DECLARATION} names no `probe`, so "
            f"nothing answers what a registry serves",
            file=sys.stderr,
        )
        return 1
    # A proof pointed at stand-in registries is the gate's own journey over
    # doubles, and the probe reaches only the real registries: asking them there
    # would put the live state of five producers into the gate. That proof is
    # held to one line on a pass, so this adds none to it.
    if os.environ.get(STANDING_IN, "").strip():
        return 0
    # A declaration this repository's own reader refuses, or one declaring
    # nothing, would reconcile nothing and pass; both are failures instead, on
    # every host, because reading the declaration needs no probe.
    findings = release_declaration.drift(repo.root)
    if sys.platform != "win32":
        findings += _asked_all(repo, probe)  # pragma: unreached on win32 - Windows spawns no probe
    elif not findings:
        print(
            "probe-registries: not run on Windows: the probe is a bash script a Unix host "
            "spawns directly, and the Unix cells of this run ask the same registries",
            file=sys.stderr,
        )
    for finding in findings:
        print(f"probe-registries: {finding}", file=sys.stderr)
    return 1 if findings else 0
