"""One canonical value of every shape, built from the shape itself.

The walk each client's generated tier runs needs two things the schemas can
give it and nobody should write by hand: a document of each answer shape for
the stub host to answer, and a value of each request shape for the method to
be called with. Both are built here, from the same model the types are
generated from — so a shape that gains a field gains it in the walk as well,
and a walk cannot fall behind the client it drives.

The values are deliberately dull and fixed. Nothing here is asserted *about*;
what the walk asserts is that the client sent what the operation declares and
answered, field for field, what the host sent.
"""

from __future__ import annotations

from contract_codegen.model import (
    Alias,
    Contract,
    Enumeration,
    Field,
    ListOf,
    MapOf,
    Nullable,
    Operation,
    Ref,
    Scalar,
    Struct,
    TypeExpr,
    Union,
)

#: The one string every string-shaped value takes. It is a well-formed
#: identifier so that a value used as a path segment needs no escaping, and it
#: is the same everywhere so that a walk reads as one value travelling rather
#: than as a set of distinct ones.
TEXT = "0198f0a1-2b3c-7d4e-8f90-123456789abc"

#: The one number every number-shaped value takes. Exact in binary, so it
#: survives three languages' JSON writers unchanged.
NUMBER = 1.5

#: The one whole number every integer-shaped value takes.
INTEGER = 7

#: What a shape that refers to itself is given. The contracts declare no such
#: shape today; a value is still needed, because a builder that recursed
#: forever would hang the generator rather than report anything.
CYCLE: dict[str, object] = {}


class ExampleError(ValueError):
    """A shape no canonical value can be built for."""


def value_of(shape: TypeExpr, contract: Contract, seen: frozenset[str] = frozenset()) -> object:
    """One canonical value of one shape.

    Raises:
        ExampleError: If the shape names a type the contract does not declare.
    """
    match shape:
        case Ref(name):
            if name in seen:
                return CYCLE
            declaration = contract.by_name.get(name)
            if declaration is None:
                msg = f"`{name}` is not a type the contract declares"
                raise ExampleError(msg)
            return _declared(declaration, contract, seen | {name})
        case Scalar("string"):
            return TEXT
        case Scalar("number"):
            return NUMBER
        case Scalar("integer"):
            return INTEGER
        case Scalar("boolean"):
            return True
        case Scalar(kind):
            msg = f"`{kind}` is not a scalar a canonical value can be built for"
            raise ExampleError(msg)
        case ListOf(item):
            return [value_of(item, contract, seen)]
        case MapOf(item):
            return {"feedrate": value_of(item, contract, seen)}
        case Nullable(inner):
            return value_of(inner, contract, seen)
    msg = f"{shape!r} is not a shape a canonical value can be built for"
    raise ExampleError(msg)


def _fields(
    fields: tuple[Field, ...], contract: Contract, seen: frozenset[str]
) -> dict[str, object]:
    """A canonical value of every property of one object shape.

    Every property, optional ones included: a client that dropped a value the
    answer carried would be one the walk's own equality catches, and it can
    only catch it for a value that was there.
    """
    return {entry.name: value_of(entry.type, contract, seen) for entry in fields}


def _declared(declaration: object, contract: Contract, seen: frozenset[str]) -> object:
    """One canonical value of one named type.

    Raises:
        ExampleError: If the declaration is one no value can be built for.
    """
    match declaration:
        case Alias(_, target, _):
            return value_of(target, contract, seen)
        case Enumeration(name, _, values):
            if not values:
                msg = f"`{name}` admits no value at all"
                raise ExampleError(msg)
            return values[0][0]
        case Struct(_, _, fields, variants, tag_field) if tag_field:
            arm = variants[0]
            return {
                **_fields(fields, contract, seen),
                tag_field: arm.tag,
                **_fields(arm.fields, contract, seen),
            }
        case Struct(_, _, fields, _, _):
            return _fields(fields, contract, seen)
        case Union(name, _, variants, tag_field):
            if not variants:
                msg = f"`{name}` admits no arm at all"
                raise ExampleError(msg)
            arm = variants[0]
            if tag_field:
                return {tag_field: arm.tag, **_fields(arm.fields, contract, seen)}
            if arm.unit:
                return arm.tag
            if arm.payload is not None:
                return {arm.tag: value_of(arm.payload, contract, seen)}
            return {arm.tag: _fields(arm.fields, contract, seen)}
    msg = f"{declaration!r} is not a declaration a canonical value can be built for"
    raise ExampleError(msg)


def answer_of(operation: Operation, contract: Contract) -> object:
    """The document a stub host answers one operation with."""
    return value_of(Ref(operation.answer), contract)


def rejection_of(operation: Operation, contract: Contract) -> object:
    """The document a stub host refuses one operation with.

    The policy's own refusal, which is the same shape as an acceptance carrying
    a decision that is a refusal — and the one refusal that carries a value
    asked for and a range allowed, because that is the one a caller acts on.
    """
    answered = answer_of(operation, contract)
    if not isinstance(answered, dict):
        msg = f"`{operation.name}` answers something a refusal cannot be built from"
        raise ExampleError(msg)
    record = answered.get("record")
    if not isinstance(record, dict):
        msg = f"`{operation.name}` answers no record for a refusal to be taken on"
        raise ExampleError(msg)
    return {
        **answered,
        "record": {
            **record,
            "decision": {
                "rejected": {
                    "out_of_bounds": {
                        "adjustable": value_of(Ref("Adjustable"), contract),
                        "requested": NUMBER,
                        "allowed": {"min": 0.5, "max": 1.25},
                    }
                }
            },
        },
    }


def argument_of(shape: TypeExpr, contract: Contract) -> object:
    """One canonical value a generated method is called with."""
    return value_of(shape, contract)
