"""Every copy of a platform fact in the tree, held to the one that declares it.

`repo_checks.platforms` declares what a platform is called and `AGENTS.md`'s
supported-platform list declares which platforms there are. Three places in this
tree state one of those facts again in a language that cannot import either: the
JavaScript launcher's own map of what Node reports to the package it resolves,
the install script's `uname` arms, and the toolchain file's list of Rust targets
a build host installs. Before this, two of the three were reconciled by nothing
at all — a platform could be named in the list and missed in the launcher, and
what the user met was a global install that resolved to no program.

So each is read here and held to the one source, and a disagreement is a finding
naming both sides rather than something a reader of two files has to notice.

The toolchain file is bound **one way only**, and deliberately: its `targets`
list is about what a *build host* installs, not about what each platform's own
runner builds natively, and six standard libraries on every runner would be
waste. Every target it names must be one the list names; it is not required to
name them all.

`unmatrixed_jobs` is the other half of the platform record: the jobs that carry
no platform matrix and the reason each carries none, so that a job running once
per change rather than once per platform is a recorded decision rather than an
omission a reader has to spot.
"""

from __future__ import annotations

import re
from typing import Any

from repo_checks import install_path as ip
from repo_checks.model import PolicyValueError, Repo, policy_strings, policy_table
from repo_checks.parsing import (
    MarkerBlockMissingError,
    jobs_of,
    load_workflow,
    marker_block,
)
from repo_checks.platforms import (
    UNMATRIXED_BLOCK,
    UNMATRIXED_LINE,
    UNMATRIXED_SHAPE,
    Platform,
    PlatformError,
    install_platforms,
    supported,
)

#: One entry of the launcher's own map: what Node reports for a platform, and
#: the package it resolves the program through there.
LAUNCHER_ENTRY = re.compile(r'^\s*"(?P<selector>[a-z0-9_]+-[a-z0-9_]+)":\s*"(?P<package>[^"]+)",?$')

#: Where the launcher's map begins and ends, so a string elsewhere in the file
#: is not read as one of its entries.
LAUNCHER_MAP_OPEN = "const PACKAGES = {"
LAUNCHER_MAP_CLOSE = "};"

#: How an install script settles which platform it is running on. The one
#: assignment its `uname` arms make, in whichever shell the script is written
#: in, and the only thing here that says which platforms that script reaches.
SCRIPT_PLATFORM = re.compile(r"""platform\s*=\s*["'](?P<id>[a-z0-9_-]+)["']""")

#: The `targets = [...]` array of the toolchain file.
TOOLCHAIN_TARGETS = re.compile(r"targets\s*=\s*\[(?P<targets>[^\]]*)\]", re.DOTALL)


def _declared(repo: Repo) -> tuple[list[Platform], list[Platform]] | None:
    """Every supported platform, and every one the install path targets."""
    try:
        return supported(repo), install_platforms(repo)
    except MarkerBlockMissingError:
        return None


def platform_facts(repo: Repo) -> list[str]:
    """No copy of a platform fact is maintained independently of its one source."""
    found = _declared(repo)
    if found is None:
        return ["AGENTS.md carries no supported-platform list to hold the tree to"]
    declared, installed = found
    if not declared:
        return ["AGENTS.md's supported-platform list is empty"]
    findings = _launcher_findings(repo, installed)
    findings.extend(_install_script_findings(repo, declared, installed))
    findings.extend(_toolchain_findings(repo, declared))
    return findings


def _launcher_findings(repo: Repo, installed: list[Platform]) -> list[str]:
    """The launcher resolves exactly the platforms the install path targets.

    Its map is the one place the JavaScript registry's own selectors are spelled
    out in this tree, and nothing in a `.mjs` can import the declaration they
    come from — so it is read back and compared here.
    """
    try:
        launcher = policy_strings(policy_table(repo, "platforms"), ("launcher",), "platforms")[
            "launcher"
        ]
    except PolicyValueError as error:
        return [str(error)]
    if not repo.exists(launcher):
        return [f"`{launcher}` is absent: it is the launcher every install of route 2 runs"]

    text = repo.read(launcher)
    if LAUNCHER_MAP_OPEN not in text:
        return [
            f"`{launcher}` carries no `{LAUNCHER_MAP_OPEN}` map, so nothing here can be "
            f"held to what AGENTS.md's supported-platform list names"
        ]
    body = text.split(LAUNCHER_MAP_OPEN, 1)[1].split(LAUNCHER_MAP_CLOSE, 1)[0]
    carried: dict[str, str] = {}
    for line in body.splitlines():
        match = LAUNCHER_ENTRY.match(line)
        if match:
            carried[match["selector"]] = match["package"]

    findings: list[str] = []
    wanted: dict[str, str] = {}
    for platform in installed:
        try:
            wanted[platform.npm_selector] = platform.npm_package
        except PlatformError as error:
            findings.append(str(error))
    findings.extend(
        f"`{launcher}` resolves `{selector}`, which is no platform the end-user "
        f"install path targets; AGENTS.md's supported-platform list reaches "
        f"{', '.join(f'`{one}`' for one in sorted(wanted)) or 'nothing'} by that route"
        for selector in sorted(carried)
        if selector not in wanted
    )
    findings.extend(
        f"`{launcher}` resolves no package for `{selector}`, which AGENTS.md's "
        f"supported-platform list answers `install path: yes` for"
        for selector in sorted(wanted)
        if selector not in carried
    )
    findings.extend(
        f"`{launcher}` resolves `{selector}` through `{carried[selector]}`, and the "
        f"platform declaration names `{wanted[selector]}`"
        for selector in sorted(wanted)
        if selector in carried and carried[selector] != wanted[selector]
    )
    return findings


def install_scripts(path: ip.InstallPath) -> list[str]:
    """Every install script the routes fetch, in the order they are stated.

    Derived from the fetch commands the routes state rather than from a list
    beside them: a script route reaches whichever platforms its own script has
    an arm for, and which scripts there are is what the section says.
    """
    found: list[str] = []
    for route in path.routes:
        for command in route.commands:
            match = ip.RAW_URL.search(command)
            if match is None:
                continue
            script = match.group(0).partition("/main/")[2]
            if script and script not in found:
                found.append(script)
    return found


def _install_script_findings(
    repo: Repo, declared: list[Platform], installed: list[Platform]
) -> list[str]:
    """Every platform the install path targets is reached by exactly one script.

    A shell script is not a route a Windows machine can take, so route 3 is one
    fetch command per script behind it rather than one command for everybody.
    What decides which platforms a script reaches is the script's own arms.
    """
    path = ip.parse(repo.agents_md)
    known = {platform.id for platform in declared}
    wanted = [platform.id for platform in installed]
    reached: dict[str, list[str]] = {}
    findings: list[str] = []
    for script in install_scripts(path):
        if not repo.exists(script):
            findings.append(
                f"AGENTS.md's `{ip.SECTION_HEADING}` fetches `{script}`, and this "
                f"repository commits no such script"
            )
            continue
        for identifier in sorted(set(SCRIPT_PLATFORM.findall(repo.read(script)))):
            reached.setdefault(identifier, []).append(script)
    findings.extend(
        f"`{', '.join(scripts)}` has an arm for `{identifier}`, which AGENTS.md's "
        f"supported-platform list does not name"
        for identifier, scripts in sorted(reached.items())
        if identifier not in known
    )
    findings.extend(
        f"no install script AGENTS.md's `{ip.SECTION_HEADING}` fetches has an arm for "
        f"`{identifier}`, which its supported-platform list answers `install path: yes` "
        f"for: a platform no script reaches cannot take route 3"
        for identifier in wanted
        if identifier not in reached
    )
    findings.extend(
        f"`{identifier}` is reached by {len(scripts)} of the install scripts AGENTS.md's "
        f"`{ip.SECTION_HEADING}` fetches ({', '.join(scripts)}); exactly one must reach it"
        for identifier, scripts in sorted(reached.items())
        if identifier in wanted and len(scripts) != 1
    )
    return findings


def _toolchain_findings(repo: Repo, declared: list[Platform]) -> list[str]:
    """Every Rust target the toolchain file installs is one the list names.

    One way only: the list is what a target must be, and the file is free to
    install fewer than all of them — see this module's own docstring.
    """
    try:
        toolchain = policy_strings(policy_table(repo, "platforms"), ("toolchain",), "platforms")[
            "toolchain"
        ]
    except PolicyValueError as error:
        return [str(error)]
    if not repo.exists(toolchain):
        return [f"`{toolchain}` is absent: it is where this repository pins its Rust toolchain"]
    match = TOOLCHAIN_TARGETS.search(repo.read(toolchain))
    if match is None:
        return [
            f"`{toolchain}` declares no `targets` array, so nothing here says which Rust "
            f"standard libraries a build host installs"
        ]
    named = [entry.strip().strip('"').strip("'") for entry in match["targets"].split(",")]
    known = {platform.target: platform.id for platform in declared}
    return [
        f"`{toolchain}` installs the Rust target `{target}`, which no platform on "
        f"AGENTS.md's supported-platform list is built for; it names "
        f"{', '.join(f'`{one}`' for one in sorted(known)) or 'nothing'}"
        for target in named
        if target and target not in known
    ]


def _owes_a_reason(repo: Repo, jobs: dict[str, dict[str, Any]]) -> dict[str, str]:
    """Every job this record has to account for, and what makes it one.

    Derived rather than listed, so the record cannot go stale against the tree:
    a job whose green a merge waits on and a job running the scheduled Obico tier
    are the two kinds a reader expects a platform matrix on, and each of them
    that carries none owes the reason it does.
    """
    from repo_checks.checks_ci import WorkflowValueError, status_contexts

    try:
        required = {
            line[2:].strip().strip("`") for line in marker_block(repo.agents_md, "required-checks")
        }
    except MarkerBlockMissingError:
        required = set()
    owed: dict[str, str] = {}
    try:
        for context in status_contexts(repo):
            if context.name in required:
                owed[context.job] = "a check a merge of this repository waits on"
    except WorkflowValueError:
        # A workflow a check run cannot be derived from is that check's finding
        # to report, and reporting it here as well would say it twice.
        pass
    tier = str(policy_table(repo, "obico").get("tier", "")).strip()
    if tier:
        from repo_checks.parsing import run_commands

        owed.update(
            {
                job_name: "the scheduled Obico tier"
                for job_name, job in jobs.items()
                if f"just {tier}" in run_commands(job)
            }
        )
    return owed


def unmatrixed_jobs(repo: Repo) -> list[str]:
    """Every job recorded as carrying no platform matrix is there, and says why."""
    from repo_checks.checks_ci import _matrix_platforms

    try:
        recorded = marker_block(repo.agents_md, UNMATRIXED_BLOCK)
    except MarkerBlockMissingError as error:
        return [str(error)]

    # `Any` at the deserialization boundary: a workflow's jobs are whatever the
    # YAML reader handed back, and the one thing read out of them here —
    # whether a job declares a platform matrix — narrows its own values.
    jobs: dict[str, dict[str, Any]] = {}
    for file_path in repo.workflow_paths:
        for job_name, job in jobs_of(load_workflow(file_path)).items():
            jobs[job_name] = job

    findings: list[str] = []
    named: set[str] = set()
    for line in recorded:
        if not line.startswith("- "):
            continue
        match = UNMATRIXED_LINE.match(line)
        if match is None:
            findings.append(
                f"AGENTS.md records the unmatrixed job `{line}`, which is not of the "
                f"form `{UNMATRIXED_SHAPE}`"
            )
            continue
        if not (match["reason"] or "").strip():
            findings.append(
                f"AGENTS.md records job `{match['job']}` as carrying no platform matrix, "
                f"with no reason it carries none"
            )
            continue
        named.add(match["job"])
    findings.extend(
        f"AGENTS.md records job `{job}` as carrying no platform matrix, and the "
        f"committed workflows declare no job by that name"
        for job in sorted(named)
        if job not in jobs
    )
    findings.extend(
        f"AGENTS.md records job `{job}` as carrying no platform matrix, and the "
        f"committed workflows declare one on it"
        for job in sorted(named)
        if job in jobs and _matrix_platforms(jobs[job]) is not None
    )
    findings.extend(
        f"the committed workflows declare job `{job}`, which is {why} and carries no "
        f"platform matrix, and AGENTS.md records no reason it carries none"
        for job, why in sorted(_owes_a_reason(repo, jobs).items())
        if job not in named and _matrix_platforms(jobs[job]) is None
    )
    return findings
