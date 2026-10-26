"""The description's own version, and the one value it says may be repeated.

Version 2 of `operations.json` gave every parameter `repeated`. These journeys
hand the real reader the committed description and copies of it changed in one
way each, and the real emitters what that reader made of it: the version is
read and an unknown one refused, `repeated` is read rather than guessed and
held to the shape it describes, and the one repeated value is sent once per
item by all three clients.
"""

from __future__ import annotations

import json
from collections.abc import Callable
from pathlib import Path
from typing import Any

import pytest
from contract_codegen import emit_python, emit_rust, emit_typescript
from contract_codegen.model import ListOf, Scalar
from contract_codegen.schemas import (
    DESCRIPTION_VERSION,
    OPERATIONS_FILE,
    SERVER_DIR,
    ContractError,
    load,
)
from repo_checks.expect import contains, equal

#: A value of the committed surface a request may carry more than once.
REPEATED = ("printer_history", "kinds")

#: Every value of the committed surface a request may carry more than once.
EVERY_REPEATED = [("events", "kinds"), ("events", "exclude_kinds"), REPEATED]


def _described(copy: Path, alter: Callable[[dict[str, Any]], None]) -> None:
    """Alter a copy's description in place, the way a server change lands."""
    path = copy / SERVER_DIR / OPERATIONS_FILE
    description = json.loads(path.read_text(encoding="utf-8"))
    alter(description)
    path.write_text(json.dumps(description, indent=2) + "\n", encoding="utf-8")


def _parameter(description: dict[str, Any], operation: str, name: str) -> dict[str, Any]:
    """One described parameter of one operation."""
    for described in description["operations"]:
        if described["name"] == operation:
            for parameter in described["parameters"]:
                if parameter["name"] == name:
                    return parameter
    message = f"the description has no `{operation}.{name}`"
    raise AssertionError(message)


def test_the_committed_description_is_the_version_this_reader_reads(
    scratch: Callable[[], Path],
) -> None:
    """Version 2, and exactly one repeated value: the printer history's kinds."""
    copy = scratch()
    description = json.loads((copy / SERVER_DIR / OPERATIONS_FILE).read_text(encoding="utf-8"))
    equal(description["description_version"], DESCRIPTION_VERSION)
    equal(DESCRIPTION_VERSION, 2)

    contract = load(copy)
    repeated = [
        (operation.name, parameter)
        for operation in contract.operations
        for parameter in operation.parameters
        if parameter.repeated
    ]
    equal([(name, parameter.name) for name, parameter in repeated], EVERY_REPEATED)
    kinds = repeated[0][1]
    equal(kinds.located, "query")
    equal(kinds.carried, ListOf(Scalar("string")))


@pytest.mark.parametrize("version", [1, 3, None])
def test_a_description_at_another_version_is_refused(
    scratch: Callable[[], Path], version: int | None
) -> None:
    """A shape this reader was not written for is refused, naming both versions."""
    copy = scratch()

    def at(description: dict[str, Any]) -> None:
        if version is None:
            del description["description_version"]
        else:
            description["description_version"] = version

    _described(copy, at)

    with pytest.raises(ContractError, match="this generator reads version 2") as refused:
        load(copy)
    contains(str(refused.value), f"at version {version!r}")


def test_a_parameter_described_without_repeated_is_refused(scratch: Callable[[], Path]) -> None:
    """`repeated` is read, never assumed: a version-2 parameter without it is malformed."""
    copy = scratch()
    _described(copy, lambda description: _parameter(description, *REPEATED).pop("repeated"))

    with pytest.raises(ContractError, match="describes `kinds` with no `repeated` flag"):
        load(copy)


def test_a_query_list_described_as_not_repeated_is_refused(scratch: Callable[[], Path]) -> None:
    """A list a client would send as one value is refused rather than generated."""
    copy = scratch()

    def single(description: dict[str, Any]) -> None:
        _parameter(description, *REPEATED)["repeated"] = False

    _described(copy, single)

    with pytest.raises(ContractError, match="describes `kinds` as not repeated"):
        load(copy)


def test_a_scalar_described_as_repeated_is_refused(scratch: Callable[[], Path]) -> None:
    """One value a client would send as though it were many is refused too."""
    copy = scratch()

    def many(description: dict[str, Any]) -> None:
        _parameter(description, "printer_history", "limit")["repeated"] = True

    _described(copy, many)

    with pytest.raises(ContractError, match="describes `limit` as repeated"):
        load(copy)


def test_every_client_sends_the_repeated_value_once_per_item(
    scratch: Callable[[], Path],
) -> None:
    """The flag the reader read is what each emitter repeats the value by."""
    contract = load(scratch())

    contains(emit_python.emit(contract), "            for value in kinds:")
    contains(emit_typescript.emit(contract), "      for (const value of kinds) {")
    contains(emit_rust.emit(contract), "        for value in kinds.into_iter().flatten() {")
