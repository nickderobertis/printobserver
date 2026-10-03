"""The skill tier's deterministic half: every scenario built, held to what it is read from.

The live tier (`tests/skilltest`) spends model turns and runs outside the gate;
this project runs inside it and spends none. It builds every scenario on disk
exactly as a live run does and refuses a tree in which:

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
import os
import re
import shutil
import tempfile
import tomllib
from collections.abc import Iterator
from pathlib import Path
from typing import Any, get_args

import pytest
import real_prints
import test_real_prints_skilltest as live
from answers import example_answers, field_names, from_labelled, labelled
from jsonschema import Draft202012Validator
from jsonschema.protocols import Validator
from real_prints import (
    CASES,
    COMMON_OPERATIONS,
    SCHEMAS,
    TURN_PROMPT,
    Scenario,
    Step,
    Trigger,
    agent_turn,
    bounds,
    commands_in,
    met_outcome,
    outcome_met,
    recorded_step,
    required_steps,
    sent_to_server,
    shipped_model,
    step_matches,
)
from repo_checks import expect, platforms, shell
from repo_checks.model import Repo
from scenario import (
    ADJUSTMENTS,
    LATER_LOOKS,
    RECORDS_PER_ACTION,
    Built,
    Render,
    StubSpec,
    build,
    declared_slots,
    dispositions,
    fill,
    harness_config,
    template_slots,
)
from skilltest_pytest import MockRefEval, SkilltestProviderError, ToolSpy, run_skill
from surface import OPERATIONS, REPO, surface, turn_tool_rules, usage, version

SCENARIOS = live.SCENARIOS
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


ONEHARNESS = shutil.which("oneharness")


def _hook(rules: list[tuple[str, str]], built: Built, command: str, description: str) -> str | None:
    """Which rule the real hook applies to a shell call, as a live run's harness hands it one.

    `oneharness mock` is the responder skilltest installs as Claude Code's
    pre-tool hook, and it matches each rule's pattern as a regex over the raw
    event Claude Code sends: here a `PreToolUse` event for the call, run from
    the scenario's workspace. Every rule answers with its own name, so the
    first that matched is the one named.
    """
    if ONEHARNESS is None:
        pytest.fail("oneharness, which a live run's hook is, is not installed")
    ruleset = {
        "rules": [
            {
                "match": {"tool": "bash", "event_regex": pattern},
                "action": {"deny": {"message": name}},
            }
            for name, pattern in rules
        ]
    }
    directory = Path(tempfile.mkdtemp())
    try:
        (directory / "rules.json").write_text(json.dumps(ruleset), encoding="utf-8")
        event = {
            "session_id": "a-session",
            "transcript_path": str(built.workspace.parent / "transcript.jsonl"),
            "cwd": str(built.workspace),
            "permission_mode": "dontAsk",
            "hook_event_name": "PreToolUse",
            "tool_name": "Bash",
            "tool_input": {"command": command, "description": description},
        }
        answered = shell.run(
            [ONEHARNESS, "mock", "claude-code", "--rules", str(directory / "rules.json")],
            stdin=json.dumps(event, separators=(",", ":"), ensure_ascii=False),
            timeout=60,
        )
    finally:
        shutil.rmtree(directory, ignore_errors=True)
    if not answered.stdout.strip():
        return None
    return json.loads(answered.stdout)["hookSpecificOutput"]["permissionDecisionReason"]


def _routed(built: Built, command: str, description: str = "Run a command") -> str | None:
    """The stub the real hook answers a command with: the first whose pattern matches."""
    return _hook([(spec.name, spec.pattern) for spec in built.stubs], built, command, description)


def _watches(watcher: ToolSpy, built: Built, command: str) -> bool:
    """Whether a spy observes a command, matched by the real hook as the run matches it."""
    pattern = watcher._match_spec()["pattern"]
    return _hook([("watched", pattern)], built, command, "Run a command") == "watched"


def _command(built: Built, step: Step, *, reason: bool = True) -> str:
    """The command an agent following its skill runs to take a step.

    It names the step's own args, and every other value the command requires
    the way a turn would supply it.
    """
    supplied: dict[str, str] = {
        "print_id": built.print_id,
        "actor": f"'{built.actor}'",
        "event_id": built.event["id"] if built.event else "an-event",
        "file_name": "calibration-box.gcode",
        "manifest": "'{}'",
    }
    if reason:
        supplied["reason"] = '"what I saw, and what a person should check"'
    spec = surface().command(step.operation)
    words = ["printobserver", step.operation, "--config", f"'{built.config}'"]
    options = {name: str(value) for name, value in step.args.items()}
    for required in (field for field in (spec.fields if spec else ()) if field.required):
        if required.name not in options and required.name in supplied:
            options[required.name] = supplied[required.name]
    for name, value in options.items():
        words += [f"--{name.replace('_', '-')}", value]
    return " ".join(words)


def _picture(content: bytes) -> bytes:
    """A JPEG's bytes without the comment segments among its headers, which show nothing.

    Every segment from the start marker up to the first that is neither a
    comment nor the JFIF header is read; the comments are dropped.
    """
    kept = bytearray(content[:2])
    at = 2
    while content[at : at + 2] in {b"\xff\xfe", b"\xff\xe0"}:
        end = at + 2 + int.from_bytes(content[at + 2 : at + 4], "big")
        if content[at : at + 2] == b"\xff\xe0":
            kept += content[at:end]
        at = end
    kept += content[at:]
    return bytes(kept)


def test_a_capture_is_the_same_picture_with_or_without_a_jfif_header() -> None:
    """A capture's comment is found and dropped after the JFIF header as after the start."""
    scan = b"\xff\xdb\x00\x03\x01rest"
    header = b"\xff\xe0\x00\x04JF"
    comment = b"\xff\xfe\x00\x0bcapture 1"
    for before in (b"", header):
        original = b"\xff\xd8" + before + scan
        captured = b"\xff\xd8" + before + comment + scan
        expect.equal(_picture(captured), _picture(original), describing="the same picture")
        expect.truth(captured != original, describing="a capture of its own")


def test_the_triggers_are_the_case_schemas() -> None:
    """The triggers a scenario is read with are exactly the ones `case.schema.json` allows."""
    schema = json.loads((CASES / "case.schema.json").read_text(encoding="utf-8"))
    allowed = schema["$defs"]["Scenario"]["properties"]["trigger"]["enum"]
    expect.equal(sorted(get_args(Trigger)), sorted(allowed), describing="the triggers")


def _spec(case: Built, name: str) -> StubSpec:
    return next(spec for spec in case.stubs if spec.name == name)


def _evals(case: Built, kind: str) -> list[MockRefEval]:
    return [e for e in case.case.evals if isinstance(e, MockRefEval) and e.type == kind]


def test_every_scenario_on_disk_is_one_live_test() -> None:
    """The live test is parametrized over every scenario of every case.json on disk."""
    on_disk = []
    for path in sorted(CASES.glob("*/case.json")):
        document = json.loads(path.read_text(encoding="utf-8"))
        on_disk += [f"{path.parent.name}/{s['id']}" for s in document["assertions"]["scenarios"]]
    expect.equal(sorted(IDS), sorted(on_disk), describing="the live tests' scenarios")
    cases = {path.parent.name for path in CASES.glob("*/case.json")}
    expect.equal({test_id.split("/")[0] for test_id in IDS}, cases, describing="the cases")


@pytest.mark.parametrize("scenario", SCENARIOS, ids=IDS)
def test_skilltest_loads_every_built_case(scenario: Scenario, built: dict[str, Built]) -> None:
    """The case loads in skilltest's own loader, every pattern a regex its engine compiles.

    A provider that does not exist is the first thing a valid case reaches, so
    the run stops there having read the whole definition.
    """
    case = built[scenario.test_id]
    missing = case.workspace.parent / "no-provider"
    with pytest.raises(SkilltestProviderError, match="no-provider"):
        # llmlint: ignore[async_typed_clients_at_boundaries] suppressions.toml has the reason.
        run_skill(
            case.case,
            provider=[str(missing)],
            platforms=["claude-code"],
            models=["unpinned"],
            cwd=case.workspace,
        )


def test_every_command_the_program_has_is_stubbed(built: dict[str, Built]) -> None:
    """A new operation is a command an agent may run, so a scenario without its stub is refused."""
    for case in built.values():
        stubbed = {spec.command for spec in case.stubs}
        missing = {command.name for command in surface().operations} - stubbed
        expect.equal(missing, set(), describing=f"{case.scenario.test_id}'s unstubbed commands")


def test_the_harness_runs_with_a_production_turns_permissions() -> None:
    """The read tools are allowed, and the shell only for the program's own commands."""
    config = tomllib.loads(harness_config())
    expect.equal(config["mode"], "default", describing="the permission mode")
    arguments = config["harness"]["claude-code"]["args"]
    tools_flag, allowed_flag, tools, allowed = turn_tool_rules()
    expect.equal(arguments, [tools_flag, *tools, allowed_flag, *allowed])
    expect.equal((tools_flag, allowed_flag), ("--tools", "--allowedTools"), describing="flags")
    shell = [rule for rule in allowed if rule.startswith("Bash(")]
    expect.equal(len(shell), len(surface().operations), describing="one shell rule per command")
    expect.truth(
        all(rule.startswith("Bash(printobserver ") for rule in shell),
        describing="every shell rule to name the program",
    )


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
            _watches(watcher, case, _command(case, step)),
            describing=f"the spy for never {step.describe()} to observe it being taken",
        )
        expect.truth(
            not _watches(watcher, case, f"printobserver {step.operation} --help"),
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
        named = [step.describe() for step in outcome]
        expect.truth(outcome_met(outcome, ran), describing=f"{named} met by its steps")
        expect.truth(met_outcome(scenario, ran) is not None, describing="the scenario to pass")
        if len(outcome) > 1:
            expect.truth(
                not outcome_met(outcome, list(reversed(ran))),
                describing=f"{named} unmet out of order",
            )
        if outcome:
            expect.truth(
                not outcome_met(outcome, ran[:-1]),
                describing=f"{named} unmet without its last step",
            )
    watched = [e.mock for e in _evals(case, "called")]
    expect.equal(len(watched), len(required_steps(scenario)), describing="the called evals")
    for step, watcher in case.required:
        expect.truth(any(w is watcher for w in watched), describing=f"called({step.describe()})")
        expect.truth(
            _watches(watcher, case, _command(case, step)), describing="it observes the step"
        )


def test_a_step_is_taken_only_by_a_command_the_program_carries_out(tmp_path: Path) -> None:
    """A request for the usage, or a change missing a value it requires, takes no step."""
    pause = Step("pause", {})
    actor = '\'{"agent":{"session_name":"print-P"}}\''
    (tmp_path / "a.json").write_text('{"agent":{"session_name":"print-P"}}', encoding="utf-8")
    acknowledge = Step("acknowledge-failure", {"disposition": "stop"})
    taken = commands_in(
        f"printobserver pause --print-id P --actor {actor} --reason 'a person should look'; "
        "printobserver acknowledge-failure --disposition stop --print-id P --actor-file a.json "
        "--event-id E --reason 'spaghetti'",
        cwd=tmp_path,
    )
    expect.truth(step_matches(pause, taken[0]), describing="a pause with a reason taken")
    expect.truth(step_matches(acknowledge, taken[1]), describing="args matched in any order")
    for untaken in (
        "printobserver pause --help",
        f"printobserver pause --print-id P --actor {actor}",
        "printobserver pause --print-id P --reason 'no actor'",
        'grep -rn "printobserver pause" reference',
        "echo printobserver pause --reason why",
    ):
        expect.truth(
            not any(step_matches(pause, c) for c in commands_in(untaken)),
            describing=f"`{untaken}` to take no pause",
        )
    expect.truth(
        not sent_to_server(commands_in("printobserver frobnicate --reason x")[0]),
        describing="a command the program does not have to take no effect",
    )
    fan = Step("set-fan-percent", {"percent": 100})
    for spelled in ("100", "100.0", "'100'"):
        ran = commands_in(
            f"printobserver set-fan-percent --print-id P --actor {actor} "
            f"--percent {spelled} --reason r"
        )
        expect.truth(step_matches(fan, ran[0]), describing=f"{spelled} to be the value 100")


@pytest.mark.parametrize("scenario", SCENARIOS, ids=IDS)
def test_looks_answer_the_scenarios_frames_in_order(
    scenario: Scenario, built: dict[str, Built]
) -> None:
    """Successive looks answer the scenario's frames, by path to a copy of each, in its order.

    After them the last frame is looked at again, each later look its own
    record at its own instant, delivering nothing already delivered.
    """
    case = built[scenario.test_id]
    frames = [look.image for look in scenario.looks] or [scenario.event_image]
    arrived = [list(look.arrived_event_ids) for look in scenario.looks] or [[]]
    frames += [frames[-1]] * LATER_LOOKS
    arrived += [[]] * LATER_LOOKS
    for name in ("look", "look-json"):
        spec = _spec(case, name)
        expect.equal(len(spec.documents), len(frames), describing=f"{name}'s responses")
        for answer, frame, events, output in zip(
            spec.documents, frames, arrived, spec.outputs(), strict=True
        ):
            path = Path(answer["image_path"])
            expect.truth(path.is_absolute(), describing=f"{path} to be absolute")
            expect.equal(
                _picture(path.read_bytes()),
                _picture((scenario.directory / frame).read_bytes()),
                describing=f"the look answering {frame}",
            )
            expect.equal(
                answer["frame"]["sha256"],
                hashlib.sha256(path.read_bytes()).hexdigest(),
                describing="the frame's digest, of the file it names",
            )
            expect.equal(
                [event["id"] for event in answer.get("arrived", [])],
                events,
                describing="the events it delivers",
            )
            expect.contains(output["output"], str(path), describing="the rendered answer")
        digests = [answer["frame"]["sha256"] for answer in spec.documents[len(scenario.looks) :]]
        expect.equal(len(set(digests)), len(digests), describing="each later look its own capture")
        records = [answer["event"]["id"] for answer in spec.documents]
        instants = [answer["event"]["received_at"] for answer in spec.documents]
        expect.equal(len(set(records)), len(records), describing="a record of each look's own")
        expect.equal(instants, sorted(set(instants)), describing="each look later than the last")


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
    changing = f"{common} --actor {actor}"
    routes = {
        f"printobserver context {common}": "context",
        f"printobserver look {common} --wait-s 30": "look",
        f"printobserver look {common} --json": "look-json",
        f"/usr/local/bin/printobserver status {common}": "status",
        "printobserver look --help": "help",
        "printobserver --version": "version",
        "printobserver": "usage",
        "printobserver frobnicate": "refused",
        f'C={config}; printobserver acknowledge-failure --config "$C" --print-id {case.print_id} '
        f"--actor {actor} --event-id {event} --disposition stop --reason 'nest'": (
            f"acknowledge-stop-{event}"
        ),
        f"printobserver acknowledge-failure {changing} --disposition watch "
        f"--event-id {event} --reason 'static debris'": f"acknowledge-watch-{event}",
        f"printobserver set-fan-percent {changing} --percent 100 --duration-s 1800 "
        "--reason 'cooling'": "set-fan-percent-100",
        f"printobserver set-flowrate-factor {changing} --factor 1.0 --reason 'flow'": (
            "set-flowrate-factor-1.00"
        ),
        f"printobserver set-flowrate-factor {changing} --factor 1.05 --reason 'flow' --json": (
            "set-flowrate-factor-1.05-json"
        ),
        f"printobserver pause {changing} --reason 'a person'": "pause",
        f"printobserver pause {changing}": "refused",
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
        named = re.fullmatch(r"acknowledge-(\w+)-(?P<event>[\w-]+?)(?:-json)?", spec.name)
        if named is None or named.group(1) not in dispositions():
            continue
        echoed += 1
        action = spec.documents[0]["record"]["request"]["action"]
        expect.equal(
            (action["disposition"], action["event_id"]),
            (named.group(1), named.group("event")),
            describing=f"{spec.name}'s echo",
        )
    handed = 0 if case.event is None else 1
    handed += sum(len(look.arrived_event_ids) for look in scenario.looks)
    renderings = len([Render.LABELLED, Render.JSON])
    expect.equal(echoed, handed * len(dispositions()) * renderings, describing="the stubs")


def test_the_template_slots_are_the_ones_the_tier_fills(built: dict[str, Built]) -> None:
    """The committed template, the slots its own crate declares and the tier's fill agree."""
    template = TURN_PROMPT.read_text(encoding="utf-8")
    written = template_slots(template)
    expect.equal(len(written), len(set(written)), describing="each slot written once")
    expect.equal(sorted(written), sorted(declared_slots()), describing="the declared slots")
    for case in built.values():
        expect.truth(
            "{{" not in case.prompt, describing=f"{case.scenario.test_id}'s prompt fully filled"
        )
        if case.event is None:
            expect.equal(case.slots, None, describing="a start request, which fills no template")
            continue
        filled = case.slots or {}
        expect.equal(sorted(filled), sorted(written), describing="the slots the tier fills")
        expect.equal(case.prompt, fill(template, filled), describing="the filled template")


def test_a_template_with_a_slot_the_tier_does_not_fill_is_refused() -> None:
    """A slot added to the template, or one dropped from it, fails the fill."""
    template = TURN_PROMPT.read_text(encoding="utf-8")
    values = dict.fromkeys(template_slots(template), "x")
    with pytest.raises(ValueError, match="slots"):
        fill(template + "\n{{camera}}\n", values)
    with pytest.raises(ValueError, match="slots"):
        fill(template.replace("{{actor}}", "someone"), values)
    filled = fill(template, {**values, "{{event}}": "{{situation}}"})
    expect.contains(filled, "{{situation}}", describing="a filling's text left as written")


def test_the_situation_is_the_turn_situation_contract(built: dict[str, Built]) -> None:
    """The situation a turn is filled with carries exactly the fields `TurnSituation` declares."""
    schema = json.loads((SCHEMAS / "printobserver-supervisor-api/TurnSituation.json").read_text())
    validator = Draft202012Validator(schema)
    for case in built.values():
        if case.situation is None:
            continue
        expect.equal(set(case.situation), set(schema["properties"]), describing="its fields")
        problems = [error.message for error in validator.iter_errors(case.situation)]
        expect.equal(problems, [], describing=f"{case.scenario.test_id}'s situation")


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


def _answer_schemas() -> dict[str, Validator]:
    document = json.loads(OPERATIONS.read_text(encoding="utf-8"))
    found = {}
    for operation in document["operations"]:
        success = next(r["type"] for r in operation["responses"] if r["answer"] == "success")
        path = next(SCHEMAS.glob(f"*/{success}.json"))
        schema = json.loads(path.read_text(encoding="utf-8"))
        found[operation["name"].replace("_", "-")] = Draft202012Validator(schema)
    return found


EXAMPLES = example_answers(COMMON_OPERATIONS.read_text(encoding="utf-8"))
SCHEMA_OF = _answer_schemas()


def _answered(case: Built) -> Iterator[tuple[str, dict[str, Any]]]:
    """Every answer a stub gives, replayed or composed, by the command it answers."""
    for spec in case.stubs:
        if spec.render == Render.TEXT or spec.command is None:
            continue
        for document in spec.documents:
            yield spec.command, document


def _unexampled(operation: str, document: dict[str, Any]) -> set[str]:
    """The composed answer's field names no example of its operation carries."""
    names = field_names(labelled(document).splitlines())
    gaps = [names - field_names(example) for example in EXAMPLES[operation]]
    return min(gaps, key=len)


@pytest.mark.parametrize("scenario", SCENARIOS, ids=IDS)
def test_composed_answers_carry_the_example_field_names(
    scenario: Scenario, built: dict[str, Built]
) -> None:
    """Every answer satisfies its schema, and no composed one names a field its example lacks.

    Replayed answers are held to the schema alone.

    The example is what `just docs-generate` captured from the real program;
    a field it no longer carries, or one renamed, is a shape the program no
    longer answers. A field the schema lets an answer leave out may be left
    out, and the schema check is what refuses a required one missing.
    """
    case = built[scenario.test_id]
    for operation, document in _answered(case):
        # A replayed answer is what an earlier build printed, so it is held to
        # the schema the server declares now rather than to today's example.
        if operation not in case.replayed:
            expect.equal(
                _unexampled(operation, document), set(), describing=f"{operation}'s field names"
            )
        problems = [error.message for error in SCHEMA_OF[operation].iter_errors(document)]
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
    recorded = next(
        command.answered
        for command in agent_turn(scenario.case) or []
        if not command.failed
        and any(c.operation == "context" for c in commands_in(command.command))
    )
    replayed = labelled(_spec(case, "context").documents[0]).splitlines()
    original = recorded.splitlines()
    expect.equal(len(replayed), len(original), describing="the replayed lines")
    picture = case.images[scenario.event_image].path
    for mine, theirs in zip(replayed, original, strict=True):
        if theirs.startswith("image_path: "):
            expect.equal(mine, f"image_path: {picture}", describing="the repointed picture")
        else:
            expect.equal(mine, theirs, describing="a replayed line")
    expect.equal(from_labelled(recorded)["context"]["print"]["id"], case.print_id)


def test_a_recorded_turn_in_another_shape_is_refused() -> None:
    """A recorded turn whose shell step carries no answer is refused rather than half read."""
    expect.equal(recorded_step({"tool": "Read"}, "t"), None, describing="a non-shell step")
    with pytest.raises(ValueError, match="shell step"):
        recorded_step({"tool": "Bash", "input": {"command": "ls"}, "result": {}}, "t")


def test_the_model_is_read_from_the_shipped_configuration() -> None:
    """The installer's `[supervisor]` table is found and read for a pinned model.

    None pinned is a valid answer, and the one a production turn takes: the
    harness's own default.
    """
    model = shipped_model()
    expect.truth(model is None or model.strip(), describing="no model, or a named one")


def _program() -> Path:
    """The `printobserver` program the workspace's debug build leaves on this host."""
    return REPO / "target" / "debug" / platforms.host(Repo(REPO)).program


def test_the_usage_and_version_stubs_are_what_the_program_prints() -> None:
    """`--help` and `--version` answer what the built program itself prints for them."""
    for option, composed in (("--help", usage()), ("--version", version())):
        printed = shell.run([str(_program()), option])
        expect.equal(printed.returncode, surface().exits["success"], describing=f"{option}'s exit")
        expect.equal(composed, printed.stdout, describing=f"the stub's answer to {option}")


def test_the_labelled_rendering_reproduces_every_generated_example() -> None:
    """Read back and rendered again, every example the program printed is printed the same."""
    rendered = 0
    for operation, answers in EXAMPLES.items():
        for lines in answers:
            again = labelled(from_labelled("\n".join(lines))).splitlines()
            expect.equal(again, lines, describing=f"{operation}'s example rendered again")
            rendered += 1
    expect.truth(rendered >= len(surface().operations), describing="every operation's example")


def test_a_labelled_answer_that_is_not_one_is_refused() -> None:
    """A line with no label, or two lines that disagree about a path, is refused."""
    for text in ("a.b 1", "a: 1\na.b: 2", "a.b: 1\na: 2", "a.0: 1\na.0: 2", "a.: 1"):
        with pytest.raises(ValueError, match=r"line [12] "):
            from_labelled(text)
    expect.equal(from_labelled("a.0.b: 1\na.1: x\nc: []"), {"a": [{"b": 1}, "x"], "c": []})


def test_bounds_that_are_not_a_range_are_refused(monkeypatch: pytest.MonkeyPatch) -> None:
    """The configuration's bounds are read as a range per adjustable, or refused."""
    expect.truth(
        {adjustment.adjustable for adjustment in ADJUSTMENTS} <= set(bounds()),
        describing="a bound for every adjustment the stubs answer",
    )
    for written in ("[safety.allowed]\nfan = { min = 100.0, max = 0.0 }\n", "[safety]\n"):
        config = Path(tempfile.mkdtemp()) / "service-config.toml"
        config.write_text(written, encoding="utf-8")
        monkeypatch.setattr(real_prints, "SERVICE_CONFIG", config)
        with pytest.raises(ValueError, match=re.escape("[safety.allowed]")):
            bounds()
        shutil.rmtree(config.parent)


def test_a_command_on_a_later_line_is_still_read() -> None:
    """A line break ends a command as a separator does; quoted or escaped, it does not."""
    ran = commands_in(
        "printobserver look --print-id P\n"
        "printobserver pause --print-id P --actor '{}' --reason \"two\nlines\"\n"
        "printobserver look --print-id P \\\n  --wait-s 30"
    )
    expect.equal([command.operation for command in ran], ["look", "pause", "look"])
    expect.equal(ran[1].options["reason"], "two\nlines", describing="a quoted line break")
    expect.equal(ran[2].options["wait_s"], "30", describing="a continued line")


def test_an_invocation_the_program_refuses_or_answers_at_once_takes_no_effect() -> None:
    """An unknown option, a value given twice or `--version` sends nothing to the server."""
    taken = "printobserver pause --print-id P --actor '{\"agent\":{}}' --reason r"
    expect.truth(sent_to_server(commands_in(taken)[0]), describing="a whole pause")
    expect.truth(
        sent_to_server(commands_in(taken + " --json --config c.toml")[0]),
        describing="the options every command takes",
    )
    expanded = 'A=$(cat actor.json); printobserver pause --print-id P --actor "$A" --reason r'
    expect.truth(sent_to_server(commands_in(expanded)[0]), describing="an actor the shell expands")
    for refused in (
        taken + " --percent 100",
        taken + " --version",
        taken + " --reason again",
        # Carried out on this host, never sent; not run here, since one starts
        # a supervisor and the other signs a harness in.
        "printobserver server --config c.toml",
        "printobserver sign-in",
    ):
        expect.truth(not sent_to_server(commands_in(refused)[0]), describing=f"`{refused}` refused")


def test_a_repeated_action_is_answered_by_a_record_of_its_own(built: dict[str, Built]) -> None:
    """Asking for the same action again is answered by a later, different record."""
    for case in built.values():
        for command in ("pause", "resume", "cancel", "start-print", "acknowledge-failure"):
            records = [document["record"] for document in _spec(case, command).documents]
            expect.equal(len(records), RECORDS_PER_ACTION, describing=f"{command}'s records")
            expect.equal(
                len({record["id"] for record in records}), len(records), describing="their ids"
            )
            instants = [record["request"]["requested_at"] for record in records]
            expect.equal(instants, sorted(set(instants)), describing="each request later")


def _unreachable(directory: Path) -> Path:
    """A client configuration naming an address nothing listens on."""
    config = directory / "client.toml"
    config.write_text('[client]\nserver = "http://127.0.0.1:9"\ncredential = "c"\n', "utf-8")
    return config


def _variants(case: Built) -> list[str]:
    """Invocations an agent might write, each departing from a whole one in one way."""
    common = f"--print-id {case.print_id}"
    actor = f"--actor '{case.actor}'"
    return [
        f"printobserver pause {common} {actor} --reason 'a person should look'",
        f"printobserver pause {common} {actor} --reason ''",
        f"printobserver pause {common} --actor not-a-document --reason r",
        f"printobserver pause {common} {actor} --actor-file a.json --reason r",
        f"printobserver pause --print-id 'a/b' {actor} --reason r",
        f"printobserver pause {common} {actor} --reason r --json",
        f"printobserver set-fan-percent {common} {actor} --percent loud --reason r",
        f"printobserver set-fan-percent {common} {actor} --percent 80 --duration-s 0 --reason r",
        f"printobserver set-fan-percent {common} {actor} --percent 80 --duration-s 86401 "
        "--reason r",
        f"printobserver set-fan-percent {common} {actor} --percent 80 --duration-s 1.5 --reason r",
        f"printobserver set-fan-percent {common} {actor} --percent 80.5 --duration-s 600 "
        "--reason r",
        f"printobserver set-tool-target-c {common} {actor} --tool 0.0 --target-c 215 --reason r",
        f"printobserver set-tool-target-c {common} {actor} --tool 0 --target-c 215 --reason r",
        f"printobserver look {common} --wait-s 30",
        f"printobserver look {common} --wait-s soon",
        f"printobserver history {common} --limit 5",
        f"printobserver context {common} --actor {actor}",
        f"printobserver frobnicate {common}",
        f"printobserver pause {common} --actor-file actor.json --reason r",
        f"printobserver set-tool-target-c {common} {actor} --tool 9223372036854775808 "
        "--target-c 215 --reason r",
        f"printobserver set-tool-target-c {common} {actor} --tool -1 --target-c 215 --reason r",
        f"printobserver set-fan-percent {common} {actor} --percent 1_0 --reason r",
        f"printobserver set-fan-percent {common} {actor} --percent 1e2 --reason r",
        f"printobserver set-fan-percent {common} {actor} --percent ' 80 ' --reason r",
        f"printobserver pause {common} --actor-file missing.json --reason r",
        f"printobserver look stray {common}",
        f"printobserver look {common} --config a.toml --config b.toml",
        f"printobserver look {common} --json",
        f"printobserver pause {common} {actor} --reason 'unbalanced",
    ]


def test_the_judge_agrees_with_the_program_on_what_it_sends(built: dict[str, Built]) -> None:
    """Every invocation the judge counts as sent, the built program sends, and no other.

    The program is pointed at an address nothing listens on, so one it parses
    reaches for the server and exits as unreachable, while one it refuses
    exits with its usage refusal: the steps of every scenario, and variants of
    a whole invocation each breaking one of the parser's rules.
    """
    directory = Path(tempfile.mkdtemp())
    config = str(_unreachable(directory))
    (directory / "actor.json").write_text(next(iter(built.values())).actor, encoding="utf-8")
    environment = {k: v for k, v in os.environ.items() if not k.startswith("PRINTOBSERVER_")}
    corpus = []
    for case in built.values():
        steps = [*case.scenario.never, *(s for o in case.scenario.accept_any_of for s in o)]
        corpus += [_command(case, step) for step in steps]
    corpus += _variants(next(iter(built.values())))
    exits = surface().exits
    try:
        for written in dict.fromkeys(corpus):
            for invocation in commands_in(written, cwd=directory):
                words = [w for w in invocation.words]
                if "--config" in words:
                    words[words.index("--config") + 1] = config
                else:
                    words += ["--config", config]
                ran = shell.run(
                    [str(_program()), *words], cwd=directory, env=environment, timeout=60
                )
                expect.truth(
                    ran.returncode in {exits["unreachable"], exits["usage"]},
                    describing=f"`{written}` to be sent or refused, not to exit {ran.returncode}",
                )
                expect.equal(
                    sent_to_server(invocation),
                    ran.returncode == exits["unreachable"],
                    describing=f"the judge on `{written}`, which the program answered "
                    f"{ran.returncode}: {ran.stderr.strip()[:160]}",
                )
    finally:
        shutil.rmtree(directory, ignore_errors=True)


def test_every_adjustment_is_a_command_with_its_value_and_a_bound() -> None:
    """Each adjustment read from the core is a command of the program, taking its value field."""
    expect.truth(ADJUSTMENTS, describing="adjustments read from the decision core")
    allowed = bounds()
    for adjustment in ADJUSTMENTS:
        spec = surface().command(adjustment.command)
        expect.truth(spec is not None, describing=f"{adjustment.command} a command")
        fields = {field.name for field in spec.fields} if spec else set()
        expect.contains(fields, adjustment.parameter, describing=f"{adjustment.command}'s fields")
        expect.contains(allowed, adjustment.adjustable, describing="the configured bounds")
