"""The real-printer smoke test: the payload it prints, and what may select it.

This is the one thing in this repository that drives a machine capable of
destroying itself, so two properties are held here rather than left to a
reader.

**What reaches the machine is a payload a person can read in full.** The
G-code the smoke prints is this repository's own, it is drawn from a closed
set of commands, and it is short. That set and that length are declared in
`repo-policy.toml` and stated in `AGENTS.md`; nothing here chooses either,
because a check that chose its own safe set could call a dangerous command
safe and one that chose its own maximum could pick a length no file could
exceed. The exclusions that matter — `M500`, `M502`, `M303`, `G29`, `M112` —
are excluded by not being in the set rather than by a list of their own.

**A payload is refused for being inert exactly as it is for being unsafe.** An
empty file satisfies every safety condition by containing nothing, so this
check requires a command that moves the machine as well as forbidding the
dangerous ones. The same reasoning covers the heat declaration: a file either
performs no heating at all or says at its head what temperatures it needs, and
a declaration is held to the commands both ways — one naming a temperature no
command sets is as wrong as a command no declaration accounts for.
"""

from __future__ import annotations

import json
import re
from dataclasses import dataclass
from typing import Any

from repo_checks.model import (
    PolicyValueError,
    Repo,
    policy_string_list,
    policy_strings,
    policy_table,
)
from repo_checks.parsing import jobs_of, load_workflow, recipes, run_commands, section

# The first word of a command line: a `G` or an `M` and its number.
COMMAND = re.compile(r"^[GM][0-9]+$")

# The commands that move the machine. A payload carrying none of these is inert:
# it satisfies every safety condition by asking for nothing, and proves nothing
# when it runs.
MOTION_COMMANDS = ("G0", "G1", "G28")

# What each heating command heats. A tool command carries its tool number in a
# `T` parameter and heats tool zero without one.
HEATERS: dict[str, str] = {
    "M104": "tool",
    "M109": "tool",
    "M140": "bed",
    "M190": "bed",
}

# One temperature declaration at the head of the file, as it is written:
# `; heat: tool0 205 C for 900 s`.
DECLARATION = re.compile(
    r"^;\s*heat:\s*(?P<target>tool[0-9]+|bed)\s+(?P<degrees>[0-9]+(?:\.[0-9]+)?)\s*C\s+"
    r"for\s+(?P<seconds>[0-9]+)\s*s$"
)
DECLARATION_MARKER = "heat:"
DECLARATION_SHAPE = "; heat: <tool0|bed> <degrees> C for <seconds> s"


@dataclass(frozen=True, slots=True)
class Heat:
    """One heater and the temperature something asked it for."""

    target: str
    degrees: float

    def __str__(self) -> str:
        """How a finding names it."""
        return f"{self.target} at {self.degrees:g} C"


def safe_commands(repo: Repo) -> tuple[str, ...]:
    """The closed set of commands the smoke's payload may contain.

    Read from `repo-policy.toml` rather than written here: the set is anchored
    outside the check that enforces it, so a check cannot widen its own rule.

    Raises:
        PolicyValueError: If the policy declares no such set.
    """
    return policy_string_list(policy_table(repo, "smoke"), "safe_commands", "smoke")


def max_lines(repo: Repo) -> int:
    """The longest the smoke's payload may be, in lines.

    Raises:
        PolicyValueError: If the policy declares no whole-number maximum.
    """
    value = policy_table(repo, "smoke").get("max_lines")
    if not isinstance(value, int) or isinstance(value, bool) or value <= 0:
        msg = "`repo-policy.toml` declares no whole-number `smoke.max_lines`"
        raise PolicyValueError(msg)
    return value


def _parameter(words: list[str], letter: str) -> float | None:
    """The value of a one-letter G-code parameter, when the line carries one."""
    for word in words:
        if word[:1].upper() == letter and len(word) > 1:
            try:
                return float(word[1:])
            except ValueError:
                return None
    return None


def _asked_heat(command: str, words: list[str]) -> Heat | None:
    """The heat one command asks for, or nothing when it asks for none.

    A command setting a heater to zero turns it off, which is a cooldown rather
    than a temperature a reader has to have been told about.
    """
    heater = HEATERS.get(command)
    if heater is None:
        return None
    degrees = _parameter(words, "S")
    if degrees is None or degrees <= 0:
        return None
    if heater == "bed":
        return Heat("bed", degrees)
    tool = _parameter(words, "T")
    return Heat(f"tool{int(tool) if tool is not None else 0}", degrees)


def _declared(line: str) -> Heat | None:
    """The heat one head declaration states, when the line is one."""
    match = DECLARATION.match(line.strip())
    if match is None:
        return None
    return Heat(match.group("target"), float(match.group("degrees")))


def gcode_findings(repo: Repo, relative: str) -> list[str]:
    """Everything wrong with one G-code payload, in the order it is read."""
    allowed = safe_commands(repo)
    limit = max_lines(repo)
    text = repo.read(relative)
    lines = text.splitlines()

    findings: list[str] = []
    if len(lines) > limit:
        findings.append(
            f"{relative} is {len(lines)} lines, longer than the {limit} lines "
            f"`repo-policy.toml` allows: a payload nobody can read in full before it "
            f"reaches a machine"
        )

    commanded: set[Heat] = set()
    declared: set[Heat] = set()
    moves = False
    at_the_head = True
    for number, raw in enumerate(lines, start=1):
        line = raw.strip()
        if not line:
            continue
        if line.startswith(";"):
            if DECLARATION_MARKER not in line:
                continue
            heat = _declared(line)
            if heat is None:
                findings.append(
                    f"{relative} line {number} states a temperature in a shape this check "
                    f"cannot read ({line!r}); the shape is `{DECLARATION_SHAPE}`"
                )
            elif not at_the_head:
                findings.append(
                    f"{relative} line {number} declares {heat} after the head of the file: "
                    f"a declaration a reader meets after the commands it is about is not one"
                )
            else:
                declared.add(heat)
            continue
        at_the_head = False
        words = line.split(";")[0].split()
        command = words[0].upper()
        if not COMMAND.match(command):
            findings.append(
                f"{relative} line {number} carries {words[0]!r}, which is no G-code command"
            )
            continue
        if command not in allowed:
            findings.append(
                f"{relative} line {number} carries `{command}`, which is not one of the "
                f"commands `repo-policy.toml` allows this payload: "
                f"{', '.join(allowed)}"
            )
            continue
        moves = moves or command in MOTION_COMMANDS
        heat = _asked_heat(command, words)
        if heat is not None:
            commanded.add(heat)

    if not moves:
        findings.append(
            f"{relative} carries no command that moves the machine "
            f"({', '.join(MOTION_COMMANDS)}): it would print nothing and prove nothing"
        )
    findings.extend(_heat_findings(relative, declared, commanded))
    return findings


def _heat_findings(relative: str, declared: set[Heat], commanded: set[Heat]) -> list[str]:
    """The declaration at the head and the heating commands account for each other."""
    if commanded and not declared:
        return [
            f"{relative} heats ({', '.join(sorted(str(heat) for heat in commanded))}) and "
            f"is neither heat-free nor carries a declaration at its head saying what "
            f"temperatures it needs and for how long"
        ]
    findings = [
        f"{relative} declares {heat} at its head, which no command in it sets"
        for heat in sorted(declared - commanded, key=str)
    ]
    findings.extend(
        f"{relative} carries a heating command asking for {heat}, which the declaration "
        f"at its head does not account for"
        for heat in sorted(commanded - declared, key=str)
    )
    return findings


def smoke_payload(repo: Repo) -> list[str]:
    """The G-code the real-printer smoke test prints is safe, short and not inert."""
    policy: dict[str, Any] = policy_table(repo, "smoke")
    if not policy:
        return ["`repo-policy.toml` declares no `[smoke]` section"]
    named = policy_strings(policy, ("gcode",), "smoke")
    relative = named["gcode"]
    if not repo.exists(relative):
        return [
            f"`repo-policy.toml` names {relative} as the payload the real-printer smoke "
            f"test prints, which is not there"
        ]
    return gcode_findings(repo, relative)


def smoke_selection(repo: Repo) -> list[str]:
    """Nothing selects the real-printer smoke test automatically.

    The one thing here that drives a machine capable of destroying itself is
    reached by one recipe, given one flag, with one variable naming the device.
    So this refuses a tree in which the ordinary gate, a graph target, a
    continuous-integration job or a scheduled workflow could reach it — each of
    which would be an unattended run that starts a print nobody was watching.
    """
    policy: dict[str, Any] = policy_table(repo, "smoke")
    if not policy:
        return ["`repo-policy.toml` declares no `[smoke]` section"]
    named = policy_strings(policy, ("script", "recipe", "flag", "device_env", "section"), "smoke")
    script, recipe, flag = named["script"], named["recipe"], named["flag"]

    findings: list[str] = []
    if not repo.exists(script):
        findings.append(
            f"`repo-policy.toml` names {script} as the real-printer smoke test, which is not there"
        )
    findings.extend(_recipe_findings(repo, recipe, script, flag))
    findings.extend(_gate_findings(repo, recipe))
    findings.extend(_graph_findings(repo, recipe, script))
    findings.extend(_workflow_findings(repo, recipe, script))
    findings.extend(_prose_findings(repo, named))
    if repo.exists(script) and named["device_env"] not in repo.read(script):
        findings.append(
            f"{script} does not read `{named['device_env']}`, which is one of the two "
            f"inputs that select it"
        )
    return findings


def _recipe_findings(repo: Repo, recipe: str, script: str, flag: str) -> list[str]:
    """The one recipe that runs it runs it, and passes its arguments through."""
    parsed = recipes(repo.justfile)
    found = parsed.get(recipe)
    if found is None:
        return [
            f"`repo-policy.toml` names `just {recipe}` as the real-printer smoke test's "
            f"recipe, which the recipe set does not declare"
        ]
    body = "\n".join(found.body)
    findings: list[str] = []
    if script not in body:
        findings.append(f"the `{recipe}` recipe does not run {script}")
    if "{{" not in body:
        findings.append(
            f"the `{recipe}` recipe passes no argument through to {script}: `{flag}` is one "
            f"of the two inputs that select the smoke test, and a recipe that swallowed it "
            f"would leave the other input selecting it alone"
        )
    return findings


def _gate_findings(repo: Repo, recipe: str) -> list[str]:
    """The gate neither declares it a tier nor invokes it."""
    findings: list[str] = []
    if recipe in repo.policy.get("gate", {}).get("tiers", []):
        findings.append(
            f"`repo-policy.toml`'s gate.tiers names `{recipe}`, which is the real-printer "
            f"smoke test: every gate run would then drive the machine"
        )
    check = recipes(repo.justfile).get("check")
    if check is None:
        return findings
    invoked = {
        line.split()[1]
        for line in check.body
        if line.split()[:1] == ["just"] and len(line.split()) > 1
    } | set(check.dependencies)
    if recipe in invoked:
        findings.append(
            f"the `check` recipe invokes `just {recipe}`, which drives the real printer"
        )
    return findings


def _graph_findings(repo: Repo, recipe: str, script: str) -> list[str]:
    """No graph target reaches it, so no fan-out tier can select it."""
    findings: list[str] = []
    for project in repo.project_paths:
        data = json.loads(project.read_text(encoding="utf-8"))
        name = data.get("name", project.parent.name)
        for target, spec in (data.get("targets") or {}).items():
            command = spec.get("command")
            if not isinstance(command, str):
                continue
            if script in command or f"just {recipe}" in command:
                findings.append(
                    f"the graph target `{name}:{target}` runs the real-printer smoke test: "
                    f"a target is what a fan-out tier selects, and nothing may select this one"
                )
    return findings


def _workflow_findings(repo: Repo, recipe: str, script: str) -> list[str]:
    """No committed workflow runs it, on a change or on a schedule."""
    findings: list[str] = []
    for path in repo.workflow_paths:
        workflow = load_workflow(path)
        for job, spec in jobs_of(workflow).items():
            findings.extend(
                f"the `{job}` job of {path.name} runs the real-printer smoke test "
                f"(`{command}`): continuous integration runs beside no printer, and an "
                f"unattended run of this starts a print nobody is watching"
                for command in run_commands(spec)
                if script in command or f"just {recipe}" in command
            )
    return findings


def _prose_findings(repo: Repo, named: dict[str, str]) -> list[str]:
    """The section that says what it does, what it requires and how to run it.

    A test that drives a real machine is one a person stays next to, and what
    tells them so is prose rather than a check. So the check is that the prose
    is there and names both of the two inputs a reader needs to run it at all.
    """
    heading = named["section"]
    body = section(repo.agents_md, heading)
    if not body.strip():
        return [
            f"AGENTS.md carries no `{heading}` section: a test that drives a real machine "
            f"is one a person stays next to, and nothing here would tell them how"
        ]
    return [
        f"AGENTS.md's `{heading}` section does not name `{what}`, which a reader needs to "
        f"run it at all"
        for what in (f"just {named['recipe']}", named["flag"], named["device_env"])
        if what not in body
    ]
