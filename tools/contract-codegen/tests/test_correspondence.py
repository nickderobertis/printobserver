"""The committed clients are what this generator writes from the committed schemas.

This is the half of the correspondence that is about the *generator*: run over
a scratch copy of the tree whose generated files have been thrown away, it
writes back exactly what the committed tree carries. The other half — that a
committed check refuses a tree in which one of them has been edited — is in
`repo-checks`, because that check is what the gate runs.
"""

from __future__ import annotations

import json
from collections.abc import Callable
from pathlib import Path

import pytest
from conftest import REPO_ROOT
from contract_codegen.generate import OUTPUTS, Output, drifted, write
from contract_codegen.schemas import TYPES_DIR, ContractError, load
from repo_checks.expect import accepted, contains, equal, truth

#: The type whose schema the alteration journeys move, and a field of it that
#: every one of the three clients carries.
ALTERED_TYPE = "ImageRecord"


def test_the_committed_clients_are_what_the_generator_writes(
    scratch: Callable[[], Path],
) -> None:
    """Regenerating over the committed tree leaves every generated file alone."""
    copy = scratch()

    equal(write(copy), [], describing="the files regeneration changed")


@pytest.mark.parametrize("output", OUTPUTS, ids=lambda output: output.client)
def test_each_client_is_regenerated_into_a_scratch_tree_as_committed(
    output: Output, scratch: Callable[[], Path]
) -> None:
    """A client thrown away and written again is byte-for-byte the committed one."""
    path = output.path
    committed = (REPO_ROOT / path).read_text(encoding="utf-8")
    copy = scratch()
    (copy / path).write_text("this is not what the generator writes\n", encoding="utf-8")

    changed = write(copy)

    contains(changed, path, describing="the files regeneration wrote")
    equal((copy / path).read_text(encoding="utf-8"), committed, describing=path)


def test_the_generator_accepts_the_committed_tree(scratch: Callable[[], Path]) -> None:
    """The tree this repository ships is one the drift walk finds nothing in."""
    accepted(drifted(scratch()))


def _schema(root: Path, name: str) -> tuple[dict[str, object], dict[str, object], list[str]]:
    """One checked-in contract schema of the copy, opened up to be altered.

    Its properties and its required names are handed back beside it, narrowed,
    so that a journey altering one shape says what it altered rather than
    reaching through a value nothing has typed.

    Raises:
        AssertionError: If the schema is not the object shape these journeys
            alter, which is a fixture that has gone stale.
    """
    schema = json.loads((root / TYPES_DIR / f"{name}.json").read_text(encoding="utf-8"))
    properties = schema.get("properties") if isinstance(schema, dict) else None
    required = schema.get("required") if isinstance(schema, dict) else None
    if (
        not isinstance(schema, dict)
        or not isinstance(properties, dict)
        or not isinstance(required, list)
    ):
        msg = f"{name} is not an object schema with properties and required names"
        raise AssertionError(msg)
    return schema, properties, required


def _write_schema(root: Path, name: str, schema: dict[str, object]) -> None:
    """Put one altered schema back where the generator reads it."""
    (root / TYPES_DIR / f"{name}.json").write_text(
        json.dumps(schema, indent=2, sort_keys=True) + "\n", encoding="utf-8"
    )


#: The three generated files carrying the clients' request and response
#: **types**. The generated walks are written from the same schemas, but what
#: moves in one when a shape moves is a document rather than a declaration, so
#: the shape journeys below read these.
TYPE_MODULES = (
    "crates/printobserver-sdk/src/contract.rs",
    "python/printobserver-sdk/src/printobserver_sdk/contract.py",
    "npm/printobserver-sdk/src/contract.ts",
)


def _regenerated(root: Path) -> dict[str, str]:
    """The three generated type modules of one copy, after regenerating it."""
    write(root)
    return {path: (root / path).read_text(encoding="utf-8") for path in TYPE_MODULES}


def _committed() -> dict[str, str]:
    """The three generated type modules of the committed tree."""
    return {path: (REPO_ROOT / path).read_text(encoding="utf-8") for path in TYPE_MODULES}


#: How a comment opens in each of the three languages. A description the
#: contracts wrote moves with the field it is about, and what a shape journey
#: is judging is the declaration rather than the sentence above it.
COMMENT_MARKERS = ("///", "//", "#", "*", "/**", "*/", '"""')


def _declarations(text: str) -> set[str]:
    """Every line of one generated file that declares something."""
    return {
        line
        for line in text.splitlines()
        if line.strip() and not line.strip().startswith(COMMENT_MARKERS)
    }


def _moved(before: str, after: str) -> tuple[set[str], set[str]]:
    """The declarations one generated file gained and lost."""
    was = _declarations(before)
    now = _declarations(after)
    return now - was, was - now


def test_a_required_field_added_to_a_schema_moves_every_client(
    scratch: Callable[[], Path],
) -> None:
    """A field the contracts gain appears in all three clients and nowhere else."""
    copy = scratch()
    schema, properties, required = _schema(copy, ALTERED_TYPE)
    properties["captured_by"] = {"description": "What captured it.", "type": "string"}
    required.append("captured_by")
    _write_schema(copy, ALTERED_TYPE, schema)

    written = _regenerated(copy)

    for path, before in _committed().items():
        gained, lost = _moved(before, written[path])
        truth(
            any("captured_by" in line for line in gained),
            describing=f"{path} to gain the field the schema gained",
        )
        truth(
            all("captured_by" in line for line in gained),
            describing=f"{path} to declare nothing but that field: it gained {sorted(gained)}",
        )
        truth(
            not lost,
            describing=f"{path} to lose nothing: it lost {sorted(lost)}",
        )


def test_a_field_renamed_in_a_schema_is_renamed_in_every_client(
    scratch: Callable[[], Path],
) -> None:
    """A rename moves exactly the lines that field was on, in all three."""
    copy = scratch()
    schema, properties, required = _schema(copy, ALTERED_TYPE)
    properties["digest"] = properties.pop("sha256")
    schema["required"] = ["digest" if name == "sha256" else name for name in required]
    _write_schema(copy, ALTERED_TYPE, schema)

    written = _regenerated(copy)

    for path, before in _committed().items():
        # Counted rather than differenced: the old name is a field of another
        # contract type as well, so the line declaring it does not disappear —
        # what moves is how many times this client declares it.
        truth(
            written[path].count("digest") > before.count("digest"),
            describing=f"{path} to carry the field's new name",
        )
        truth(
            written[path].count("sha256") < before.count("sha256"),
            describing=f"{path} to declare the field's old name less often",
        )


def test_a_fields_type_changed_in_a_schema_is_changed_in_every_client(
    scratch: Callable[[], Path],
) -> None:
    """A field that becomes a number is a number in all three clients."""
    copy = scratch()
    schema, properties, _ = _schema(copy, ALTERED_TYPE)
    properties["sha256"] = {"description": "The digest of its bytes.", "type": "number"}
    _write_schema(copy, ALTERED_TYPE, schema)

    written = _regenerated(copy)

    declared = {
        "crates/printobserver-sdk/src/contract.rs": "pub sha256: f64,",
        "python/printobserver-sdk/src/printobserver_sdk/contract.py": "sha256: float",
        "npm/printobserver-sdk/src/contract.ts": "sha256: number;",
    }
    for path, spelled in declared.items():
        contains(written[path], spelled, describing=path)


def test_a_schema_the_generator_was_never_taught_is_refused(
    scratch: Callable[[], Path],
) -> None:
    """A shape no client can be generated for stops the generator naming it."""
    copy = scratch()
    schema, properties, _ = _schema(copy, ALTERED_TYPE)
    properties["sha256"] = {
        "anyOf": [{"type": "string"}, {"type": "number"}],
        "description": "Two shapes with nothing to tell them apart.",
    }
    _write_schema(copy, ALTERED_TYPE, schema)

    with pytest.raises(ContractError, match="anyOf"):
        load(copy)


def test_the_generators_own_command_surface_writes_and_checks(
    scratch: Callable[[], Path],
) -> None:
    """`write` and `check` are what the recipe and the gate run."""
    from contract_codegen.__main__ import main

    copy = scratch()

    equal(main(["check", "--root", str(copy)]), 0, describing="the committed tree")
    equal(main(["write", "--root", str(copy)]), 0, describing="regenerating it")

    (copy / TYPE_MODULES[0]).write_text("a line somebody wrote by hand\n", encoding="utf-8")
    equal(main(["check", "--root", str(copy)]), 1, describing="a hand-edited client")
    equal(main(["write", "--root", str(copy)]), 0, describing="writing it back")
    equal(main(["check", "--root", str(copy)]), 0, describing="the client written back")
