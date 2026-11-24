"""`just skilltest-record`: run the suite and write what it did to `results/latest.json`.

The record is `{"figures": {...}, "detail": {...}}`, the shape `repo_checks
telemetry_budget` reads: the commit the run used, the harness, every model a turn
answered with, each test's decision points with the commands the agent ran, and
each turn's usage, model calls and class, and the probe of the supervisor
configuration alone. Given a directory of a finished run's test records, it folds
them; given none, it runs `just skilltest` to make one, and exits as that did.

Each check's input figure is held free of the conversation it carries: its calls'
input, cache included, less `calls x (history - base)`, where `history` is the
previous check's last-call input plus output in the same test — the base, for a
test's first check, and what an idle compaction left, for the check after it — and
`base` is the probe's first-call input. What one check
image adds to a call is the image probe's first-call input less the text-only
probe's. A figure the run cannot work out — no probe, or no check of a class — is
left out of `figures`, which `telemetry_budget` refuses by name.

Each figure's detail splits it by part where the probes measured the parts: the
harness's own (the harness-only probe's first call: its framing and its tools'
definitions) apart from printobserver's (the base less that: the skill, the tool
framing and the answer instruction), the check image (the image probe's first call
less the text probe's), and the facts and framing a check's prompt carries (the
rest); for an acting check, also what its later calls re-read of its first and what
acting added to them — the call that ran the command and the command's result.
"""

from __future__ import annotations

import itertools
import json
import os
import shutil
import statistics
import sys
import tempfile
from pathlib import Path
from typing import Any, TypedDict, get_args

from replay import (
    BARE_PROBE_RECORD,
    IMAGE_PROBE_RECORD,
    PROBE_RECORD,
    REPORTED_COUNTS,
    BareProbeRecord,
    CallUsage,
    CheckWait,
    Compaction,
    DecisionRecord,
    ImageKind,
    ImageProbeRecord,
    LedgerCompaction,
    ProbeRecord,
    TestRecord,
    is_assessment,
    is_compaction,
)
from repo_checks.shell import run
from turns_per_run import CASES, UnreadableCaseError, replays

REPO = Path(__file__).resolve().parents[2]
RESULT = REPO / "tests" / "skilltest" / "results" / "latest.json"
#: The directory beside the record a failed test's transcript and server log are
#: kept in, for the job's report to carry: what the record keeps of a turn is counts.
FAILED_DIRECTORY = "failed"
#: What a test's own transcript and server log are called in a run's directory.
KEPT_SUFFIXES = (".transcript.jsonl", ".server.log")
HARNESS = "claude-code"


def commit() -> str:
    """The commit this tree's run used."""
    # llmlint: ignore[async_typed_clients_at_boundaries] See suppressions.toml.
    return run(["git", "rev-parse", "HEAD"], cwd=REPO, check=True).stdout.strip()


#: The decision points of a supervision check carry these beside a start request's.
CHECK_FIELDS = {"calls", "check_class"}
#: The fields a decision point may leave out: what a record kept before they existed lacks.
OPTIONAL_FIELDS = {"facts_bytes", "image", "wait", "follows_compaction"}
#: The side of the square Anthropic's rule counts one image token for, in pixels.
TOKEN_SIDE_PX = 28
#: The whole camera frame a check is shown when it is not shown the composite.
WHOLE_FRAME_PX = (1280, 720)
#: The triggers of the decision points an operator's agent's conversation answers, each
#: a start request or a watch line an outside supervisor is handed, which are no checks.
CONVERSATION_TRIGGERS = {"start_request", "outside_supervision"}


class Detail(TypedDict):
    """What the record says beside its figures."""

    commit: str
    harness: str
    models: list[str]
    turns_the_cases_count: int
    compactions: int
    #: The compaction settings every replay's server ran with.
    compaction: Compaction | None
    #: Every idle compaction the run's replays made.
    idle_compactions: int
    passed: bool
    tests: list[TestRecord]
    probe: ProbeRecord | None
    image_probe: ImageProbeRecord | None
    bare_probe: BareProbeRecord | None
    #: What each figure was worked out from, by the figure's name.
    figures: FiguresDetail


class RunRecord(TypedDict):
    """The record of one run."""

    figures: dict[str, float]
    detail: Detail


class UnreadableRunError(Exception):
    """A run's directory holding something that is no test's record, or nothing at all."""


def _is_usage(usage: object) -> bool:
    """Whether one call's usage carries token counts alone, its input and output among them.

    A call reporting neither is no measurement, and would be folded as a call that read
    nothing.
    """
    return (
        isinstance(usage, dict)
        and set(REPORTED_COUNTS) <= usage.keys()
        and usage.keys() <= CallUsage.__annotations__.keys()
        and all(type(count) is int and count >= 0 for count in usage.values())
    )


def _check_is_readable(point: dict[str, Any]) -> bool:
    """Whether a supervision check says its calls and its class; a conversation says neither.

    A check that passed made at least one call: one that reports none measured
    nothing, and would fold into the figures as a check that cost nothing.
    """
    if point.get("trigger") in CONVERSATION_TRIGGERS:
        return True
    return (
        isinstance(point.get("calls"), list)
        and (bool(point["calls"]) or point.get("passed") is False)
        and all(_is_usage(usage) for usage in point["calls"])
        and point.get("check_class") in {"quiet", "acting"}
    )


def _image_is_readable(image: object) -> bool:
    """Whether a check's image says which it was and why, or is absent as an older record's is."""
    return image is None or (
        isinstance(image, dict)
        and image.get("kind") in get_args(ImageKind)
        and isinstance(image.get("note"), str)
        and (image.get("why") is None or isinstance(image["why"], str))
    )


def _decision_point_is_readable(point: object) -> bool:
    """Whether a decision point has every field of `DecisionRecord`, with those read here typed."""
    return (
        isinstance(point, dict)
        and DecisionRecord.__annotations__.keys() - CHECK_FIELDS - OPTIONAL_FIELDS <= point.keys()
        and (point.get("facts_bytes") is None or type(point["facts_bytes"]) is int)
        and _image_is_readable(point.get("image"))
        and _check_is_readable(point)
        and isinstance(point["scenario"], str)
        and isinstance(point["at"], str)
        and isinstance(point["trigger"], str)
        and isinstance(point["requests"], list)
        and all(isinstance(request, str) for request in point["requests"])
        and isinstance(point["opened"], list)
        and all(isinstance(name, str) for name in point["opened"])
        and (point["why_not"] is None or isinstance(point["why_not"], str))
        and point["passed"] is (point["why_not"] is None)
        and is_assessment(point["assessment"])
        and type(point["looks"]) is int
        and point["looks"] >= 0
        and (point["usage"] is None or isinstance(point["usage"], dict))
        and isinstance(point["passed"], bool)
        and isinstance(point["compacted"], bool)
        and isinstance(point["commands"], list)
        and all(isinstance(command, str) for command in point["commands"])
        and isinstance(point["models"], list)
        and all(isinstance(model, str) for model in point["models"])
    )


def _compound_commands_are_readable(found: object, sessions: list[str]) -> bool:
    """Whether a test says, for each of its conversations, what it ran beside another command."""
    return (
        isinstance(found, dict)
        and set(found) == set(sessions)
        and all(
            isinstance(commands, list) and all(isinstance(command, str) for command in commands)
            for commands in found.values()
        )
    )


def _test_record(path: Path) -> TestRecord:
    """One test's record, once it has the fields the record is folded from."""
    document = json.loads(path.read_text(encoding="utf-8"))
    if (
        not isinstance(document, dict)
        or not isinstance(document.get("test"), str)
        or not isinstance(document.get("passed"), bool)
        or type(document.get("turns")) is not int
        or not isinstance(document.get("sessions"), list)
        or not all(isinstance(session, str) for session in document["sessions"])
        or not _compound_commands_are_readable(
            document.get("compound_commands"), document["sessions"]
        )
        or not isinstance(document.get("decision_points"), list)
        or not all(_decision_point_is_readable(point) for point in document["decision_points"])
        or document["turns"] != len(document["decision_points"])
        or document["passed"] != all(point["passed"] for point in document["decision_points"])
        or ("compaction_probe" in document and not is_compaction(document["compaction_probe"]))
    ):
        msg = f"{path} is no test's record"
        raise UnreadableRunError(msg)
    return document


def _probe_record(directory: Path) -> ProbeRecord | None:
    """The run's probe, when its test ran."""
    path = directory / PROBE_RECORD
    if not path.is_file():
        return None
    document = json.loads(path.read_text(encoding="utf-8"))
    if (
        not isinstance(document, dict)
        or not isinstance(document.get("prompt"), str)
        or type(document.get("skill_bytes")) is not int
        or not isinstance(document.get("calls"), list)
        or not document["calls"]
        or not all(_is_usage(usage) for usage in document["calls"])
    ):
        msg = f"{path} is no probe's record"
        raise UnreadableRunError(msg)
    return document


def _bare_probe_record(directory: Path) -> BareProbeRecord | None:
    """The run's harness-only probe, when its test ran."""
    path = directory / BARE_PROBE_RECORD
    if not path.is_file():
        return None
    document = json.loads(path.read_text(encoding="utf-8"))
    if (
        not isinstance(document, dict)
        or not isinstance(document.get("prompt"), str)
        or not isinstance(document.get("calls"), list)
        or not document["calls"]
        or not all(_is_usage(usage) for usage in document["calls"])
    ):
        msg = f"{path} is no harness-only probe's record"
        raise UnreadableRunError(msg)
    return document


def _image_probe_record(directory: Path) -> ImageProbeRecord | None:
    """The run's image probe, when its test ran."""
    path = directory / IMAGE_PROBE_RECORD
    if not path.is_file():
        return None
    document = json.loads(path.read_text(encoding="utf-8"))
    if (
        not isinstance(document, dict)
        or not isinstance(document.get("prompt"), str)
        or not isinstance(document.get("case"), str)
        or not isinstance(document.get("scenario"), str)
        or not all(
            type(document.get(name)) is int and document[name] > 0
            for name in ("width", "height", "bytes")
        )
        or not isinstance(document.get("calls"), list)
        or not document["calls"]
        or not all(_is_usage(usage) for usage in document["calls"])
    ):
        msg = f"{path} is no image probe's record"
        raise UnreadableRunError(msg)
    return document


def rule_tokens(width: int, height: int) -> int:
    """An image's tokens by Anthropic's rule: one per 28 by 28 pixel square, rounded up."""
    return -(-width // TOKEN_SIDE_PX) * -(-height // TOKEN_SIDE_PX)


def call_input(usage: CallUsage) -> int:
    """What one call read, its prompt cache's reads and writes included."""
    return (
        usage.get("input_tokens", 0)
        + usage.get("cache_read_tokens", 0)
        + usage.get("cache_write_tokens", 0)
    )


class Check(TypedDict):
    """One supervision check, as its figures are worked out."""

    test: str
    scenario: str
    check_class: str
    calls: int
    commands: list[str]
    looks: int
    #: Its calls' input, cache included.
    raw_input: int
    #: Its first call's input less the history it carried, when there is a base.
    first_call_input: int | None
    #: The previous check's last-call input plus output, the base for a first, or
    #: what a completed compaction left for the check after it.
    history: int | None
    #: `raw_input - calls x (history - base)`, when there is a base.
    input: int | None
    #: The conversation its last call leaves: that call's input, cache included, and output.
    size: int | None
    #: Its calls' input not read from the prompt cache.
    uncached_input: int
    #: Its calls' input read from the prompt cache.
    cached_input: int
    #: Whether an idle compaction of its session completed since the previous check.
    follows_compaction: bool
    #: How its event waited for its turn, when the record says.
    wait: CheckWait | None


def checks_of(tests: list[TestRecord], base: int | None) -> list[Check]:
    """Every supervision check of the run, in each test's order, with its figures."""
    found: list[Check] = []
    for test in tests:
        previous: list[CallUsage] | None = None
        # Each completed compaction is followed by exactly one check marked as
        # following it, in the same order, and that check carries in what the
        # compaction left rather than the previous check's last call.
        left = iter(
            [
                compaction.get("reported_after")
                for compaction in test.get("compactions", [])
                if compaction["ended"] == "completed"
            ]
        )
        for point in test["decision_points"]:
            if point["trigger"] in CONVERSATION_TRIGGERS:
                continue
            calls = point["calls"]
            raw = sum(call_input(call) for call in calls)
            history: int | None
            if point.get("follows_compaction", False):
                history = next(left, None)
            elif previous:
                last = previous[-1]
                history = call_input(last) + last.get("output_tokens", 0)
            else:
                history = base
            found.append(
                {
                    "test": test["test"],
                    "scenario": point["scenario"],
                    "check_class": point["check_class"],
                    "calls": len(calls),
                    "commands": point["commands"],
                    "looks": point["looks"],
                    "raw_input": raw,
                    "first_call_input": None
                    if base is None or history is None or not calls
                    else call_input(calls[0]) - (history - base),
                    "history": history,
                    "input": None
                    if base is None or history is None
                    else raw - len(calls) * (history - base),
                    "size": conversation_size(calls[-1]) if calls else None,
                    "uncached_input": sum(
                        call.get("input_tokens", 0) + call.get("cache_write_tokens", 0)
                        for call in calls
                    ),
                    "cached_input": sum(call.get("cache_read_tokens", 0) for call in calls),
                    "follows_compaction": bool(point.get("follows_compaction", False)),
                    "wait": point.get("wait"),
                }
            )
            previous = calls
    return found


def _median(values: list[int]) -> float:
    return float(statistics.median(values))


def conversation_size(usage: CallUsage) -> int:
    """The conversation one call leaves: its input, cache included, and its output."""
    return call_input(usage) + usage.get("output_tokens", 0)


class Workload(TypedDict):
    """One print's conversation a projection is made over."""

    checks: int
    acting: int


#: A typical 2.5 h print checked every 120 s.
TYPICAL_PRINT: Workload = {"checks": 76, "acting": 4}
#: The long plate: 9 h 21 min checked every 120 s.
LONG_PLATE: Workload = {"checks": 281, "acting": 15}


class Rates(TypedDict):
    """What the projection is made from, each from the run's own per-call figures."""

    #: The base context per call: the text probe's first-call input.
    base: int
    #: The median history-free input of a quiet check.
    quiet: float
    #: The median growth in conversation size per quiet check.
    growth: float
    #: The median model calls per acting check.
    acting_calls: float
    #: The median conversation size after an idle compaction.
    after_compaction: float
    #: The median input of an idle compaction.
    compaction_input: float
    #: The idle compaction threshold the replays ran with.
    threshold: int


class BaseSplit(TypedDict):
    """A projection's base context, as the harness's part and printobserver's."""

    #: Calls x the harness-only probe's first call.
    harness: float
    #: Calls x the base less that.
    printobserver: float


class Projection(TypedDict):
    """One print's projected input, split by what each part of it re-reads."""

    #: Every call's base context: calls x the base.
    base: float
    #: The base's harness part and printobserver's, when the harness-only probe ran.
    base_split: BaseSplit | None
    #: What each check adds to its calls with no history: calls x (quiet - base).
    checks: float
    #: The conversation each call carries beyond the base.
    history: float
    #: What the idle compactions read.
    compaction: float


def projection_parts(workload: Workload, rates: Rates, harness: int | None) -> Projection:
    """[`projected`]'s total, split into base, per-check additions, history and compaction."""
    checks, acting = workload["checks"], workload["acting"]
    acting_at = {((index * 2 + 1) * checks) // (acting * 2) for index in range(acting)}
    added = rates["quiet"] - rates["base"]
    carried = float(rates["base"])
    parts: Projection = {
        "base": 0.0,
        "base_split": None,
        "checks": 0.0,
        "history": 0.0,
        "compaction": 0.0,
    }
    calls_made = 0.0
    for check in range(checks):
        calls = rates["acting_calls"] if check in acting_at else 1.0
        calls_made += calls
        parts["checks"] += calls * added
        parts["history"] += calls * (carried - rates["base"])
        carried += rates["growth"]
        if carried >= rates["threshold"]:
            parts["compaction"] += rates["compaction_input"]
            carried = rates["after_compaction"]
    parts["base"] = calls_made * rates["base"]
    if harness is not None:
        parts["base_split"] = {
            "harness": calls_made * harness,
            "printobserver": calls_made * (rates["base"] - harness),
        }
    return parts


def projected(workload: Workload, rates: Rates) -> tuple[float, int]:
    """The input one print's conversation reads, and the idle compactions it makes.

    Each check's calls read the conversation it carries plus what the check adds
    to a call with no history (`quiet - base`); the conversation starts at the
    base and grows by `growth` per check, and once it reaches the threshold it
    is compacted, which reads `compaction_input`, and restarts at
    `after_compaction`. An acting check makes `acting_calls` calls, the quiet
    ones one; the acting checks are spread evenly over the print.
    """
    checks, acting = workload["checks"], workload["acting"]
    acting_at = {((index * 2 + 1) * checks) // (acting * 2) for index in range(acting)}
    added = rates["quiet"] - rates["base"]
    carried = float(rates["base"])
    total = 0.0
    compactions = 0
    for check in range(checks):
        calls = rates["acting_calls"] if check in acting_at else 1.0
        total += calls * (carried + added)
        carried += rates["growth"]
        if carried >= rates["threshold"]:
            total += rates["compaction_input"]
            carried = rates["after_compaction"]
            compactions += 1
    return total, compactions


def _growth(checks: list[Check]) -> list[int]:
    """How much each quiet check grew its test's conversation over the check before it."""
    grown = []
    for previous, check in itertools.pairwise(checks):
        if (
            previous["test"] == check["test"]
            and check["check_class"] == "quiet"
            and not check["follows_compaction"]
            and previous["size"] is not None
            and check["size"] is not None
        ):
            grown.append(check["size"] - previous["size"])
    return grown


def _rates(
    checks: list[Check],
    base: int | None,
    compactions: list[LedgerCompaction],
    threshold: int | None,
) -> Rates | None:
    """The projection's rates, when the run measured every one of them.

    What a compaction reads and leaves comes from `compactions`: the idle ones the
    run observed, or, when none completed, the one it probed.
    """
    quiet = [check["input"] for check in checks if check["check_class"] == "quiet"]
    quiet = [figure for figure in quiet if figure is not None]
    acting = [check["calls"] for check in checks if check["check_class"] == "acting"]
    grown = _growth(checks)
    completed = [compaction for compaction in compactions if compaction["ended"] == "completed"]
    after = [
        compaction["reported_after"] for compaction in completed if "reported_after" in compaction
    ]
    read = [call_input(compaction.get("usage", {})) for compaction in completed]
    if base is None or threshold is None or not (quiet and acting and grown and after and read):
        return None
    return {
        "base": base,
        "quiet": _median(quiet),
        "growth": _median(grown),
        "acting_calls": _median(acting),
        "after_compaction": _median(after),
        "compaction_input": _median(read),
        "threshold": threshold,
    }


def compaction_figures(
    checks: list[Check],
    base: int | None,
    compactions: list[LedgerCompaction],
    threshold: int | None,
    harness: int | None = None,
    probed: list[LedgerCompaction] | None = None,
) -> tuple[dict[str, float], dict[str, Any]]:
    """The token projections and the delayed-check count, and what each was worked out from.

    The projection's compaction is an idle one the run observed; with none completed,
    the one compaction probe the run made, which its detail says.
    """
    figures: dict[str, float] = {}
    detail: dict[str, Any] = {}
    observed = any(compaction["ended"] == "completed" for compaction in compactions)
    measured = compactions if observed or not probed else probed
    rates = _rates(checks, base, measured, threshold)
    if rates is not None:
        typical, typical_compactions = projected(TYPICAL_PRINT, rates)
        long, long_compactions = projected(LONG_PLATE, rates)
        figures["tokens_per_print"] = typical
        figures["tokens_per_long_plate"] = long
        read = sum(check["cached_input"] for check in checks)
        detail["tokens_per_print"] = {
            "workload": TYPICAL_PRINT,
            "rates": rates,
            "split": projection_parts(TYPICAL_PRINT, rates, harness),
            "projected_compactions": typical_compactions,
            "compactions_observed": compactions,
            "compaction_measured_by": "an idle compaction" if measured is compactions else "probe",
            **({"compaction_probe": probed} if probed else {}),
            "long_plate": {"workload": LONG_PLATE, "tokens": long},
            "checks": [
                {
                    "test": check["test"],
                    "scenario": check["scenario"],
                    "calls": check["calls"],
                    "uncached_input": check["uncached_input"],
                }
                for check in checks
            ],
        }
        detail["tokens_per_long_plate"] = {
            "workload": LONG_PLATE,
            "rates": rates,
            "projected_compactions": long_compactions,
            "cache_read_share": read / max(1, read + sum(c["uncached_input"] for c in checks)),
        }
    waits = [(check, check["wait"]) for check in checks if check["wait"] is not None]
    if waits:
        cancelling = [
            wait["wait_ms"]
            for _, wait in waits
            if wait["wait_ms"] is not None
            and any(compaction["ended"] == "cancelled" for compaction in wait["compactions"])
        ]
        figures["compaction_delayed_checks"] = float(sum(1 for _, wait in waits if wait["delayed"]))
        detail["compaction_delayed_checks"] = {
            "started": len(compactions),
            "completed": sum(1 for c in compactions if c["ended"] == "completed"),
            "cancelled": sum(1 for c in compactions if c["ended"] == "cancelled"),
            "longest_wait_while_cancelling_ms": max(cancelling) if cancelling else None,
            "longest_scheduled_wait_ms": max(
                (
                    wait["waited_for_compaction_ms"]
                    for _, wait in waits
                    if wait["waited_for_compaction_ms"] is not None
                ),
                default=None,
            ),
            "each": [
                {"test": check["test"], "scenario": check["scenario"], **wait}
                for check, wait in waits
            ],
        }
    return figures, detail


class CheckCalls(TypedDict):
    """One check and how many model calls it made."""

    test: str
    scenario: str
    calls: int


class QuietCalls(TypedDict):
    """What the quiet checks' calls figure was worked out from."""

    checks: int
    max: int
    above_one_call: list[CheckCalls]


class ActingCheck(TypedDict):
    """One acting check, its calls and the commands it ran."""

    test: str
    scenario: str
    calls: int
    commands: list[str]


class CheckLooks(TypedDict):
    """One check and how many looks it took."""

    test: str
    scenario: str
    looks: int


class ActingCalls(TypedDict):
    """What the acting checks' calls figure was worked out from."""

    checks: int
    max: int
    each: list[ActingCheck]
    looked_again: list[CheckLooks]


class CheckInput(TypedDict):
    """One check's history-free input, its calls and the history it carried in."""

    test: str
    scenario: str
    input: int
    calls: int
    history: int | None


class ClassInput(TypedDict):
    """What one class's input figure was worked out from."""

    median_raw_input: float
    each: list[CheckInput]
    #: The median check's figure, split by part.
    split: CheckParts
    #: The median growth in conversation size a check of the class leaves behind.
    leaves: float | None


class BaseContext(TypedDict):
    """What the base-context figure was worked out from: the probe's first call."""

    cache_write_tokens: int
    cache_read_tokens: int
    input_tokens: int
    skill_bytes: int
    #: The base split by part, when the harness-only probe ran.
    split: Parts | None


class FrameImage(TypedDict):
    """What the image-token figure was worked out from: the two probes' first calls."""

    case: str
    scenario: str
    pixels: int
    width: int
    height: int
    bytes: int
    image_probe_first_call_input: int
    text_probe_first_call_input: int
    #: Anthropic's rule for the check image, and for a whole 1280x720 frame.
    rule_tokens: int
    whole_frame_rule_tokens: int


class Parts(TypedDict):
    """What a call's base context is made of: the harness's part and printobserver's."""

    #: The harness-only probe's first call: the harness's framing and its tools.
    harness: int
    #: The base less the harness's part: the skill, the tool framing, the instruction.
    printobserver: int


class CheckParts(TypedDict, total=False):
    """One check's history-free input, split by part as far as the probes measured."""

    test: str
    scenario: str
    input: int
    calls: int
    #: The harness's framing and tools; absent with no harness-only probe.
    harness: int
    #: The skill, the tool framing and the answer instruction.
    skill_and_instruction: int
    #: The base whole, when the harness's part is not measured apart from it.
    base: int
    #: The check image, as the image probe measured one.
    image: int
    #: The check's own prompt beside its image: its facts and their framing.
    facts_and_framing: int
    #: An acting check's later calls re-reading its first call's input.
    repeated: int
    #: What acting added to those calls: the command's call and its result.
    action_and_result: int


class FiguresDetail(TypedDict, total=False):
    """What each figure the run could work out was worked out from, by its name."""

    quiet_check_model_calls: QuietCalls
    acting_check_model_calls: ActingCalls
    quiet_check_input_tokens: ClassInput
    acting_check_input_tokens: ClassInput
    base_context_tokens: BaseContext
    frame_image_tokens: FrameImage


def _parts(base: int, harness: int | None) -> Parts | None:
    """The base split into the harness's part and printobserver's, when both are measured."""
    if harness is None:
        return None
    return {"harness": harness, "printobserver": base - harness}


def _check_parts(
    check: Check, base: int | None, harness: int | None, image: int | None
) -> CheckParts:
    """One check's history-free input split by part, as far as the probes measured.

    Its first call is the base, the image and the facts and framing beside it; each
    later call re-reads that first call and adds what acting added: the call running
    the command and the command's result.
    """
    figure = check["input"] if check["input"] is not None else 0
    first = check["first_call_input"] if check["first_call_input"] is not None else figure
    parts: CheckParts = {
        "test": check["test"],
        "scenario": check["scenario"],
        "input": figure,
        "calls": check["calls"],
    }
    if base is None:
        return parts
    split = _parts(base, harness)
    if split is None:
        parts["base"] = base
    else:
        parts["harness"] = split["harness"]
        parts["skill_and_instruction"] = split["printobserver"]
    if image is not None:
        parts["image"] = image
    parts["facts_and_framing"] = first - base - (image or 0)
    if check["calls"] > 1:
        parts["repeated"] = first * (check["calls"] - 1)
        parts["action_and_result"] = figure - first * check["calls"]
    return parts


def _leaves(checks: list[Check], every: list[Check]) -> float | None:
    """The median growth in conversation size one of `checks` leaves over the check before it."""
    wanted = {id(check) for check in checks}
    grown = [
        check["size"] - previous["size"]
        for previous, check in itertools.pairwise(every)
        if id(check) in wanted
        and previous["test"] == check["test"]
        and not check["follows_compaction"]
        and previous["size"] is not None
        and check["size"] is not None
    ]
    return _median(grown) if grown else None


def _class_input(
    checks: list[Check],
    every: list[Check] | None = None,
    base: int | None = None,
    harness: int | None = None,
    image: int | None = None,
) -> tuple[float, ClassInput] | None:
    """One class's median history-free input and its detail, when any check has one.

    Its split is the median check's — the lower middle one, ordered by figure.
    """
    each: list[CheckInput] = [
        {
            "test": check["test"],
            "scenario": check["scenario"],
            "input": check["input"],
            "calls": check["calls"],
            "history": check["history"],
        }
        for check in checks
        if check["input"] is not None
    ]
    if not each:
        return None
    raw = [check["raw_input"] for check in checks if check["input"] is not None]
    figured = sorted(
        (check for check in checks if check["input"] is not None),
        key=lambda check: check["input"] or 0,
    )
    middle = figured[(len(figured) - 1) // 2]
    return _median([entry["input"] for entry in each]), {
        "median_raw_input": _median(raw),
        "each": each,
        "split": _check_parts(middle, base, harness, image),
        "leaves": _leaves(checks, every if every is not None else checks),
    }


def figures_of(
    checks: list[Check],
    probe: ProbeRecord | None,
    image_probe: ImageProbeRecord | None = None,
    bare_probe: BareProbeRecord | None = None,
) -> tuple[dict[str, float], FiguresDetail]:
    """The figures this run's budgets read, and what each was worked out from."""
    figures: dict[str, float] = {}
    detail: FiguresDetail = {}
    base = call_input(probe["calls"][0]) if probe is not None else None
    harness = call_input(bare_probe["calls"][0]) if bare_probe is not None else None
    image = (
        call_input(image_probe["calls"][0]) - base
        if image_probe is not None and base is not None
        else None
    )
    quiet = [check for check in checks if check["check_class"] == "quiet"]
    acting = [check for check in checks if check["check_class"] == "acting"]
    if quiet:
        figures["quiet_check_model_calls"] = _median([check["calls"] for check in quiet])
        detail["quiet_check_model_calls"] = {
            "checks": len(quiet),
            "max": max(check["calls"] for check in quiet),
            "above_one_call": [
                {"test": check["test"], "scenario": check["scenario"], "calls": check["calls"]}
                for check in quiet
                if check["calls"] > 1
            ],
        }
    if acting:
        figures["acting_check_model_calls"] = _median([check["calls"] for check in acting])
        detail["acting_check_model_calls"] = {
            "checks": len(acting),
            "max": max(check["calls"] for check in acting),
            "each": [
                {
                    "test": check["test"],
                    "scenario": check["scenario"],
                    "calls": check["calls"],
                    "commands": check["commands"],
                }
                for check in acting
            ],
            "looked_again": [
                {"test": check["test"], "scenario": check["scenario"], "looks": check["looks"]}
                for check in acting
                if check["looks"]
            ],
        }
    quiet_input = _class_input(quiet, checks, base, harness, image)
    if quiet_input is not None:
        figures["quiet_check_input_tokens"], detail["quiet_check_input_tokens"] = quiet_input
    acting_input = _class_input(acting, checks, base, harness, image)
    if acting_input is not None:
        figures["acting_check_input_tokens"], detail["acting_check_input_tokens"] = acting_input
    if probe is not None:
        first = probe["calls"][0]
        figures["base_context_tokens"] = float(call_input(first))
        detail["base_context_tokens"] = {
            "cache_write_tokens": first.get("cache_write_tokens", 0),
            "cache_read_tokens": first.get("cache_read_tokens", 0),
            "input_tokens": first.get("input_tokens", 0),
            "skill_bytes": probe["skill_bytes"],
            "split": _parts(int(call_input(first)), harness),
        }
    if probe is not None and image_probe is not None:
        text, imaged = call_input(probe["calls"][0]), call_input(image_probe["calls"][0])
        width, height = image_probe["width"], image_probe["height"]
        figures["frame_image_tokens"] = float(imaged - text)
        detail["frame_image_tokens"] = {
            "case": image_probe["case"],
            "scenario": image_probe["scenario"],
            "pixels": width * height,
            "width": width,
            "height": height,
            "bytes": image_probe["bytes"],
            "image_probe_first_call_input": imaged,
            "text_probe_first_call_input": text,
            "rule_tokens": rule_tokens(width, height),
            "whole_frame_rule_tokens": rule_tokens(*WHOLE_FRAME_PX),
        }
    return figures, detail


def folded(directory: Path, commit_id: str, cases: Path = CASES) -> RunRecord:
    """The record of one run, from each test's record in its directory and its probe's."""
    tests = [
        _test_record(path)
        for path in sorted(directory.glob("*.json"))
        if path.name not in {PROBE_RECORD, IMAGE_PROBE_RECORD, BARE_PROBE_RECORD}
    ]
    probe = _probe_record(directory)
    image_probe = _image_probe_record(directory)
    bare_probe = _bare_probe_record(directory)
    named = [test["test"] for test in tests]
    if len(set(named)) != len(named):
        msg = f"{directory} holds one test twice: {sorted(named)}"
        raise UnreadableRunError(msg)
    points = [point for test in tests for point in test["decision_points"]]
    replayed = replays(cases)
    for test in tests:
        if replayed.get(test["test"]) != [point["scenario"] for point in test["decision_points"]]:
            msg = f"{test['test']} is no case with a replay of those decision points"
            raise UnreadableRunError(msg)
    base = call_input(probe["calls"][0]) if probe is not None else None
    checks = checks_of(tests, base)
    figures, figured = figures_of(checks, probe, image_probe, bare_probe)
    configured = _configured(tests)
    compactions = [compaction for test in tests for compaction in test.get("compactions", [])]
    compacted, compacted_detail = compaction_figures(
        checks,
        base,
        compactions,
        configured["idle_compact_tokens"] if configured is not None else None,
        call_input(bare_probe["calls"][0]) if bare_probe is not None else None,
        [test["compaction_probe"] for test in tests if "compaction_probe" in test],
    )
    return {
        "figures": {"turns": len(points), **figures, **compacted},
        "detail": {
            "commit": commit_id,
            "harness": HARNESS,
            "models": sorted({model for point in points for model in point["models"]}),
            "turns_the_cases_count": sum(len(replayed[test["test"]]) for test in tests),
            "compactions": sum(1 for point in points if point["compacted"]),
            "compaction": configured,
            "idle_compactions": len(compactions),
            "passed": all(test["passed"] for test in tests),
            "tests": tests,
            "probe": probe,
            "image_probe": image_probe,
            "bare_probe": bare_probe,
            "figures": {**figured, **compacted_detail},
        },
    }


def _configured(tests: list[TestRecord]) -> Compaction | None:
    """The compaction settings every replay ran with, refused when two replays differ."""
    found = [test["configured"] for test in tests if test.get("configured") is not None]
    if not found:
        return None
    if any(each != found[0] for each in found):
        msg = f"the replays ran with different compaction settings: {found}"
        raise UnreadableRunError(msg)
    return found[0]


def keep_the_failed(directory: Path, written: RunRecord, into: Path) -> list[Path]:
    """Copy each failed test's transcript and server log from a run's directory into `into`.

    `into` is emptied first, so it holds this run's failures and no earlier one's.
    """
    shutil.rmtree(into, ignore_errors=True)
    kept: list[Path] = []
    for test in written["detail"]["tests"]:
        if test["passed"]:
            continue
        for suffix in KEPT_SUFFIXES:
            source = directory / f"{test['test']}{suffix}"
            if source.is_file():
                into.mkdir(parents=True, exist_ok=True)
                kept.append(Path(shutil.copy2(source, into / source.name)))
    return kept


def main(arguments: list[str]) -> int:
    """Run the suite, or read a finished run, write the record, and exit as the run went."""
    directory = Path(arguments[0]) if arguments and Path(arguments[0]).is_dir() else None
    status = 0
    if directory is None:
        directory = Path(tempfile.mkdtemp())
        # llmlint: ignore[async_typed_clients_at_boundaries] See suppressions.toml.
        status = run(
            ["just", "skilltest", *arguments],
            cwd=REPO,
            env={**os.environ, "SKILLTEST_RUN_DIR": str(directory)},
            capture=False,
        ).returncode
    try:
        written = folded(directory, commit())
    except (UnreadableRunError, UnreadableCaseError, ValueError) as error:
        print(f"record: {error}", file=sys.stderr)
        return status or 1
    RESULT.parent.mkdir(parents=True, exist_ok=True)
    RESULT.write_text(json.dumps(written, indent=2) + "\n", encoding="utf-8")
    print(f"wrote {RESULT.relative_to(REPO)}", file=sys.stderr)
    failed = RESULT.parent / FAILED_DIRECTORY
    kept = keep_the_failed(directory, written, failed)
    if kept:
        print(
            f"kept {len(kept)} files of failed tests in {failed.relative_to(REPO)}", file=sys.stderr
        )
    if status:
        return status
    if not written["detail"]["tests"] or not written["detail"]["passed"]:
        print("record: the run has no tests, or a test that failed", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    raise SystemExit(main(sys.argv[1:]))
