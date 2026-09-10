"""Checks over what this repository publishes and how it publishes it."""

from __future__ import annotations

import re
import tomllib
from pathlib import Path
from typing import Any

from repo_checks import install_path as ip
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


#: Every registry this repository publishes to. A target naming any other is
#: one nothing knows how to publish.
REGISTRIES = ("crate", "pypi", "npm", "release")


def release_targets(repo: Repo) -> list[str]:
    """The declaration covers every publishable crate and every end-user route."""
    declaration = repo.read_toml("release-targets.toml")
    targets = declaration.get("target", [])
    findings: list[str] = []

    declared_crates: set[str] = set()
    for target in targets:
        identifier = str(target.get("id", ""))
        registry, _, name = identifier.partition(":")
        if registry not in REGISTRIES:
            findings.append(
                f"release-targets.toml declares `{identifier}`, a "
                f"{registry or 'nameless'} target; this repository publishes to "
                f"{', '.join(REGISTRIES)} and nothing else"
            )
            continue
        if registry != "crate":
            if not str(target.get("description", "")).strip():
                findings.append(
                    f"release-targets.toml declares `{identifier}` with no description, "
                    f"so its own registry would show none"
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

    findings.extend(_route_findings(repo, targets))
    findings.extend(_conventional_commit_findings(repo))
    return findings


def _route_findings(repo: Repo, targets: list[dict[str, Any]]) -> list[str]:
    """Every route the install path names has a target, under that route's own name.

    `AGENTS.md`'s "The end-user install path" is the authoritative source of
    the routes and of the distribution name each of the two registry routes
    installs. A declaration and that section that disagree on a name are a
    command a reader pastes and a package nobody published.
    """
    path = ip.parse(repo.agents_md)
    findings: list[str] = []
    backing: dict[str, list[str]] = {}
    for target in targets:
        route = str(target.get("route", "")).strip()
        if route:
            backing.setdefault(route, []).append(str(target.get("id", "")))

    stated = {route.heading for route in path.routes}
    findings.extend(
        f"release-targets.toml declares `{backing[route][0]}` as backing route "
        f"`{route}`, which AGENTS.md's `{ip.SECTION_HEADING}` does not state"
        for route in sorted(backing)
        if route not in stated
    )
    for route in path.routes:
        behind = backing.get(route.heading, [])
        if not behind:
            findings.append(
                f"AGENTS.md's `{ip.SECTION_HEADING}` names route `{route.heading}`, for "
                f"which release-targets.toml declares no target: it is a route to a "
                f"program nothing publishes"
            )
            continue
        if len(behind) > 1:
            findings.append(
                f"route `{route.heading}` is backed by {len(behind)} targets "
                f"({', '.join(behind)}); a route is one artifact"
            )
            continue
        findings.extend(_name_findings(route, behind[0]))
    return findings


def _name_findings(route: ip.Route, identifier: str) -> list[str]:
    """The distribution a route's own command installs is the one declared.

    Read out of the command a reader pastes: `pip install X` and `npm install
    -g X` each name the distribution, and that name is the registry's rather
    than one chosen in the declaration. A route whose command names none — the
    script route fetches a path rather than a package — is held to its script
    path by the `install-script` check instead.
    """
    named = route.command.split()
    if not named or ip.RAW_URL.search(route.command):
        return []
    installed = named[-1]
    declared = identifier.partition(":")[2]
    if installed != declared:
        return [
            f"route `{route.heading}` installs `{installed}`, and release-targets.toml "
            f"declares `{identifier}`: the command a reader pastes and the artifact "
            f"this repository publishes are two different names"
        ]
    return []


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

    findings.extend(_coverage_findings(repo))

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


#: The recipe release automation builds every artifact beside the crates with,
#: and the one it publishes them with. Declared in `repo-policy.toml` so that a
#: workflow and this check cannot disagree about which step is which.
ARTIFACT_RECIPES = ("build_recipe", "publish_recipe")

#: What a keyless trusted publisher looks like in a workflow: the permission it
#: needs, and the action that uses it. Every publish here authenticates with an
#: API token carried in a repository secret instead.
TRUSTED_PUBLISHER = ("id-token", "gh-action-pypi-publish", "trusted-publish")


def _release_policy(repo: Repo) -> dict[str, str]:
    """The two recipes release automation is recognized by."""
    from repo_checks.model import policy_strings, policy_table

    return policy_strings(policy_table(repo, "release"), ARTIFACT_RECIPES, "release")


def _coverage_findings(repo: Repo) -> list[str]:
    """Every declared target is covered, on every platform, by a committed step.

    Three things, and the third is what a matrix cannot be narrowed past:
    every target the declaration names is published by some step; each of the
    three end-user routes has a build; and that build runs once per platform
    `AGENTS.md`'s own list names — that section being the authority rather than
    the matrix beside it, since a check reading the matrix is satisfied by
    narrowing the matrix.
    """
    from repo_checks.checks_ci import platforms_of
    from repo_checks.model import PolicyValueError
    from repo_checks.parsing import MarkerBlockMissingError

    try:
        recipes = _release_policy(repo)
    except PolicyValueError as error:
        return [str(error)]

    building: list[dict[str, Any]] = []
    publishing: list[str] = []
    for path in repo.workflow_paths:
        for job_name, job in jobs_of(load_workflow(path)).items():
            commands = run_commands(job)
            if f"just {recipes['build_recipe']}" in commands:
                building.append(job)
            if f"just {recipes['publish_recipe']}" in commands:
                publishing.append(f"{path.name}: `{job_name}`")
            if any("release-plz release" in command for command in commands):
                publishing.append(f"{path.name}: `{job_name}`")

    findings: list[str] = []
    declared = repo.read_toml("release-targets.toml").get("target", [])
    if not publishing:
        findings.append(
            f"no committed step publishes anything: release automation runs neither "
            f"`just {recipes['publish_recipe']}` nor `release-plz release`"
        )
    if not building:
        findings.append(
            f"no committed job builds the artifacts beside the crates: none runs "
            f"`just {recipes['build_recipe']}`"
        )
        return findings

    try:
        wanted = [platform.id for platform in platforms_of(repo)]
    except MarkerBlockMissingError as error:
        return [*findings, str(error)]

    built_for: set[str] = set()
    for job in building:
        entries = ((job.get("strategy") or {}).get("matrix") or {}).get("platform")
        for entry in entries if isinstance(entries, list) else []:
            if isinstance(entry, dict) and "id" in entry:
                built_for.add(str(entry["id"]))
    findings.extend(
        f"AGENTS.md's supported-platform list names `{platform}`, and no committed job "
        f"builds this repository's artifacts for it: every one of the three end-user "
        f"routes carries the program already built for the platform"
        for platform in wanted
        if platform not in built_for
    )

    path = ip.parse(repo.agents_md)
    routed = {
        str(target.get("route", "")).strip()
        for target in declared
        if str(target.get("route", "")).strip()
    }
    findings.extend(
        f"AGENTS.md's `{ip.SECTION_HEADING}` names route `{route.heading}`, for which "
        f"release automation declares no build at all"
        for route in path.routes
        if route.heading not in routed
    )
    return findings


def publish_credentials(repo: Repo) -> list[str]:
    """Every publish authenticates with an API token carried in a repository secret.

    Not a keyless trusted publisher: `gh-secrets.json` is the authoritative
    list of what this repository holds, and a publish authenticated by
    something outside it would be a credential nobody declared. The names of
    the tokens are not written here — they are what the publishing tool reads
    from its environment, and what that manifest declares.
    """
    import json
    import re

    from repo_checks.model import PolicyValueError

    try:
        recipes = _release_policy(repo)
    except PolicyValueError as error:
        return [str(error)]

    manifest = json.loads(repo.read("gh-secrets.json"))
    held = {entry["name"] for entry in manifest["secrets"]}
    reference = re.compile(r"secrets\.([A-Z0-9_]+)")
    findings: list[str] = []
    found = False

    for path in repo.workflow_paths:
        workflow = load_workflow(path)
        for job_name, job in jobs_of(workflow).items():
            commands = run_commands(job)
            publishes = f"just {recipes['publish_recipe']}" in commands or any(
                "release-plz release" in command for command in commands
            )
            if not publishes:
                continue
            found = True
            where = f"{path.name}: publishing job `{job_name}`"
            named = set()
            for step in steps_of(job):
                named |= set(reference.findall(str(step.get("env", ""))))
                uses = str(step.get("uses", ""))
                findings.extend(
                    f"{where} uses `{uses}`, which publishes by a keyless trusted "
                    f"publisher; every publish here authenticates with an API token "
                    f"carried in a repository secret"
                    for marker in TRUSTED_PUBLISHER
                    if marker in uses
                )
            declared_permissions = json.dumps(
                {**(workflow.get("permissions") or {}), **(job.get("permissions") or {})}
            )
            if TRUSTED_PUBLISHER[0] in declared_permissions:
                findings.append(
                    f"{where} is granted `{TRUSTED_PUBLISHER[0]}`, which is what a "
                    f"keyless trusted publisher needs and no publish here uses"
                )
            if not named:
                findings.append(f"{where} names no secret as its credential")
            findings.extend(
                f"{where} authenticates with `{secret}`, which gh-secrets.json does not declare"
                for secret in sorted(named - held)
            )
    if not found:
        findings.append("no committed job publishes anything")
    return findings
