"""Checks over the committed continuous-integration configuration."""

from __future__ import annotations

import re
from collections.abc import Mapping
from dataclasses import dataclass
from enum import StrEnum
from typing import Any

from repo_checks import install_path as ip
from repo_checks.model import PolicyValueError, Repo, policy_strings, policy_table
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
from repo_checks.shell import run

#: The prefix every recipe that takes one shipped artifact the way its own
#: consumer takes it is named with.
PROVE_PREFIX = "prove-"

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
    ARTIFACT = "artifact"
    OTHER = "other"


class WorkflowValueError(ValueError):
    """A committed workflow declares something a check cannot derive from."""


def _bring_up_command(repo: Repo) -> str:
    """The command line the printer-integration job is recognized by, or empty."""
    recipe = policy_table(repo, "integration").get("bring_up")
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
    declared = policy_table(repo, "workflows").get("platform_dependent_kinds")
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


def _job_kind(
    job: dict[str, Any], path: ip.InstallPath, bring_up: str, artifact: tuple[str, ...] = ()
) -> JobKind:
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
    if any(command in artifact for command in commands):
        return JobKind.ARTIFACT
    return JobKind.OTHER


def artifact_commands(repo: Repo) -> tuple[str, ...]:
    """Every command a job that builds or proves a shipped artifact runs.

    An artifact job is platform-dependent for the reason the three end-user
    routes exist: each carries the `printobserver` program **already built for
    the platform**, so a build or an install of one on a second platform is a
    second thing proven rather than the same thing twice.
    """
    from repo_checks.parsing import recipes as parse_recipes

    declared = policy_table(repo, "release")
    building = str(declared.get("build_recipe", "")).strip()
    named = [f"just {building}"] if building else []
    named.extend(
        f"just {name}"
        for name in sorted(parse_recipes(repo.justfile))
        if name.startswith(PROVE_PREFIX)
    )
    return tuple(named)


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


@dataclass(frozen=True, slots=True)
class MatrixCell:
    """One cell of a job's `platform` matrix, as a check-run name can read it.

    The field names are the workflow author's own, so that set is open; what is
    closed is what a value may be. `read` settles it, and `fields` holds only
    what survived, so nothing downstream re-checks a value it was handed.
    """

    where: str
    fields: Mapping[str, str]
    declared: frozenset[str]

    @classmethod
    def read(cls, entry: dict[str, Any], where: str) -> MatrixCell:
        """Read one declared cell of a matrix.

        `entry` is the mapping the YAML reader handed back, so its values are
        `Any` at that deserialization boundary; this is the one place they stop
        being. A field carrying anything but a non-empty scalar is not one a
        check-run name can be built from, so it is kept out rather than coerced,
        and `declared` remembers it was there so `substituted` can say which of
        the two went wrong.
        """
        return cls(
            where,
            {
                str(key): str(value)
                for key, value in entry.items()
                if isinstance(value, str | int) and str(value).strip()
            },
            frozenset(str(key) for key in entry),
        )

    def substituted(self, base: str) -> str:
        """`base` with this cell's own values put into it.

        Raises:
            WorkflowValueError: If `base` interpolates a field this cell has not
                got. Substituting an empty string there would derive a context
                nothing reports, and the record would then read as having drifted
                rather than as the typo in the workflow that it is.
        """

        def value(found: re.Match[str]) -> str:
            key = found["key"]
            if key in self.fields:
                return self.fields[key]
            if key in self.declared:
                msg = (
                    f"{self.where}'s name interpolates `{found[0]}`, whose value is not "
                    f"something a status context can be named after"
                )
            else:
                carried = ", ".join(f"`{one}`" for one in sorted(self.fields)) or "nothing"
                msg = (
                    f"{self.where}'s name interpolates `{found[0]}`, but its matrix "
                    f"cells carry {carried}"
                )
            raise WorkflowValueError(msg)

        return MATRIX_EXPRESSION.sub(value, base)


def _context_names(job_name: str, job: dict[str, Any], where: str) -> list[str]:
    """The check-run name each of a job's cells reports under.

    GitHub names a check run after the job's `name` where it sets one and after
    the job's key otherwise, substituting the cell's own matrix values into it. A
    matrixed job whose name interpolates none of them therefore reports every
    cell under one repeated name, which this returns as the repetition it is
    rather than collapsing.

    `job` is the mapping the YAML reader handed back, so its values are `Any` at
    that deserialization boundary; the name is narrowed here and the cells by
    `MatrixCell.read`.

    Raises:
        WorkflowValueError: If a cell cannot be substituted into the name.
    """
    declared = job.get("name")
    base = declared if isinstance(declared, str) and declared else job_name
    entries = _matrix_platforms(job)
    if entries is None:
        return [base]
    return [MatrixCell.read(entry, where).substituted(base) for entry in entries]


def status_contexts(repo: Repo) -> list[StatusContext]:
    """Every check run the committed workflows report, one per matrix cell.

    Raises:
        WorkflowValueError: If a job's name cannot be resolved to the names its
            cells report under.
    """
    path = ip.parse(repo.agents_md)
    bring_up = _bring_up_command(repo)
    artifact = artifact_commands(repo)
    return [
        StatusContext(name, file_name, job_name, _job_kind(job, path, bring_up, artifact))
        for file_name, workflow in _workflows(repo).items()
        for job_name, job in jobs_of(workflow).items()
        for name in _context_names(job_name, job, f"{file_name}: job `{job_name}`")
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
    artifact = artifact_commands(repo)
    for file_name, workflow in _workflows(repo).items():
        for job_name, job in jobs_of(workflow).items():
            kind = _job_kind(job, path, bring_up, artifact)
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
            cells = [MatrixCell.read(entry, f"{file_name}: job `{job_name}`") for entry in entries]
            findings.extend(
                f"{file_name}: job `{job_name}`'s matrix has a cell whose `id` is not a "
                f"platform name, so there is nothing to hold to AGENTS.md's list"
                for cell in cells
                if "id" not in cell.fields
            )
            ids = [cell.fields["id"] for cell in cells if "id" in cell.fields]
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
                f"{file_name}: job `{job_name}`'s matrix runs `{cell.fields['id']}` on "
                f"`{cell.fields.get('runner')}`, but AGENTS.md declares "
                f"`{runners[cell.fields['id']]}`"
                for cell in cells
                if cell.fields.get("id") in runners
                and cell.fields.get("runner") != runners[cell.fields["id"]]
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
    findings.extend(_verification_findings(path))

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
    try:
        repository = policy_strings(
            policy_table(repo, "repository"), ("owner", "name", "base_branch"), "repository"
        )
        expected = set(
            policy_strings(
                policy_table(repo, "workflows"),
                ("install_script_path", "install_service_script_path"),
                "workflows",
            ).values()
        )
    except PolicyValueError as error:
        return [str(error)]
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


def _verification_findings(path: ip.InstallPath) -> list[str]:
    """The section states one command that reads back what a route installed.

    Every route ends the same way — a program on a path — and whether that
    program runs is not something the install commands can report: a package
    manager that unpacked an unrunnable program exits zero. So the check on
    what was installed is stated here, in the one authoritative source of this
    path, which is what lets the jobs that prove these routes run it without
    growing a command nobody declared.
    """
    if len(path.verification) != 1:
        return [
            f"AGENTS.md's `{ip.SECTION_HEADING}` states {len(path.verification)} commands "
            f"under a `{ip.VERIFICATION_HEADING}...` subsection; it must state exactly one, "
            f"which runs the program whichever route was taken installed"
        ]
    checked = path.checked
    findings = [
        f"the check on what was installed (`{checked}`) carries {marker!r} where a "
        f"literal value belongs"
        for marker in ip.PLACEHOLDER_MARKERS
        if marker in checked
    ]
    if checked.split()[:1] != [ip.VERIFICATION_PROGRAM]:
        findings.append(
            f"the check on what was installed (`{checked}`) does not run "
            f"`{ip.VERIFICATION_PROGRAM}`, which is the program every route installs"
        )
    if ip.VERIFICATION_OPTION not in checked.split():
        findings.append(
            f"the check on what was installed (`{checked}`) does not ask the program "
            f"which version it is (`{ip.VERIFICATION_OPTION}`), so it cannot tell an "
            f"install that worked from one that installed something else"
        )
    return findings


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
    artifact = artifact_commands(repo)
    declared_recipes = set(parse_recipes(repo.justfile))
    findings: list[str] = []

    gate: tuple[str, str, dict[str, Any]] | None = None
    llmlint: tuple[str, str, dict[str, Any]] | None = None
    install: list[tuple[str, str, dict[str, Any]]] = []
    for file_name, workflow in _workflows(repo).items():
        for job_name, job in jobs_of(workflow).items():
            kind = _job_kind(job, path, bring_up, artifact)
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

    findings.extend(_install_job_findings(repo, path, install))
    return findings


def _waived(repo: Repo) -> tuple[tuple[str, ...], str]:
    """The steps of an install job whose failure does not fail it, and what it costs."""
    declared = repo.policy.get("workflows", {})
    steps = declared.get("waived_steps")
    named = (
        tuple(str(step) for step in steps)
        if isinstance(steps, list) and all(isinstance(step, str) and step.strip() for step in steps)
        else ()
    )
    return named, str(declared.get("waived_report", "")).strip()


def _reporting_findings(
    repo: Repo, file_name: str, job_name: str, job: dict[str, Any]
) -> list[str]:
    """The two waived commands carry their ids, and the run says what they reached.

    Their failure not failing the job is the waiver, and this is its price: a
    completed run has to tell a route whose service came up from one whose did
    not. Unreported, a green job says nothing about whether the service was
    ever established — which is a credential-dependent failure disappearing,
    and is exactly what making them non-fatal must not buy.
    """
    waived, summary = _waived(repo)
    if not waived or not summary:
        return [
            "`repo-policy.toml`'s `[workflows]` declares no `waived_steps` and "
            "`waived_report`, so nothing can say which steps of an install job may fail "
            "without failing it, or what a run must then report"
        ]
    where = f"{file_name}: install job `{job_name}`"
    findings: list[str] = []
    carried = {str(step.get("id", "")): step for step in steps_of(job)}
    for wanted in waived:
        step = carried.get(wanted)
        if step is None:
            findings.append(
                f"{where} declares no `{wanted}` step, so nothing of this run can report "
                f"what that command reached"
            )
        elif step.get("continue-on-error") is not True:
            findings.append(
                f"{where}: step `{wanted}` is fatal, and the two commands after the routes "
                f"cannot succeed unattended — made fatal this job fails on every run "
                f"whatever the world looks like, which is how it went unread"
            )
    if findings:
        return findings

    # `outcome` and not `conclusion`: `continue-on-error` is what makes
    # `conclusion` read `success` for a step that failed, so a report reading
    # it would say every route's service came up every time.
    reporting = [
        command
        for command in run_commands(job)
        if summary in command and all(f"{wanted}.outcome" in str(job) for wanted in waived)
    ]
    if not reporting:
        findings.append(
            f"{where} lets {', '.join(f'`{step}`' for step in waived)} fail without failing "
            f"it, and writes neither outcome into `${summary}`: a reader of a green run "
            f"cannot then tell a route whose service was established from one whose was not"
        )
    return findings


def _install_job_findings(
    repo: Repo, path: ip.InstallPath, install: list[tuple[str, str, dict[str, Any]]]
) -> list[str]:
    """One job per route, each running that route and then the two commands, in order."""
    findings: list[str] = []
    for route in path.routes:
        if not any(route.command in run_commands(job) for _, _, job in install):
            findings.append(
                f"AGENTS.md names route `{route.heading}` (`{route.command}`), for which "
                f"the committed configuration declares no install job"
            )
    _, summary = _waived(repo)
    for file_name, job_name, job in install:
        commands = run_commands(job)
        findings.extend(
            f"{file_name}: install job `{job_name}` runs `{command}`, which AGENTS.md's "
            f"`{ip.SECTION_HEADING}` does not state"
            for command in commands
            # The one command such a job may run beside the stated ones: the
            # report the waiver above costs. It installs nothing and reaches no
            # registry — it writes what the two waived steps reached into the
            # run's own summary.
            if command not in path.canonical and not (summary and summary in command)
        )
        findings.extend(_reporting_findings(repo, file_name, job_name, job))
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
        findings.extend(_checked_findings(path, file_name, job_name, commands, positions))
    return findings


def _checked_findings(
    path: ip.InstallPath,
    file_name: str,
    job_name: str,
    commands: list[str],
    positions: list[int],
) -> list[str]:
    """The job runs the check on what it installed, after the route and before the rest.

    A job that only installs cannot tell an install that worked from one that
    put something on the path that does not run, which is the whole of what
    these jobs are for. The ordering is load-bearing in both directions: run
    before the route it is checking, it reads whatever was already on the host,
    and run after the service commands, it has let a broken program reach a
    service before anything looked at it.
    """
    checked = path.checked
    where = f"{file_name}: install job `{job_name}`"
    if not checked:
        return []
    if checked not in commands:
        return [
            f"{where} omits `{checked}`, which AGENTS.md states as the check on what a "
            f"route installed: a job that only installs cannot tell an install that "
            f"worked from one that put an unrunnable program on the path"
        ]
    at = commands.index(checked)
    taken = [commands.index(route.command) for route in path.routes if route.command in commands]
    findings: list[str] = []
    if taken and at < min(taken):
        findings.append(
            f"{where} runs `{checked}` before the route it is checking, so it reads "
            f"whatever was already on the host rather than what this run installed"
        )
    afterwards = [position for position in positions if position >= 0]
    if afterwards and at > min(afterwards):
        findings.append(
            f"{where} runs `{checked}` after the commands that put the service in "
            f"place, so a program that does not run reaches a service before anything "
            f"has looked at it"
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

    try:
        reported = status_contexts(repo)
    except WorkflowValueError as error:
        return [*findings, str(error)]

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
    artifact = artifact_commands(repo)
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
            if _job_kind(job, path, bring_up, artifact) != JobKind.LLMLINT:
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


#: What `release-targets.toml` says of a target release automation assembles
#: rather than publishes straight from the workspace.
ASSEMBLED = "release-artifacts"


def _shipped(repo: Repo) -> list[dict[str, Any]]:
    """Every artifact this repository ships beside the crates it publishes."""
    return [
        target
        for target in repo.read_toml("release-targets.toml").get("target", [])
        if str(target.get("built_by", "")).strip() == ASSEMBLED
    ]


def proving(repo: Repo) -> dict[str, tuple[str, ...]]:
    """Which recipes take each shipped artifact and prove what they took.

    Read out of each recipe's own body — the target it names — rather than out
    of a list beside it, so a recipe pointed at another artifact is one this
    stops finding for the artifact it used to prove.

    Every one of them, rather than the last one read: a route is proven twice
    and the two answer different questions — one over an artifact built from
    the committed tree, which a change can run before anything is published,
    and one over what its own registry serves, which is what a user meets. A
    mapping that kept one would leave this answering for a proof that had gone.
    """
    from repo_checks.parsing import recipes as parse_recipes

    found: dict[str, list[str]] = {}
    for name, recipe in parse_recipes(repo.justfile).items():
        if not name.startswith(PROVE_PREFIX):
            continue
        for line in recipe.body:
            words = line.split()
            if "prove" not in words or "--target" not in words:
                continue
            proven = found.setdefault(words[words.index("--target") + 1], [])
            if name not in proven:
                proven.append(name)
    return {identifier: tuple(names) for identifier, names in found.items()}


def artifact_jobs(repo: Repo) -> list[str]:
    """One job per shipped artifact, taking it the way its own consumer does.

    Each of the three clients has a job that builds it, installs it and runs
    that client's own smoke check; each of the three end-user routes has one
    that builds its artifact and takes it the way that route's own command
    takes it, once per platform `AGENTS.md`'s install-path section names. That
    section is the authority for both sets rather than the matrix beside them,
    because a check reading the matrix is satisfied by narrowing the matrix.
    """
    try:
        wanted = [platform.id for platform in platforms_of(repo)]
    except MarkerBlockMissingError as error:
        return [str(error)]

    path = ip.parse(repo.agents_md)
    proven_by = proving(repo)
    findings: list[str] = []
    jobs: dict[str, tuple[str, str, dict[str, Any]]] = {}
    for file_name, workflow in _workflows(repo).items():
        for job_name, job in jobs_of(workflow).items():
            for command in run_commands(job):
                recipe = command.removeprefix("just ").strip()
                if recipe.startswith(PROVE_PREFIX):
                    jobs[recipe] = (file_name, job_name, job)

    routed = {
        str(target.get("route", "")).strip(): str(target.get("id", ""))
        for target in _shipped(repo)
        if str(target.get("route", "")).strip()
    }
    for target in _shipped(repo):
        identifier = str(target.get("id", ""))
        for recipe in proven_by.get(identifier, ()) or [""]:
            if not recipe:
                findings.append(
                    f"release-targets.toml declares `{identifier}`, and no `{PROVE_PREFIX}` "
                    f"recipe builds it, installs it and proves what it installed"
                )
                continue
            if recipe not in jobs:
                findings.append(
                    f"the committed configuration declares no job running `just {recipe}`, "
                    f"so nothing proves `{identifier}` that way"
                )
                continue
            file_name, job_name, job = jobs[recipe]
            entries = _matrix_platforms(job) or []
            named = {
                str(entry["id"]) for entry in entries if isinstance(entry, dict) and "id" in entry
            }
            findings.extend(
                f"{file_name}: job `{job_name}` proves `{identifier}` on "
                f"{sorted(named) or 'no'} platform(s), and AGENTS.md names `{platform}`"
                for platform in wanted
                if platform not in named
            )

    findings.extend(
        f"AGENTS.md's `{ip.SECTION_HEADING}` names route `{route.heading}`, for which "
        f"the committed configuration declares no job of its own: the three routes are "
        f"alternatives, and a route with no job is a route nothing proves"
        for route in path.routes
        if route.heading not in routed
    )
    return findings


def install_script_path(repo: Repo) -> list[str]:
    """The install script is committed at exactly the path its own fetch URL names.

    Without this the section could go on offering a runnable one-line command
    that fetches nothing.
    """
    path = ip.parse(repo.agents_md)
    findings: list[str] = []
    fetched = False
    for command in path.canonical:
        match = ip.RAW_URL.search(command)
        if match is None:
            continue
        script = match.group(0).rpartition("/main/")[2]
        if not script:
            findings.append(f"`{command}` fetches a URL naming no path in this repository")
            continue
        fetched = True
        if not repo.exists(script):
            findings.append(
                f"AGENTS.md's `{ip.SECTION_HEADING}` states `{command}`, and this "
                f"repository commits no {script}: the command it offers fetches nothing"
            )
    if not fetched:
        findings.append(
            f"AGENTS.md's `{ip.SECTION_HEADING}` states no command that fetches a script "
            f"from this repository"
        )
    return findings


def cut_from(repo: Repo, base: str) -> tuple[str | None, str]:
    """The commit this work was cut from, and why there is none where there is not.

    Neither branch tip is the answer and neither is the merge base. A tip moves
    whenever anything lands on it, and the merge base moves with it the moment
    that branch is merged into this one — which is the *first* thing publishing a
    branch here does. So a route deleted on the base branch and then merged in
    would move the merge base past the commit that still had it, and the one
    thing this check exists to refuse would pass the moment the deletion arrived
    from the other side.

    What no branch moving can change is this branch's own line of descent. A
    merge of the base into it arrives on the **second** parent, so the first-
    parent walk from `HEAD` is exactly the commits this work is, and the newest
    of those the base branch also carries is the commit it was cut from. Landing
    more on the base branch puts no commit on that line, and merging it in puts
    only the merge itself there — so the reference this answers with is the same
    before the base moves, after it moves, and after it has been merged in.
    """
    references = [
        reference
        for reference in (f"origin/{base}", base)
        if run(
            ["git", "rev-parse", "--verify", "--quiet", f"{reference}^{{commit}}"],
            cwd=repo.root,
        ).returncode
        == 0
    ]
    if not references:
        return None, (
            f"neither `origin/{base}` nor `{base}` is in this repository's history, so "
            f"nothing can say whether the install path has been narrowed"
        )
    descent = run(["git", "rev-list", "--first-parent", "HEAD"], cwd=repo.root)
    if descent.returncode != 0:
        return None, (
            "this tree has no history of its own, so nothing can say which install "
            "path this work was cut from"
        )
    carried = run(["git", "rev-list", *references], cwd=repo.root)
    if carried.returncode != 0:
        return None, (
            f"`{base}` could not be read, so nothing can say which install path this "
            f"work was cut from"
        )
    on_the_base = set(carried.stdout.split())
    for commit in descent.stdout.split():
        if commit in on_the_base:
            return commit, ""
    return None, (
        f"this tree's history and `{base}`'s share no commit, so nothing can say "
        f"which install path this work was cut from"
    )


def install_path_not_narrowed(repo: Repo) -> list[str]:
    """The install path still names every platform and route it was cut with.

    Read out of the commit this work was cut from rather than out of this tree,
    because everything else about the platform set and the route set is derived
    from that section — so a change that deleted an entry from it and narrowed
    the automation to match would satisfy every other check here.
    """
    try:
        base = policy_strings(policy_table(repo, "repository"), ("base_branch",), "repository")[
            "base_branch"
        ]
    except PolicyValueError as error:
        return [str(error)]

    commit, why = cut_from(repo, base)
    if commit is None:
        return [why]
    found = run(["git", "show", f"{commit}:AGENTS.md"], cwd=repo.root)
    if found.returncode != 0:
        return [
            f"the commit this work was cut from ({commit[:12]}) carries no AGENTS.md, so "
            f"nothing can say whether the install path has been narrowed"
        ]
    was = found.stdout
    whence = f"the commit this work was cut from ({commit[:12]})"

    findings: list[str] = []
    try:
        before = {
            match["id"]
            for line in marker_block(was, "supported-platforms")
            if (match := PLATFORM_LINE.match(line))
        }
        now = {platform.id for platform in platforms_of(repo)}
    except MarkerBlockMissingError as error:
        return [str(error)]
    findings.extend(
        f"AGENTS.md's supported-platform list named `{platform}` on {whence} and no "
        f"longer does: every artifact, matrix and route here is derived from that list, "
        f"so narrowing it narrows all of them at once"
        for platform in sorted(before - now)
    )

    stated = {route.heading for route in ip.parse(repo.agents_md).routes}
    findings.extend(
        f"AGENTS.md's `{ip.SECTION_HEADING}` stated route `{heading}` on {whence} and no "
        f"longer does: a route deleted here is a way to the program nobody has any more"
        for heading in sorted({route.heading for route in ip.parse(was).routes} - stated)
    )
    return findings
