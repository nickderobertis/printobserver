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
GATING = ("answer_recipe", "answer_output", "answer_source")

#: The two halves of the release program: what drafts the next release's pull
#: request, and what publishes what is already due. Told apart by the command a
#: job runs rather than by what the job is called.
DRAFTING = "release-plz release-pr"
PUBLISHING = "release-plz release"

ANSWER_OPTIONS = ("--output json", "-o json")
JOB_OUTPUT = "GITHUB_OUTPUT"
STATUS_GATES = (".result", "success()", "failure()")

#: One job as the YAML reader hands it back. Its keys are the workflow author's
#: own — `needs`, `if`, `outputs`, `steps` and whatever else GitHub accepts — so
#: there is no narrower shape to read it as; each reader below narrows the one
#: value it needs.
Job = dict[str, Any]

#: One step of a job, and one whole workflow, as the same reader hands them
#: back and for the same reason: `uses`, `with`, `if`, `on`, `concurrency` and
#: whatever else the author wrote, each narrowed by the one reader that needs
#: it rather than by a model of a GitHub workflow held here.
Step = dict[str, Any]
Workflow = dict[str, Any]


def _needs(job: Job) -> list[str]:
    """The jobs a job waits on, however the workflow spells them."""
    match job.get("needs"):
        case str() as one:
            return [one]
        case list() as several:
            return [str(name) for name in several]
        case _:
            return []


def _jobs_running(jobs: dict[str, Job], command: str) -> list[str]:
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
    recipe = declared.get(gating["answer_recipe"])
    if recipe is None:
        findings.append(
            f"the justfile declares no `{gating['answer_recipe']}` recipe, which "
            f"`repo-policy.toml` names as what reads the release program's answer"
        )
    if not repo.exists(gating["answer_source"]):
        findings.append(
            f"`repo-policy.toml` names {gating['answer_source']} as what reads the release "
            f"program's answer, and this repository commits no such file"
        )
    elif f'"{gating["answer_output"]}"' not in repo.read(gating["answer_source"]):
        findings.append(
            f"{gating['answer_source']} declares no `{gating['answer_output']}`, which is the "
            f"field `repo-policy.toml` and the committed workflow gate the artifacts on"
        )
    return findings


def _independence_findings(jobs: dict[str, Job], name: str, file: str) -> list[str]:
    """The publishing job waits on no job that drafts the next release."""
    drafting = set(_jobs_running(jobs, DRAFTING))
    return [
        f"{file}: job `{name}` publishes a release and waits on `{waited}`, which drafts "
        f"the next one: a drafting job that cannot draft must not be able to stop a "
        f"publication that is ready"
        for waited in _needs(jobs[name])
        if waited in drafting
    ]


def _answer_findings(job: Job, name: str, gating: dict[str, str], file: str) -> list[str]:
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

    recipe = f"just {gating['answer_recipe']}"
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
    declared = outputs.get(gating["answer_output"], "") if isinstance(outputs, dict) else ""
    published = " ".join(str(declared).split())
    expected = f"steps.{identifier}.outputs.{gating['answer_output']}"
    if expected not in published:
        findings.append(
            f"{where} publishes no output `{gating['answer_output']}` from `{expected}`, which "
            f"is what the artifact jobs are gated on"
        )
    return findings


def _gated_findings(
    jobs: dict[str, Job],
    publishing: str,
    recipes: dict[str, str],
    gating: dict[str, str],
    file: str,
) -> list[str]:
    """Every job building or publishing the artifacts follows the answer, not the status."""
    gate = f"needs.{publishing}.outputs.{gating['answer_output']} != ''"
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


#: The names the dispatched shape of the release workflow is held to, declared
#: once in `repo-policy.toml`'s `[release]`: the dispatch's one input, the
#: recipe that verifies the tag and the one that reads its record, the name the
#: record crosses between the two workflows under, and the variable and module
#: the publisher takes the dispatched version through.
DISPATCH = (
    "dispatch_input",
    "dispatched_recipe",
    "record_recipe",
    "record_artifact",
    "publish_version_env",
    "publish_version_source",
)

#: The two conditions the release workflow tells its triggers apart by. A step
#: is held to one of these at the step or at its job, as the forge reads them.
PUSH_ONLY = "github.event_name == 'push'"
DISPATCH_ONLY = "github.event_name == 'workflow_dispatch'"

#: The actions the dispatched shape leans on, matched by name up to the pin.
CHECKOUT = "actions/checkout@"
UPLOAD = "actions/upload-artifact@"
DOWNLOAD = "actions/download-artifact@"

#: The one `fetch-depth` under which an existing tag is resolvable: the whole
#: history and every tag. Anything narrower is a checkout that refuses every
#: dispatch there is — and the copy the end-to-end tier drives cannot tell the
#: two apart, because it carries its tags already.
WHOLE_HISTORY = "0"

#: The triggering run's number, which is what a cross-run download reads.
TRIGGERING_RUN = "github.event.workflow_run.id"


def _conditioned(job: Job, step: Step, on: str) -> bool:
    """Whether a step runs only under `on`, by its own condition or its job's."""
    return any(
        on in " ".join(str(condition).split())
        for condition in (job.get("if", ""), step.get("if", ""))
    )


def _with(step: Step) -> dict[str, str]:
    """A `uses:` step's inputs, each as one whitespace-normalised string."""
    given = step.get("with")
    if not isinstance(given, dict):
        return {}
    return {str(key): " ".join(str(value).split()) for key, value in given.items()}


def _using(job: Job, action: str) -> list[Step]:
    """The steps of a job using one action, whatever it is pinned at."""
    return [step for step in steps_of(job) if str(step.get("uses", "")).startswith(action)]


def _running(job: Job, recipe: str) -> list[Step]:
    """The steps of a job running `just <recipe>`, as the whole line or its first words."""
    return [
        step
        for step in steps_of(job)
        if any(
            line.strip() == f"just {recipe}" or line.strip().startswith(f"just {recipe} ")
            for line in str(step.get("run", "")).splitlines()
        )
    ]


def release_dispatch(repo: Repo) -> list[str]:
    """A hand dispatch of the release workflow finishes an existing release, and only that.

    Dispatched on `main` with an existing tag, the workflow builds that tag's
    tree's artifacts and publishes them with the publisher at the dispatched
    ref — the same jobs, the same publisher and the same secrets as a push.
    What this holds is that the dispatch changes nothing about a push (the
    release program runs on a push alone), that nothing is built or published
    for a tag the declared recipe did not verify, that the build checks out
    the tag's tree and the publish is handed the tag's version, that the
    record saying which version was published crosses to the install-path
    proof under one name, that the verifying job's checkout is one an existing
    tag is resolvable in, and that a dispatch and a push on one ref cannot
    publish at once.
    """
    from repo_checks.model import PolicyValueError, policy_strings, policy_table
    from repo_checks.parsing import recipes as parse_recipes

    try:
        table = policy_table(repo, "release")
        recipes = _release_policy(repo)
        gating = policy_strings(table, GATING, "release")
        dispatch = policy_strings(table, DISPATCH, "release")
        proof = policy_strings(
            policy_table(repo, "install_proof"),
            ("workflow", "release_job", "release_output"),
            "install_proof",
        )
    except PolicyValueError as error:
        return [str(error)]

    findings: list[str] = []
    found = False
    for path in repo.workflow_paths:
        workflow = load_workflow(path)
        jobs = jobs_of(workflow)
        publishing = _jobs_running(jobs, PUBLISHING)
        if not publishing:
            continue
        found = True
        findings.extend(_input_findings(workflow, dispatch, path.name))
        findings.extend(_concurrency_findings(workflow, path.name))
        findings.extend(_push_only_findings(jobs, path.name))
        findings.extend(_dispatched_findings(jobs, publishing, dispatch, gating, path.name))
        findings.extend(_tag_findings(jobs, recipes, dispatch, path.name))
    if not found:
        findings.append(f"no committed job runs `{PUBLISHING}`, so nothing publishes a release")

    findings.extend(_resolve_findings(repo, dispatch, proof))

    declared = parse_recipes(repo.justfile)
    findings.extend(
        f"the justfile declares no `{dispatch[key]}` recipe, which `repo-policy.toml` names "
        f"as what {purpose}"
        for key, purpose in (
            ("dispatched_recipe", "verifies a dispatched tag and answers the release"),
            ("record_recipe", "reads the version a dispatched run recorded"),
        )
        if dispatch[key] not in declared
    )
    source = dispatch["publish_version_source"]
    if not repo.exists(source):
        findings.append(
            f"`repo-policy.toml` names {source} as what reads the dispatched version to "
            f"publish, and this repository commits no such file"
        )
    elif f'"{dispatch["publish_version_env"]}"' not in repo.read(source):
        findings.append(
            f"{source} declares no `{dispatch['publish_version_env']}`, which is the name "
            f"`repo-policy.toml` and the committed workflow hand the dispatched version in under"
        )
    return findings


def _input_findings(workflow: Workflow, dispatch: dict[str, str], file: str) -> list[str]:
    """The dispatch takes the tag as its one required input."""
    name = dispatch["dispatch_input"]
    triggers = workflow.get("on")
    settings = triggers.get("workflow_dispatch") if isinstance(triggers, dict) else None
    inputs = settings.get("inputs") if isinstance(settings, dict) else None
    declared = inputs.get(name) if isinstance(inputs, dict) else None
    if not isinstance(declared, dict):
        return [
            f"{file}: the release workflow's `workflow_dispatch` declares no `{name}` input, "
            f"so a dispatch cannot name the existing release it finishes"
        ]
    findings: list[str] = []
    if declared.get("required") is not True:
        findings.append(
            f"{file}: the `{name}` input is not `required: true`, and a dispatch naming no "
            f"tag would build and publish nothing anybody named"
        )
    if str(declared.get("type", "")) != "string":
        findings.append(f"{file}: the `{name}` input is not `type: string`")
    return findings


def _concurrency_findings(workflow: Workflow, file: str) -> list[str]:
    """A dispatch and a push on one ref are serialised, and neither is cancelled."""
    concurrency = workflow.get("concurrency")
    group = str(concurrency.get("group", "")) if isinstance(concurrency, dict) else ""
    findings: list[str] = []
    if "github.ref" not in group:
        findings.append(
            f"{file}: the release workflow's `concurrency.group` does not read `github.ref`, "
            f"so a dispatched run and a push-triggered run of one ref can publish at once"
        )
    if isinstance(concurrency, dict) and concurrency.get("cancel-in-progress") is True:
        findings.append(
            f"{file}: the release workflow cancels a run in progress, and a publish stopped "
            f"partway is the state the publisher exists to recover from rather than to cause"
        )
    return findings


def _push_only_findings(jobs: dict[str, Job], file: str) -> list[str]:
    """The release program runs on a push alone: a dispatch drafts and cuts nothing."""
    findings: list[str] = []
    for name, job in jobs.items():
        for step in steps_of(job):
            command = str(step.get("run", "")).strip()
            if not command.startswith((DRAFTING, PUBLISHING)):
                continue
            if not _conditioned(job, step, PUSH_ONLY):
                findings.append(
                    f"{file}: job `{name}` runs `{command.split()[0]} {command.split()[1]}` "
                    f"without `{PUSH_ONLY}` at the step or the job, so a dispatch finishing an "
                    f"existing release would run the release program over `main` as well"
                )
    return findings


def _dispatched_findings(
    jobs: dict[str, Job],
    publishing: list[str],
    dispatch: dict[str, str],
    gating: dict[str, str],
    file: str,
) -> list[str]:
    """The tag reaches the artifact jobs through the verifying recipe, its record uploaded."""
    recipe = dispatch["dispatched_recipe"]
    output = gating["answer_output"]
    steps = [(name, step) for name, job in jobs.items() for step in _running(job, recipe)]
    if not steps:
        return [
            f"{file}: no step runs `just {recipe}`, so a dispatched tag would reach the "
            f"artifact jobs without anything having verified that it names an existing "
            f"release whose tree is that release's"
        ]
    name, step = steps[0]
    job = jobs[name]
    where = f"{file}: job `{name}`'s `just {recipe}` step"
    findings: list[str] = []
    if name not in publishing:
        findings.append(
            f"{where} is in a job the artifact jobs are not gated on: they read "
            f"`{', '.join(publishing)}`'s `{output}`, and this answer reaches it from nowhere"
        )
    identifier = str(step.get("id", "")).strip()
    if not identifier:
        findings.append(f"{where} carries no `id`, so no output can name it")
    if not _conditioned(job, step, DISPATCH_ONLY):
        findings.append(
            f"{where} is not conditioned on `{DISPATCH_ONLY}`, so a push would verify a tag "
            f"nobody named"
        )
    if JOB_OUTPUT not in str(step.get("run", "")):
        findings.append(f"{where} does not append to `${JOB_OUTPUT}`, so its answer reaches no job")
    outputs = job.get("outputs")
    declared = outputs.get(output, "") if isinstance(outputs, dict) else ""
    expected = f"steps.{identifier}.outputs.{output}"
    if identifier and expected not in " ".join(str(declared).split()):
        findings.append(
            f"{file}: job `{name}` publishes no output `{output}` from `{expected}`, so what "
            f"the dispatched tag was verified as is not what the artifact jobs are gated on"
        )

    checkouts = _using(job, CHECKOUT)
    if not any(_with(checkout).get("fetch-depth") == WHOLE_HISTORY for checkout in checkouts):
        findings.append(
            f"{file}: job `{name}` runs `just {recipe}` and its checkout does not carry "
            f"`fetch-depth: {WHOLE_HISTORY}`, the only checkout an existing tag is resolvable "
            f"in: a shallow checkout of `main` carries no tag, and would refuse every dispatch"
        )

    artifact = dispatch["record_artifact"]
    uploads = [upload for upload in _using(job, UPLOAD) if _conditioned(job, upload, DISPATCH_ONLY)]
    if not uploads:
        findings.append(
            f"{file}: job `{name}` uploads no artifact on a dispatch, so the record saying "
            f"which version was published never reaches the install-path proof"
        )
    findings.extend(
        f"{file}: job `{name}` uploads the dispatched release's record as "
        f"`{_with(upload).get('name', '')}`, and `repo-policy.toml` names it `{artifact}`: the "
        f"install-path proof downloads it by that name and would find nothing"
        for upload in uploads
        if _with(upload).get("name") != artifact
    )
    return findings


def _tag_findings(
    jobs: dict[str, Job], recipes: dict[str, str], dispatch: dict[str, str], file: str
) -> list[str]:
    """The build checks out the dispatched tag, and the publish is handed its version."""
    ref = f"inputs.{dispatch['dispatch_input']}"
    variable = dispatch["publish_version_env"]
    findings: list[str] = []
    for name in _jobs_running(jobs, f"just {recipes['build_recipe']}"):
        checkouts = _using(jobs[name], CHECKOUT)
        if not any(ref in _with(checkout).get("ref", "") for checkout in checkouts):
            findings.append(
                f"{file}: job `{name}` runs `just {recipes['build_recipe']}` and its checkout "
                f"does not take `ref: ${{{{ {ref} }}}}`, so a dispatch would build `main`'s "
                f"tree and publish it as the tag's release"
            )
    for name in _jobs_running(jobs, f"just {recipes['publish_recipe']}"):
        for step in _running(jobs[name], recipes["publish_recipe"]):
            handed = " ".join(str((step.get("env") or {}).get(variable, "")).split())
            if ref not in handed:
                findings.append(
                    f"{file}: job `{name}`'s `just {recipes['publish_recipe']}` step does not "
                    f"hand the recipe `{variable}` from `{ref}`, so a dispatch would publish "
                    f"the tag's artifacts under `main`'s own version"
                )
    return findings


def _resolve_findings(repo: Repo, dispatch: dict[str, str], proof: dict[str, str]) -> list[str]:
    """The install-path proof reads the record under the declared name, into its version."""
    relative = f".github/workflows/{proof['workflow']}"
    if not repo.exists(relative):
        return [f"`repo-policy.toml` names {relative}, which is not there"]
    jobs = jobs_of(load_workflow(repo.path(relative)))
    job = jobs.get(proof["release_job"])
    if job is None:
        return [f"{relative} declares no `{proof['release_job']}` job"]
    where = f"{relative}: job `{proof['release_job']}`"
    artifact = dispatch["record_artifact"]
    findings: list[str] = []
    downloads = _using(job, DOWNLOAD)
    if not any(_with(download).get("name") == artifact for download in downloads):
        named = ", ".join(f"`{_with(download).get('name', '')}`" for download in downloads)
        findings.append(
            f"{where} downloads no artifact named `{artifact}` ({named or 'none at all'}), "
            f"which is what the release workflow uploads a dispatched run's record as"
        )
    findings.extend(
        f"{where} downloads `{artifact}` without `run-id: ${{{{ {TRIGGERING_RUN} }}}}`, and "
        f"the record is the triggering run's rather than this one's"
        for download in downloads
        if _with(download).get("name") == artifact
        and TRIGGERING_RUN not in _with(download).get("run-id", "")
    )

    recipe = dispatch["record_recipe"]
    output = proof["release_output"]
    reading = _running(job, recipe)
    if not reading:
        return [
            *findings,
            f"{where} runs no `just {recipe}` step, so a dispatched run's record is read "
            f"into no `{output}` and the version it published is proven by nothing",
        ]
    step = reading[0]
    identifier = str(step.get("id", "")).strip()
    if not identifier:
        findings.append(f"{where}'s `just {recipe}` step carries no `id`, so no output can name it")
    if JOB_OUTPUT not in str(step.get("run", "")):
        findings.append(
            f"{where}'s `just {recipe}` step does not append to `${JOB_OUTPUT}`, so what it "
            f"answered reaches no job output"
        )
    outputs = job.get("outputs")
    declared = " ".join(str(outputs.get(output, "") if isinstance(outputs, dict) else "").split())
    if identifier and f"steps.{identifier}.outputs.{output}" not in declared:
        findings.append(
            f"{where} publishes no output `{output}` from `steps.{identifier}.outputs.{output}`, "
            f"which is the version every route proof takes"
        )
    return findings
