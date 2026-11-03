"""The strings the Rust client names without enforcing what the contracts say of them.

Every alias the generator writes for a string is a bare `String` in the Rust
client, whatever the schema says the string must look like, so the emitter
writes a directive above each alias of a constrained string — and above none
of an unconstrained one. A manifest's `sheet` is a string the server reads
unvalidated on purpose, judging a blank one in its own check, and the schema
marks it `x-unchecked-text` so the generated field carries core's directive
too; the preflight read's `sheet`, which the server validates, is typed as the
sheet identifier itself. These hand the real reader the committed schemas and
copies changed one way at a time, and read what the real Rust emitter made of
each.
"""

from __future__ import annotations

import json
from collections.abc import Callable
from pathlib import Path
from typing import Any

from contract_codegen import emit_rust
from contract_codegen.model import Alias, Contract, Field, Ref, Struct
from contract_codegen.schemas import (
    SCHEMAS_DIR,
    STRING_CONSTRAINTS,
    UNCHECKED_TEXT_MARKER,
    ContractError,
    load,
)
from repo_checks.expect import contains, equal, truth

#: Where core's schemas are checked in, one file per declaration.
CORE = Path(SCHEMAS_DIR) / "printobserver-core"

#: The aliases of a constrained string the contracts declare today, and two
#: of a string nothing constrains, which a client alias loses nothing of.
CONSTRAINED = ("FileName", "MaterialName", "SheetId", "SpoolId", "Timestamp")
UNCONSTRAINED = ("EventSource", "RawBytes")


def _directed(emitted: str, declared: str, directive: str) -> bool:
    """Whether the generated line `declared` is written under `directive`.

    The attributes written between them — a field's `#[serde(...)]` — are
    passed over, since a directive covers the item they belong to.
    """
    lines = [line.strip() for line in emitted.splitlines()]
    for index, line in enumerate(lines):
        if line != declared:
            continue
        above = index - 1
        while above >= 0 and lines[above].startswith("#["):
            above -= 1
        if above >= 0 and lines[above] == directive:
            return True
    return False


def _alias(contract: Contract, name: str) -> Alias:
    """One declared alias."""
    declared = contract.by_name[name]
    if not isinstance(declared, Alias):
        message = f"`{name}` is not an alias: {declared!r}"
        raise AssertionError(message)
    return declared


def _sheet(contract: Contract) -> Field:
    """The manifest's `sheet` field."""
    declared = contract.by_name["JobManifest"]
    if not isinstance(declared, Struct):
        message = f"`JobManifest` is not an object: {declared!r}"
        raise AssertionError(message)
    return next(entry for entry in declared.fields if entry.name == "sheet")


def _rewritten(copy: Path, name: str, change: Callable[[dict[str, Any]], None]) -> None:
    """Change one declaration's schema in the file it is read from.

    `Any` at the deserialization boundary: a schema is whatever JSON reads.
    """
    path = next((copy / SCHEMAS_DIR).glob(f"*/{name}.json"))
    schema = json.loads(path.read_text(encoding="utf-8"))
    change(schema)
    path.write_text(json.dumps(schema, indent=2) + "\n", encoding="utf-8")


def test_every_alias_of_a_constrained_string_is_written_under_the_directive(
    scratch: Callable[[], Path],
) -> None:
    """A constrained string's alias is directed; an unconstrained one's is not."""
    contract = load(scratch())
    emitted = emit_rust.emit(contract)

    for name in CONSTRAINED:
        equal(_alias(contract, name).constrained, True, describing=f"{name} read as constrained")
        truth(
            _directed(emitted, f"pub type {name} = String;", emit_rust.ALIAS_DIRECTIVE),
            describing=f"{name}'s alias written under the directive",
        )
    for name in UNCONSTRAINED:
        equal(_alias(contract, name).constrained, False, describing=f"{name} read as free")
        equal(
            _directed(emitted, f"pub type {name} = String;", emit_rust.ALIAS_DIRECTIVE),
            False,
            describing=f"{name}'s alias written under the directive",
        )


def test_an_alias_whose_constraints_are_taken_away_is_written_bare(
    scratch: Callable[[], Path],
) -> None:
    """The directive follows the schema: a sheet identifier nothing constrains is plain text."""
    copy = scratch()

    def unconstrained(schema: dict[str, Any]) -> None:
        for keyword in STRING_CONSTRAINTS:
            schema.pop(keyword, None)

    _rewritten(copy, "SheetId", unconstrained)
    contract = load(copy)
    emitted = emit_rust.emit(contract)

    equal(_alias(contract, "SheetId").constrained, False, describing="SheetId read as free")
    equal(
        _directed(emitted, "pub type SheetId = String;", emit_rust.ALIAS_DIRECTIVE),
        False,
        describing="SheetId's alias written under the directive",
    )


def test_the_manifest_sheet_is_unchecked_text_and_written_under_the_directive(
    scratch: Callable[[], Path],
) -> None:
    """The marked field carries core's directive, and loses it with its marker."""
    contract = load(scratch())
    equal(_sheet(contract).unchecked_text, True, describing="JobManifest.sheet's marker")
    truth(
        _directed(
            emit_rust.emit(contract), "pub sheet: Option<String>,", emit_rust.UNCHECKED_DIRECTIVE
        ),
        describing="JobManifest.sheet written under the directive",
    )

    copy = scratch()

    def unmarked(schema: dict[str, Any]) -> None:
        del schema["properties"]["sheet"][UNCHECKED_TEXT_MARKER]

    _rewritten(copy, "JobManifest", unmarked)
    bare = load(copy)
    equal(_sheet(bare).unchecked_text, False, describing="the unmarked field's marker")
    equal(
        _directed(
            emit_rust.emit(bare), "pub sheet: Option<String>,", emit_rust.UNCHECKED_DIRECTIVE
        ),
        False,
        describing="the unmarked field written under the directive",
    )


def test_the_preflight_sheet_is_typed_as_the_sheet_identifier() -> None:
    """The server validates the read's sheet, so the method takes the identifier's alias."""
    contract = load(Path(__file__).resolve().parents[3])
    preflight = next(one for one in contract.operations if one.name == "preflight")
    sheet = next(one for one in preflight.parameters if one.name == "sheet")

    equal(sheet.type, Ref("SheetId"), describing="the preflight sheet's type")
    contains(
        emit_rust.emit(contract),
        "sheet: Option<SheetId>",
        describing="the generated preflight method",
    )


def test_an_unchecked_text_marker_off_a_string_is_refused_naming_the_field(
    scratch: Callable[[], Path],
) -> None:
    """A marker that is not `true`, or sits on what is not text, refuses the contract."""
    for declaration, name, value in (
        ("JobManifest", "sheet", "yes"),
        ("JobManifest", "nozzle_diameter_mm", True),
    ):
        copy = scratch()

        def marked(schema: dict[str, Any], name: str = name, value: object = value) -> None:
            schema["properties"][name][UNCHECKED_TEXT_MARKER] = value

        _rewritten(copy, declaration, marked)
        try:
            load(copy)
        except ContractError as refused:
            contains(str(refused), f"`{name}`", describing=f"{name} marked {value!r}")
        else:
            equal(f"{name} marked {value!r}", "refused", describing="a malformed marker")
