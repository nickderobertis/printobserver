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

import pytest
from contract_codegen.examples import UNKNOWN_KIND, value_of
from contract_codegen.generate import write
from contract_codegen.model import ENVELOPE, AnyValue, Ref, Struct
from contract_codegen.schemas import (
    EVENT_KIND_MARKER,
    SCHEMAS_DIR,
    ContractError,
    load,
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


def test_a_marker_that_is_not_a_kind_name_is_refused(scratch: Callable[[], Path]) -> None:
    """A marker carrying something other than a name is not a kind."""
    copy = scratch()
    schema = _marked(THIRD_KIND, THIRD_TYPE)
    schema[EVENT_KIND_MARKER] = 7
    _declare(copy, THIRD_CRATE, THIRD_TYPE, schema)

    with pytest.raises(ContractError, match="is not a kind name"):
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


def test_an_envelope_the_table_cannot_read_is_refused(scratch: Callable[[], Path]) -> None:
    """A kind table needs a kind name and a payload of any form to read; without them it stops."""
    copy = scratch()
    path = copy / SCHEMAS_DIR / "printobserver-types" / f"{ENVELOPE}.json"
    schema = json.loads(path.read_text(encoding="utf-8"))
    schema["properties"]["payload"] = {"type": "string"}
    path.write_text(json.dumps(schema, indent=2) + "\n", encoding="utf-8")

    with pytest.raises(ContractError, match="is not a value of any form"):
        load(copy)


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
