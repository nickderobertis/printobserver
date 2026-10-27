"""The two field markers the Rust emitter carries a declaring field's directive over by.

A recovery allowance's `allowed` is held in step with its `refusal`, and a
paused print's `resumable` and a Z home's `sweeps_bed` are yes-or-no facts
whose two answers are both real. Each declaring field in core carries a
suppression saying so, and the schema marks it so that the generated Rust
client's field carries the same directive. These hand the real reader the
committed schemas and a copy whose markers were taken away, and read what the
real Rust emitter made of each.
"""

from __future__ import annotations

import json
from collections.abc import Callable
from pathlib import Path

from contract_codegen import emit_rust
from contract_codegen.model import Contract, Field, Struct
from contract_codegen.schemas import (
    PAIRED_WITH_MARKER,
    SCHEMAS_DIR,
    TWO_VALUED_FACT_MARKER,
    load,
)
from repo_checks.expect import equal

#: The core schema carrying every declaration these read under its `$defs`.
STATUS = Path(SCHEMAS_DIR) / "printobserver-core" / "RecoveryStatus.json"

#: Each marked field: its declaration, its name, and the marker it carries.
MARKED = (
    ("ActionAllowance", "allowed", PAIRED_WITH_MARKER),
    ("PausedPrint", "resumable", TWO_VALUED_FACT_MARKER),
    ("ZHome", "sweeps_bed", TWO_VALUED_FACT_MARKER),
)


def _field(contract: Contract, declaration: str, name: str) -> Field:
    """One field of one declared object."""
    declared = contract.by_name[declaration]
    if not isinstance(declared, Struct):
        message = f"`{declaration}` is not an object: {declared!r}"
        raise AssertionError(message)
    return next(entry for entry in declared.fields if entry.name == name)


def _directed(emitted: str, name: str) -> bool:
    """Whether the generated field `name` is written under the directive."""
    lines = emitted.splitlines()
    return any(
        line.strip() == f"pub {name}: bool,"
        and lines[index - 1].strip() == emit_rust.TWO_VALUED_DIRECTIVE
        for index, line in enumerate(lines)
        if index > 0
    )


def test_each_marked_field_is_read_as_marked_and_written_under_the_directive(
    scratch: Callable[[], Path],
) -> None:
    """Every field the schema marks is read with its marker and directed in the client."""
    contract = load(scratch())
    emitted = emit_rust.emit(contract)

    for declaration, name, marker in MARKED:
        read = _field(contract, declaration, name)
        if marker == PAIRED_WITH_MARKER:
            equal(read.paired_with, "refusal", describing=f"{declaration}.{name}'s pairing")
        else:
            equal(read.two_valued_fact, True, describing=f"{declaration}.{name}'s marker")
        equal(_directed(emitted, name), True, describing=f"{declaration}.{name}'s directive")


def test_without_the_markers_no_field_is_written_under_the_directive(
    scratch: Callable[[], Path],
) -> None:
    """Taking the markers away gives back plain fields, with nothing written above them."""
    copy = scratch()
    # Each declaration is checked in on its own and again under the status's
    # `$defs`, and the reader may take either.
    status = copy / STATUS
    schema = json.loads(status.read_text(encoding="utf-8"))
    for declaration, name, marker in MARKED:
        del schema["$defs"][declaration]["properties"][name][marker]
        own = status.with_name(f"{declaration}.json")
        alone = json.loads(own.read_text(encoding="utf-8"))
        del alone["properties"][name][marker]
        own.write_text(json.dumps(alone, indent=2) + "\n", encoding="utf-8")
    status.write_text(json.dumps(schema, indent=2) + "\n", encoding="utf-8")

    contract = load(copy)
    emitted = emit_rust.emit(contract)

    for declaration, name, _ in MARKED:
        read = _field(contract, declaration, name)
        equal(read.paired_with, "", describing=f"{declaration}.{name}'s pairing")
        equal(read.two_valued_fact, False, describing=f"{declaration}.{name}'s marker")
        equal(_directed(emitted, name), False, describing=f"{declaration}.{name}'s directive")
