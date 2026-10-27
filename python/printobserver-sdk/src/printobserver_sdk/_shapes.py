"""Whether a parsed document is of one declared type of the contracts.

What a type is comes from `SHAPES`, the table generated from the same
declarations as every type in `contract.py`; this module only walks it. An
object admits members it does not declare, as the Rust client's types do, and
an optional member is either left out or of its declared shape: one sent as
null is refused unless that shape is nullable. Every closed set admits only
what it declares: an enumeration's names, a union's arms, a tagged object's
tags.
"""

from __future__ import annotations

import math

from printobserver_sdk.contract import SHAPES


def conforms(value: object, declared: str) -> bool:
    """Whether `value` is a document of the type the contracts declare as `declared`."""
    return _declared(value, SHAPES.get(declared))


def _conforms(value: object, shape: object) -> bool:
    """Whether `value` is of one type expression of the table."""
    match shape:
        case {"ref": str() as name}:
            return _declared(value, SHAPES.get(name))
        case {"scalar": str() as kind}:
            return _scalar(value, kind)
        case {"list": item, "non_empty": bool() as non_empty}:
            return (
                isinstance(value, list)
                and (bool(value) or not non_empty)
                and all(_conforms(entry, item) for entry in value)
            )
        case {"map": inner}:
            return isinstance(value, dict) and all(
                isinstance(key, str) and _conforms(entry, inner) for key, entry in value.items()
            )
        case {"nullable": inner}:
            return value is None or _conforms(value, inner)
        case {"any": True}:
            return True
        case _:
            return False


def _scalar(value: object, kind: str) -> bool:
    """Whether `value` is one scalar of JSON: a number is finite, and no boolean is one."""
    match kind:
        case "string":
            return isinstance(value, str)
        case "null":
            return value is None
        case "boolean":
            return isinstance(value, bool)
        case "integer":
            return isinstance(value, int) and not isinstance(value, bool)
        case "number":
            if isinstance(value, bool) or not isinstance(value, int | float):
                return False
            try:
                return math.isfinite(value)
            except OverflowError:
                return False
        case _:
            return False


def _declared(value: object, declaration: object) -> bool:
    """Whether `value` is of one declaration of the table."""
    match declaration:
        case {"alias": target}:
            return _conforms(value, target)
        case {"enum": list() as names}:
            return isinstance(value, str) and value in names
        case {
            "struct": {
                "fields": dict() as fields,
                "tag_field": str() as tag_field,
                "variants": dict() as variants,
            }
        }:
            if not (isinstance(value, dict) and _fields(value, fields)):
                return False
            return not tag_field or _tagged(value, tag_field, variants)
        case {"union": {"tag_field": str() as tag_field, "variants": dict() as variants}}:
            if tag_field:
                return isinstance(value, dict) and _tagged(value, tag_field, variants)
            return _external(value, variants)
        case _:
            return False


def _fields(value: dict[object, object], fields: dict[object, object]) -> bool:
    """Whether an object carries each member its declaration requires, each of its type.

    A member paired with an optional sibling is false exactly when that sibling
    is sent, and the sibling is never sent as null.
    """
    for name, field in fields.items():
        match field:
            case {"shape": shape, "required": bool() as required, "paired_with": str() as pair}:
                pass
            case _:
                return False
        if pair and not _paired(value.get(name), value, pair):
            return False
        if name not in value:
            if required:
                return False
            continue
        member = value[name]
        if not _conforms(member, shape):
            return False
    return True


def _paired(flag: object, value: dict[object, object], pair: str) -> bool:
    """Whether a flag and the sibling it is held in step with agree."""
    if pair in value and value[pair] is None:
        return False
    return isinstance(flag, bool) and flag != (pair in value)


def _tagged(value: dict[object, object], tag_field: str, variants: dict[object, object]) -> bool:
    """Whether an object names one declared arm in `tag_field`, and carries that arm."""
    tag = value.get(tag_field)
    if not isinstance(tag, str):
        return False
    return _arm(value, variants.get(tag))


def _external(value: object, variants: dict[object, object]) -> bool:
    """Whether a value is one arm of an externally tagged union.

    An arm carrying nothing travels as its own name; any other travels as an
    object of exactly one member, its name, holding what it carries.
    """
    if isinstance(value, str):
        match variants.get(value):
            case {"unit": True}:
                return True
            case _:
                return False
    if not (isinstance(value, dict) and len(value) == 1):
        return False
    ((tag, carried),) = value.items()
    match variants.get(tag):
        case {"unit": False, "payload": dict() as payload}:
            return _conforms(carried, payload)
        case {"unit": False, "fields": dict() as fields}:
            return isinstance(carried, dict) and _fields(carried, fields)
        case _:
            return False


def _arm(value: dict[object, object], variant: object) -> bool:
    """Whether an object carries one arm of an internally tagged set beside its tag."""
    match variant:
        case {"payload": dict() as payload}:
            return _conforms(value, payload)
        case {"fields": dict() as fields}:
            return _fields(value, fields)
        case _:
            return False


__all__ = ["conforms"]
