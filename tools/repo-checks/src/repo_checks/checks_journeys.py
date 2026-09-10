"""The one journey all three clients drive, held to one plan.

Three clients of one server are one product only for as long as the walk that
proves them is the same walk. Each client's journey is hand-written, in its own
language, against a real supervisor over a real printer — which is what makes it
worth having and also what makes it possible for one of them to quietly lose a
step, or to take them in an order that stops proving what the step before it
established.

So the nine steps are declared here, once, and every journey is read against
them. A step is declared by a **marker comment** at the point in the journey
where that step is taken, and what the marker claims is held to the code beside
it: the operations that step is about have to be called between that marker and
the next, either there or inside a helper defined in the same file that the
region calls. A marker with no call under it is a comment, and a call with no
marker over it is a step nothing declared.

The order is the other half, and only the part of it that is load-bearing is
asked for. The manifest is written before the print is started, so the print
runs under it; both adjustments happen after it has started and before history
is read, so history has them to account for; and the print is cancelled last,
because a cancelled print is not one anything else can be asked of. `ORDERINGS`
states each of those in the words of the thing it would break, and states
nothing else — the two reads may come either way round and the call the client
refuses to make may be written wherever it reads best, because a journey holding
those in one arrangement proves exactly what a journey holding them in another
does.
"""

from __future__ import annotations

import re
from dataclasses import dataclass
from json import JSONDecodeError

from contract_codegen.naming import method_name
from contract_codegen.schemas import ContractError, load

from repo_checks.model import Repo


@dataclass(frozen=True, slots=True)
class Step:
    """One of the nine steps every client's journey takes."""

    #: The number the marker declares it by. Not a position: what a step has to
    #: come before or after is `ORDERINGS` alone, and a step named in none of
    #: those may be taken wherever the journey reads best.
    number: int
    #: What the marker calls it, in every language.
    name: str
    #: The operations that step is about, spelled as the server declares them.
    operations: tuple[str, ...]


#: The nine steps, numbered as the walk a real machine admits reads.
STEPS: tuple[Step, ...] = (
    Step(1, "status", ("status",)),
    Step(2, "context", ("context", "image")),
    Step(3, "manifest", ("manifest_set", "manifest_get")),
    Step(4, "start", ("start_print",)),
    Step(5, "adjustment", ("set_feedrate_factor",)),
    Step(6, "refusal", ("set_feedrate_factor",)),
    Step(7, "unreasoned", ("pause", "history")),
    Step(8, "history", ("history",)),
    Step(9, "cancel", ("cancel",)),
)

#: What being cancelled last holds up, said once for the eight relationships
#: below that say it.
CANCELLED_LAST = (
    "the print is cancelled last, because a cancelled print is not one the steps "
    "before it could be taken against"
)

#: Every ordering the walk stands on, and **only** those. The nine steps are not
#: a sequence a journey has to recite: which comes first of the two reads, and
#: where the call the client refuses to make is written, is the journey author's
#: to choose, and a check demanding one arrangement would be a check over layout.
#: What each of these holds up is stated beside it, in the words of the thing
#: taking it the other way round would stop proving.
ORDERINGS: tuple[tuple[int, int, str], ...] = (
    (
        3,
        4,
        "the manifest is written before the print is started, so the print runs under it",
    ),
    (
        4,
        5,
        "the accepted adjustment is asked for after the print has started, so there is "
        "a print for the policy to admit it against",
    ),
    (
        4,
        6,
        "the refused adjustment is asked for after the print has started, so what "
        "refuses it is the bound it asked outside of rather than the state it asked in",
    ),
    (
        5,
        8,
        "the accepted adjustment comes before history is read, so history has it to account for",
    ),
    (
        6,
        8,
        "the refused adjustment comes before history is read, so history has it to account for",
    ),
    *((step, 9, CANCELLED_LAST) for step in range(1, 9)),
)


@dataclass(frozen=True, slots=True)
class Journey:
    """One client's journey, and how its own language is read."""

    #: Which client drives it, spelled as the generator spells it.
    language: str
    #: Where it lives, relative to the repository root.
    path: str
    #: How a comment opens in that language.
    comment: str
    #: How a function's definition opens at the top level of that file.
    definition: str


#: The three journeys, in the order the generator writes their clients.
JOURNEYS: tuple[Journey, ...] = (
    Journey(
        "rust",
        "crates/printobserver-sdk/tests/integration.rs",
        "//",
        r"^(?:pub )?fn (?P<name>\w+)",
    ),
    Journey(
        "python",
        "python/printobserver-sdk/integration/test_journey.py",
        "#",
        r"^def (?P<name>\w+)",
    ),
    Journey(
        "typescript",
        "npm/printobserver-sdk/integration/journey.test.ts",
        "//",
        r"^(?:async )?function (?P<name>\w+)",
    ),
)

#: How a step declares itself, wherever in a journey that step is taken.
MARKER = re.compile(r"journey step (?P<number>\d+): (?P<name>[a-z_]+)")

#: How this repository writes one of those markers, for a finding to quote.
MARKER_FORM = "journey step <number>: <name>"


def journey_completeness(repo: Repo) -> list[str]:
    """Every client's journey takes the same nine steps, in the same order."""
    findings = _plan_findings(repo)
    for journey in JOURNEYS:
        if not repo.exists(journey.path):
            findings.append(
                f"the {journey.language} client carries no journey at {journey.path}, so "
                f"nothing drives the nine steps in it"
            )
            continue
        findings.extend(_journey_findings(journey, repo.read(journey.path)))
    return findings


def _plan_findings(repo: Repo) -> list[str]:
    """Every step is about operations the server actually declares.

    Without this the plan could go on demanding a call no client has a method
    for, which is a journey nobody could write and a check nobody could pass.
    """
    try:
        contract = load(repo.root)
    except (ContractError, JSONDecodeError, KeyError, OSError) as refused:
        return [f"the server's own operation list could not be read: {refused}"]
    declared = {operation.name for operation in contract.operations}
    return [
        f"journey step {step.number} (`{step.name}`) is about `{operation}`, which no "
        f"operation the server declares backs"
        for step in STEPS
        for operation in step.operations
        if operation not in declared
    ]


def _markers(journey: Journey, source: str) -> list[tuple[int, str, int]]:
    """Every step marker in one journey, as its number, name and line."""
    found: list[tuple[int, str, int]] = []
    for number, line in enumerate(source.splitlines(), start=1):
        match = MARKER.search(line)
        if match is not None and line.lstrip().startswith(journey.comment):
            found.append((int(match["number"]), match["name"], number))
    return found


def _journey_findings(journey: Journey, source: str) -> list[str]:
    """Every way one journey has come apart from the plan."""
    marked = _markers(journey, source)
    findings = _presence_findings(journey, marked)
    findings.extend(_order_findings(journey, marked))
    findings.extend(_carrier_findings(journey, source, marked))
    return findings


def _presence_findings(journey: Journey, marked: list[tuple[int, str, int]]) -> list[str]:
    """Each of the nine steps is declared exactly once, and nothing else is."""
    findings: list[str] = []
    seen = [number for number, _, _ in marked]
    named = {number: name for number, name, _ in marked}
    for step in STEPS:
        taken = seen.count(step.number)
        if taken == 0:
            findings.append(
                f"{journey.path} takes no journey step {step.number} (`{step.name}`): "
                f"the step is either gone or no longer says which step it is, and a "
                f"journey missing one of the nine proves a walk the other two do not "
                f"({MARKER_FORM}, in a comment)"
            )
        elif taken > 1:
            findings.append(
                f"{journey.path} declares journey step {step.number} (`{step.name}`) "
                f"{taken} times, so which of them the step is cannot be read"
            )
        elif named[step.number] != step.name:
            findings.append(
                f"{journey.path} calls journey step {step.number} `{named[step.number]}`, "
                f"and the walk all three clients drive calls it `{step.name}`"
            )
    findings.extend(
        f"{journey.path} declares journey step {number} (`{name}`), and the walk all "
        f"three clients drive has nine steps and no such one"
        for number, name, _ in marked
        if number not in {step.number for step in STEPS}
    )
    return findings


def _order_findings(journey: Journey, marked: list[tuple[int, str, int]]) -> list[str]:
    """Every relationship the walk stands on holds, and nothing beyond them is asked.

    A relationship over a step the journey does not declare is left alone: that
    is already a finding of its own, and reporting the order of a step that is
    not there would be the same defect said twice.
    """
    at = {number: line for number, _, line in marked}
    named = {step.number: step.name for step in STEPS}
    return [
        f"{journey.path} takes journey step {later} (`{named[later]}`) at line "
        f"{at[later]}, before journey step {earlier} (`{named[earlier]}`) at line "
        f"{at[earlier]}: {why}"
        for earlier, later, why in ORDERINGS
        if earlier in at and later in at and at[later] < at[earlier]
    ]


def _carrier_findings(
    journey: Journey, source: str, marked: list[tuple[int, str, int]]
) -> list[str]:
    """Each step's own operations are called where the marker says they are."""
    lines = source.splitlines()
    at = {number: line for number, _, line in marked}
    definitions = _definitions(journey, source)
    findings: list[str] = []
    for step in STEPS:
        if step.number not in at:
            continue
        opens = at[step.number]
        closes = min(
            (line for _, _, line in marked if line > opens),
            default=len(lines) + 1,
        )
        region = "\n".join(lines[opens - 1 : closes - 1])
        findings.extend(
            f"{journey.path} declares journey step {step.number} (`{step.name}`) at line "
            f"{opens} and calls `{method_name(operation, journey.language)}` nowhere "
            f"under it: the marker says what the step is and the code beside it is what "
            f"the step does"
            for operation in step.operations
            if not _reaches(region, operation, journey, definitions)
        )
    return findings


def _definitions(journey: Journey, source: str) -> dict[str, str]:
    """Every function defined at the top level of one journey, by name.

    A step may be written where its marker is or handed to a helper beside it,
    and both are the same walk to a reader — so a region that calls one of these
    is read as carrying what that helper does.
    """
    opens = list(re.finditer(journey.definition, source, flags=re.MULTILINE))
    return {
        match["name"]: source[
            match.start() : (opens[index + 1].start() if index + 1 < len(opens) else len(source))
        ]
        for index, match in enumerate(opens)
    }


#: How a call on the client reads, in every one of the three languages: the
#: leading dot is what makes this a call *on something* rather than a helper
#: that happens to be named after an operation.
CALL = r"\.{spelled}\s*\("

#: How a call on anything at all reads, which is how a region says it hands a
#: step to a helper defined beside it.
HANDED = re.compile(r"(?<![.\w])(?P<name>\w+)\s*\(")


def _reaches(region: str, operation: str, journey: Journey, definitions: dict[str, str]) -> bool:
    """Whether one step's region calls an operation, itself or through a helper."""
    spelled = re.escape(method_name(operation, journey.language))
    called = re.compile(CALL.format(spelled=spelled))
    if called.search(region) is not None:
        return True
    return any(
        called.search(definitions[match["name"]]) is not None
        for match in HANDED.finditer(region)
        if match["name"] in definitions
    )
