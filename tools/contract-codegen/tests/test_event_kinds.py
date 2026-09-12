"""The open event log, as the generator reads it and the three clients carry it.

Every journey here drives the real generator over a real copy of the committed
tree with its schema set altered the way a domain alters it: a payload marked
with a kind lands under a directory of its own, a type is claimed twice, a
kind is claimed twice, and the envelope carries a field of any form. What is
read back is the emitted client — the Python one imported and driven, the
other two read — rather than the model the emitters were written from.
"""

from __future__ import annotations

import importlib.util
import json
import sys
from collections.abc import Callable
from pathlib import Path
from types import ModuleType
from typing import Any

import pytest
from contract_codegen.examples import (
    UNKNOWN_KIND,
    ExampleError,
    envelope_of,
    known_event,
    value_of,
)
from contract_codegen.generate import write
from contract_codegen.model import ENVELOPE, AnyValue, Ref, Struct
from contract_codegen.schemas import (
    EVENT_KIND_MARKER,
    SCHEMAS_DIR,
    SERVER_DIR,
    ContractError,
    load,
    schema_files,
)
from contract_codegen.walk import event_step
from repo_checks.expect import contains, equal, truth
from repo_checks.shell import run

#: The directory a third domain checks its schemas in under, which no crate of
#: the workspace owns today.
THIRD_CRATE = "printobserver-third"

#: The payload that domain declares, and the kind it is written under.
THIRD_TYPE = "ThirdPayload"
THIRD_KIND = "third_kind"

#: The three generated modules, by client.
GENERATED = {
    "rust": "crates/printobserver-sdk/src/contract.rs",
    "python": "python/printobserver-sdk/src/printobserver_sdk/contract.py",
    "typescript": "npm/printobserver-sdk/src/contract.ts",
}


def _marked(kind: str, title: str) -> dict[str, object]:
    """One payload schema, marked with a kind, as an owning crate writes it."""
    return {
        "$schema": "https://json-schema.org/draft/2020-12/schema",
        "additionalProperties": False,
        "description": "What a third domain writes down.",
        "properties": {"seen": {"description": "What it saw.", "type": "string"}},
        "required": ["seen"],
        "title": title,
        "type": "object",
        EVENT_KIND_MARKER: kind,
    }


def _declare(root: Path, crate: str, name: str, schema: dict[str, object]) -> None:
    """Check one schema in under one crate's directory of the copy."""
    directory = root / SCHEMAS_DIR / crate
    directory.mkdir(parents=True, exist_ok=True)
    (directory / f"{name}.json").write_text(json.dumps(schema, indent=2) + "\n", encoding="utf-8")


def _rewrite(path: Path, alter: Callable[[dict[str, Any]], None]) -> None:
    """Alter one checked-in schema of a copy in place, the way a contract change lands."""
    schema = json.loads(path.read_text(encoding="utf-8"))
    alter(schema)
    path.write_text(json.dumps(schema, indent=2) + "\n", encoding="utf-8")


def _unmarked(copy: Path) -> None:
    """Strip every kind marker from a copy: a schema set declaring no event at all."""
    for path in schema_files(copy):
        schema = json.loads(path.read_text(encoding="utf-8"))
        if EVENT_KIND_MARKER in schema:
            del schema[EVENT_KIND_MARKER]
            path.write_text(json.dumps(schema, indent=2) + "\n", encoding="utf-8")


#: The envelope's schema and the schema of its `kind` type, in a copy.
ENVELOPE_FILE = f"{SCHEMAS_DIR}/printobserver-types/{ENVELOPE}.json"
KIND_TYPE_FILE = f"{SCHEMAS_DIR}/printobserver-types/EventKind.json"
HISTORY_ANSWER_FILE = f"{SERVER_DIR}/HistoryAnswer.json"


def _module(path: Path) -> ModuleType:
    """The generated Python module of one copy, imported from that copy."""
    surface = path.parent / "_surface.py"
    for name, file in (
        ("printobserver_sdk._surface", surface),
        ("printobserver_sdk.contract", path),
    ):
        spec = importlib.util.spec_from_file_location(name, file)
        if spec is None or spec.loader is None:  # pragma: no cover - a copy carries both
            msg = f"{file} is not importable"
            raise AssertionError(msg)
        module = importlib.util.module_from_spec(spec)
        sys.modules[name] = module
        spec.loader.exec_module(module)
    return sys.modules["printobserver_sdk.contract"]


def test_a_payload_marked_under_a_third_directory_is_emitted_by_every_client(
    scratch: Callable[[], Path],
) -> None:
    """A domain's payload is emitted as a standalone type, and its kind is in each table.

    No operation reaches the payload by reference: it is a root because it is
    marked, which is what lets a domain add a kind without the server's
    answer shapes changing.
    """
    copy = scratch()
    _declare(copy, THIRD_CRATE, THIRD_TYPE, _marked(THIRD_KIND, THIRD_TYPE))

    contract = load(copy)
    contains(
        [kind.name for kind in contract.event_kinds],
        THIRD_KIND,
        describing="the kinds the generator read",
    )
    written = write(copy)
    for client, path in GENERATED.items():
        contains(written, path, describing=f"the files regeneration wrote for {client}")

    rust = (copy / GENERATED["rust"]).read_text(encoding="utf-8")
    contains(rust, f"pub struct {THIRD_TYPE} {{", describing="the Rust client's declarations")
    contains(
        rust,
        f"impl EventPayloadKind for {THIRD_TYPE} {{\n"
        f'    const KIND: &\'static str = "{THIRD_KIND}";',
        describing="the Rust client's kind table",
    )

    typescript = (copy / GENERATED["typescript"]).read_text(encoding="utf-8")
    contains(
        typescript, f"export interface {THIRD_TYPE} {{", describing="the Node client's declarations"
    )
    contains(
        typescript, f"  {THIRD_KIND}: {THIRD_TYPE};", describing="the Node client's kind table"
    )
    answered = run(
        [
            "bun",
            "-e",
            f'import {{ EVENT_KINDS, payloadOf }} from "./{GENERATED["typescript"]}";'
            f'const event = {{ kind: "{THIRD_KIND}", payload: {{ seen: "a spool" }} }};'
            f'console.log(JSON.stringify([EVENT_KINDS.includes("{THIRD_KIND}"), '
            f'payloadOf(event, "{THIRD_KIND}"), payloadOf(event, "{UNKNOWN_KIND}")]));',
        ],
        cwd=copy,
        check=True,
    ).stdout
    equal(
        json.loads(answered),
        [True, {"seen": "a spool"}, None],
        describing="what the Node client's table and accessor answered",
    )

    module = _module(copy / GENERATED["python"])
    equal(
        module.EVENT_PAYLOAD_TYPES[THIRD_KIND].__name__,
        THIRD_TYPE,
        describing="the Python client's kind table",
    )
    event = {"kind": THIRD_KIND, "payload": {"seen": "a spool"}}
    equal(module.payload_of(event, THIRD_KIND), {"seen": "a spool"}, describing="a known kind")
    equal(module.payload_of(event, UNKNOWN_KIND), None, describing="an unknown kind")


def test_a_type_declared_under_two_directories_is_refused(scratch: Callable[[], Path]) -> None:
    """A type has one owner: the same name under two crates is a tree that came apart."""
    copy = scratch()
    _declare(copy, THIRD_CRATE, "ImageRef", _marked(THIRD_KIND, "ImageRef"))

    with pytest.raises(ContractError, match="`ImageRef` is declared under both"):
        load(copy)


def test_a_kind_declared_by_two_payloads_is_refused(scratch: Callable[[], Path]) -> None:
    """A kind has one owner: two payloads marked with one name is a collision."""
    copy = scratch()
    _declare(copy, THIRD_CRATE, THIRD_TYPE, _marked("port_failure", THIRD_TYPE))

    with pytest.raises(ContractError, match="the kind `port_failure` is declared by both"):
        load(copy)


def test_a_marker_that_is_not_a_string_is_refused(scratch: Callable[[], Path]) -> None:
    """A marker is the kind's name, and a name is a string."""
    copy = scratch()
    schema = _marked(THIRD_KIND, THIRD_TYPE)
    schema[EVENT_KIND_MARKER] = 7
    _declare(copy, THIRD_CRATE, THIRD_TYPE, schema)

    with pytest.raises(ContractError, match="is not a string"):
        load(copy)


@pytest.mark.parametrize("marker", ["", "Third-Kind", "third kind"])
def test_a_marker_that_is_not_a_kind_name_is_refused(
    marker: str, scratch: Callable[[], Path]
) -> None:
    """A marker outside the pattern the envelope's `kind` type declares is not a kind.

    The envelope's schema holds a record's `kind` to that pattern, so a payload
    marked outside it would be one no record could be read back under — and
    the refusal names the type whose pattern it failed, because that schema is
    where the rule is written.
    """
    copy = scratch()
    schema = _marked(THIRD_KIND, THIRD_TYPE)
    schema[EVENT_KIND_MARKER] = marker
    _declare(copy, THIRD_CRATE, THIRD_TYPE, schema)

    with pytest.raises(ContractError, match=r"is not a kind name.*`EventKind` declares"):
        load(copy)


def test_what_a_kind_name_is_comes_from_the_kind_types_own_schema(
    scratch: Callable[[], Path],
) -> None:
    """The generator restates no pattern: tightening `EventKind`'s refuses what it now excludes.

    Every kind this repository declares carries an underscore, so a pattern
    admitting none of them is one the committed set cannot pass — which is
    what proves the marker is held to the schema's rule and not to a copy.
    """
    copy = scratch()

    def tighten(schema: dict[str, Any]) -> None:
        schema["pattern"] = "^[a-z]+$"

    _rewrite(copy / KIND_TYPE_FILE, tighten)

    with pytest.raises(ContractError, match=r"is not a kind name.*\^\[a-z\]\+\$"):
        load(copy)


def test_a_kind_type_declaring_no_pattern_is_refused(scratch: Callable[[], Path]) -> None:
    """A kind type saying nothing about what a name is leaves nothing to hold a marker to."""
    copy = scratch()

    def strip(schema: dict[str, Any]) -> None:
        del schema["pattern"]

    _rewrite(copy / KIND_TYPE_FILE, strip)

    with pytest.raises(ContractError, match="`EventKind` declares no `pattern`"):
        load(copy)


def test_a_marked_payload_that_is_not_an_object_is_refused(scratch: Callable[[], Path]) -> None:
    """A payload type a client's table can name is an object shape."""
    copy = scratch()
    _declare(
        copy,
        THIRD_CRATE,
        THIRD_TYPE,
        {"title": THIRD_TYPE, "type": "string", EVENT_KIND_MARKER: THIRD_KIND},
    )

    with pytest.raises(ContractError, match="is not an object"):
        load(copy)


def test_the_envelope_carries_a_field_of_any_form_in_every_client(
    scratch: Callable[[], Path],
) -> None:
    """The envelope's payload is a value of any form, rendered as each language's own."""
    copy = scratch()
    contract = load(copy)
    envelope = contract.by_name[ENVELOPE]
    truth(isinstance(envelope, Struct), describing="the envelope to be an object shape")
    if not isinstance(envelope, Struct):  # pragma: no cover - asserted above
        return
    equal(
        {entry.name: entry.type for entry in envelope.fields}["payload"],
        AnyValue(),
        describing="what the envelope's payload is read as",
    )
    example = value_of(Ref(ENVELOPE), contract)
    equal(
        example["kind"] if isinstance(example, dict) else None,
        contract.event_kinds[0].name,
        describing="the kind the envelope's example is under",
    )
    rendered = {
        "rust": "pub payload: serde_json::Value,",
        "python": "payload: object",
        "typescript": "payload: unknown;",
    }
    for client, path in GENERATED.items():
        contains(
            (copy / path).read_text(encoding="utf-8"),
            rendered[client],
            describing=f"how the {client} client carries the payload",
        )


def test_an_envelope_whose_payload_has_a_form_is_refused(scratch: Callable[[], Path]) -> None:
    """A kind table needs a payload of any form to read; a typed one stops it."""
    copy = scratch()

    def type_the_payload(schema: dict[str, Any]) -> None:
        schema["properties"]["payload"] = {"type": "string"}

    _rewrite(copy / ENVELOPE_FILE, type_the_payload)

    with pytest.raises(ContractError, match="is not a value of any form"):
        load(copy)


def test_an_envelope_whose_kind_is_not_a_named_type_is_refused(
    scratch: Callable[[], Path],
) -> None:
    """A kind table needs the envelope's `kind` to be a named type: it says what a name is."""
    copy = scratch()

    def inline_the_kind(schema: dict[str, Any]) -> None:
        schema["properties"]["kind"] = {"type": "string"}

    _rewrite(copy / ENVELOPE_FILE, inline_the_kind)

    with pytest.raises(ContractError, match="is not a named type"):
        load(copy)


def test_an_envelope_that_is_not_an_object_is_refused(scratch: Callable[[], Path]) -> None:
    """A kind table reads `kind` and `payload` off an object; a string has neither."""
    copy = scratch()

    def flatten(schema: dict[str, Any]) -> None:
        for member in ("properties", "required", "additionalProperties", "$defs"):
            schema.pop(member, None)
        schema["type"] = "string"

    _rewrite(copy / ENVELOPE_FILE, flatten)

    with pytest.raises(ContractError, match="is not an object shape"):
        load(copy)


def test_a_set_declaring_no_kind_has_no_known_event_to_build(
    scratch: Callable[[], Path],
) -> None:
    """A known event is one under a declared kind, and a set declaring none has no such event.

    The generated walk asks for one only when the set declares a kind; the
    example builder refuses rather than inventing a kind the table would not
    hold.
    """
    copy = scratch()
    _unmarked(copy)
    contract = load(copy)
    equal(contract.event_kinds, (), describing="the kinds a set with no marker declares")
    equal(event_step(contract), None, describing="the walk's step over a set with no kind")

    with pytest.raises(ExampleError, match="declares no event kind"):
        known_event(contract)


def test_an_event_cannot_be_built_from_an_envelope_that_is_not_an_object(
    scratch: Callable[[], Path],
) -> None:
    """With no kind declared the loader lets a string envelope through; the builder does not."""
    copy = scratch()
    _unmarked(copy)

    def flatten(schema: dict[str, Any]) -> None:
        for member in ("properties", "required", "additionalProperties", "$defs"):
            schema.pop(member, None)
        schema["type"] = "string"

    _rewrite(copy / ENVELOPE_FILE, flatten)
    contract = load(copy)

    with pytest.raises(ExampleError, match="is not an object for an event"):
        envelope_of(contract, THIRD_KIND, {"seen": "a spool"})


def test_a_kind_table_no_read_lists_events_for_stops_the_walk(
    scratch: Callable[[], Path],
) -> None:
    """A set declaring kinds needs a read answering a list of the envelope to drive them."""
    copy = scratch()

    def one_event(schema: dict[str, Any]) -> None:
        schema["properties"]["events"] = {"$ref": f"#/$defs/{ENVELOPE}"}

    _rewrite(copy / HISTORY_ANSWER_FILE, one_event)
    contract = load(copy)

    with pytest.raises(ValueError, match="no operation answers a list of"):
        event_step(contract)


def test_the_walk_drives_the_kind_table_through_the_read_that_lists_events(
    scratch: Callable[[], Path],
) -> None:
    """The generated walk serves a known and an unknown kind through the events read."""
    contract = load(scratch())
    step = event_step(contract)
    truth(step is not None, describing="a step for the kind table")
    if step is None:  # pragma: no cover - asserted above
        return
    equal(step.known_kind, contract.event_kinds[0].name, describing="the known kind")
    equal(step.unknown["kind"], UNKNOWN_KIND, describing="the unknown kind")
    equal(
        step.answer[step.field],
        [step.known, step.unknown],
        describing="the events the answer carries",
    )
