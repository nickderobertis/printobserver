"""The real-print cases under `tests/real-prints`, held to their own claims.

Each case directory is evidence from a real printer — camera frames, the exact
pictures a supervision turn was given, the history printobserver recorded — and
a `case.json` saying what it shows and what an agent shown it should do. The
skill tests read that `case.json`'s `assertions` at run time, so a case that
names a file it does not carry, an event its history does not hold, or a
command printobserver does not have is a test that asserts nothing true.

Every check here reads files and nothing else: no model, network or printer.
The frames and G-code are Git LFS objects, so what a checkout carries for each
is a pointer until `just real-prints-fetch` downloads it: `real-prints-lfs`
holds every such file to being committed as one, and `real-prints-images` reads
a pointer's own object id as the sha256 of the picture it stands for.
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
import os
import re
import tomllib
from collections.abc import Iterator
from dataclasses import dataclass
from pathlib import Path
from typing import Any, NewType

import yaml
from contract_codegen.schemas import ContractError, load, read_schemas
from jsonschema import Draft202012Validator
from jsonschema.exceptions import SchemaError
from jsonschema.protocols import Validator

from repo_checks.model import Repo
from repo_checks.parsing import Recipe, jobs_of, load_workflow, recipes, run_commands, steps_of
from repo_checks.shell import discovered_no_higher_than, run

ROOT = "tests/real-prints"
SCHEMA = f"{ROOT}/case.schema.json"
CASE_FILE = "case.json"
HISTORY = "printobserver-history.json"
HISTORY_SCHEMA = "schemas/printobserver-server/HistoryAnswer.json"
SERVICE_CONFIG = f"{ROOT}/service-config.toml"
OPERATIONS = "schemas/printobserver-server/operations.json"
PRINTER_STATE = "schemas/printobserver-types/PrinterState.json"

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

#: Where the patterns Git LFS tracks are declared.
ATTRIBUTES = ".gitattributes"
#: The first line of every Git LFS pointer, and the most bytes one can hold.
POINTER_VERSION = "version https://git-lfs.github.com/spec/v1"
POINTER_LIMIT = 1024
POINTER_OID = re.compile(r"oid sha256:(?P<oid>[0-9a-f]{64})")
POINTER_SIZE = re.compile(r"size (?P<size>0|[1-9][0-9]*)")

#: The one list of steps a continuous-integration job fetches the cases with.
CI_STEPS = "tests/real-prints-lfs/ci-steps.yml"
#: The programs a command reaches the cases' LFS objects through; a recipe
#: reaches them when its body, or a recipe it calls, names one of these.
LFS_PROGRAMS = ("git lfs", "git-lfs")
#: A recipe a command line calls.
JUST_CALL = re.compile(r"\bjust (?P<recipe>[a-z][\w-]*)")

#: The triggers that happen during a print, whose position in its G-code is known.
POSITIONED_TRIGGERS = frozenset({"periodic_observation", "recorded_alert", "synthetic_alert"})

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


#: A Git LFS object id: the sha256 of the file a pointer stands for.
Oid = NewType("Oid", str)


@dataclass(frozen=True, slots=True)
class Pointer:
    """What a Git LFS pointer says of the object it stands for."""

    #: The object's sha256, which is the sha256 of the file it stands for.
    oid: Oid
    #: The object's size in bytes.
    size: int


def pointer(data: bytes) -> Pointer | None:
    """The Git LFS pointer `data` is, or none when it is any other bytes.

    A pointer is the three lines the specification orders — `version`, `oid`
    and `size` — each ending in a newline, and never more than
    `POINTER_LIMIT` bytes.
    """
    if len(data) > POINTER_LIMIT or not data.endswith(b"\n"):
        return None
    try:
        lines = data.decode("ascii").split("\n")[:-1]
    except UnicodeDecodeError:
        return None
    if len(lines) != 3 or lines[0] != POINTER_VERSION:
        return None
    oid = POINTER_OID.fullmatch(lines[1])
    size = POINTER_SIZE.fullmatch(lines[2])
    if oid is None or size is None:
        return None
    return Pointer(Oid(oid["oid"]), int(size["size"]))


def lfs_patterns(repo: Repo) -> tuple[str, ...]:
    """The patterns `.gitattributes` hands to the Git LFS filter, in its order."""
    path = repo.root / ATTRIBUTES
    if not path.is_file():
        return ()
    patterns: list[str] = []
    for line in path.read_text(encoding="utf-8").splitlines():
        fields = line.split()
        if fields and not fields[0].startswith("#") and "filter=lfs" in fields[1:]:
            patterns.append(fields[0])
    return tuple(patterns)


def _pattern(pattern: str) -> re.Pattern[str]:
    """A gitattributes pattern as a regular expression over a tree-relative path.

    A pattern carrying a slash is anchored at the root, `**/` matches any
    number of directories and `*` and `?` stay inside one; one carrying none
    matches a file's name in any directory. This is the reading for a tree that
    is no git work tree, where git cannot be asked; `test_real_prints.py`
    reconciles it with git's own over every file the committed tree carries.
    """
    anchored = "/" in pattern.rstrip("/")
    body = pattern.lstrip("/")
    translated = ""
    index = 0
    while index < len(body):
        if body.startswith("**/", index):
            translated += "(?:[^/]+/)*"
            index += 3
            continue
        match body[index]:
            case "*":
                translated += "[^/]*"
            case "?":
                translated += "[^/]"
            case literal:
                translated += re.escape(literal)
        index += 1
    return re.compile(translated if anchored else f"(?:.*/)?{translated}")


def lfs_tracked(repo: Repo, relative: str) -> bool:
    """Whether `.gitattributes` hands `relative` to the Git LFS filter, read without git."""
    return any(_pattern(pattern).fullmatch(relative) for pattern in lfs_patterns(repo))


def _work_tree(repo: Repo) -> dict[str, str] | None:
    """The environment git reads `repo` under, or none when it is no git work tree of its own.

    A copy of the tree with its history left behind is none, and neither is a
    directory inside some other checkout: git's search stops at `repo`.
    """
    environment = discovered_no_higher_than(repo.root, os.environ)
    top = run(["git", "rev-parse", "--show-toplevel"], cwd=repo.root, env=environment)
    if top.returncode != 0 or Path(top.stdout.strip()).resolve() != repo.root.resolve():
        return None
    return environment


def lfs_filtered(repo: Repo, relatives: list[str]) -> list[str]:
    """The paths of `relatives` git hands to the Git LFS filter, in their order.

    In a git work tree this is git's own answer (`git check-attr filter`),
    every attributes file and override included; in a tree that is none, the
    patterns the root `.gitattributes` declares, read by `lfs_tracked`.
    """
    environment = _work_tree(repo)
    if environment is None:
        return [relative for relative in relatives if lfs_tracked(repo, relative)]
    answered = run(
        ["git", "check-attr", "--stdin", "-z", "filter"],
        cwd=repo.root,
        env=environment,
        check=True,
        stdin="".join(f"{relative}\0" for relative in relatives),
    ).stdout.split("\0")[:-1]
    filtered = {
        path
        for path, _, value in zip(answered[0::3], answered[1::3], answered[2::3], strict=True)
        if value == "lfs"
    }
    return [relative for relative in relatives if relative in filtered]


def _committed(
    repo: Repo, environment: dict[str, str], relatives: list[str]
) -> dict[str, bytes | None]:
    """What the index holds for each path — none for a blob too large to be a pointer.

    Only a blob small enough to be a pointer is read, so a frame committed as
    raw bytes is answered without reading it. A path the index does not hold
    yet is not answered.
    """
    listing = run(
        ["git", "ls-files", "-s", "-z", "--", *relatives],
        cwd=repo.root,
        env=environment,
        check=True,
    ).stdout
    blobs: dict[str, str] = {}
    for entry in filter(None, listing.split("\0")):
        stage, _, name = entry.partition("\t")
        blobs[name] = stage.split()[1]
    sizes = run(
        ["git", "cat-file", "--batch-check=%(objectname) %(objectsize)"],
        cwd=repo.root,
        env=environment,
        check=True,
        stdin="".join(f"{blob}\n" for blob in blobs.values()),
    ).stdout.split()
    size_of = dict(zip(sizes[0::2], map(int, sizes[1::2]), strict=True))
    committed: dict[str, bytes | None] = {}
    for name, blob in blobs.items():
        if size_of[blob] > POINTER_LIMIT:
            committed[name] = None
            continue
        shown = run(
            ["git", "cat-file", "blob", blob], cwd=repo.root, env=environment, check=True
        ).stdout
        committed[name] = shown.encode("utf-8")
    return committed


def committed_pointers(repo: Repo, relatives: list[str]) -> dict[str, bytes]:
    """The pointer the index holds for each of `relatives` git hands to Git LFS.

    This is what a clone carries for such a file while `.lfsconfig` leaves it
    undownloaded, whatever the working tree holds after `just real-prints-fetch`.
    A path the index does not hold, or holds as anything but a pointer, is not
    answered; nor is any path of a tree that is no git work tree of its own.
    """
    environment = _work_tree(repo)
    if environment is None:
        return {}
    held = _committed(repo, environment, lfs_filtered(repo, relatives))
    return {
        name: data for name, data in held.items() if data is not None and pointer(data) is not None
    }


def real_prints_lfs(repo: Repo) -> list[str]:
    """Every case file git hands to Git LFS is committed as an LFS pointer.

    What is read is what git holds for the file — the index — rather than the
    working tree, which carries the real bytes once `just real-prints-fetch`
    has run. A host without git-lfs adds a frame as its raw bytes, and this is
    what refuses that commit rather than a clone discovering it. A tree that is
    no git work tree of its own is read as the files it carries.
    """
    root = repo.root / ROOT
    if not root.is_dir():
        return []
    relatives = sorted(
        path.relative_to(repo.root).as_posix() for path in root.rglob("*") if path.is_file()
    )
    tracked = lfs_filtered(repo, relatives)
    if not tracked:
        return [f"{ATTRIBUTES} hands no file under {ROOT} to Git LFS"]
    environment = _work_tree(repo)
    committed = None if environment is None else _committed(repo, environment, tracked)
    findings: list[str] = []
    for relative in tracked:
        if committed is None:
            held = (repo.root / relative).read_bytes()
        elif relative not in committed:
            continue
        else:
            held = committed[relative]
        if held is None or pointer(held) is None:
            findings.append(
                f"{relative} is committed as its raw bytes, not as a Git LFS pointer: install "
                f"git-lfs (`just bootstrap`), then `git add --renormalize {relative}`"
            )
    return findings


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
    at: str
    trigger: str
    file_position_bytes: int | None
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
            at=raw["at"],
            trigger=raw["trigger"],
            file_position_bytes=raw.get("file_position_bytes"),
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
    replay: tuple[ScenarioId, ...]
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

    def mentions(self, name: str) -> bool:
        """Whether the case.json names a file anywhere, a name holding a space included."""
        return name in json.dumps(self.document, ensure_ascii=False)


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
        replay=tuple(ScenarioId(entry) for entry in document["assertions"].get("replay", ())),
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
        findings.extend(
            f"{case.name}: {path.name} is in the case but {CASE_FILE} names it nowhere"
            for path in sorted(case.directory.iterdir())
            if path.name != CASE_FILE and not case.mentions(path.name)
        )
    return findings


def real_prints_images(repo: Repo) -> list[str]:
    """Every `agent-*` image is the exact picture its event's history records.

    Every `agent-*` file but the turn's own `agent-turn.json` is a picture. One
    the checkout carries as a Git LFS pointer is the picture whose sha256 is the
    pointer's object id, which is how git-lfs names an object.
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
            data = path.read_bytes()
            stands_for = pointer(data)
            actual = stands_for.oid if stands_for else hashlib.sha256(data).hexdigest()
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


def printed_gcode(directory: Path) -> Path | None:
    """The G-code a case's print ran, when the case carries it.

    Its one `.gcode` that is not the `.clean.gcode` slice a fault was planted
    in. A case that printed a sibling's file names it with `../` and carries none.
    """
    printed = sorted(
        path
        for path in directory.glob("*.gcode")
        if path.is_file() and not path.name.endswith(".clean.gcode")
    )
    return printed[0] if printed else None


def stored_size(path: Path) -> int:
    """The size of the file a path stands for, read off its pointer where it holds one."""
    with path.open("rb") as held:
        head = held.read(POINTER_LIMIT + 1)
    stands_for = pointer(head)
    return stands_for.size if stands_for else path.stat().st_size


def _position_findings(case: Case) -> Iterator[str]:
    """Each way a scenario's file position is missing or outside its case's G-code."""
    gcode = printed_gcode(case.directory)
    size = stored_size(gcode) if gcode else None
    for scenario in case.scenarios:
        where = f"{case.name}: scenario {scenario.id}"
        position = scenario.file_position_bytes
        if position is None:
            if size is not None and scenario.trigger in POSITIONED_TRIGGERS:
                yield (
                    f"{where} is a {scenario.trigger} of a case carrying its G-code and "
                    "names no file_position_bytes"
                )
            continue
        if size is None:
            yield f"{where} names a file_position_bytes, but the case carries no G-code"
        elif position >= size:
            yield (
                f"{where} has file_position_bytes {position}, past the end of "
                f"{gcode.name if gcode else 'its G-code'} at {size} bytes"
            )


def _replay_findings(case: Case) -> Iterator[str]:
    """Each way a case's `replay` names a scenario it lacks, or leaves time order."""
    moments = {scenario.id: scenario.at for scenario in case.scenarios}
    previous: tuple[ScenarioId, str] | None = None
    for entry in case.replay:
        at = moments.get(entry)
        if at is None:
            yield f"{case.name}: replay names {entry}, which is no scenario of the case"
            continue
        if previous is not None and at <= previous[1]:
            yield (
                f"{case.name}: replay has {entry} at {at} after {previous[0]} at {previous[1]}, "
                "which is not strictly increasing time"
            )
        previous = (entry, at)


def real_prints_replay(repo: Repo) -> list[str]:
    """Every `replay` is in time order over its own scenarios; every position is in its file."""
    findings: list[str] = []
    for case in _readable(repo, "real-prints-replay", findings):
        findings.extend(_replay_findings(case))
        findings.extend(_position_findings(case))
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


def _ci_steps(repo: Repo) -> list[dict[str, Any]] | str:
    """The committed CI steps, or why they could not be read."""
    path = repo.root / CI_STEPS
    try:
        steps = yaml.safe_load(path.read_text(encoding="utf-8"))
    except (OSError, yaml.YAMLError) as error:
        return f"{CI_STEPS} could not be read: {error}"
    if not isinstance(steps, list) or not steps or not all(isinstance(s, dict) for s in steps):
        return f"{CI_STEPS} is not a list of steps"
    return steps


def _reaching(justfile: dict[str, Recipe]) -> frozenset[str]:
    """Every recipe whose run reaches git-lfs: its own body, or a recipe it calls or depends on."""
    reaching: set[str] = set()
    changed = True
    while changed:
        changed = False
        for name, recipe in justfile.items():
            if name in reaching:
                continue
            called = {found["recipe"] for line in recipe.body for found in JUST_CALL.finditer(line)}
            if (
                any(program in line for line in recipe.body for program in LFS_PROGRAMS)
                or (called | set(recipe.dependencies)) & reaching
            ):
                reaching.add(name)
                changed = True
    return frozenset(reaching)


def _reaches(command: str, reaching: frozenset[str]) -> bool:
    """Whether one run line reaches git-lfs, itself or through a recipe it calls."""
    return any(program in command for program in LFS_PROGRAMS) or any(
        found["recipe"] in reaching for found in JUST_CALL.finditer(command)
    )


def _split(
    steps: list[dict[str, Any]], block: list[dict[str, Any]]
) -> tuple[list[dict[str, Any]], list[dict[str, Any]]] | None:
    """The steps before and after the first verbatim run of `block`, or none if it has none."""
    for start in range(len(steps) - len(block) + 1):
        if steps[start : start + len(block)] == block:
            return steps[:start], steps[start + len(block) :]
    return None


def real_prints_lfs_ci(repo: Repo) -> list[str]:
    """No CI job downloads the cases but through the committed steps, and every checkout is bare.

    Every `actions/checkout` declares `lfs: false`, so no checkout downloads an
    LFS object — the action's default is the same today, and declaring it is
    what keeps a later default or a copied step from spending the account's LFS
    bandwidth on every job. A run line reaches the cases when it runs git-lfs,
    or calls a recipe that does, however many recipes down: which recipes do is
    read off the justfile, so `just skilltest`, which fetches before replaying,
    counts as surely as `just real-prints-fetch`. A job with such a line must
    carry `ci-steps.yml` verbatim, which restores the store from the Actions
    cache and downloads only on a miss, and may have none before those steps;
    after them the store is whole, and a fetch there downloads nothing.
    """
    canonical = _ci_steps(repo)
    if isinstance(canonical, str):
        return [canonical]
    justfile = repo.root / "justfile"
    reaching = (
        _reaching(recipes(justfile.read_text(encoding="utf-8")))
        if justfile.is_file()
        else frozenset()
    )
    findings: list[str] = []
    for path in repo.workflow_paths:
        for job_name, job in jobs_of(load_workflow(path)).items():
            where = f"{path.name}: job `{job_name}`"
            steps = steps_of(job)
            for step in steps:
                uses = step.get("uses")
                if not isinstance(uses, str) or uses.split("@", 1)[0] != "actions/checkout":
                    continue
                options = step.get("with")
                if not isinstance(options, dict) or options.get("lfs") is not False:
                    findings.append(
                        f"{where} checks out with `{uses}` and no `lfs: false`, so the checkout "
                        f"may download every LFS object: declare `lfs: false`"
                    )
            if not any(_reaches(command, reaching) for command in run_commands(job)):
                continue
            split = _split(steps, canonical)
            if split is None:
                findings.append(
                    f"{where} reaches the real-print cases' LFS objects without carrying "
                    f"{CI_STEPS} verbatim: fetch them through those steps"
                )
                continue
            findings.extend(
                f"{where} runs `{command}`, which reaches the cases' LFS objects, before "
                f"{CI_STEPS}: run it after those steps, once the store is whole"
                for command in run_commands({"steps": split[0]})
                if _reaches(command, reaching)
            )
    return findings
