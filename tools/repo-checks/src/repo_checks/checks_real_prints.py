"""The real-print cases under `tests/real-prints`, held to their own claims.

Each case directory is evidence from a real printer — camera frames, the exact
pictures a supervision turn was given, the history printobserver recorded — and
a `case.json` saying what it shows and what an agent shown it should do. The
skill tests read that `case.json`'s `assertions` at run time, so a case that
names a file it does not carry, an event its history does not hold, or a
command printobserver does not have is a test that asserts nothing true.

Every check here reads files and nothing else: no model, network or printer.
Each file is validated against the contract that declares it before anything
is read out of it — a `case.json` against `case.schema.json`, a history against
the server's own `HistoryAnswer` — and then read into the typed records below.
A case that fails either is reported once by `real-prints-schema`, and by every
other check as one it could not read, so a check run on its own never passes a
case it skipped.
"""

from __future__ import annotations

import hashlib
import json
import re
import tomllib
from collections.abc import Iterator
from dataclasses import dataclass
from pathlib import Path
from typing import Any, NewType

from contract_codegen.schemas import ContractError, load, read_schemas
from jsonschema import Draft202012Validator
from jsonschema.exceptions import SchemaError
from jsonschema.protocols import Validator

from repo_checks.model import Repo

ROOT = "tests/real-prints"
SCHEMA = f"{ROOT}/case.schema.json"
CASE_FILE = "case.json"
HISTORY = "printobserver-history.json"
HISTORY_SCHEMA = "schemas/printobserver-server/HistoryAnswer.json"
SERVICE_CONFIG = f"{ROOT}/service-config.toml"
OPERATIONS = "schemas/printobserver-server/operations.json"
PRINTER_STATE = "schemas/printobserver-printer-api/PrinterState.json"

# A file name as a case.json's prose names one: an optional relative directory,
# then a name with one of the extensions a case carries. A path after `@` or `:`
# is read as one too, so `<ref>:<path>` naming another commit's copy of a file
# names a file this tree must carry, like any other.
FILE_TOKEN = re.compile(
    r"(?<![\w./-])(?:\.\./)?(?:[\w-]+/)*[\w.-]+\.(?:jpg|png|json|py|gcode|diff|toml)\b"
)

# A field of the service configuration that holds a secret, by how this
# configuration names one — `octoprint.api_key`, `ingress.shared_secret`,
# `obico.access_token`, `api.credential` — rather than by a list of today's
# fields, so a secret field added later is held to the same rule.
SECRET_FIELD = re.compile(r"(?:key|secret|token|credential|password)$")
REDACTED = "<redacted>"

# An event's id in a case's history, and a scenario's id within its case.
EventId = NewType("EventId", str)
ScenarioId = NewType("ScenarioId", str)


class UnreadableContractError(ValueError):
    """A committed contract a check reads cannot be read as one."""


def _valid_schema(schema: object, named: str) -> dict[str, Any]:
    """A committed schema, checked to be a valid JSON Schema before it is used.

    Raises:
        UnreadableContractError: If it is not an object or not a valid schema.
    """
    if not isinstance(schema, dict):
        msg = f"{named} is not a JSON Schema object"
        raise UnreadableContractError(msg)
    try:
        Draft202012Validator.check_schema(schema)
    except SchemaError as error:
        msg = f"{named} is not a valid JSON Schema: {error.message}"
        raise UnreadableContractError(msg) from error
    return schema


@dataclass(frozen=True, slots=True)
class Step:
    """One printobserver command a scenario accepts or forbids."""

    operation: str
    args: dict[str, str | float]

    @classmethod
    def read(cls, raw: dict[str, Any]) -> Step:
        """A step from its validated JSON."""
        return cls(raw["operation"], dict(raw.get("args", {})))


@dataclass(frozen=True, slots=True)
class Look:
    """One answer a `printobserver look` gives."""

    image: str
    arrived_event_ids: tuple[EventId, ...]

    @classmethod
    def read(cls, raw: dict[str, Any]) -> Look:
        """A look from its validated JSON."""
        arrived = tuple(EventId(event_id) for event_id in raw.get("arrived_event_ids", ()))
        return cls(raw["image"], arrived)


@dataclass(frozen=True, slots=True)
class Scenario:
    """One situation a case puts the agent in, and what passes it."""

    id: ScenarioId
    event_id: EventId | None
    event_image: str
    printer_state: str
    looks: tuple[Look, ...]
    accept_any_of: tuple[tuple[Step, ...], ...]
    never: tuple[Step, ...]

    @classmethod
    def read(cls, raw: dict[str, Any]) -> Scenario:
        """A scenario from its validated JSON."""
        return cls(
            id=ScenarioId(raw["id"]),
            event_id=EventId(raw["event_id"]) if "event_id" in raw else None,
            event_image=raw["event_image"],
            printer_state=raw["printer_state"],
            looks=tuple(Look.read(look) for look in raw["looks"]),
            accept_any_of=tuple(
                tuple(Step.read(step) for step in outcome) for outcome in raw["accept_any_of"]
            ),
            never=tuple(Step.read(step) for step in raw["never"]),
        )

    @property
    def images(self) -> tuple[str, ...]:
        """Every image the scenario hands the agent."""
        return (self.event_image, *(look.image for look in self.looks))

    @property
    def steps(self) -> Iterator[Step]:
        """Every step the scenario names, accepted or never."""
        for outcome in self.accept_any_of:
            yield from outcome
        yield from self.never


@dataclass(frozen=True, slots=True)
class Event:
    """One event of a case's history: its kind and the sha256 of its image."""

    id: EventId
    kind: str
    image_sha256: str | None

    @classmethod
    def read(cls, raw: dict[str, Any]) -> Event:
        """An event from its record, validated against `HistoryAnswer`."""
        image = raw.get("image")
        return cls(EventId(raw["id"]), raw["kind"], image["sha256"] if image else None)


@dataclass(frozen=True, slots=True)
class Case:
    """One case directory, read and validated."""

    name: str
    directory: Path
    document: dict[str, Any]
    scenarios: tuple[Scenario, ...]
    agent_images: dict[str, EventId]
    history: dict[EventId, Event] | None

    def referenced(self) -> Iterator[str]:
        """Every file the case.json refers to and so must carry.

        Each `file` field, each scenario image, and every file named in
        `sources` and `obico_scores`. Every other field's prose may mention a
        file — `see its make_plate.py` — without that being a reference.
        """
        yield from _file_fields({k: v for k, v in self.document.items() if k != "assertions"})
        for scenario in self.scenarios:
            yield from scenario.images
        texts = [*self.document.get("sources", {}).values(), self.document.get("obico_scores", "")]
        for text in texts:
            yield from FILE_TOKEN.findall(text)

    def mentioned(self) -> set[str]:
        """Every file name the case.json mentions anywhere."""
        return set(FILE_TOKEN.findall(json.dumps(self.document)))


def _file_fields(value: object) -> Iterator[str]:
    """Every string a `file` key holds, at any depth of a JSON value."""
    match value:
        case dict():
            for key, item in value.items():
                if key == "file" and isinstance(item, str):
                    yield item
                else:
                    yield from _file_fields(item)
        case list():
            for item in value:
                yield from _file_fields(item)
        case _:
            pass


def _validator(repo: Repo, relative: str) -> Validator:
    """The validator for one committed schema, itself checked to be a valid schema.

    Raises:
        UnreadableContractError: If the file is not a valid schema.
    """
    return Draft202012Validator(_valid_schema(json.loads(repo.read(relative)), relative))


def _problems(validator: Validator, document: object, named: str) -> list[str]:
    """Each way a document breaks its schema, located by its JSON path."""
    errors = sorted(validator.iter_errors(document), key=lambda error: error.json_path)
    return [f"{named} at {error.json_path}: {error.message}" for error in errors]


def _read_json(path: Path) -> tuple[object, str | None]:
    """A JSON file's value, or the reason it could not be read."""
    try:
        return json.loads(path.read_text(encoding="utf-8")), None
    except json.JSONDecodeError as error:
        return None, f"{path.name} does not parse: {error}"


def _read_case(
    directory: Path, case_schema: Validator, history_schema: Validator
) -> Case | list[str]:
    """One case directory read into a `Case`, or every problem that stopped it."""
    path = directory / CASE_FILE
    if not path.is_file():
        return [f"has no {CASE_FILE}"]
    document, unreadable = _read_json(path)
    if unreadable:
        return [unreadable]
    problems = _problems(case_schema, document, CASE_FILE)
    history: dict[EventId, Event] | None = None
    if (directory / HISTORY).is_file():
        recorded, unreadable = _read_json(directory / HISTORY)
        if unreadable:
            problems.append(unreadable)
        else:
            history_problems = _problems(history_schema, recorded, HISTORY)
            problems.extend(history_problems)
            if not history_problems and isinstance(recorded, dict):
                events = (Event.read(event) for event in recorded["events"])
                history = {event.id: event for event in events}
    if problems or not isinstance(document, dict):
        return problems
    if document["case"] != directory.name:
        return [f"{CASE_FILE} names its case {document['case']!r}, not its directory"]
    return Case(
        name=directory.name,
        directory=directory,
        document=document,
        scenarios=tuple(Scenario.read(raw) for raw in document["assertions"]["scenarios"]),
        agent_images={
            entry["file"]: EventId(entry["event_id"]) for entry in document.get("agent_images", [])
        },
        history=history,
    )


def _cases(repo: Repo) -> Iterator[tuple[str, Case | list[str]]]:
    """Every case directory by name, read, or the problems that stopped it."""
    case_schema = _validator(repo, SCHEMA)
    history_schema = _validator(repo, HISTORY_SCHEMA)
    for directory in sorted(p for p in repo.path(ROOT).iterdir() if p.is_dir()):
        yield directory.name, _read_case(directory, case_schema, history_schema)


def _readable(repo: Repo, check: str, findings: list[str]) -> Iterator[Case]:
    """Each case a check can read; one it cannot, or an unreadable schema, is a finding."""
    try:
        for name, case in _cases(repo):
            if isinstance(case, Case):
                yield case
            else:
                findings.append(f"{name}: its files are unreadable, so {check} could not check it")
    except UnreadableContractError as error:
        findings.append(str(error))


def real_prints_schema(repo: Repo) -> list[str]:
    """Every case carries a `case.json` satisfying `case.schema.json`, and a valid history."""
    try:
        return [
            f"{name}: {problem}"
            for name, case in _cases(repo)
            if not isinstance(case, Case)
            for problem in case
        ]
    except UnreadableContractError as error:
        return [str(error)]


def real_prints_files(repo: Repo) -> list[str]:
    """Every file a case.json refers to is carried, and every file of a case is named by it.

    A reference is relative to the case directory, or climbs to a sibling with
    `../`, and names a file this tree must carry — a frame, a G-code and a crop
    alike. Anything else that names a file, such as a `<ref>:<path>` pointing
    at another commit, is a reference to no file here and is refused as one.
    """
    findings: list[str] = []
    for case in _readable(repo, "real-prints-files", findings):
        for named in case.referenced():
            if not (case.directory / named).is_file():
                findings.append(f"{case.name}: {CASE_FILE} names {named}, which is not a file")
        mentioned = case.mentioned()
        findings.extend(
            f"{case.name}: {path.name} is in the case but {CASE_FILE} names it nowhere"
            for path in sorted(case.directory.iterdir())
            if path.name != CASE_FILE and path.name not in mentioned
        )
    return findings


def real_prints_images(repo: Repo) -> list[str]:
    """Every `agent-*` image is the exact picture its event's history records.

    Every `agent-*` file but the turn's own `agent-turn.json` is a picture.
    """
    findings: list[str] = []
    for case in _readable(repo, "real-prints-images", findings):
        history = case.history or {}
        pictures = (p for p in case.directory.glob("agent-*") if p.suffix != ".json")
        for path in sorted(pictures):
            event_id = case.agent_images.get(path.name)
            if event_id is None:
                findings.append(
                    f"{case.name}: {path.name} is not listed under agent_images with its event"
                )
                continue
            event = history.get(event_id)
            recorded = event.image_sha256 if event else None
            if recorded is None:
                findings.append(
                    f"{case.name}: {path.name}'s event {event_id} records no image in {HISTORY}"
                )
                continue
            actual = hashlib.sha256(path.read_bytes()).hexdigest()
            if actual != recorded:
                findings.append(
                    f"{case.name}: {path.name} has sha256 {actual}, but {HISTORY} records "
                    f"{recorded} for event {event_id}"
                )
    return findings


def _printer_states(repo: Repo) -> frozenset[str]:
    """The string spellings the printer contract's `PrinterState` names.

    Raises:
        UnreadableContractError: If the schema is invalid or names no state.
    """
    schema = _valid_schema(json.loads(repo.read(PRINTER_STATE)), PRINTER_STATE)
    arms = schema.get("oneOf")
    states = frozenset(
        arm["const"]
        for arm in (arms if isinstance(arms, list) else [])
        if isinstance(arm, dict) and isinstance(arm.get("const"), str)
    )
    if not states:
        msg = f"{PRINTER_STATE} names no state as a string constant of its oneOf"
        raise UnreadableContractError(msg)
    return states


def _scenario_findings(case: Case, scenario: Scenario, states: frozenset[str]) -> Iterator[str]:
    """Each way one scenario names an event, a state or an image its case does not have."""
    where = f"{case.name}: scenario {scenario.id}"
    if scenario.printer_state not in states:
        yield (
            f"{where} has printer_state {scenario.printer_state!r}, which {PRINTER_STATE} "
            "does not name"
        )
    events = case.history or {}
    if scenario.event_id is not None:
        event = events.get(scenario.event_id)
        if case.history is None:
            yield f"{where} names event {scenario.event_id}, but the case has no {HISTORY}"
        elif event is None or event.kind != "obico_failure_alert":
            yield (
                f"{where} names event {scenario.event_id}, which is no "
                f"obico_failure_alert in {HISTORY}"
            )
        # A recorded alert is handed with the picture it recorded, which the
        # case keeps as the agent image listed against that event.
        pictured = [name for name, owner in case.agent_images.items() if owner == scenario.event_id]
        if not pictured:
            yield (
                f"{where} names event {scenario.event_id}, but no agent_images entry keeps "
                "the picture it recorded"
            )
        elif scenario.event_image not in pictured:
            yield (
                f"{where} hands {scenario.event_image} with event {scenario.event_id}, whose "
                f"image is {pictured[0]}"
            )
    for look in scenario.looks:
        for arrived in look.arrived_event_ids:
            if arrived not in events:
                yield f"{where} has a look deliver event {arrived}, which {HISTORY} does not hold"
    for image in scenario.images:
        if "/" in image or not (case.directory / image).is_file():
            yield f"{where} names image {image}, which is not a file of the case"


def real_prints_scenarios(repo: Repo) -> list[str]:
    """Every scenario names events its case's history holds and images its case carries."""
    findings: list[str] = []
    try:
        states = _printer_states(repo)
    except UnreadableContractError as error:
        return [str(error)]
    for case in _readable(repo, "real-prints-scenarios", findings):
        seen: set[ScenarioId] = set()
        for scenario in case.scenarios:
            if scenario.id in seen:
                findings.append(f"{case.name}: scenario {scenario.id} is declared twice")
            seen.add(scenario.id)
            findings.extend(_scenario_findings(case, scenario, states))
    return findings


@dataclass(frozen=True, slots=True)
class Operation:
    """One printobserver command, and the shape of each parameter it takes."""

    command: str
    parameters: dict[str, Validator]

    def argument_finding(self, name: str, value: str | float) -> str | None:
        """Why a step cannot give this command this argument, or nothing when it can."""
        shape = self.parameters.get(name)
        if shape is None:
            return f"argument {name!r}, which is not one of its parameters in {OPERATIONS}"
        error = next(iter(shape.iter_errors(value)), None)
        if error is not None:
            return (
                f"argument {name!r} the value {value!r}, which its shape refuses: {error.message}"
            )
        return None


def _type_definitions(repo: Repo) -> dict[str, Any]:
    """Every checked-in schema by type name, as the `$defs` a parameter's shape refers into.

    A type file's own `$defs` are copies of the types it refers to, so they are
    pooled first and the files themselves, each standing for its own name, win.
    """
    declared = {
        name: _valid_schema(schema, name) for name, schema in read_schemas(repo.root).items()
    }
    pooled: dict[str, dict[str, Any]] = {}
    for name, schema in declared.items():
        for inner, definition in schema.get("$defs", {}).items():
            pooled[inner] = _valid_schema(definition, f"{name}'s $defs.{inner}")
    pooled.update(declared)
    return {
        name: {key: value for key, value in schema.items() if key not in {"$schema", "$defs"}}
        for name, schema in pooled.items()
    }


def _operations(repo: Repo) -> dict[str, Operation]:
    """Each printobserver command, hyphenated, read from the server's description.

    The description is first read the way the clients are generated from it,
    which refuses one whose operations, parameters or referenced types are
    malformed; only then are its parameter shapes taken, each checked to be a
    valid schema on its own, beside the pool of types it refers into.

    Raises:
        UnreadableContractError: If the description or a shape cannot be read.
    """
    try:
        load(repo.root)
    except ContractError as error:
        msg = f"{OPERATIONS} cannot be read as the server's description: {error}"
        raise UnreadableContractError(msg) from error
    described = json.loads(repo.read(OPERATIONS))
    definitions = _type_definitions(repo)
    operations = (
        Operation(
            operation["name"].replace("_", "-"),
            {
                parameter["name"]: Draft202012Validator(
                    {
                        "$defs": definitions,
                        **_valid_schema(
                            parameter["shape"],
                            f"{operation['name']}'s parameter {parameter['name']}",
                        ),
                    }
                )
                for parameter in operation["parameters"]
            },
        )
        for operation in described["operations"]
    )
    return {operation.command: operation for operation in operations}


def real_prints_operations(repo: Repo) -> list[str]:
    """Every scenario step is a printobserver command, with that command's own parameters."""
    findings: list[str] = []
    try:
        operations = _operations(repo)
    except UnreadableContractError as error:
        return [str(error)]
    for case in _readable(repo, "real-prints-operations", findings):
        for scenario in case.scenarios:
            where = f"{case.name}: scenario {scenario.id}"
            for step in scenario.steps:
                operation = operations.get(step.operation)
                if operation is None:
                    findings.append(
                        f"{where} names operation {step.operation!r}, which {OPERATIONS} "
                        "does not have"
                    )
                    continue
                for name, value in step.args.items():
                    problem = operation.argument_finding(name, value)
                    if problem is not None:
                        findings.append(f"{where} gives {step.operation} {problem}")
    return findings


def _secret_fields(table: dict[str, Any], prefix: str = "") -> Iterator[tuple[str, object]]:
    """Every secret field of a TOML table, by its dotted name."""
    for key, value in table.items():
        dotted = f"{prefix}{key}"
        match value:
            case dict():
                yield from _secret_fields(value, f"{dotted}.")
            case _ if SECRET_FIELD.search(key):
                yield dotted, value
            case _:
                pass


def real_prints_service_config(repo: Repo) -> list[str]:
    """The shared service configuration parses and carries no secret."""
    try:
        config = repo.read_toml(SERVICE_CONFIG)
    except (OSError, tomllib.TOMLDecodeError) as error:
        return [f"{SERVICE_CONFIG} does not parse: {error}"]
    return [
        f"{SERVICE_CONFIG}: {field} holds a value rather than {REDACTED!r}"
        for field, value in _secret_fields(config)
        if value != REDACTED
    ]
