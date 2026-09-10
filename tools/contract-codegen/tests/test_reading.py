"""Every shape no client can be generated for is refused, naming it.

A generator that guessed at a form it had never been taught would write three
clients that quietly disagreed with the contracts. Each journey here hands the
real reader one such form — the forms a contract could actually grow — and
asserts it stops rather than guesses.

The whole-tree journeys drive `load` over a real copy of the committed tree
with one schema broken, because that is how a contract grows a form: one file
at a time.
"""

from __future__ import annotations

import json
from collections.abc import Callable
from pathlib import Path

import pytest
from contract_codegen.examples import (
    CYCLE,
    ExampleError,
    answer_of,
    rejection_of,
    value_of,
)
from contract_codegen.model import (
    Contract,
    Declaration,
    Enumeration,
    Operation,
    Ref,
    Scalar,
    Union,
)
from contract_codegen.schemas import (
    OPERATIONS_FILE,
    SERVER_DIR,
    TYPES_DIR,
    ContractError,
    declaration_of,
    load,
    type_of,
    workspace_version,
)
from repo_checks.expect import equal, truth

#: Every shape a property could take that no client can be generated for, with
#: the words the refusal has to carry so a reader knows which one it met.
UNREADABLE_PROPERTIES = [
    ({"$ref": "https://example.invalid/Thing"}, "does not name one of the contracts"),
    ({"anyOf": [{"type": "string"}, {"type": "number"}]}, "anyOf"),
    ({"type": ["string", "integer"]}, "type list"),
    ({"type": "array"}, "no item shape"),
    ({"description": "a shape with no shape"}, "not one a property may take"),
    ({"type": "chimera"}, "not a scalar"),
    ({"type": "object"}, "neither a map nor a named shape"),
    (
        {
            "type": "object",
            "patternProperties": {"^a$": {"type": "string"}, "^b$": {"type": "number"}},
        },
        "different shapes",
    ),
]


@pytest.mark.parametrize(
    ("shape", "naming"), UNREADABLE_PROPERTIES, ids=range(len(UNREADABLE_PROPERTIES))
)
def test_a_property_shape_no_client_can_be_generated_for_is_refused(
    shape: dict[str, object], naming: str
) -> None:
    """A form the generator has never been taught stops it, naming the form."""
    with pytest.raises(ContractError, match=naming):
        type_of(shape)


def test_an_arm_that_is_not_a_shape_is_refused() -> None:
    """A union arm that is not an object is not one a client can carry."""
    with pytest.raises(ContractError, match="an arm that is not a shape"):
        declaration_of("Odd", {"oneOf": ["not a shape at all"]}, set())


def test_an_externally_tagged_arm_carrying_more_than_its_own_name_is_refused() -> None:
    """An arm outside its tag carries exactly one property: the tag itself."""
    two = {
        "oneOf": [
            {
                "type": "object",
                "properties": {"first": {"type": "string"}, "second": {"type": "string"}},
            }
        ]
    }

    with pytest.raises(ContractError, match="carries exactly one"):
        declaration_of("Odd", two, set())


def test_an_object_with_arms_that_are_not_tagged_inside_them_is_refused() -> None:
    """An object that is its own properties and a set of arms tags them inside."""
    both = {
        "type": "object",
        "properties": {"id": {"type": "string"}},
        "oneOf": [{"const": "one", "type": "string"}, {"const": "two", "type": "string"}],
    }

    with pytest.raises(ContractError, match="not tagged inside them"):
        declaration_of("Odd", both, set())


def test_an_operation_describing_no_answer_is_refused(scratch: Callable[[], Path]) -> None:
    """A client generated from a description with no answer would answer nothing."""
    copy = scratch()
    described = json.loads((copy / SERVER_DIR / OPERATIONS_FILE).read_text(encoding="utf-8"))
    described["operations"][0]["responses"] = []
    (copy / SERVER_DIR / OPERATIONS_FILE).write_text(json.dumps(described), encoding="utf-8")

    with pytest.raises(ContractError, match="describes no answer"):
        load(copy)


def test_an_answer_shape_no_schema_declares_is_refused(scratch: Callable[[], Path]) -> None:
    """A description naming a shape nothing declares is a tree that came apart."""
    copy = scratch()
    described = (copy / SERVER_DIR / OPERATIONS_FILE).read_text(encoding="utf-8")
    (copy / SERVER_DIR / OPERATIONS_FILE).write_text(
        described.replace('"StatusAnswer"', '"NoSuchAnswer"'), encoding="utf-8"
    )

    with pytest.raises(ContractError, match="NoSuchAnswer"):
        load(copy)


def test_a_schema_file_that_is_not_a_schema_is_refused(scratch: Callable[[], Path]) -> None:
    """A file that parses and is not an object is not a shape."""
    copy = scratch()
    (copy / TYPES_DIR / "ImageRecord.json").write_text("[]", encoding="utf-8")

    with pytest.raises(ContractError, match="is not a schema"):
        load(copy)


def test_a_workspace_declaring_no_version_is_refused(tmp_path: Path) -> None:
    """A client records the contract it was generated against, from the workspace."""
    (tmp_path / "Cargo.toml").write_text("[workspace]\nmembers = []\n", encoding="utf-8")

    with pytest.raises(ContractError, match="declares no version"):
        workspace_version(tmp_path)


def _contract(*declarations: Declaration) -> Contract:
    """A contract carrying nothing but the declarations one journey is about."""
    return Contract(
        version="0.0.0",
        version_prefix="/v1",
        media_type="application/json",
        declarations=declarations,
        by_name={declaration.name: declaration for declaration in declarations},
    )


def test_a_shape_that_refers_to_itself_is_given_a_value_rather_than_a_loop(
    scratch: Callable[[], Path],
) -> None:
    """A builder that recursed forever would hang the generator rather than report."""
    equal(value_of(Ref("Loop"), _contract(), frozenset({"Loop"})), CYCLE)


def test_a_value_of_a_type_the_contract_does_not_declare_is_refused() -> None:
    """A reference to nothing is a description and a schema set that came apart."""
    with pytest.raises(ExampleError, match="is not a type the contract declares"):
        value_of(Ref("NoSuchType"), _contract())


def test_a_scalar_no_value_can_be_built_for_is_refused() -> None:
    """A kind the builder has never been taught stops it, naming the kind."""
    with pytest.raises(ExampleError, match="chimera"):
        value_of(Scalar("chimera"), _contract())


def test_a_closed_set_admitting_nothing_is_refused() -> None:
    """A vocabulary with no words in it is one no value can be built of."""
    empty = Enumeration(name="Empty", doc="", values=())

    with pytest.raises(ExampleError, match="admits no value at all"):
        value_of(Ref("Empty"), _contract(empty))


def test_a_union_admitting_no_arm_is_refused() -> None:
    """A union with no arms is one no value can be built of."""
    empty = Union(name="Nothing", doc="", variants=())

    with pytest.raises(ExampleError, match="admits no arm at all"):
        value_of(Ref("Nothing"), _contract(empty))


def _operation(answer: str) -> Operation:
    """One operation answering the named shape, for a refusal to be built from."""
    return Operation(
        name="an_operation",
        method="POST",
        path="/v1/anything",
        effect="mutating:pause",
        answer=answer,
        rejection=answer,
        parameters=(),
        image_path_field="",
    )


def test_a_refusal_cannot_be_built_from_an_answer_that_is_not_a_record() -> None:
    """The policy's refusal is the record carrying its decision, or it is nothing."""
    text = Enumeration(name="Text", doc="", values=(("said", ""),))

    with pytest.raises(ExampleError, match="a refusal cannot be built from"):
        rejection_of(_operation("Text"), _contract(text))


def test_a_refusal_cannot_be_built_from_an_answer_carrying_no_record(
    scratch: Callable[[], Path],
) -> None:
    """An answer with no record — a history read's — is one no decision was taken on."""
    contract = load(scratch())
    truth(contract.by_name, describing="the contract to declare its own shapes")

    with pytest.raises(ExampleError, match="answers no record"):
        rejection_of(_operation("HistoryAnswer"), contract)


def test_every_answer_the_server_declares_has_a_canonical_value(
    scratch: Callable[[], Path],
) -> None:
    """The walk each client runs needs one of every shape, built from the shape."""
    contract = load(scratch())

    for operation in contract.operations:
        truth(
            answer_of(operation, contract) is not None,
            describing=f"a canonical value of what `{operation.name}` answers",
        )


def test_an_operation_with_no_place_in_the_real_walk_stops_the_generator(
    scratch: Callable[[], Path],
) -> None:
    """A new operation cannot arrive with nothing driving it against a machine.

    The walk against a real supervisor has an order, because a printer is a
    state machine — so an operation with no place in it is one the generator
    refuses to write a walk for at all, which is what the drift gate then
    reports.
    """
    from contract_codegen.walk import plan_live

    copy = scratch()
    described = json.loads((copy / SERVER_DIR / OPERATIONS_FILE).read_text(encoding="utf-8"))
    invented = {**described["operations"][0], "name": "reboot"}
    described["operations"].append(invented)
    (copy / SERVER_DIR / OPERATIONS_FILE).write_text(json.dumps(described), encoding="utf-8")

    with pytest.raises(ValueError, match="reboot"):
        plan_live(load(copy))


def test_a_value_the_real_walk_cannot_supply_stops_the_generator(
    scratch: Callable[[], Path],
) -> None:
    """A value a real machine has to be given cannot be guessed at."""
    from contract_codegen.walk import plan_live

    copy = scratch()
    described = json.loads((copy / SERVER_DIR / OPERATIONS_FILE).read_text(encoding="utf-8"))
    for operation in described["operations"]:
        if operation["name"] == "status":
            operation["parameters"].append(
                {
                    "name": "invented",
                    "required": True,
                    "located": "query",
                    "kind": "text",
                    "shape": {"type": "string"},
                }
            )
    (copy / SERVER_DIR / OPERATIONS_FILE).write_text(json.dumps(described), encoding="utf-8")

    with pytest.raises(ValueError, match="invented"):
        plan_live(load(copy))


def test_every_action_the_server_serves_is_driven_as_a_refusal(
    scratch: Callable[[], Path],
) -> None:
    """One refused call per method carrying an action of the vocabulary."""
    from contract_codegen.walk import plan_live, rejected_live

    contract = load(scratch())

    equal(
        {step.name for step in rejected_live(contract)},
        {operation.name for operation in contract.operations if operation.rejectable},
        describing="the actions the real walk drives as refusals",
    )
    equal(
        len(plan_live(contract)),
        len(contract.operations),
        describing="the operations the real walk drives",
    )
