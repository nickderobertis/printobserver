"""The skill tier's deterministic half: every scenario built, held to what it is read from.

The live tier spends model turns and runs outside the gate; this module runs
inside it and spends none. It builds every scenario on disk exactly as a live
run does and refuses a tree in which:

- a scenario on disk yields no live test;
- a `never` step has no `not_called` eval, or an acceptable outcome is not
  checked, or a stub or spy would not route the command it exists for;
- a `look` answers a frame that is not the scenario's, or not in its order;
- the committed turn template's slots differ from the ones the tier fills, or
  one is left unfilled;
- a composed answer carries a field name its operation's generated example in
  `skills/printobserver/reference/common-operations.md` does not, or breaks
  the answer schema the server declares for it.

skilltest's own loader — the Rust one a live run uses — reads every built case
too, so a pattern its regex engine would refuse fails here rather than at the
first live run.
"""

from __future__ import annotations

import hashlib
import json
import re
import shutil
import tempfile
from collections.abc import Iterator
from pathlib import Path
from typing import Any

import pytest
import test_real_prints_skilltest as live
from answers import example_answers, field_names, from_labelled, labelled
from jsonschema import Draft202012Validator
from jsonschema.protocols import Validator
from real_prints import (
    CASES,
    COMMON_OPERATIONS,
    OPERATIONS,
    REPO,
    TURN_PROMPT,
    Scenario,
    Step,
    agent_turn,
    commands_in,
    met_outcome,
    operations,
    outcome_met,
    reads,
    required_steps,
    scenarios,
    shipped_model,
    step_matches,
)
from repo_checks import expect
from scenario import (
    Built,
    StubSpec,
    build,
    declared_slots,
    fill,
    template_slots,
)
from skilltest_pytest import MockRefEval, SkilltestProviderError, ToolSpy, run_skill

SCENARIOS = scenarios()
IDS = [scenario.test_id for scenario in SCENARIOS]


@pytest.fixture(scope="module")
def built() -> Iterator[dict[str, Built]]:
    """Every scenario, built once under a directory of its own."""
    roots = []
    found = {}
    for scenario in SCENARIOS:
        root = Path(tempfile.mkdtemp())
        roots.append(root)
        found[scenario.test_id] = build(scenario, root)
    yield found
    for root in roots:
        shutil.rmtree(root, ignore_errors=True)


def _haystack(command: str, description: str = "Run a command") -> str:
    """A shell call as the harness's hook matches it: the compact JSON of the call."""
    call = {"tool_name": "Bash", "tool_input": {"command": command, "description": description}}
    return json.dumps(call, separators=(",", ":"), ensure_ascii=False)


def _routed(built: Built, command: str, description: str = "Run a command") -> str | None:
    """The stub that answers a command: the first whose pattern matches, as the hook picks."""
    haystack = _haystack(command, description)
    return next(
        (spec.name for spec in built.stubs if re.search(spec.pattern, haystack)),
        None,
    )


def _watches(watcher: ToolSpy, command: str) -> bool:
    """Whether a spy observes a command, matched as the hook matches it."""
    return re.search(watcher._match_spec()["pattern"], _haystack(command)) is not None


def _command(built: Built, step: Step, *, reason: bool = True) -> str:
    """The command an agent following its skill runs to take a step."""
    words = [
        "printobserver",
        step.operation,
        "--config",
        f"'{built.config}'",
        "--print-id",
        built.print_id,
    ]
    if step.operation not in reads():
        words += ["--actor", f"'{built.actor}'"]
    options = dict(step.args)
    if step.operation == "acknowledge-failure" and "event_id" not in options and built.event:
        options["event_id"] = built.event["id"]
    for name, value in options.items():
        words += [f"--{name.replace('_', '-')}", str(value)]
    if reason and step.operation not in reads():
        words += ["--reason", '"what I saw, and what a person should check"']
    return " ".join(words)


# ---------------------------------------------------------------------------
# Every scenario is a test
# ---------------------------------------------------------------------------


def test_every_scenario_on_disk_is_one_live_test() -> None:
    """The live test is parametrized over every scenario of every case.json on disk."""
    on_disk = []
    for path in sorted(CASES.glob("*/case.json")):
        document = json.loads(path.read_text(encoding="utf-8"))
        on_disk += [f"{path.parent.name}/{s['id']}" for s in document["assertions"]["scenarios"]]
    collected = [scenario.test_id for scenario in live.SCENARIOS]
    expect.equal(sorted(collected), sorted(on_disk), describing="the live tests' scenarios")
    cases = {path.parent.name for path in CASES.glob("*/case.json")}
    expect.equal(
        {test_id.split("/")[0] for test_id in collected}, cases, describing="the cases covered"
    )


@pytest.mark.parametrize("scenario", SCENARIOS, ids=IDS)
def test_skilltest_loads_every_built_case(scenario: Scenario, built: dict[str, Built]) -> None:
    """The case loads in skilltest's own loader, every pattern a regex its engine compiles.

    A provider that does not exist is the first thing a valid case reaches, so
    the run stops there having read the whole definition.
    """
    case = built[scenario.test_id]
    missing = case.workspace.parent / "no-provider"
    with pytest.raises(SkilltestProviderError, match="no-provider"):
        run_skill(
            case.case,
            provider=[str(missing)],
            platforms=["claude-code"],
            models=["unpinned"],
            cwd=case.workspace,
        )


def test_every_operation_the_server_serves_is_stubbed(built: dict[str, Built]) -> None:
    """A new operation is a command an agent may run, so a scenario without its stub is refused."""
    for case in built.values():
        stubbed = {spec.operation for spec in case.stubs}
        expect.equal(
            set(operations()) - stubbed, set(), describing=f"{case.scenario.test_id} unstubbed"
        )


# ---------------------------------------------------------------------------
# The case's assertions, as evals and as the outcome check
# ---------------------------------------------------------------------------


def _evals(case: Built, kind: str) -> list[MockRefEval]:
    return [e for e in case.case.evals if isinstance(e, MockRefEval) and e.type == kind]


@pytest.mark.parametrize("scenario", SCENARIOS, ids=IDS)
def test_every_never_step_is_a_not_called_eval(scenario: Scenario, built: dict[str, Built]) -> None:
    """Each never step's spy is held by a not_called eval and observes that step being taken."""
    case = built[scenario.test_id]
    watched = [e.mock for e in _evals(case, "not_called")]
    expect.equal(len(watched), len(scenario.never), describing="one not_called eval per never step")
    for step, watcher in case.never:
        expect.truth(
            any(w is watcher for w in watched), describing=f"a not_called eval on {step.describe()}"
        )
        expect.truth(
            _watches(watcher, _command(case, step)),
            describing=f"the spy for never {step.describe()} to observe it being taken",
        )
        expect.truth(
            not _watches(watcher, f"printobserver {step.operation} --help"),
            describing=f"the spy for never {step.describe()} to ignore a request for its usage",
        )


@pytest.mark.parametrize("scenario", SCENARIOS, ids=IDS)
def test_every_outcome_is_checked(scenario: Scenario, built: dict[str, Built]) -> None:
    """Each acceptable outcome is met by its own steps, in order, and by nothing short of them.

    Steps every outcome shares are also `called` evals, which no passing run
    can skip.
    """
    case = built[scenario.test_id]
    for outcome in scenario.accept_any_of:
        ran = [command for step in outcome for command in commands_in(_command(case, step))]
        expect.truth(
            outcome_met(outcome, ran),
            describing=f"{[s.describe() for s in outcome]} met by its steps",
        )
        expect.truth(met_outcome(scenario, ran) is not None, describing="the scenario to pass")
        if len(outcome) > 1:
            expect.truth(
                not outcome_met(outcome, list(reversed(ran))),
                describing=f"{[s.describe() for s in outcome]} unmet out of order",
            )
        if outcome:
            expect.truth(
                not outcome_met(outcome, ran[:-1]),
                describing=f"{[s.describe() for s in outcome]} unmet without its last step",
            )
    watched = [e.mock for e in _evals(case, "called")]
    expect.equal(len(watched), len(required_steps(scenario)), describing="the called evals")
    for step, watcher in case.required:
        expect.truth(any(w is watcher for w in watched), describing=f"called({step.describe()})")
        expect.truth(_watches(watcher, _command(case, step)), describing="it observes the step")


def test_a_step_is_taken_only_by_a_command_the_program_carries_out() -> None:
    """A request for the usage, or a change asked for with no reason, takes no step."""
    pause = Step("pause", {})
    acknowledge = Step("acknowledge-failure", {"disposition": "stop"})
    taken = commands_in(
        "printobserver pause --print-id P --actor A --reason 'a person should look'; "
        "printobserver acknowledge-failure --disposition stop --print-id P --actor A "
        "--event-id E --reason 'spaghetti'"
    )
    expect.truth(step_matches(pause, taken[0]), describing="a pause with a reason taken")
    expect.truth(step_matches(acknowledge, taken[1]), describing="args matched in any order")
    for untaken in (
        "printobserver pause --help",
        "printobserver pause --print-id P --actor A",
        'grep -rn "printobserver pause" reference',
        "echo printobserver pause --reason why",
    ):
        expect.truth(
            not any(step_matches(pause, c) for c in commands_in(untaken)),
            describing=f"`{untaken}` to take no pause",
        )
    fan = Step("set-fan-percent", {"percent": 100})
    for spelled in ("100", "100.0", "'100'"):
        ran = commands_in(f"printobserver set-fan-percent --percent {spelled} --reason r")
        expect.truth(step_matches(fan, ran[0]), describing=f"{spelled} to be the value 100")


# ---------------------------------------------------------------------------
# The stubs
# ---------------------------------------------------------------------------


def _spec(case: Built, name: str) -> StubSpec:
    return next(spec for spec in case.stubs if spec.name == name)


@pytest.mark.parametrize("scenario", SCENARIOS, ids=IDS)
def test_looks_answer_the_scenarios_frames_in_order(
    scenario: Scenario, built: dict[str, Built]
) -> None:
    """Successive looks answer the scenario's frames, by path to a copy of each, in its order."""
    case = built[scenario.test_id]
    frames = [look.image for look in scenario.looks] or [scenario.event_image]
    arrived = [list(look.arrived_event_ids) for look in scenario.looks] or [[]]
    for name in ("look", "look-json"):
        spec = _spec(case, name)
        expect.equal(len(spec.documents), len(frames), describing=f"{name}'s responses")
        for answer, frame, events, output in zip(
            spec.documents, frames, arrived, spec.outputs(), strict=True
        ):
            path = Path(answer["image_path"])
            expect.truth(path.is_absolute(), describing=f"{path} to be absolute")
            expect.equal(
                hashlib.sha256(path.read_bytes()).hexdigest(),
                hashlib.sha256((scenario.directory / frame).read_bytes()).hexdigest(),
                describing=f"the look answering {frame}",
            )
            expect.equal(
                [event["id"] for event in answer.get("arrived", [])],
                events,
                describing="the events it delivers",
            )
            expect.contains(output["output"], str(path), describing="the rendered answer")


def test_status_reads_the_state_the_sequence_a_case_fixes_produced(
    built: dict[str, Built],
) -> None:
    """Where every acceptable outcome pauses the print, a later status read says it is paused."""
    for case in built.values():
        states = [doc["printer"]["connection"] for doc in _spec(case, "status").documents]
        every_pauses = all(
            any(step.operation == "pause" for step in outcome)
            for outcome in case.scenario.accept_any_of
        )
        expected = [case.scenario.printer_state, "paused"] if every_pauses else states[:1]
        expect.equal(states, expected, describing=f"{case.scenario.test_id}'s status reads")


def test_commands_route_to_the_stub_that_answers_them(built: dict[str, Built]) -> None:
    """The hook picks the first stub matching a command; each command reaches the right one."""
    case = next(c for c in built.values() if len(c.answers["look"]) > 1 and c.event)
    if case.event is None:
        pytest.fail("no scenario with two looks carries an event")
    event = case.event["id"]
    config = f"'{case.config}'"
    actor = f"'{case.actor}'"
    common = f"--config {config} --print-id {case.print_id}"
    routes = {
        f"printobserver context {common}": "context",
        f"printobserver look {common} --wait-s 30": "look",
        f"printobserver look {common} --json": "look-json",
        f"/usr/local/bin/printobserver status {common}": "status",
        "printobserver look --help": "help",
        "printobserver --version": "version",
        "printobserver": "usage",
        f'C={config}; printobserver acknowledge-failure --config "$C" --print-id {case.print_id} '
        f"--actor {actor} --event-id {event} --disposition stop --reason 'nest'": (
            f"acknowledge-stop-{event}"
        ),
        f"printobserver acknowledge-failure {common} --actor {actor} --disposition watch "
        f"--event-id {event} --reason 'static debris'": f"acknowledge-watch-{event}",
        f"printobserver set-fan-percent {common} --actor {actor} --percent 100 "
        f"--duration-s 1800 --reason 'cooling'": "set-fan-percent-100",
        f"printobserver set-flowrate-factor {common} --actor {actor} --factor 1.0 "
        f"--reason 'flow'": "set-flowrate-factor-1.00",
        f"printobserver pause {common} --actor {actor} --reason 'a person'": "pause",
        f"printobserver pause {common} --actor {actor}": "pause-without-reason",
        f"cd {case.workspace}/reference; cat command-surface.md": None,
        f'grep -rn "printobserver look" {case.workspace}': None,
    }
    for command, expected in routes.items():
        expect.equal(_routed(case, command), expected, describing=f"the stub for `{command}`")
    expect.equal(
        _routed(case, "ls", description="List the printobserver look reference"),
        None,
        describing="a description naming a command to route nothing",
    )


@pytest.mark.parametrize("scenario", SCENARIOS, ids=IDS)
def test_an_acknowledgement_answers_what_was_decided(
    scenario: Scenario, built: dict[str, Built]
) -> None:
    """Each disposition of each alert the turn was handed is echoed by the answer it gets."""
    case = built[scenario.test_id]
    echoed = 0
    for spec in case.stubs:
        named = re.fullmatch(
            r"acknowledge-(continue|watch|stop)-(?P<event>[\w-]+?)(?:-json)?", spec.name
        )
        if named is None:
            continue
        echoed += 1
        disposition, event_id = named.group(1), named.group("event")
        action = spec.documents[0]["record"]["request"]["action"]
        expect.equal(
            (action["disposition"], action["event_id"]),
            (disposition, event_id),
            describing=f"{spec.name}'s echo",
        )
    alerts = (
        0 if case.event is None else 1 + sum(len(look.arrived_event_ids) for look in scenario.looks)
    )
    expect.equal(echoed, alerts * 3 * 2, describing="a stub per disposition, alert and rendering")


# ---------------------------------------------------------------------------
# The prompt
# ---------------------------------------------------------------------------


def test_the_template_slots_are_the_ones_the_tier_fills(built: dict[str, Built]) -> None:
    """The committed template, the slots its own crate declares and the tier's fill agree."""
    template = TURN_PROMPT.read_text(encoding="utf-8")
    written = template_slots(template)
    expect.equal(len(written), len(set(written)), describing="each slot written once")
    expect.equal(sorted(written), sorted(declared_slots()), describing="the declared slots")
    for case in built.values():
        if case.event is None:
            expect.equal(case.slots, None, describing="a start request, which fills no template")
            continue
        filled = case.slots or {}
        expect.equal(sorted(filled), sorted(written), describing="the slots the tier fills")
        expect.equal(
            case.prompt, fill(template, filled), describing="the prompt as the template filled"
        )
    for case in built.values():
        expect.truth(
            "{{" not in case.prompt, describing=f"{case.scenario.test_id}'s prompt fully filled"
        )


def test_a_template_with_a_slot_the_tier_does_not_fill_is_refused() -> None:
    """A slot added to the template, or one dropped from it, fails the fill."""
    template = TURN_PROMPT.read_text(encoding="utf-8")
    values = {slot: "x" for slot in template_slots(template)}
    with pytest.raises(ValueError, match="slots"):
        fill(template + "\n{{camera}}\n", values)
    with pytest.raises(ValueError, match="slots"):
        fill(template.replace("{{actor}}", "someone"), values)
    filled = fill(template, {**values, "{{event}}": "{{situation}}"})
    expect.contains(filled, "{{situation}}", describing="a filling's text left as written")


@pytest.mark.parametrize("scenario", SCENARIOS, ids=IDS)
def test_the_input_hands_the_cases_picture_by_absolute_path(
    scenario: Scenario, built: dict[str, Built]
) -> None:
    """The event's picture is a copy of the case's, named by an absolute path in the input."""
    case = built[scenario.test_id]
    picture = case.images[scenario.event_image]
    expect.truth(picture.path.is_absolute(), describing="an absolute path")
    expect.contains(case.prompt, str(picture.path), describing="the input")
    expect.equal(
        picture.path.read_bytes(),
        (scenario.directory / scenario.event_image).read_bytes(),
        describing="the copy's bytes",
    )
    if case.event is not None:
        expect.equal(case.event["image"]["sha256"], picture.sha256, describing="the event's image")
    expect.truth((case.workspace / "SKILL.md").is_file(), describing="the skill as the workspace")


# ---------------------------------------------------------------------------
# The answers, against the program's own examples and the server's schemas
# ---------------------------------------------------------------------------


def _answer_schemas() -> dict[str, Validator]:
    document = json.loads(OPERATIONS.read_text(encoding="utf-8"))
    found = {}
    for operation in document["operations"]:
        success = next(r["type"] for r in operation["responses"] if r["answer"] == "success")
        path = next(REPO.glob(f"schemas/*/{success}.json"))
        schema = json.loads(path.read_text(encoding="utf-8"))
        found[operation["name"].replace("_", "-")] = Draft202012Validator(schema)
    return found


EXAMPLES = example_answers(COMMON_OPERATIONS.read_text(encoding="utf-8"))
SCHEMAS = _answer_schemas()


def _composed(case: Built) -> Iterator[tuple[str, dict[str, Any]]]:
    for spec in case.stubs:
        if spec.render not in {"labelled", "json"} or not spec.operation:
            continue
        if spec.operation in case.replayed:
            continue
        for document in spec.documents:
            yield spec.operation, document


def _unexampled(operation: str, document: dict[str, Any]) -> set[str]:
    """The composed answer's field names no example of its operation carries."""
    names = field_names(labelled(document).splitlines())
    gaps = [names - field_names(example) for example in EXAMPLES[operation]]
    return min(gaps, key=len)


@pytest.mark.parametrize("scenario", SCENARIOS, ids=IDS)
def test_composed_answers_carry_the_example_field_names(
    scenario: Scenario, built: dict[str, Built]
) -> None:
    """No composed answer names a field its operation's generated example does not.

    The example is what `just docs-generate` captured from the real program;
    a field it no longer carries, or one renamed, is a shape the program no
    longer answers. A field the schema lets an answer leave out may be left
    out, and the schema check below is what refuses a required one missing.
    """
    for operation, document in _composed(built[scenario.test_id]):
        expect.equal(
            _unexampled(operation, document), set(), describing=f"{operation}'s field names"
        )
        problems = [error.message for error in SCHEMAS[operation].iter_errors(document)]
        expect.equal(problems, [], describing=f"{operation}'s answer against its schema")


def test_a_renamed_field_is_refused(built: dict[str, Built]) -> None:
    """A composed answer naming a field the example does not is caught, not passed."""
    case = next(iter(built.values()))
    look = dict(_spec(case, "look").documents[0])
    expect.equal(_unexampled("look", look), set(), describing="the composed look")
    look["frame_path"] = look.pop("image_path")
    expect.equal(_unexampled("look", look), {"frame_path"}, describing="a renamed look field")


@pytest.mark.parametrize(
    "scenario",
    [s for s in SCENARIOS if agent_turn(s.case) is not None],
    ids=lambda s: s.test_id,
)
def test_a_replayed_context_is_the_recorded_answer(
    scenario: Scenario, built: dict[str, Built]
) -> None:
    """A case's recorded context is replayed line for line, its picture's path repointed."""
    case = built[scenario.test_id]
    turn = agent_turn(scenario.case) or {}
    recorded = next(
        step["result"]["content"]
        for step in turn["steps"]
        if step.get("tool") == "Bash"
        and "printobserver context" in step["input"]["command"]
        and not step["result"]["is_error"]
    )
    replayed = labelled(_spec(case, "context").documents[0]).splitlines()
    original = recorded.splitlines()
    expect.equal(len(replayed), len(original), describing="the replayed lines")
    for mine, theirs in zip(replayed, original, strict=True):
        if theirs.startswith("image_path: "):
            expect.equal(mine, f"image_path: {case.images[scenario.event_image].path}")
        else:
            expect.equal(mine, theirs, describing="a replayed line")
    expect.equal(from_labelled(recorded)["context"]["print"]["id"], case.print_id)


def test_the_model_is_read_from_the_shipped_configuration() -> None:
    """The installer's `[supervisor]` table is found and read for a pinned model.

    None pinned is a valid answer, and the one a production turn takes: the
    harness's own default.
    """
    model = shipped_model()
    expect.truth(model is None or model.strip(), describing="no model, or a named one")
