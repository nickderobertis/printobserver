"""Checks over what this repository publishes and how it publishes it."""

from __future__ import annotations

import re
import tomllib
from pathlib import Path
from typing import Any

from repo_checks.model import UNCOMMITTED_DIRECTORIES, Repo
from repo_checks.parsing import jobs_of, load_workflow, programs_in, run_commands, steps_of

MANIFEST_NAMES = ("Cargo.toml", "pyproject.toml", "package.json")
SKIPPED_DIRECTORIES = UNCOMMITTED_DIRECTORIES
HALTS_FOR_A_PERSON = ("manual-approval", "wait-for-approval", "approval-action", "await-approval")
HALTING_COMMANDS = ("read -p", "read -r -p")


def _publishable_crates(repo: Repo) -> list[str]:
    """Every workspace member Cargo would publish, derived from the workspace itself."""
    publishable: list[str] = []
    for directory in repo.crate_dirs:
        with (directory / "Cargo.toml").open("rb") as handle:
            manifest = tomllib.load(handle)
        if manifest.get("package", {}).get("publish") is False:
            continue
        publishable.append(directory.name)
    return publishable


def _manifest_paths(repo: Repo) -> list[Path]:
    """Every manifest in the tree, ignoring build products and vendored trees."""
    found: list[Path] = []
    for path in sorted(repo.root.rglob("*")):
        if not path.is_file() or path.name not in MANIFEST_NAMES:
            continue
        if SKIPPED_DIRECTORIES & set(path.relative_to(repo.root).parts):
            continue
        found.append(path)
    return found


def _has_version(path: Path) -> bool:
    """Whether a manifest carries a version field of its own."""
    text = path.read_text(encoding="utf-8")
    if path.name == "package.json":
        import json

        return "version" in json.loads(text)
    with path.open("rb") as handle:
        data = tomllib.load(handle)
    for table in ("package", "project", "workspace"):
        section = data.get(table)
        if isinstance(section, dict):
            if "version" in section and section["version"] != {"workspace": True}:
                return True
            inner = section.get("package")
            if isinstance(inner, dict) and "version" in inner:
                return True
    return False


def release_targets(repo: Repo) -> list[str]:
    """The declaration covers every publishable crate, and nothing else, at this node."""
    declaration = repo.read_toml("release-targets.toml")
    targets = declaration.get("target", [])
    findings: list[str] = []

    declared_crates: set[str] = set()
    for target in targets:
        identifier = str(target.get("id", ""))
        registry, _, name = identifier.partition(":")
        if registry != "crate":
            findings.append(
                f"release-targets.toml declares `{identifier}`, a {registry or 'nameless'} "
                f"target; at this node the repository publishes crates and nothing else"
            )
            continue
        declared_crates.add(name)
        manifest = target.get("manifest", "")
        if not repo.exists(manifest):
            findings.append(f"release-targets.toml points `{identifier}` at a missing {manifest}")

    publishable = set(_publishable_crates(repo))
    findings.extend(
        f"release-targets.toml omits publishable crate `{name}`"
        for name in sorted(publishable - declared_crates)
    )
    findings.extend(
        f"release-targets.toml declares `crate:{name}`, which no workspace member backs"
        for name in sorted(declared_crates - publishable)
    )

    owned = {
        repo.path(pattern)
        for pattern in repo.policy["manifests"]["automation_owned"]
        if "*" not in pattern
    }
    for pattern in repo.policy["manifests"]["automation_owned"]:
        if "*" in pattern:
            owned |= set(repo.root.glob(pattern))
    findings.extend(
        f"{path.relative_to(repo.root)} carries a version field, and release automation "
        f"does not own it (`repo-policy.toml`'s manifests.automation_owned)"
        for path in _manifest_paths(repo)
        if _has_version(path) and path not in owned
    )

    findings.extend(_conventional_commit_findings(repo))
    return findings


def _conventional_commit_findings(repo: Repo) -> list[str]:
    """Release automation derives every version and the changelog from the subjects."""
    if not repo.exists("release-plz.toml"):
        return ["no release-plz.toml: release automation is not configured"]
    config = repo.read_toml("release-plz.toml")
    findings: list[str] = []

    pattern = config.get("workspace", {}).get("release_commits")
    if not isinstance(pattern, str):
        return ["release-plz.toml declares no `release_commits`: every commit would release"]
    regex = re.compile(pattern)
    release_types: list[str] = repo.policy["commits"]["release_types"]
    findings.extend(
        f"release-plz.toml's `release_commits` does not admit `{kind}`, which "
        f"`repo-policy.toml` records as a type this repository releases from"
        for kind in release_types
        if not regex.search(f"{kind}: a subject")
    )
    findings.extend(
        f"release-plz.toml's `release_commits` releases from `{kind}`, which "
        f"`repo-policy.toml` records as a type that releases nothing"
        for kind in repo.policy["commits"]["non_release_types"]
        if regex.search(f"{kind}: a subject")
    )

    parsers = config.get("changelog", {}).get("commit_parsers")
    if not parsers:
        findings.append(
            "release-plz.toml declares no `changelog.commit_parsers`: the changelog "
            "would not be generated from Conventional Commits"
        )
    else:
        grouped = {str(parser.get("message", "")) for parser in parsers}
        findings.extend(
            f"release-plz.toml's changelog does not group `{kind}` commits"
            for kind in release_types
            if f"^{kind}" not in grouped
        )

    hook = repo.path(".githooks/commit-msg")
    if not hook.is_file():
        findings.append(".githooks/commit-msg is absent: no hook rules on a commit subject")
    elif "repo_checks" not in hook.read_text(encoding="utf-8"):
        findings.append(
            ".githooks/commit-msg does not read the type list from `repo-policy.toml`, "
            "so the hook and the release rule can disagree"
        )
    return findings


def release_automation(repo: Repo) -> list[str]:
    """The release path is executable and runs by itself."""
    installed = {tool["command"] for tool in repo.policy["toolchain"]["tool"]}
    release_programs = {program for program in installed if "release" in program}

    findings: list[str] = []
    found: list[tuple[str, str, dict[str, Any]]] = []
    for path in repo.workflow_paths:
        workflow = load_workflow(path)
        for job_name, job in jobs_of(workflow).items():
            programs = {
                program for command in run_commands(job) for program in programs_in(command)
            }
            if programs & release_programs:
                found.append((path.name, job_name, job))
                continue
            unknown = {p for p in programs if "release-plz" in p or p == "release"}
            if unknown - installed:
                findings.append(
                    f"{path.name}: job `{job_name}` runs release program "
                    f"`{sorted(unknown - installed)[0]}`, which this repository's "
                    f"toolchain does not install"
                )

    if not found:
        return [
            *findings,
            "no committed workflow performs releases: no job runs a release program "
            f"({', '.join(sorted(release_programs))})",
        ]

    workflow_names = {file_name for file_name, _, _ in found}
    for file_name in sorted(workflow_names):
        workflow = load_workflow(repo.path(f".github/workflows/{file_name}"))
        triggers = workflow.get("on")
        trigger_names = set(triggers) if isinstance(triggers, dict) else {str(triggers)}
        if trigger_names <= {"workflow_dispatch"}:
            findings.append(
                f"{file_name}: the release workflow's only trigger is manual invocation"
            )
        elif "push" not in trigger_names:
            findings.append(
                f"{file_name}: the release workflow does not fire on a change reaching "
                f"the base branch"
            )

    covered = {
        registry
        for _, _, job in found
        for command in run_commands(job)
        for registry in (["crate"] if "release-plz release" in command else [])
    }
    for target in repo.read_toml("release-targets.toml").get("target", []):
        registry = str(target.get("id", "")).partition(":")[0]
        if registry not in covered:
            findings.append(
                f"release-targets.toml declares `{target.get('id')}`, which no committed "
                f"publishing step covers"
            )

    for file_name, job_name, job in found:
        if "environment" in job:
            findings.append(
                f"{file_name}: release job `{job_name}` declares a deployment environment, "
                f"which can halt for a person before a target reaches its registry"
            )
        for step in steps_of(job):
            uses = str(step.get("uses", ""))
            findings.extend(
                f"{file_name}: release job `{job_name}` uses `{uses}`, which halts for a person"
                for marker in HALTS_FOR_A_PERSON
                if marker in uses
            )
        findings.extend(
            f"{file_name}: release job `{job_name}` runs `{command}`, which halts for a human input"
            for command in run_commands(job)
            for marker in HALTING_COMMANDS
            if marker in command
        )
    return findings
