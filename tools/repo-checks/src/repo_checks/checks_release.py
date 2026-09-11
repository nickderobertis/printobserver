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


#: The recipe the publishing job reads the release program's answer with, the
#: output it publishes that answer under, and the module that reads it — all
#: declared in `repo-policy.toml`, so the workflow, the recipe and the reader
#: cannot drift on a name.
GATING = ("cut_recipe", "cut_output", "cut_source")

#: The two halves of the release program: what drafts the next release's pull
#: request, and what publishes what is already due. Told apart by the command a
#: job runs rather than by what the job is called.
DRAFTING = "release-plz release-pr"
PUBLISHING = "release-plz release"

#: How the publishing command is made to say what it released.
ANSWER_OPTIONS = ("--output json", "-o json")

#: Where a step publishes a job output from.
JOB_OUTPUT = "GITHUB_OUTPUT"

#: What gating on a job's exit status rather than on its answer looks like.
STATUS_GATES = (".result", "success()", "failure()")


def _needs(job: dict[str, Any]) -> list[str]:
    """The jobs a job waits on, however the workflow spells them."""
    needs = job.get("needs")
    if isinstance(needs, str):
        return [needs]
    if isinstance(needs, list):
        return [str(name) for name in needs]
    return []


def _jobs_running(jobs: dict[str, dict[str, Any]], command: str) -> list[str]:
    """The jobs with a step running `command`, as the whole command or its first words."""
    return [
        name
        for name, job in jobs.items()
        if any(run == command or run.startswith(f"{command} ") for run in run_commands(job))
    ]


def release_gating(repo: Repo) -> list[str]:
    """Publishing does not wait on drafting, and the artifacts follow only a cut release.

    `release-plz release-pr` computes each package's difference against what
    the registry serves and can die doing it; `release-plz release` publishes
    what is already due and exits zero having published nothing. So the job
    running the second must not `need` the job running the first, and the
    jobs that build and publish the artifacts must be gated on what the second
    ANSWERED — read into a job output by the declared recipe — rather than on
    its exit status, which says nothing.
    """
    from repo_checks.model import PolicyValueError, policy_strings, policy_table
    from repo_checks.parsing import recipes as parse_recipes

    try:
        recipes = _release_policy(repo)
        gating = policy_strings(policy_table(repo, "release"), GATING, "release")
    except PolicyValueError as error:
        return [str(error)]

    findings: list[str] = []
    found = False
    for path in repo.workflow_paths:
        jobs = jobs_of(load_workflow(path))
        publishing = _jobs_running(jobs, PUBLISHING)
        if not publishing:
            continue
        found = True
        for name in publishing:
            findings.extend(_independence_findings(jobs, name, path.name))
            findings.extend(_answer_findings(jobs[name], name, gating, path.name))
            findings.extend(_gated_findings(jobs, name, recipes, gating, path.name))
    if not found:
        findings.append(f"no committed job runs `{PUBLISHING}`, so nothing publishes a release")

    declared = parse_recipes(repo.justfile)
    recipe = declared.get(gating["cut_recipe"])
    if recipe is None:
        findings.append(
            f"the justfile declares no `{gating['cut_recipe']}` recipe, which "
            f"`repo-policy.toml` names as what reads the release program's answer"
        )
    if not repo.exists(gating["cut_source"]):
        findings.append(
            f"`repo-policy.toml` names {gating['cut_source']} as what reads the release "
            f"program's answer, and this repository commits no such file"
        )
    elif f'"{gating["cut_output"]}"' not in repo.read(gating["cut_source"]):
        findings.append(
            f"{gating['cut_source']} declares no `{gating['cut_output']}`, which is the "
            f"field `repo-policy.toml` and the committed workflow gate the artifacts on"
        )
    return findings


def _independence_findings(jobs: dict[str, dict[str, Any]], name: str, file: str) -> list[str]:
    """The publishing job waits on no job that drafts the next release."""
    drafting = set(_jobs_running(jobs, DRAFTING))
    return [
        f"{file}: job `{name}` publishes a release and waits on `{waited}`, which drafts "
        f"the next one: a drafting job that cannot draft must not be able to stop a "
        f"publication that is ready"
        for waited in _needs(jobs[name])
        if waited in drafting
    ]


def _answer_findings(
    job: dict[str, Any], name: str, gating: dict[str, str], file: str
) -> list[str]:
    """The publishing job asks the program what it released and publishes that as an output."""
    where = f"{file}: job `{name}`"
    findings: list[str] = []
    for command in run_commands(job):
        if command.startswith(f"{PUBLISHING} ") and not any(
            option in command for option in ANSWER_OPTIONS
        ):
            findings.append(
                f"{where} runs `{command}` without `{ANSWER_OPTIONS[0]}`, so nothing says "
                f"what it released and the artifact jobs cannot be gated on it"
            )

    recipe = f"just {gating['cut_recipe']}"
    reading = [
        step
        for step in steps_of(job)
        if any(line.startswith(f"{recipe} ") for line in str(step.get("run", "")).splitlines())
    ]
    if not reading:
        return [
            *findings,
            f"{where} runs no `{recipe}` step, so what the release program answered is "
            f"read into no output",
        ]
    step = reading[0]
    identifier = str(step.get("id", "")).strip()
    if not identifier:
        findings.append(f"{where}'s `{recipe}` step carries no `id`, so no output can name it")
    if JOB_OUTPUT not in str(step.get("run", "")):
        findings.append(
            f"{where}'s `{recipe}` step does not append to `${JOB_OUTPUT}`, so what it "
            f"answered reaches no job output"
        )
    outputs = job.get("outputs")
    declared = outputs.get(gating["cut_output"], "") if isinstance(outputs, dict) else ""
    published = " ".join(str(declared).split())
    expected = f"steps.{identifier}.outputs.{gating['cut_output']}"
    if expected not in published:
        findings.append(
            f"{where} publishes no output `{gating['cut_output']}` from `{expected}`, which "
            f"is what the artifact jobs are gated on"
        )
    return findings


def _gated_findings(
    jobs: dict[str, dict[str, Any]],
    publishing: str,
    recipes: dict[str, str],
    gating: dict[str, str],
    file: str,
) -> list[str]:
    """Every job building or publishing the artifacts follows the answer, not the status."""
    gate = f"needs.{publishing}.outputs.{gating['cut_output']} != ''"
    findings: list[str] = []
    for recipe in ARTIFACT_RECIPES:
        for name in _jobs_running(jobs, f"just {recipes[recipe]}"):
            job = jobs[name]
            where = f"{file}: job `{name}`"
            if publishing not in _needs(job):
                findings.append(
                    f"{where} runs `just {recipes[recipe]}` and does not wait on `{publishing}`, "
                    f"whose answer is what says whether a release was cut"
                )
            condition = " ".join(str(job.get("if", "")).split())
            if gate not in condition:
                findings.append(
                    f"{where} runs `just {recipes[recipe]}` and is not gated on `{gate}`: "
                    f"the release program exits zero having released nothing, and the "
                    f"registries refuse a version they already serve"
                )
            findings.extend(
                f"{where} is gated on `{condition}`, which reads a job's exit status rather "
                f"than what it answered — and that status is zero whether or not a release "
                f"was cut"
                for marker in STATUS_GATES
                if marker in condition
            )
    return findings
