"""Checks over the service this repository installs, and its ingress bound.

Two things this node's server has to be held to are facts about the tree that a
running system would only reveal after it was too late.

The first is the pair of names in `AGENTS.md`'s install-path section — the unit
the operator enables, and the path the installer is fetched from. That section
is the authoritative source of both, and the other party to the contract is a
continuous-integration job that installs the service on a real machine, so a
tree whose installer writes a differently named unit is one nothing catches
until that job runs.

The second is the ingress answer bound. `Obico` posts best-effort with a short
timeout and does not retry, so the bound this repository ships has to be below
that timeout — and the timeout is an external producer's number, written down in
`AGENTS.md` with the release it was read from. What can be checked here is that
the written-down number is there with its provenance, that the shipped default
is below it, and that the server's own copy of it agrees. Whether the number is
still true of a live `Obico` is the scheduled `Obico` tier's to say.
"""

from __future__ import annotations

import json
import re
from dataclasses import dataclass

from repo_checks import install_path as ip
from repo_checks.model import (
    PolicyValueError,
    Repo,
    policy_string_list,
    policy_strings,
    policy_table,
)
from repo_checks.parsing import MarkerBlockMissingError, marker_block, section

# `sudo systemctl enable --now <unit>`: the install path's third command, whose
# last word is the unit's name.
ENABLE_COMMAND = re.compile(r"\bsystemctl\s+enable\s+--now\s+(?P<unit>\S+)\s*$")

# `UNIT_NAME="printobserver.service"` in the installer.
UNIT_ASSIGNMENT = re.compile(r'^UNIT_NAME="(?P<unit>[^"]+)"\s*$', re.MULTILINE)

# `pub const NAME: u64 = 1_000;` in a Rust source.
CONSTANT = re.compile(
    r"pub const (?P<name>[A-Z_]+): u64 = (?P<value>[0-9_]+)\s*;",
)

# `- posting timeout: `5000` ms` in the recorded block.
RECORDED_TIMEOUT = re.compile(r"^-\s*posting timeout:\s*`(?P<milliseconds>[0-9_]+)`\s*ms\s*$")

# A shell line that opens a heredoc, and the delimiter it ends at.
HEREDOC_OPEN = re.compile(r"<<-?(?P<delimiter>[A-Za-z_][A-Za-z0-9_]*)")

# `"pause"` and the rest, as the configuration template's grants spell them.
QUOTED_NAME = re.compile(r'"([a-z_]+)"')


@dataclass(frozen=True, slots=True)
class Named:
    """What the install-path section names the service by."""

    unit: str
    installer: str


def _named(repo: Repo) -> tuple[Named | None, list[str]]:
    """The unit's name and the installer's path, read out of the section."""
    path = ip.parse(repo.agents_md)
    findings: list[str] = []
    unit = ""
    for command in path.commands:
        match = ENABLE_COMMAND.search(command)
        if match:
            unit = match["unit"]
    if not unit:
        findings.append(
            f"AGENTS.md's `{ip.SECTION_HEADING}` states no `systemctl enable --now "
            f"<unit>` command, so nothing here names the unit this repository installs"
        )
    try:
        installer = policy_strings(
            policy_table(repo, "workflows"), ("install_service_script_path",), "workflows"
        )["install_service_script_path"]
    except PolicyValueError as error:
        return None, [*findings, str(error)]
    if not any(installer in command for command in path.commands):
        findings.append(
            f"AGENTS.md's `{ip.SECTION_HEADING}` states no command fetching "
            f"`{installer}`, which is the installer this repository ships"
        )
    if findings:
        return None, findings
    return Named(unit, installer), []


def _executes(script: str, program: str) -> list[int]:
    """The line numbers on which `script` runs `program` as a command.

    A line inside a heredoc is what the script *prints*, a line beginning with
    `echo` or `printf` is what it *says*, and a comment is what it explains — so
    none of the three is a command it runs. The installer names the command that
    starts the service in exactly those places and runs it nowhere, which is the
    whole point of the separation, so a check that could not tell them apart
    would refuse the correct script.
    """
    found: list[int] = []
    delimiter: str | None = None
    for number, line in enumerate(script.splitlines(), start=1):
        if delimiter is not None:
            if line.strip() == delimiter:
                delimiter = None
            continue
        opened = HEREDOC_OPEN.search(line)
        if opened:
            delimiter = opened["delimiter"]
            continue
        stripped = line.strip()
        if stripped.startswith(("#", "echo ", "printf ")):
            continue
        if re.search(rf"(^|[;&|]\s*|\bsudo\s+){re.escape(program)}\b", stripped):
            found.append(number)
    return found


def service_install(repo: Repo) -> list[str]:
    """The installer sits where the section says, ships that unit, and starts nothing."""
    named, findings = _named(repo)
    if named is None:
        return findings

    if not repo.exists(named.installer):
        return [
            f"AGENTS.md's `{ip.SECTION_HEADING}` states the service installer is "
            f"`{named.installer}`, and the tree holds no such file"
        ]
    installer_path = repo.path(named.installer)
    script = repo.read(named.installer)

    findings = []
    if not installer_path.stat().st_mode & 0o111:
        findings.append(
            f"`{named.installer}` is not executable, so the command "
            f"AGENTS.md's `{ip.SECTION_HEADING}` states could not run it"
        )

    declared = UNIT_ASSIGNMENT.search(script)
    if not declared:
        findings.append(
            f'`{named.installer}` carries no `UNIT_NAME="..."` line, so nothing '
            f"here can be held to the unit the install path names"
        )
    elif declared["unit"] != named.unit:
        findings.append(
            f"`{named.installer}` installs the unit `{declared['unit']}`, and "
            f"AGENTS.md's `{ip.SECTION_HEADING}` states `{named.unit}`"
        )

    try:
        service = policy_table(repo, "service")
        directory = policy_strings(service, ("unit_directory",), "service")["unit_directory"]
        forbidden = policy_string_list(service, "may_not_invoke", "service")
    except PolicyValueError as error:
        return [*findings, str(error)]

    if directory not in script:
        findings.append(
            f"`{named.installer}` does not write the unit into `{directory}`, "
            f"which is where the service manager reads units from"
        )

    for program in forbidden:
        for number in _executes(script, program):
            findings.append(
                f"{named.installer}:{number} runs `{program}`. The installer places "
                f"the service and starts nothing: this service commands a 3D printer, "
                f"so installing must not start a process that can move a machine"
            )

    findings.extend(_granted_findings(repo, named.installer, script))
    return findings


def _granted_findings(repo: Repo, installer: str, script: str) -> list[str]:
    """The template grants actions the contracts declare, and no others.

    The names in the installed configuration's grants are the contracts'
    vocabulary rather than the installer's, and there is no way to generate a
    shell heredoc from a Rust enum — so what holds them together is this: the
    generated action schema is read, and a name the template grants that the
    contracts do not declare is refused.
    """
    try:
        table = policy_strings(policy_table(repo, "service"), ("granting_table",), "service")[
            "granting_table"
        ]
        schema_path = policy_strings(
            policy_table(repo, "supervisor"), ("action_schema",), "supervisor"
        )["action_schema"]
    except PolicyValueError as error:
        return [str(error)]
    if table not in script:
        return [
            f"`{installer}` carries no `{table}` table, so nothing here reads the "
            f"actions its configuration grants"
        ]
    if not repo.exists(schema_path):
        return [f"`{schema_path}` is absent: it is the action vocabulary this reads"]
    declared = {
        variant.get("properties", {}).get("action", {}).get("const")
        for variant in json.loads(repo.read(schema_path)).get("oneOf", [])
        if isinstance(variant, dict)
    }
    if not declared:
        return [f"`{schema_path}` declares no action, so it is not the vocabulary"]

    granted: set[str] = set()
    for line in script[script.index(table) + len(table) :].splitlines():
        if line.startswith("["):
            break
        granted.update(QUOTED_NAME.findall(line))
    if not granted:
        return [f"`{installer}`'s `{table}` grants nothing at all"]
    return [
        f"`{installer}` grants `{name}`, which the action vocabulary at "
        f"`{schema_path}` does not declare"
        for name in sorted(granted - declared)
    ]


def ingress_answer_bound(repo: Repo) -> list[str]:
    """The shipped answer bound is below the Obico posting timeout recorded in prose."""
    try:
        table = policy_table(repo, "ingress")
        declared = policy_strings(
            table,
            ("source", "default_constant", "timeout_constant", "timeout_block", "section"),
            "ingress",
        )
        required = policy_string_list(table, "required_block_keys", "ingress")
    except PolicyValueError as error:
        return [str(error)]

    findings: list[str] = []
    if not section(repo.agents_md, declared["section"]):
        findings.append(
            f"AGENTS.md carries no `{declared['section']}` section, so nothing states "
            f"why the ingress answer bound is where it is"
        )

    try:
        block = marker_block(repo.agents_md, declared["timeout_block"])
    except MarkerBlockMissingError as error:
        return [*findings, str(error)]

    findings.extend(
        f"AGENTS.md's `{declared['timeout_block']}` block records no `{key}`, so the "
        f"number it carries has lost where it came from"
        for key in required
        if not any(line.lstrip("- ").lower().startswith(f"{key}:") for line in block)
    )

    recorded: int | None = None
    for line in block:
        match = RECORDED_TIMEOUT.match(line)
        if match:
            recorded = int(match["milliseconds"].replace("_", ""))
    if recorded is None:
        findings.append(
            f"AGENTS.md's `{declared['timeout_block']}` block records no posting "
            f"timeout, so there is nothing for the shipped default to be below"
        )
        return findings

    if not repo.exists(declared["source"]):
        return [*findings, f"`{declared['source']}` is absent: it is where the bound is shipped"]
    constants = {
        match["name"]: int(match["value"].replace("_", ""))
        for match in CONSTANT.finditer(repo.read(declared["source"]))
    }

    default = constants.get(declared["default_constant"])
    if default is None:
        findings.append(
            f"`{declared['source']}` declares no `{declared['default_constant']}`, so "
            f"this repository ships no stated default for the ingress answer bound"
        )
    elif default >= recorded:
        findings.append(
            f"the shipped answer bound {default}ms is not below the {recorded}ms "
            f"AGENTS.md's `{declared['timeout_block']}` records Obico posting under: "
            f"an alert answered at that bound is one Obico has already abandoned, and "
            f"its posting does not retry"
        )

    carried = constants.get(declared["timeout_constant"])
    if carried is None:
        findings.append(
            f"`{declared['source']}` declares no `{declared['timeout_constant']}`, so "
            f"the server carries no copy of the producer's own timeout to be held to"
        )
    elif carried != recorded:
        findings.append(
            f"`{declared['source']}` says Obico posts under {carried}ms and AGENTS.md's "
            f"`{declared['timeout_block']}` records {recorded}ms: one claim about an "
            f"external producer, written down twice, disagreeing with itself"
        )
    return findings
