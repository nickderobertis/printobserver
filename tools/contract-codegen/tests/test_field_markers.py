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
    ContractError,
    load,
)
from repo_checks.expect import contains, equal

#: Where core's schemas are checked in, one file per declaration: the file the
#: reader resolves each declaration from.
CORE = Path(SCHEMAS_DIR) / "printobserver-core"

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
    for declaration, name, marker in MARKED:
        own = copy / CORE / f"{declaration}.json"
        alone = json.loads(own.read_text(encoding="utf-8"))
        del alone["properties"][name][marker]
        own.write_text(json.dumps(alone, indent=2) + "\n", encoding="utf-8")

    contract = load(copy)
    emitted = emit_rust.emit(contract)

    for declaration, name, _ in MARKED:
        read = _field(contract, declaration, name)
        equal(read.paired_with, "", describing=f"{declaration}.{name}'s pairing")
        equal(read.two_valued_fact, False, describing=f"{declaration}.{name}'s marker")
        equal(_directed(emitted, name), False, describing=f"{declaration}.{name}'s directive")


def _marked(copy: Path, declaration: str, name: str, marker: str, value: object) -> None:
    """Set one property's marker to `value`, in the file the declaration is read from."""
    path = copy / CORE / f"{declaration}.json"
    schema = json.loads(path.read_text(encoding="utf-8"))
    schema["properties"][name][marker] = value
    path.write_text(json.dumps(schema, indent=2) + "\n", encoding="utf-8")


def test_a_marker_no_reading_could_hold_is_refused_naming_the_field(
    scratch: Callable[[], Path],
) -> None:
    """A pairing naming no sibling, and a two-valued mark off a boolean, refuse the contract."""
    for declaration, name, marker, value in (
        ("ActionAllowance", "allowed", PAIRED_WITH_MARKER, 7),
        ("ActionAllowance", "allowed", PAIRED_WITH_MARKER, "nothing_here"),
        ("ActionAllowance", "allowed", PAIRED_WITH_MARKER, "allowed"),
        ("PausedPrint", "resumable", TWO_VALUED_FACT_MARKER, "yes"),
        ("PausedPrint", "print_id", TWO_VALUED_FACT_MARKER, True),
    ):
        copy = scratch()
        _marked(copy, declaration, name, marker, value)
        try:
            load(copy)
        except ContractError as refused:
            contains(str(refused), f"`{name}`", describing=f"{marker} {value!r}")
        else:
            equal(f"{marker} {value!r}", "refused", describing="a malformed marker")
