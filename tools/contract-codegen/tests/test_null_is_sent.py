"""A field whose null is sent, kept apart from its absence in the Rust client.

The events answer's `print_ended` is absent from a read naming no print, null
while the print it names is open, and its end once it has ended. These hand the
real reader the committed schemas and a copy whose marker was taken away, and
read what the real Rust emitter made of each. That the generated client then
answers the null the supervisor sent is the live tier's to prove.
"""

from __future__ import annotations

import json
from collections.abc import Callable
from pathlib import Path

from contract_codegen import emit_rust
from contract_codegen.model import Contract, Field, Struct
from contract_codegen.schemas import NULL_IS_SENT_MARKER, SERVER_DIR, load
from repo_checks.expect import contains, equal

ANSWER = "EventsAnswer"
TRI_STATE = "pub print_ended: Option<Option<PrintEnded>>,"


def _fields(contract: Contract) -> dict[str, Field]:
    """The events answer's fields, by name."""
    declaration = contract.by_name[ANSWER]
    if not isinstance(declaration, Struct):
        message = f"`{ANSWER}` is not an object: {declaration!r}"
        raise AssertionError(message)
    return {entry.name: entry for entry in declaration.fields}


def test_the_marked_field_is_read_as_one_whose_null_is_sent(
    scratch: Callable[[], Path],
) -> None:
    """`print_ended` carries the marker, and `cursor`, always sent, is required."""
    fields = _fields(load(scratch()))

    equal(fields["print_ended"].null_is_sent, True, describing="print_ended's marker")
    equal(fields["print_ended"].required, False, describing="print_ended's requirement")
    equal(fields["cursor"].required, True, describing="cursor's requirement")
    equal(fields["cursor"].null_is_sent, False, describing="cursor's marker")


def test_the_rust_client_keeps_a_sent_null_apart_from_an_absence(
    scratch: Callable[[], Path],
) -> None:
    """Two options, read through the one helper that reads a null as sent."""
    emitted = emit_rust.emit(load(scratch()))

    contains(emitted, TRI_STATE)
    contains(emitted, f'deserialize_with = "{emit_rust.PRESENT}"')
    contains(emitted, f"fn {emit_rust.PRESENT}<'de, D, T>(")


def test_without_the_marker_the_field_reads_as_any_other_optional_one(
    scratch: Callable[[], Path],
) -> None:
    """Taking the marker away gives back the one option, and no helper nothing calls."""
    copy = scratch()
    path = copy / SERVER_DIR / f"{ANSWER}.json"
    schema = json.loads(path.read_text(encoding="utf-8"))
    del schema["properties"]["print_ended"][NULL_IS_SENT_MARKER]
    path.write_text(json.dumps(schema, indent=2) + "\n", encoding="utf-8")

    contract = load(copy)
    emitted = emit_rust.emit(contract)

    equal(_fields(contract)["print_ended"].null_is_sent, False, describing="the unmarked field")
    contains(emitted, "pub print_ended: Option<PrintEnded>,")
    equal(TRI_STATE in emitted, False, describing="a tri-state field in the unmarked client")
    equal(f"fn {emit_rust.PRESENT}<" in emitted, False, describing="the helper nothing calls")
