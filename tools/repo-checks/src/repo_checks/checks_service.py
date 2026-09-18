"""Checks over the service this repository installs, and its ingress bound.

Two things this node's server has to be held to are facts about the tree that a
running system would only reveal after it was too late.

The first is the pair of names in `AGENTS.md`'s install-path section — the
service the operator activates, and the path the installer is fetched from —
stated once per service manager the supported-platform list names. That section
is the authoritative source of both, and the other party to the contract is a
job that installs the service on a real machine of that platform, so a tree
whose installer registers a differently named service is one nothing catches
until that job runs. Each manager's installer is read against that manager's
own rules, which `repo-policy.toml`'s `service.managers.<manager>` declares:
the programs it may not run and the settings it may not write, because
installing must start nothing; and the two settings, in the manager's own
vocabulary, that make the service come back — at boot once the operator has
activated it, and after its process ends abruptly.

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
import sys
from dataclasses import dataclass
from pathlib import Path

from repo_checks import install_path as ip
from repo_checks.model import (
    PolicyValueError,
    Repo,
    policy_string_list,
    policy_strings,
    policy_table,
)
from repo_checks.parsing import MarkerBlockMissingError, marker_block, section
from repo_checks.platforms import ServiceManager, supported

# How the activation command — the second of a manager's pair — names the
# service, per manager: `sudo systemctl enable --now <unit>` on systemd, whose
# last word is the unit; `Set-Service -Name <service> ...` on Windows.
ACTIVATION_NAMES: dict[str, re.Pattern[str]] = {
    ServiceManager.SYSTEMD: re.compile(r"\bsystemctl\s+enable\s+--now\s+(?P<service>\S+)\s*$"),
    ServiceManager.WINDOWS_SERVICE: re.compile(r"\bSet-Service\s+-Name\s+(?P<service>\S+)\b"),
}

# What each manager's activation command has to look like, quoted back at a
# section that states none of that shape.
ACTIVATION_SHAPES: dict[str, str] = {
    ServiceManager.SYSTEMD: "systemctl enable --now <unit>",
    ServiceManager.WINDOWS_SERVICE: "Set-Service -Name <service> ...",
}

# How each manager's installer declares the name it registers: `UNIT_NAME="..."`
# in the shell installer, `$ServiceName = '...'` in the PowerShell one.
NAME_DECLARATIONS: dict[str, tuple[re.Pattern[str], str]] = {
    ServiceManager.SYSTEMD: (
        re.compile(r'^UNIT_NAME="(?P<service>[^"]+)"\s*$', re.MULTILINE),
        'UNIT_NAME="..."',
    ),
    ServiceManager.WINDOWS_SERVICE: (
        re.compile(r"^\$ServiceName\s*=\s*'(?P<service>[^']+)'\s*$", re.MULTILINE),
        "$ServiceName = '...'",
    ),
}

# `pub const NAME: &str = "...";` in a Rust source.
STRING_CONSTANT = re.compile(r'pub const (?P<name>[A-Z_]+): &str = "(?P<value>[^"]*)"\s*;')

# `pub const NAME: u64 = 1_000;` in a Rust source.
CONSTANT = re.compile(
    r"pub const (?P<name>[A-Z_]+): u64 = (?P<value>[0-9_]+)\s*;",
)

# `- posting timeout: `5000` ms` in the recorded block.
RECORDED_TIMEOUT = re.compile(r"^-\s*posting timeout:\s*`(?P<milliseconds>[0-9_]+)`\s*ms\s*$")

# A shell line that opens a heredoc, and the delimiter it ends at.
HEREDOC_OPEN = re.compile(r"<<-?(?P<delimiter>[A-Za-z_][A-Za-z0-9_]*)")

# What PowerShell reads as text rather than as a command: a here-string of
# either kind, a single-quoted string, a double-quoted string with its backtick
# escapes, and a comment to the end of its line. Matched in this order so that a
# `#` inside a string is text and a quote inside a comment is a comment.
POWERSHELL_TEXT = re.compile(
    r"@\"\r?\n.*?\r?\n\"@|@'\r?\n.*?\r?\n'@|'[^']*'|\"(?:[^\"`]|`.)*\"|#[^\r\n]*",
    re.DOTALL,
)

# `"pause"` and the rest, as the configuration template's grants spell them.
QUOTED_NAME = re.compile(r'"([a-z_]+)"')

# The installer whose interpreter runs it by path rather than by its mode.
POWERSHELL_SUFFIX = ".ps1"


@dataclass(frozen=True, slots=True)
class Named:
    """What the install-path section names one manager's service by."""

    manager: str
    service: str
    installer: str
    activation: str


@dataclass(frozen=True, slots=True)
class Rules:
    """One manager's rules, as `service.managers.<manager>` declares them."""

    installer: str
    may_not_invoke: tuple[str, ...]
    may_not_set: tuple[str, ...]
    starts_at_boot: str
    restarts_after_crash: str
    registration_directory: str | None
    program_name: tuple[str, str] | None


def _rules(repo: Repo, manager: str) -> Rules:
    """The rules `repo-policy.toml` declares for one manager.

    Raises:
        PolicyValueError: If the table is absent or a required field is not a
            non-empty string or list.
    """
    where = f"service.managers.{manager}"
    managers = policy_table(repo, "service").get("managers")
    table = managers.get(manager) if isinstance(managers, dict) else None
    if not isinstance(table, dict):
        msg = (
            f"`repo-policy.toml` declares no `{where}` table, so nothing states the rules "
            f"the `{manager}` installer is held to"
        )
        raise PolicyValueError(msg)
    named = policy_strings(table, ("installer", "starts_at_boot", "restarts_after_crash"), where)
    may_not_set = table.get("may_not_set")
    directory = table.get("registration_directory")
    program: tuple[str, str] | None = None
    if "program_source" in table or "program_constant" in table:
        declared = policy_strings(table, ("program_source", "program_constant"), where)
        program = (declared["program_source"], declared["program_constant"])
    return Rules(
        named["installer"],
        policy_string_list(table, "may_not_invoke", where),
        policy_string_list(table, "may_not_set", where) if may_not_set is not None else (),
        named["starts_at_boot"],
        named["restarts_after_crash"],
        policy_strings(table, ("registration_directory",), where)["registration_directory"]
        if directory is not None
        else None,
        program,
    )


def installer_for(repo: Repo, manager: str) -> str:
    """The installer `repo-policy.toml` declares for one manager.

    Raises:
        PolicyValueError: If the manager has no rules or they name no installer.
    """
    return _rules(repo, manager).installer


def installers(repo: Repo) -> list[str]:
    """Every installer `repo-policy.toml` declares, one per manager it has rules for."""
    managers = policy_table(repo, "service").get("managers")
    if not isinstance(managers, dict):
        return []
    return [
        str(table["installer"])
        for table in managers.values()
        if isinstance(table, dict) and isinstance(table.get("installer"), str)
    ]


def _service_managers(repo: Repo) -> list[str]:
    """Every service manager the supported-platform list names, in order.

    Raises:
        MarkerBlockMissingError: If `AGENTS.md` carries no supported-platform list.
    """
    named: list[str] = []
    for platform in supported(repo):
        if platform.service_manager not in named:
            named.append(platform.service_manager)
    return named


def _named(manager: str, pair: tuple[str, ...], installer: str) -> tuple[Named | None, list[str]]:
    """The service's name and the installer's path, read out of one manager's pair.

    The name is read off the activation command in that manager's own shape,
    and the installer is required to be the one the pair fetches.
    """
    findings: list[str] = []
    activation = ACTIVATION_NAMES.get(manager)
    if activation is None:
        return None, [
            f"AGENTS.md's `{ip.SECTION_HEADING}` states a pair of commands for the "
            f"`{manager}` service manager, and nothing here knows how that manager's "
            f"activation command names a service"
        ]
    service = ""
    for command in pair:
        match = activation.search(command)
        if match:
            service = match["service"]
    if not service:
        findings.append(
            f"AGENTS.md's `{ip.SECTION_HEADING}` states no `{ACTIVATION_SHAPES[manager]}` "
            f"command under `{manager}`, so nothing here names the service this repository "
            f"installs there"
        )
    if not any(installer in command for command in pair):
        findings.append(
            f"AGENTS.md's `{ip.SECTION_HEADING}` states no command fetching `{installer}` "
            f"under `{manager}`, which is the installer this repository ships for it"
        )
    if findings:
        return None, findings
    return Named(manager, service, installer, pair[-1]), []


def _executes(script: str, program: str) -> list[int]:
    """The line numbers on which a shell `script` runs `program` as a command.

    A line inside a heredoc is what the script *prints*, a line beginning with
    `echo` or `printf` is what it *says*, and a comment is what it explains — so
    none of the three is a command it runs. The installer names the command that
    starts the service in exactly those places and runs it nowhere, which is the
    whole point of the separation, so a check that could not tell them apart
    would refuse the correct script.

    A line ending in a backslash continues into the next, and both are one
    command: they are joined before any of that is decided, so the second half
    of an `echo` is read as part of the `echo` rather than as a command of its
    own.
    """
    found: list[int] = []
    for number, command in _commands(script):
        if command.startswith(("#", "echo ", "printf ")):
            continue
        if re.search(rf"(^|[;&|]\s*|\bsudo\s+){re.escape(program)}\b", command):
            found.append(number)
    return found


def _commands(script: str) -> list[tuple[int, str]]:
    """Every command a shell script runs, with the line it starts on.

    Heredoc bodies are dropped — they are what the script writes rather than
    what it runs — and a line continued with a backslash is joined to the one
    after it.
    """
    found: list[tuple[int, str]] = []
    delimiter: str | None = None
    started: int | None = None
    joined = ""
    for number, line in enumerate(script.splitlines(), start=1):
        if delimiter is not None:
            if line.strip() == delimiter:
                delimiter = None
            continue
        if started is None:
            started = number
            joined = ""
        joined += line.strip()
        if line.rstrip().endswith("\\"):
            joined = joined[:-1]
            continue
        opened = HEREDOC_OPEN.search(joined)
        if opened:
            delimiter = opened["delimiter"]
        found.append((started, joined))
        started = None
    return found


def _powershell_code(script: str) -> str:
    """A PowerShell script with everything it says or explains blanked out.

    Every string and here-string is what the script writes or prints, and a
    comment is what it explains; each is replaced by spaces of the same shape,
    newlines kept, so that what remains is what the script *runs* and every
    line number still means what it did.
    """
    return POWERSHELL_TEXT.sub(
        lambda match: "".join("\n" if char == "\n" else " " for char in match.group(0)), script
    )


def _executes_powershell(script: str, program: str) -> list[int]:
    """The line numbers on which a PowerShell `script` runs `program` as a command.

    A command is a program named at the head of a statement: at the start of a
    line, after `;`, `|`, `{` or `(`, or after the call operator `&`. Strings
    and comments are blanked out first, so the command the installer prints for
    the operator to run next is not read as one it ran. A multi-word entry
    matches with any whitespace between its words.
    """
    words = r"\s+".join(re.escape(word) for word in program.split())
    pattern = re.compile(rf"(?:^|[;|{{(]\s*|&\s*)['\"]?{words}\b", re.MULTILINE)
    code = _powershell_code(script)
    return sorted({code.count("\n", 0, match.start()) + 1 for match in pattern.finditer(code)})


def _sets(script: str, setting: str, powershell: bool) -> list[int]:
    """The line numbers on which a script writes `setting` into something it runs.

    A setting is a fragment of a program's arguments rather than a program —
    `start= auto`, which hands the manager a registration it will start at the
    next boot — so it is looked for anywhere in what the script runs, with any
    whitespace between its words.
    """
    words = r"\s+".join(re.escape(word) for word in setting.split())
    pattern = re.compile(words)
    if powershell:
        code = _powershell_code(script)
        return sorted({code.count("\n", 0, match.start()) + 1 for match in pattern.finditer(code)})
    return [
        number
        for number, command in _commands(script)
        if not command.startswith(("#", "echo ", "printf ")) and pattern.search(command)
    ]


def runnable(path: Path) -> bool:
    """Whether a script can be run as a program on this host.

    A POSIX host says so in the file's mode. Windows keeps no execute bit: the
    shell a command reaches a script through there — Git's bash — runs a file as
    a program when it opens with an interpreter line, so that line is what says
    it can be run.
    """
    if sys.platform == "win32":
        with path.open("rb") as opened:
            return opened.read(2) == b"#!"
    return bool(path.stat().st_mode & 0o111)


def service_install(repo: Repo) -> list[str]:
    """Each installer sits where its pair says, registers that service, and starts nothing."""
    path = ip.parse(repo.agents_md)
    try:
        managers = _service_managers(repo)
    except MarkerBlockMissingError as error:
        return [str(error)]
    findings: list[str] = []
    stated = 0
    for manager in managers:
        pair = path.commands_for(manager)
        if not pair:
            continue
        stated += 1
        findings.extend(_one_manager(repo, manager, pair))
    if not stated:
        findings.append(
            f"AGENTS.md's `{ip.SECTION_HEADING}` states a pair of commands for no service "
            f"manager its supported-platform list names, so nothing here names the service "
            f"this repository installs"
        )
    return findings


def _one_manager(repo: Repo, manager: str, pair: tuple[str, ...]) -> list[str]:
    """One manager's installer, held to its pair and to its own rules."""
    try:
        rules = _rules(repo, manager)
    except PolicyValueError as error:
        return [str(error)]
    named, findings = _named(manager, pair, rules.installer)
    if named is None:
        return findings

    if not repo.exists(named.installer):
        return [
            f"AGENTS.md's `{ip.SECTION_HEADING}` states the `{manager}` service installer is "
            f"`{named.installer}`, and the tree holds no such file"
        ]
    installer_path = repo.path(named.installer)
    script = repo.read(named.installer)
    powershell = named.installer.endswith(POWERSHELL_SUFFIX)

    findings = []
    if not powershell and not runnable(installer_path):
        findings.append(
            f"`{named.installer}` is not executable, so the command "
            f"AGENTS.md's `{ip.SECTION_HEADING}` states could not run it"
        )

    declaration = NAME_DECLARATIONS.get(manager)
    if declaration is None:
        findings.append(
            f"nothing here knows how a `{manager}` installer declares the service it "
            f"registers, so `{named.installer}` cannot be held to `{named.service}`"
        )
    else:
        pattern, shape = declaration
        declared = pattern.search(script)
        if not declared:
            findings.append(
                f"`{named.installer}` carries no `{shape}` line, so nothing here can be "
                f"held to the service the install path names"
            )
        elif declared["service"] != named.service:
            findings.append(
                f"`{named.installer}` installs the service `{declared['service']}`, and "
                f"AGENTS.md's `{ip.SECTION_HEADING}` states `{named.service}` under `{manager}`"
            )

    if rules.program_name is not None:
        findings.extend(_program_name_findings(repo, named, rules.program_name))

    if rules.registration_directory is not None and rules.registration_directory not in script:
        findings.append(
            f"`{named.installer}` does not write the registration into "
            f"`{rules.registration_directory}`, which is where the service manager reads "
            f"units from"
        )

    executes = _executes_powershell if powershell else _executes
    for program in rules.may_not_invoke:
        findings.extend(
            f"{named.installer}:{number} runs `{program}`. The installer places the service "
            f"and starts nothing: this service commands a 3D printer, so installing must not "
            f"start a process that can move a machine"
            for number in executes(script, program)
        )
    for setting in rules.may_not_set:
        findings.extend(
            f"{named.installer}:{number} writes `{setting}`, which registers a service the "
            f"manager starts by itself. The installer enables nothing: starting the service "
            f"at boot is the operator's own command, `{named.activation}`"
            for number in _sets(script, setting, powershell)
        )

    if rules.restarts_after_crash not in script:
        findings.append(
            f"`{named.installer}` writes no `{rules.restarts_after_crash}`, so the service it "
            f"registers stays down after its process ends abruptly"
        )
    if rules.starts_at_boot not in script and rules.starts_at_boot not in named.activation:
        findings.append(
            f"neither `{named.installer}` nor the `{manager}` activation command carries "
            f"`{rules.starts_at_boot}`, so nothing makes the service start at boot once the "
            f"operator has activated it"
        )

    findings.extend(_granted_findings(repo, named.installer, script))
    return findings


def _program_name_findings(repo: Repo, named: Named, program: tuple[str, str]) -> list[str]:
    """The program answers the manager under the name the install path states.

    A service manager that dispatches by name hands the process the name it was
    registered under, so the program carries its own copy of that name — and
    that copy is held here to the one the manager's pair states.
    """
    source, constant = program
    if not repo.exists(source):
        return [
            f"`{source}` is absent: it is where the program names the `{named.manager}` service"
        ]
    carried = {
        match["name"]: match["value"] for match in STRING_CONSTANT.finditer(repo.read(source))
    }.get(constant)
    if carried is None:
        return [
            f"`{source}` declares no `{constant}`, so nothing holds the name the program "
            f"answers the `{named.manager}` manager under to the install path"
        ]
    if carried != named.service:
        return [
            f"`{source}`'s `{constant}` is `{carried}`, and AGENTS.md's "
            f"`{ip.SECTION_HEADING}` states `{named.service}` under `{named.manager}`"
        ]
    return []


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
