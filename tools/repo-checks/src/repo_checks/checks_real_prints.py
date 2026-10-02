"""The real-print cases under `tests/real-prints`, held to their own claims.

Each case directory is evidence from a real printer — camera frames, the exact
pictures a supervision turn was given, the history printobserver recorded — and
a `case.json` saying what it shows and what an agent shown it should do. The
skill tests read that `case.json`'s `assertions` at run time, so a case that
names a file it does not carry, an event its history does not hold, or a
command printobserver does not have is a test that asserts nothing true.

Every check here reads files and nothing else: no model, network or printer.
A case whose `case.json` does not parse or does not satisfy
`case.schema.json` is reported once by `real-prints-schema`, and by every
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
from typing import Any

from jsonschema import Draft202012Validator
from jsonschema.protocols import Validator

from repo_checks.model import Repo

ROOT = "tests/real-prints"
SCHEMA = f"{ROOT}/case.schema.json"
CASE_FILE = "case.json"
HISTORY = "printobserver-history.json"
SERVICE_CONFIG = f"{ROOT}/service-config.toml"
OPERATIONS = "schemas/printobserver-server/operations.json"
PRINTER_STATE = "schemas/printobserver-printer-api/PrinterState.json"

# Where the files this tree dropped — the G-code and the derived crops — remain,
# spelled as a case.json names one of them: `<branch>@<commit>:<path>`.
BRANCH_REFERENCE = re.compile(
    r"fix/windows-supervision-turns@45e7fed:(?P<path>tests/real-prints/[\w./-]+\.\w+)"
)

# A file name as a case.json's prose names one: an optional relative directory,
# then a name with one of the extensions a case carries.
FILE_TOKEN = re.compile(
    r"(?<![\w./@:-])(?:\.\./)?(?:[\w-]+/)*[\w.-]+\.(?:jpg|png|json|py|gcode|diff|toml)\b"
)

# The fields of `sources` and `obico_scores` lead with the file they describe.
LEADING_FIELDS = ("obico_scores",)

# A credential field of the service configuration, and what it must hold here.
CREDENTIAL_KEYS = frozenset({"api_key", "shared_secret", "access_token"})
REDACTED = "<redacted>"


@dataclass(frozen=True, slots=True)
class Case:
    """One case directory, and its `case.json` when it could be read."""

    name: str
    directory: Path
    data: dict[str, Any] | None
    problems: tuple[str, ...]


def _validator(repo: Repo) -> Validator:
    """The validator for `case.schema.json`, itself checked to be a valid schema."""
    schema = json.loads(repo.read(SCHEMA))
    Draft202012Validator.check_schema(schema)
    return Draft202012Validator(schema)


def _cases(repo: Repo) -> list[Case]:
    """Every case directory, each read and validated against the schema."""
    root = repo.path(ROOT)
    validator = _validator(repo)
    cases: list[Case] = []
    for directory in sorted(p for p in root.iterdir() if p.is_dir()):
        name = directory.name
        path = directory / CASE_FILE
        if not path.is_file():
            cases.append(Case(name, directory, None, (f"has no {CASE_FILE}",)))
            continue
        try:
            data = json.loads(path.read_text(encoding="utf-8"))
        except json.JSONDecodeError as error:
            cases.append(Case(name, directory, None, (f"{CASE_FILE} does not parse: {error}",)))
            continue
        problems = [
            f"{CASE_FILE} at {error.json_path}: {error.message}"
            for error in sorted(validator.iter_errors(data), key=lambda e: e.json_path)
        ]
        if not problems and data["case"] != name:
            problems.append(f"{CASE_FILE} names its case {data['case']!r}, not its directory")
        cases.append(Case(name, directory, None if problems else data, tuple(problems)))
    return cases


def _readable(repo: Repo, check: str) -> Iterator[tuple[Case, dict[str, Any]] | str]:
    """Each case a check can read, or the finding saying it could not read it."""
    for case in _cases(repo):
        if case.data is None:
            yield f"{case.name}: {CASE_FILE} is unreadable, so {check} could not check it"
        else:
            yield case, case.data


def real_prints_schema(repo: Repo) -> list[str]:
    """Every case directory carries a `case.json` that satisfies `case.schema.json`."""
    return [f"{case.name}: {problem}" for case in _cases(repo) for problem in case.problems]


def _named_files(data: dict[str, Any]) -> Iterator[str]:
    """Every value a case.json gives as a file.

    Each `file` field, each scenario image, and the file the `sources` fields
    and `obico_scores` lead with.
    """

    def files(value: object) -> Iterator[str]:
        if isinstance(value, dict):
            for key, item in value.items():
                if key == "file" and isinstance(item, str):
                    yield item
                else:
                    yield from files(item)
        elif isinstance(value, list):
            for item in value:
                yield from files(item)

    yield from files({k: v for k, v in data.items() if k != "assertions"})
    leading = [data.get(field) for field in LEADING_FIELDS]
    leading.extend((data.get("sources") or {}).values())
    for text in leading:
        if not isinstance(text, str):
            continue
        reference = BRANCH_REFERENCE.match(text)
        if reference:
            yield reference.group(0)
            continue
        token = FILE_TOKEN.match(text)
        if token:
            yield token.group(0)
    for scenario in data["assertions"]["scenarios"]:
        yield scenario["event_image"]
        yield from (look["image"] for look in scenario["looks"])


def _is_dropped(path: str) -> bool:
    """Whether a path is one of the kinds this tree keeps only on the branch."""
    name = path.rsplit("/", 1)[-1]
    return name.endswith(".gcode") or name.startswith("crop-")


def real_prints_files(repo: Repo) -> list[str]:
    """Every file a case.json names exists, and every file of a case is named by it.

    A name may be relative to the case directory, may climb to a sibling with
    `../`, or may name a dropped file on the branch it remains on; a G-code or a
    crop named any other way names a file this tree does not carry.
    """
    findings: list[str] = []
    for item in _readable(repo, "real-prints-files"):
        if isinstance(item, str):
            findings.append(item)
            continue
        case, data = item
        text = json.dumps(data)
        for named in _named_files(data):
            reference = BRANCH_REFERENCE.fullmatch(named)
            if reference:
                if not _is_dropped(reference.group("path")):
                    findings.append(
                        f"{case.name}: {named} names a file on the branch that this tree should "
                        "carry: only G-code and crops are left there"
                    )
                continue
            if not (case.directory / named).is_file():
                findings.append(f"{case.name}: {CASE_FILE} names {named}, which is not a file")
        bare = BRANCH_REFERENCE.sub("", text)
        for token in FILE_TOKEN.findall(bare):
            if _is_dropped(token):
                findings.append(
                    f"{case.name}: {CASE_FILE} names {token}, a dropped file, without the branch "
                    "it remains on"
                )
        mentioned = set(FILE_TOKEN.findall(bare))
        for path in sorted(case.directory.iterdir()):
            if path.name != CASE_FILE and path.name not in mentioned:
                findings.append(
                    f"{case.name}: {path.name} is in the case but {CASE_FILE} names it nowhere"
                )
    return findings


def _history(case: Case) -> dict[str, dict[str, Any]] | None:
    """The events of a case's history by id, or nothing where it has none."""
    path = case.directory / HISTORY
    if not path.is_file():
        return None
    return {event["id"]: event for event in json.loads(path.read_text(encoding="utf-8"))["events"]}


def real_prints_images(repo: Repo) -> list[str]:
    """Every `agent-*` image is the exact picture its event's history records."""
    findings: list[str] = []
    for item in _readable(repo, "real-prints-images"):
        if isinstance(item, str):
            findings.append(item)
            continue
        case, data = item
        listed = {entry["file"]: entry["event_id"] for entry in data.get("agent_images", [])}
        history = _history(case) or {}
        for path in sorted(case.directory.glob("agent-*")):
            if path.name == "agent-turn.json":
                continue
            event_id = listed.get(path.name)
            if event_id is None:
                findings.append(
                    f"{case.name}: {path.name} is not listed under agent_images with its event"
                )
                continue
            event = history.get(event_id)
            recorded = (event or {}).get("image", {}).get("sha256")
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


def _printer_states(repo: Repo) -> set[str]:
    """The string spellings the printer contract's `PrinterState` names."""
    schema = json.loads(repo.read(PRINTER_STATE))
    return {arm["const"] for arm in schema["oneOf"] if "const" in arm}


def real_prints_scenarios(repo: Repo) -> list[str]:
    """Every scenario names events its case's history holds and images its case carries."""
    findings: list[str] = []
    states = _printer_states(repo)
    for item in _readable(repo, "real-prints-scenarios"):
        if isinstance(item, str):
            findings.append(item)
            continue
        case, data = item
        history = _history(case)
        events = history or {}
        own_images = {entry["file"]: entry["event_id"] for entry in data.get("agent_images", [])}
        seen: set[str] = set()
        for scenario in data["assertions"]["scenarios"]:
            where = f"{case.name}: scenario {scenario['id']}"
            if scenario["id"] in seen:
                findings.append(f"{where} is declared twice")
            seen.add(scenario["id"])
            if scenario["printer_state"] not in states:
                findings.append(
                    f"{where} has printer_state {scenario['printer_state']!r}, which "
                    f"{PRINTER_STATE} does not name"
                )
            event_id = scenario.get("event_id")
            if event_id is not None:
                kind = events.get(event_id, {}).get("kind")
                if history is None:
                    findings.append(
                        f"{where} names event {event_id}, but the case has no {HISTORY}"
                    )
                elif kind != "obico_failure_alert":
                    findings.append(
                        f"{where} names event {event_id}, which is no "
                        f"obico_failure_alert in {HISTORY}"
                    )
                pictured = [name for name, owner in own_images.items() if owner == event_id]
                if pictured and scenario["event_image"] not in pictured:
                    findings.append(
                        f"{where} hands {scenario['event_image']} with event {event_id}, whose "
                        f"image is {pictured[0]}"
                    )
            for look in scenario["looks"]:
                for arrived in look.get("arrived_event_ids", []):
                    if arrived not in events:
                        findings.append(
                            f"{where} has a look deliver event {arrived}, which "
                            f"{HISTORY} does not hold"
                        )
            images = [scenario["event_image"], *(look["image"] for look in scenario["looks"])]
            for image in images:
                if "/" in image or not (case.directory / image).is_file():
                    findings.append(f"{where} names image {image}, which is not a file of the case")
    return findings


def _operations(repo: Repo) -> dict[str, dict[str, str]]:
    """Each printobserver command, hyphenated, with its parameters' kinds by name."""
    described = json.loads(repo.read(OPERATIONS))
    return {
        operation["name"].replace("_", "-"): {
            parameter["name"]: parameter["kind"] for parameter in operation["parameters"]
        }
        for operation in described["operations"]
    }


def _steps(scenario: dict[str, Any]) -> Iterator[dict[str, Any]]:
    """Every step a scenario names, accepted or never."""
    for outcome in scenario["accept_any_of"]:
        yield from outcome
    yield from scenario["never"]


def real_prints_operations(repo: Repo) -> list[str]:
    """Every scenario step is a printobserver command, with that command's own parameters."""
    findings: list[str] = []
    operations = _operations(repo)
    for item in _readable(repo, "real-prints-operations"):
        if isinstance(item, str):
            findings.append(item)
            continue
        case, data = item
        for scenario in data["assertions"]["scenarios"]:
            where = f"{case.name}: scenario {scenario['id']}"
            for step in _steps(scenario):
                operation = step["operation"]
                parameters = operations.get(operation)
                if parameters is None:
                    findings.append(
                        f"{where} names operation {operation!r}, which {OPERATIONS} does not have"
                    )
                    continue
                for name, value in step.get("args", {}).items():
                    kind = parameters.get(name)
                    if kind is None:
                        findings.append(
                            f"{where} gives {operation} argument {name!r}, which is not one of its "
                            f"parameters in {OPERATIONS}"
                        )
                    elif (kind in {"number", "integer"}) != isinstance(value, int | float):
                        findings.append(
                            f"{where} gives {operation} argument {name!r} the value {value!r}, "
                            f"which a {kind} parameter cannot hold"
                        )
    return findings


def _credentials(table: dict[str, Any], prefix: str = "") -> Iterator[tuple[str, object]]:
    """Every credential-bearing field of a TOML table, by its dotted name."""
    for key, value in table.items():
        dotted = f"{prefix}{key}"
        if isinstance(value, dict):
            yield from _credentials(value, f"{dotted}.")
        elif key in CREDENTIAL_KEYS or "credential" in key:
            yield dotted, value


def real_prints_service_config(repo: Repo) -> list[str]:
    """The shared service configuration parses and carries no credential."""
    try:
        config = repo.read_toml(SERVICE_CONFIG)
    except (OSError, tomllib.TOMLDecodeError) as error:
        return [f"{SERVICE_CONFIG} does not parse: {error}"]
    return [
        f"{SERVICE_CONFIG}: {field} holds a value rather than {REDACTED!r}"
        for field, value in _credentials(config)
        if value != REDACTED
    ]
