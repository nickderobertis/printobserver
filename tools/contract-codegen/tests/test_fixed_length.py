"""A list the schema fixes at one length, read and written as Rust's array.

A rise's two heights and a bed area's four edges are lists whose `minItems` and
`maxItems` agree: a fixed number of values that mean something only together.
These hand the real reader a copy of the committed schemas, and one whose bound
was loosened, and read what the real Rust emitter made of each field.
"""

from __future__ import annotations

import json
from collections.abc import Callable
from pathlib import Path

from contract_codegen import emit_rust
from contract_codegen.model import Contract, Field, ListOf, Struct
from contract_codegen.schemas import SCHEMAS_DIR, load
from repo_checks.expect import contains, equal, truth

#: Where core's schemas are checked in, one file per declaration.
CORE = Path(SCHEMAS_DIR) / "printobserver-core"

#: Each fixed-length field, as the generated Rust client declares it.
FIXED = ("pub mm: [f64; 2],", "pub objects: Option<Vec<[i64; 4]>>,")


def _field(contract: Contract, declaration: str, name: str) -> Field:
    """One field of one declared object."""
    declared = contract.by_name[declaration]
    if not isinstance(declared, Struct):
        message = f"`{declaration}` is not an object: {declared!r}"
        raise AssertionError(message)
    return next(entry for entry in declared.fields if entry.name == name)


def test_a_list_fixed_at_one_length_is_a_rust_array_with_no_directive(
    scratch: Callable[[], Path],
) -> None:
    """A rise's heights and each bed area are arrays, and carry no non-empty directive."""
    lines = [line.strip() for line in emit_rust.emit(load(scratch())).splitlines()]
    for declared in FIXED:
        contains(lines, declared)
        truth(
            lines[lines.index(declared) - 1] != emit_rust.NON_EMPTY_DIRECTIVE,
            describing=f"`{declared}` is written under the non-empty directive",
        )


def test_a_list_whose_bounds_differ_keeps_its_vec(scratch: Callable[[], Path]) -> None:
    """Loosen the rise's `maxItems` and its heights are a non-empty `Vec` again."""
    copy = scratch()
    rise = copy / CORE / "Rise.json"
    document = json.loads(rise.read_text(encoding="utf-8"))
    document["properties"]["mm"]["maxItems"] = 3
    rise.write_text(json.dumps(document, indent=2) + "\n", encoding="utf-8")

    loosened = _field(load(copy), "Rise", "mm").type
    if not isinstance(loosened, ListOf):
        message = f"the rise's heights are not a list: {loosened!r}"
        raise AssertionError(message)
    equal(loosened, ListOf(loosened.item, non_empty=True))
    equal(emit_rust.type_name(loosened), "Vec<f64>")
