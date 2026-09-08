"""Checks over the committed continuous-integration configuration."""

from __future__ import annotations

import re
from dataclasses import dataclass
from enum import StrEnum
from typing import Any

from repo_checks import install_path as ip
from repo_checks.model import Repo
from repo_checks.parsing import (
    MarkerBlockMissingError,
    fenced_commands,
    jobs_of,
    load_workflow,
    marker_block,
    programs_in,
    run_commands,
    steps_of,
)

PLATFORM_LINE = re.compile(
    r"^- `(?P<id>[a-z0-9_-]+)` — runner `(?P<runner>[^`]+)`, Rust target `(?P<target>[^`]+)`, "
    r"service manager `(?P<service_manager>[^`]+)`, install path: (?P<install>yes|no)$"
)
SECRET_REFERENCE = re.compile(r"secrets\.([A-Z0-9_]+)")
BUILT_IN_TOKEN = re.compile(r"secrets\.GITHUB_TOKEN|github\.token")
JUDGE_SECRETS = ("ANTHROPIC_API_KEY", "CLAUDE_CODE_OAUTH_TOKEN", "OPENAI_API_KEY")
HALTS_FOR_A_PERSON = ("manual-approval", "wait-for-approval", "approval-action")


@dataclass(frozen=True, slots=True)
class Platform:
    """One supported platform, as `AGENTS.md` declares it."""

    id: str
    runner: str
    target: str
    service_manager: str
    install_path: bool


def platforms_of(repo: Repo) -> list[Platform]:
    """Read the supported-platform list out of `AGENTS.md`."""
    found: list[Platform] = []
    for line in marker_block(repo.agents_md, "supported-platforms"):
        match = PLATFORM_LINE.match(line)
        if match:
            found.append(
                Platform(
                    match["id"],
                    match["runner"],
                    match["target"],
                    match["service_manager"],
                    match["install"] == "yes",
                )
            )
    return found


def _workflows(repo: Repo) -> dict[str, dict[str, Any]]:
    """Every committed workflow, keyed by file name."""
    return {path.name: load_workflow(path) for path in repo.workflow_paths}


def _matrix_platforms(job: dict[str, Any]) -> list[dict[str, Any]] | None:
    """The `platform` matrix a job declares, if it declares one."""
    matrix = (job.get("strategy") or {}).get("matrix")
    if not isinstance(matrix, dict):
        return None
    entries = matrix.get("platform")
    if not isinstance(entries, list):
        return None
    return [entry for entry in entries if isinstance(entry, dict)]


# What a matrix cell substitutes into a job's name. GitHub evaluates the
# expression per cell, so this is what turns one declared name into one status
# context per platform.
MATRIX_EXPRESSION = re.compile(r"\$\{\{\s*matrix\.platform\.(?P<key>[A-Za-z0-9_-]+)\s*\}\}")


class JobKind(StrEnum):
    """What a workflow job is, judged by what its own steps run.

    A closed set: `repo-policy.toml` may name only these, and every comparison
    below is against a member rather than a bare string.
    """

    GATE = "gate"
    INTEGRATION = "integration"
    LLMLINT = "llmlint"
    INSTALL = "install"
    OTHER = "other"


class PolicyValueError(ValueError):
    """`repo-policy.toml` declares a value a check cannot act on."""


def _bring_up_command(repo: Repo) -> str:
    """The command line the printer-integration job is recognized by, or empty."""
    recipe = repo.policy.get("integration", {}).get("bring_up")
    if not isinstance(recipe, str) or not recipe.strip():
        return ""
    return f"just {recipe.strip()}"


def platform_dependent_kinds(repo: Repo) -> frozenset[JobKind]:
    """The job kinds `repo-policy.toml` declares platform-dependent.

    Validated rather than coerced: a misspelt or mistyped declaration would
    silently stop the matrix rule applying to any job at all, which is a check
    that passes because it inspected nothing.

    Raises:
        PolicyValueError: If the declaration is absent, empty, or names anything
            that is not one of `JobKind`.
    """
    declared = repo.policy.get("workflows", {}).get("platform_dependent_kinds")
    if not isinstance(declared, list) or not declared:
        msg = (
            "`repo-policy.toml` declares no non-empty "
            "`workflows.platform_dependent_kinds` list, so no job could be held "
            "to the supported-platform list"
        )
        raise PolicyValueError(msg)
    known = {kind.value for kind in JobKind}
    kinds: set[JobKind] = set()
    for entry in declared:
        name = entry.strip() if isinstance(entry, str) else entry
        if name not in known:
            msg = (
                f"`repo-policy.toml`'s `workflows.platform_dependent_kinds` names "
                f"{entry!r}, which is not one of the job kinds these checks "
                f"classify ({', '.join(sorted(known))})"
            )
            raise PolicyValueError(msg)
        kinds.add(JobKind(name))
    return frozenset(kinds)


def _job_kind(job: dict[str, Any], path: ip.InstallPath, bring_up: str) -> JobKind:
    """Classify a job by what its own steps run, not by what it is called.

    `job` is the mapping the YAML reader handed back, so its values are `Any` at
    that deserialization boundary; every field this reads is narrowed before use.
    """
    commands = run_commands(job)
    if bring_up and bring_up in commands:
        return JobKind.INTEGRATION
    if "just check" in commands and "just bootstrap" in commands:
        return JobKind.GATE
    if any(command.startswith("just lint-llm-diff") for command in commands):
        return JobKind.LLMLINT
    if any(command in path.canonical for command in commands):
        return JobKind.INSTALL
    return JobKind.OTHER


@dataclass(frozen=True, slots=True)
class StatusContext:
    """One check run a workflow reports, under the name it reports it by.

    That name — not the job's key — is what a branch-protection rule requires,
    so it is what `AGENTS.md`'s required-checks record has to name.
    """

    name: str
    file: str
    job: str
    kind: JobKind


def _substituted(base: str, entry: dict[str, Any]) -> str:
    """A job's declared name with one matrix cell's own values put into it."""
    return MATRIX_EXPRESSION.sub(lambda found: str(entry.get(found["key"], "")), base)


def _context_names(job_name: str, job: dict[str, Any]) -> list[str]:
    """The check-run name each of a job's cells reports under.

    GitHub names a check run after the job's `name` where it sets one and after
    the job's key otherwise, substituting the cell's own matrix values into it. A
    matrixed job whose name interpolates none of them therefore reports every
    cell under one repeated name, which this returns as the repetition it is
    rather than collapsing.
    """
    declared = job.get("name")
    base = declared if isinstance(declared, str) and declared else job_name
    entries = _matrix_platforms(job)
    if entries is None:
        return [base]
    return [_substituted(base, entry) for entry in entries]


def status_contexts(repo: Repo) -> list[StatusContext]:
    """Every check run the committed workflows report, one per matrix cell."""
    path = ip.parse(repo.agents_md)
    bring_up = _bring_up_command(repo)
    return [
        StatusContext(name, file_name, job_name, _job_kind(job, path, bring_up))
        for file_name, workflow in _workflows(repo).items()
        for job_name, job in jobs_of(workflow).items()
        for name in _context_names(job_name, job)
    ]


def platforms(repo: Repo) -> list[str]:
    """The supported-platform list is the one source every CI matrix comes from."""
    try:
        declared = platforms_of(repo)
    except MarkerBlockMissingError as error:
        return [str(error)]

    try:
        dependent = platform_dependent_kinds(repo)
    except PolicyValueError as error:
        return [str(error)]

    findings: list[str] = []
    if not declared:
        return ["AGENTS.md's supported-platform list is empty"]
    if not any(item.install_path for item in declared):
        findings.append(
            "AGENTS.md's supported-platform list names no platform the end-user "
            "install path targets"
        )
    if not any(item.install_path and item.service_manager == "systemd" for item in declared):
        findings.append(
            "AGENTS.md's supported-platform list names no systemd platform, which is "
            "the service manager the unit in the end-user install path is written for"
        )

    declared_ids = [item.id for item in declared]
    runners = {item.id: item.runner for item in declared}
    path = ip.parse(repo.agents_md)
    bring_up = _bring_up_command(repo)
    for file_name, workflow in _workflows(repo).items():
        for job_name, job in jobs_of(workflow).items():
            kind = _job_kind(job, path, bring_up)
            entries = _matrix_platforms(job)
            if kind == JobKind.INTEGRATION:
                # The printer integration job's matrix is the `integration-tier`
                # check's: it is the one matrix the exclusions recorded in
                # AGENTS.md may narrow, and a rule stated in two places is a
                # rule that can disagree with itself.
                continue
            if kind not in dependent:
                if entries is not None:
                    findings.append(
                        f"{file_name}: job `{job_name}` declares a platform matrix, but "
                        f"`repo-policy.toml` names only "
                        f"{', '.join(f'`{one.value}`' for one in sorted(dependent))} as "
                        f"platform-dependent: running a job with no platform-dependent "
                        f"behaviour once per platform is repetition rather than coverage, "
                        f"and for a non-deterministic one it is two independent verdicts "
                        f"on one change"
                    )
                continue
            if entries is None:
                findings.append(
                    f"{file_name}: job `{job_name}` is a {kind} job but declares no platform matrix"
                )
                continue
            ids = [entry.get("id") for entry in entries]
            findings.extend(
                f"{file_name}: job `{job_name}`'s matrix names platform `{found}`, "
                f"which AGENTS.md's supported-platform list does not"
                for found in ids
                if found not in declared_ids
            )
            findings.extend(
                f"{file_name}: job `{job_name}`'s matrix omits platform `{wanted}`, "
                f"which AGENTS.md's supported-platform list names"
                for wanted in declared_ids
                if wanted not in ids
            )
            findings.extend(
                f"{file_name}: job `{job_name}`'s matrix runs `{entry.get('id')}` on "
                f"`{entry.get('runner')}`, but AGENTS.md declares `{runners[entry['id']]}`"
                for entry in entries
                if entry.get("id") in runners and entry.get("runner") != runners[entry["id"]]
            )
    return findings


def install_path_section(repo: Repo) -> list[str]:
    """The install-path section is complete, pasteable, and the only source of itself."""
    path = ip.parse(repo.agents_md)
    findings: list[str] = []

    if len(path.routes) != 3:
        findings.append(
            f"AGENTS.md's `{ip.SECTION_HEADING}` states {len(path.routes)} routes; "
            f"it must state exactly three alternatives"
        )
    if len(path.commands) != 2:
        findings.append(
            f"AGENTS.md's `{ip.SECTION_HEADING}` states {len(path.commands)} commands "
            f"after the routes; it must state exactly two, in order"
        )

    intro = " ".join(path.intro.split())
    if ip.ALTERNATIVES_SENTENCE not in intro:
        findings.append(
            f"AGENTS.md's `{ip.SECTION_HEADING}` does not say the routes are "
            f"alternatives: it must carry the sentence {ip.ALTERNATIVES_SENTENCE!r}"
        )
    findings.extend(
        f"AGENTS.md's `{ip.SECTION_HEADING}` presents the routes as a sequence "
        f"({marker!r}); they are alternatives, and a reader takes one of them"
        for marker in ip.SEQUENCE_MARKERS
        if marker in intro.lower()
    )

    for route in path.routes:
        if not route.commands:
            findings.append(f"route `{route.heading}` states no command")
            continue
        for command in route.commands:
            findings.extend(
                f"route `{route.heading}` states `{command}`, which carries "
                f"{marker!r} where a literal value belongs"
                for marker in ip.PLACEHOLDER_MARKERS
                if marker in command
            )

    findings.extend(_fetch_url_findings(repo, path))
    findings.extend(_pinned_form_findings(path))

    if path.commands and ip.STARTS_SERVICE.search(path.commands[0]):
        findings.append(
            f"the first command after the routes (`{path.commands[0]}`) starts or "
            f"enables the service; installing must not start a process that can move a "
            f"3D printer"
        )
    if "commands a 3D printer" not in repo.agents_md:
        findings.append(
            f"AGENTS.md's `{ip.SECTION_HEADING}` does not state why enabling and "
            f"starting is a command of its own"
        )

    findings.extend(ip.drifted_statements(path, _restatements(repo)))
    return findings


def _fetch_url_findings(repo: Repo, path: ip.InstallPath) -> list[str]:
    """Every fetch URL names this repository, its base branch and the declared path."""
    repository = repo.policy["repository"]
    expected = {
        repo.policy["workflows"]["install_script_path"],
        repo.policy["workflows"]["install_service_script_path"],
    }
    prefix = (
        f"https://raw.githubusercontent.com/{repository['owner']}/"
        f"{repository['name']}/{repository['base_branch']}/"
    )
    findings: list[str] = []
    for command in path.canonical:
        match = ip.RAW_URL.search(command)
        if not match:
            continue
        url = match.group(0)
        if not url.startswith(prefix):
            findings.append(
                f"`{command}` fetches from {url}, which does not name "
                f"{repository['owner']}/{repository['name']} on `{repository['base_branch']}`"
            )
            continue
        script = url[len(prefix) :]
        if script not in expected:
            findings.append(
                f"`{command}` fetches `{script}`, which is not a path this section "
                f"declares ({', '.join(sorted(expected))})"
            )
    return findings


def _pinned_form_findings(path: ip.InstallPath) -> list[str]:
    """The script route's pinned form carries concrete values, not metavariables."""
    for route in path.routes:
        if len(route.commands) < 2:
            continue
        pinned = route.commands[1]
        findings = []
        if not ip.PINNED_VERSION.search(pinned):
            findings.append(
                f"the pinned form `{pinned}` carries no concrete release tag "
                f"(expected `--version vX.Y.Z`)"
            )
        if not ip.PINNED_DIRECTORY.search(pinned):
            findings.append(
                f"the pinned form `{pinned}` carries no concrete install directory "
                f"(expected `--to <an absolute or ~ path>`)"
            )
        return findings
    return []


def _restatements(repo: Repo) -> dict[str, list[str]]:
    """Every other place in the tree that states one of the install path's commands."""
    candidates: dict[str, list[str]] = {}
    if repo.exists("README.md"):
        candidates["README.md"] = fenced_commands(repo.read("README.md"))
    for file_name, workflow in _workflows(repo).items():
        for job_name, job in jobs_of(workflow).items():
            commands = run_commands(job)
            if commands:
                candidates[f"{file_name} job `{job_name}`"] = commands
    return candidates


def continuous_integration(repo: Repo) -> list[str]:
    """The declared jobs are drawn from the recipe set and the install-path section."""
    from repo_checks.parsing import recipes as parse_recipes

    path = ip.parse(repo.agents_md)
    bring_up = _bring_up_command(repo)
    declared_recipes = set(parse_recipes(repo.justfile))
    findings: list[str] = []

    gate: tuple[str, str, dict[str, Any]] | None = None
    llmlint: tuple[str, str, dict[str, Any]] | None = None
    install: list[tuple[str, str, dict[str, Any]]] = []
    for file_name, workflow in _workflows(repo).items():
        for job_name, job in jobs_of(workflow).items():
            kind = _job_kind(job, path, bring_up)
            if kind == JobKind.GATE:
                gate = (file_name, job_name, job)
            elif kind == JobKind.LLMLINT:
                llmlint = (file_name, job_name, job)
            elif kind == JobKind.INSTALL:
                install.append((file_name, job_name, job))

    if gate is None:
        return [
            "the committed configuration declares no complete-gate job "
            "(a job whose steps run both `just bootstrap` and `just check`)"
        ]
    gate_commands = run_commands(gate[2])
    for command in gate_commands:
        words = command.split()
        if words[:1] != ["just"] or len(words) < 2:
            findings.append(
                f"{gate[0]}: the gate job runs `{command}`, which is not one of this "
                f"repository's recipes"
            )
        elif words[1] not in declared_recipes:
            findings.append(
                f"{gate[0]}: the gate job runs `just {words[1]}`, which the recipe set "
                f"does not declare"
            )
    for required in ("just bootstrap", "just check"):
        if required not in gate_commands:
            findings.append(f"{gate[0]}: the gate job does not run `{required}`")

    if llmlint is None:
        findings.append(
            "the committed configuration declares no judged-lint job "
            "(a job whose steps run `just lint-llm-diff`)"
        )
    elif llmlint[1] == gate[1] and llmlint[0] == gate[0]:
        findings.append(
            f"{gate[0]}: the judged-lint tier is a step of the gate job `{gate[1]}` "
            f"rather than a job of its own"
        )
    if any(command.startswith("just lint-llm") for command in gate_commands):
        findings.append(
            f"{gate[0]}: the judged-lint tier is a step of the gate job `{gate[1]}` "
            f"rather than a job of its own"
        )

    findings.extend(_install_job_findings(path, install))
    return findings


def _install_job_findings(
    path: ip.InstallPath, install: list[tuple[str, str, dict[str, Any]]]
) -> list[str]:
    """One job per route, each running that route and then the two commands, in order."""
    findings: list[str] = []
    for route in path.routes:
        if not any(route.command in run_commands(job) for _, _, job in install):
            findings.append(
                f"AGENTS.md names route `{route.heading}` (`{route.command}`), for which "
                f"the committed configuration declares no install job"
            )
    for file_name, job_name, job in install:
        commands = run_commands(job)
        findings.extend(
            f"{file_name}: install job `{job_name}` runs `{command}`, which AGENTS.md's "
            f"`{ip.SECTION_HEADING}` does not state"
            for command in commands
            if command not in path.canonical
        )
        positions = [commands.index(c) if c in commands else -1 for c in path.commands]
        for stated, position in zip(path.commands, positions, strict=True):
            if position < 0:
                findings.append(
                    f"{file_name}: install job `{job_name}` omits `{stated}`, which "
                    f"AGENTS.md states after the routes"
                )
        if all(position >= 0 for position in positions) and positions != sorted(positions):
            findings.append(
                f"{file_name}: install job `{job_name}` runs the two commands after the "
                f"routes out of the order AGENTS.md states"
            )
    return findings


def merge_model(repo: Repo) -> list[str]:
    """Every check `AGENTS.md` records as required is one the configuration reports."""
    try:
        required = [
            line[2:].strip().strip("`") for line in marker_block(repo.agents_md, "required-checks")
        ]
    except MarkerBlockMissingError as error:
        return [str(error)]

    findings: list[str] = []
    if not required:
        findings.append("AGENTS.md records no required check at all")

    reported = status_contexts(repo)
    by_name: dict[str, list[StatusContext]] = {}
    for context in reported:
        by_name.setdefault(context.name, []).append(context)

    findings.extend(
        f"AGENTS.md records `{name}` as a required check, but no committed workflow "
        f"reports a status context by that name; the names they report are "
        f"{', '.join(f'`{one}`' for one in sorted(by_name))}"
        for name in required
        if name not in by_name
    )
    findings.extend(
        f"AGENTS.md records `{name}` as a required check, but {len(by_name[name])} "
        f"matrix cells of job `{by_name[name][0].job}` report under that one name, so "
        f"a rule requiring it cannot say which of them was green"
        for name in required
        if name in by_name and len(by_name[name]) > 1
    )
    # Every context a required job reports is required too: a gate required on
    # one platform and not the other is a merge path the other never blocked.
    required_jobs = {
        (context.file, context.job) for name in required for context in by_name.get(name, [])
    }
    findings.extend(
        f"AGENTS.md records some but not all of job `{context.job}`'s status contexts as "
        f"required: `{context.name}` is not one of them, so a change could merge with "
        f"that cell red"
        for context in sorted(set(reported), key=lambda one: one.name)
        if (context.file, context.job) in required_jobs and context.name not in required
    )
    for kind, label in (
        (JobKind.GATE, "complete-gate"),
        (JobKind.LLMLINT, "judged-lint"),
    ):
        if not any(context.kind == kind for name in required for context in by_name.get(name, [])):
            findings.append(f"AGENTS.md's required checks omit the {label} job")

    if "takes no direct push" not in repo.agents_md:
        findings.append("AGENTS.md does not record that the base branch takes no direct push")
    return findings


def secrets(repo: Repo) -> list[str]:
    """Every secret a workflow names is one `gh-secrets.json` declares."""
    import json

    manifest = json.loads(repo.read("gh-secrets.json"))
    declared = {entry["name"] for entry in manifest["secrets"]}
    findings: list[str] = []

    path = ip.parse(repo.agents_md)
    bring_up = _bring_up_command(repo)
    for file_path in repo.workflow_paths:
        text = file_path.read_text(encoding="utf-8")
        findings.extend(
            f"{file_path.name} references secret `{name}`, which gh-secrets.json does not declare"
            for name in sorted(set(SECRET_REFERENCE.findall(text)))
            if name not in declared
        )

    release = _release_workflow(repo)
    if release is None:
        findings.append("no committed workflow performs releases")
    else:
        file_name, workflow = release
        text = repo.path(f".github/workflows/{file_name}").read_text(encoding="utf-8")
        names = set(SECRET_REFERENCE.findall(text))
        if "RELEASE_PLZ_TOKEN" not in names:
            findings.append(
                f"{file_name} does not open its release change request under `RELEASE_PLZ_TOKEN`"
            )
        if BUILT_IN_TOKEN.search(text):
            findings.append(
                f"{file_name} names the workflow's built-in token; a change request "
                f"opened by it does not trigger the workflows that gate it"
            )
        if "CARGO_REGISTRY_TOKEN" not in names:
            findings.append(f"{file_name} names no crate-publishing credential")

    for file_name, workflow in _workflows(repo).items():
        for job_name, job in jobs_of(workflow).items():
            if _job_kind(job, path, bring_up) != JobKind.LLMLINT:
                continue
            used = sorted(
                {
                    name
                    for step in steps_of(job)
                    for name in SECRET_REFERENCE.findall(str(step.get("env", "")))
                    if name in JUDGE_SECRETS
                }
            )
            if len(used) != 1:
                findings.append(
                    f"{file_name}: the judged-lint job `{job_name}` authenticates with "
                    f"{len(used)} of {', '.join(JUDGE_SECRETS)} ({used or 'none'}); it "
                    f"must reference exactly one"
                )
    return findings


def _release_workflow(repo: Repo) -> tuple[str, dict[str, Any]] | None:
    """The committed workflow that performs releases, if there is one."""
    for file_name, workflow in _workflows(repo).items():
        for job in jobs_of(workflow).values():
            if any(
                "release-plz" in program
                for command in run_commands(job)
                for program in programs_in(command)
            ):
                return file_name, workflow
    return None


def workflow_policy(repo: Repo) -> list[str]:
    """Actions are pinned, and every command a step runs is one the allowlist names."""
    from repo_checks.checks_repo import _allowlist_entries

    pin = re.compile(repo.policy["workflows"]["action_pin_pattern"])
    allowed = set(_allowlist_entries(repo))
    stated = set(ip.parse(repo.agents_md).canonical)
    findings: list[str] = []

    for file_path in repo.workflow_paths:
        workflow = load_workflow(file_path)
        for job_name, job in jobs_of(workflow).items():
            for step in steps_of(job):
                uses = step.get("uses")
                if isinstance(uses, str) and not pin.match(uses):
                    findings.append(
                        f"{file_path.name}: job `{job_name}` references action `{uses}`, "
                        f"which is not pinned in the form this repository declares "
                        f"({repo.policy['workflows']['action_pin_pattern']})"
                    )
            for command in run_commands(job):
                if command in stated:
                    continue
                findings.extend(
                    f"{file_path.name}: job `{job_name}` runs `{command}`, whose program "
                    f"`{program}` the command allowlist does not name"
                    for program in programs_in(command)
                    if program not in allowed
                )
    return findings
