"""Reading the checked-in schemas into the model the emitters are written from.

The schemas are the one source. `schemas/printobserver-types/` holds one file
per type the contracts declare and `schemas/printobserver-server/` holds one
per shape a route answers, plus the description of the operations themselves.
Every `$ref` in either resolves to a *file* rather than to the `$defs` copy
beside it, so altering a type's own file moves every client type built from it
— which is what makes the drift gate over the generated clients a gate over the
contracts rather than over a checksum.
"""

from __future__ import annotations

import json
import re
import tomllib
from pathlib import Path
from typing import Any

from contract_codegen.model import (
    Alias,
    Contract,
    Declaration,
    Enumeration,
    Field,
    ListOf,
    MapOf,
    Nullable,
    Operation,
    Parameter,
    Ref,
    Scalar,
    Struct,
    TypeExpr,
    Union,
    Variant,
)

#: Where the contracts' own types are checked in.
TYPES_DIR = "schemas/printobserver-types"

#: Where the shapes this server answers, and its operation list, are checked in.
SERVER_DIR = "schemas/printobserver-server"

#: The file the operation list is checked in under.
OPERATIONS_FILE = "operations.json"

#: The reference every named type is pointed at by.
REF_PREFIX = "#/$defs/"

#: The prefix an operation's effect carries when it asks for an action.
MUTATING = "mutating:"

#: How a rustdoc link is spelled in a description the contracts generated,
#: with or without the item it points at. The brackets and the target are a
#: Rust convention rather than part of the sentence, so both are taken off: two
#: of the three clients are not Rust, and the third would carry a link to a
#: type it may not have generated.
DOC_LINK = re.compile(r"\[(`[^`]+`)\](\([^)]*\))?")


class ContractError(ValueError):
    """The checked-in schemas say something a client cannot be generated from."""


def _read(path: Path) -> dict[str, Any]:
    """One checked-in schema."""
    with path.open(encoding="utf-8") as handle:
        parsed = json.load(handle)
    if not isinstance(parsed, dict):
        msg = f"{path} is not a schema"
        raise ContractError(msg)
    return parsed


def _doc(schema: dict[str, Any], known: set[str]) -> str:
    """The sentence the contracts give for a shape, as a client documents it.

    Every one of the contracts' own type names is put in code marks: a client
    is read by a linter that holds a documentation comment to naming code as
    code, and a description that named `ActionRecord` in prose would be one
    finding per generated client.
    """
    said = schema.get("description")
    if not isinstance(said, str):
        return ""
    said = DOC_LINK.sub(r"\1", said)
    for name in sorted(known, key=len, reverse=True):
        said = re.sub(rf"(?<![`\w]){re.escape(name)}(?![`\w])", f"`{name}`", said)
    return said


def _scalar(named: str) -> Scalar:
    """One JSON type as the model spells it."""
    if named not in {"string", "number", "integer", "boolean"}:
        msg = f"`{named}` is not a scalar a client can be generated for"
        raise ContractError(msg)
    return Scalar(named)


def type_of(schema: dict[str, Any]) -> TypeExpr:
    """The shape one property or item declares.

    Raises:
        ContractError: If the schema declares a shape no client can be
            generated for, which is a contract that has grown a form this
            generator has never been taught.
    """
    reference = schema.get("$ref")
    if isinstance(reference, str):
        if not reference.startswith(REF_PREFIX):
            msg = f"`{reference}` does not name one of the contracts' own types"
            raise ContractError(msg)
        return Ref(reference.removeprefix(REF_PREFIX))

    branches = schema.get("anyOf")
    if isinstance(branches, list):
        without_null = [
            branch
            for branch in branches
            if isinstance(branch, dict) and branch.get("type") != "null"
        ]
        if len(without_null) == len(branches) - 1 and len(without_null) == 1:
            return Nullable(type_of(without_null[0]))
        msg = f"an `anyOf` of {len(branches)} shapes is not one a property may take"
        raise ContractError(msg)

    declared = schema.get("type")
    if isinstance(declared, list):
        named = [entry for entry in declared if entry != "null"]
        if len(named) != 1 or "null" not in declared:
            msg = f"the type list {declared!r} is not one a property may take"
            raise ContractError(msg)
        return Nullable(_scalar(str(named[0])))
    if declared == "array":
        items = schema.get("items")
        if not isinstance(items, dict):
            msg = "an array property declares no item shape"
            raise ContractError(msg)
        return ListOf(type_of(items))
    if declared == "object":
        return MapOf(_map_value(schema))
    if isinstance(declared, str):
        return _scalar(declared)
    msg = f"the shape {json.dumps(schema)[:120]} is not one a property may take"
    raise ContractError(msg)


def _map_value(schema: dict[str, Any]) -> TypeExpr:
    """The shape the values of one keyed object take.

    Raises:
        ContractError: If the object declares properties rather than being a
            map, which is an anonymous shape a client would have to invent a
            name for.
    """
    additional = schema.get("additionalProperties")
    if isinstance(additional, dict):
        return type_of(additional)
    patterned = schema.get("patternProperties")
    if isinstance(patterned, dict) and patterned:
        shapes = {json.dumps(value, sort_keys=True) for value in patterned.values()}
        if len(shapes) != 1:
            msg = "a keyed object whose patterns admit different shapes"
            raise ContractError(msg)
        return type_of(next(iter(patterned.values())))
    msg = f"the object {json.dumps(schema)[:120]} is neither a map nor a named shape"
    raise ContractError(msg)


def _fields(schema: dict[str, Any], known: set[str], skip: str = "") -> tuple[Field, ...]:
    """Every property one object shape declares, in a stable order."""
    properties = schema.get("properties")
    required = schema.get("required")
    required_names = set(required) if isinstance(required, list) else set()
    if not isinstance(properties, dict):
        return ()
    return tuple(
        Field(
            name=name,
            type=type_of(property_schema),
            required=name in required_names,
            doc=_doc(property_schema, known),
        )
        for name, property_schema in sorted(properties.items())
        if name != skip and isinstance(property_schema, dict)
    )


def _internal_tag(branches: list[Any]) -> str:
    """The property every arm of an internally tagged union puts its tag in."""
    tags: set[str] = set()
    for branch in branches:
        if not isinstance(branch, dict):
            return ""
        properties = branch.get("properties")
        if not isinstance(properties, dict):
            return ""
        named = {
            name
            for name, shape in properties.items()
            if isinstance(shape, dict) and isinstance(shape.get("const"), str)
        }
        if len(named) != 1:
            return ""
        tags |= named
    return next(iter(tags)) if len(tags) == 1 else ""


def declaration_of(name: str, schema: dict[str, Any], known: set[str]) -> Declaration:
    """One named type, as the model spells it.

    Raises:
        ContractError: If the schema is a form no client can be generated for.
    """
    doc = _doc(schema, known - {name})
    branches = schema.get("oneOf")
    properties = schema.get("properties")
    if isinstance(branches, list) and branches and isinstance(properties, dict):
        # An object that is its own properties *and* a closed set of arms. The
        # arms are tagged inside themselves, beside the properties every arm
        # carries; an externally tagged set here would have no place to put its
        # tag that was not already one of those properties.
        tagged = _union(name, doc, branches, known - {name})
        if not isinstance(tagged, Union) or not tagged.internally_tagged:
            msg = f"`{name}` declares properties beside arms that are not tagged inside them"
            raise ContractError(msg)
        return Struct(
            name=name,
            doc=doc,
            fields=_fields(schema, known - {name}),
            variants=tagged.variants,
            tag_field=tagged.tag_field,
        )
    if isinstance(branches, list) and branches:
        return _union(name, doc, branches, known - {name})
    if schema.get("type") == "object" and isinstance(properties, dict):
        return Struct(name=name, doc=doc, fields=_fields(schema, known - {name}))
    return Alias(name=name, target=type_of(schema), doc=doc)


def _union(name: str, doc: str, branches: list[Any], known: set[str]) -> Declaration:
    """One closed set of arms, tagged inside its arms or outside them."""
    if all(isinstance(branch, dict) and "const" in branch for branch in branches):
        return Enumeration(
            name=name,
            doc=doc,
            values=tuple((str(branch["const"]), _doc(branch, known)) for branch in branches),
        )

    tag = _internal_tag(branches)
    if tag:
        variants = []
        for branch in branches:
            constant = branch["properties"][tag]["const"]
            variants.append(
                Variant(
                    tag=str(constant),
                    doc=_doc(branch, known),
                    fields=_fields(branch, known, skip=tag),
                )
            )
        return Union(name=name, doc=doc, variants=tuple(variants), tag_field=tag)

    variants = []
    for branch in branches:
        if not isinstance(branch, dict):
            msg = f"`{name}` declares an arm that is not a shape"
            raise ContractError(msg)
        if "const" in branch:
            variants.append(Variant(tag=str(branch["const"]), doc=_doc(branch, known)))
            continue
        properties = branch.get("properties")
        if not isinstance(properties, dict) or len(properties) != 1:
            msg = (
                f"`{name}` declares an arm carrying {len(properties or {})} properties; "
                f"an externally tagged arm carries exactly one, its own name"
            )
            raise ContractError(msg)
        [(arm, payload)] = properties.items()
        # An arm carrying an object nobody named carries that object's own
        # properties; one carrying a shape with a name of its own carries the
        # shape, so that a client names it once rather than inlining it here.
        carried = _fields(payload, known)
        variants.append(
            Variant(
                tag=arm,
                doc=_doc(branch, known),
                fields=carried,
                payload=None if carried else type_of(payload),
            )
        )
    return Union(name=name, doc=doc, variants=tuple(variants))


def _reachable(roots: list[str], schemas: dict[str, dict[str, Any]]) -> set[str]:
    """Every named type a client needs, following each reference in turn."""
    seen: set[str] = set()
    pending = list(roots)
    while pending:
        name = pending.pop()
        if name in seen:
            continue
        seen.add(name)
        pending.extend(_references(schemas[name]))
    return seen


def _references(node: object) -> list[str]:
    """Every named type one schema names, at any depth.

    `$defs` is skipped: it is the copy of the referenced types the generator
    that wrote the file inlined, and this generator resolves a reference to
    that type's own file instead.
    """
    found: list[str] = []
    if isinstance(node, dict):
        for key, value in node.items():
            if key == "$defs":
                continue
            if key == "$ref" and isinstance(value, str) and value.startswith(REF_PREFIX):
                found.append(value.removeprefix(REF_PREFIX))
                continue
            found.extend(_references(value))
    elif isinstance(node, list):
        for value in node:
            found.extend(_references(value))
    return found


def _operation(described: dict[str, Any], answers: dict[str, str]) -> Operation:
    """One described operation, as a client calls it."""
    responses = described.get("responses")
    if not isinstance(responses, list) or not responses:
        msg = f"the operation `{described.get('name')}` describes no answer"
        raise ContractError(msg)
    for entry in responses:
        answers[str(entry["answer"])] = str(entry["type"])
    return Operation(
        name=str(described["name"]),
        method=str(described["method"]),
        path=str(described["path"]),
        effect=str(described["effect"]),
        answer=answers.get("success", ""),
        rejection=answers.get("rejected", ""),
        image_path_field=str(described.get("image_path_field") or ""),
        parameters=tuple(
            Parameter(
                name=str(value["name"]),
                type=type_of(value["shape"]),
                required=bool(value["required"]),
                located=str(value["located"]),
            )
            for value in described.get("parameters", [])
        ),
    )


def load(root: Path) -> Contract:
    """Read every checked-in schema a client is generated from.

    Raises:
        ContractError: If the description or a schema is one no client can be
            generated from.
    """
    schemas: dict[str, dict[str, Any]] = {}
    for directory in (TYPES_DIR, SERVER_DIR):
        for path in sorted((root / directory).glob("*.json")):
            if path.name == OPERATIONS_FILE:
                continue
            schemas[path.stem] = _read(path)

    description = _read(root / SERVER_DIR / OPERATIONS_FILE)
    operations = tuple(_operation(described, {}) for described in description["operations"])

    roots = sorted(
        {operation.answer for operation in operations}
        | {operation.rejection for operation in operations if operation.rejection}
        | {
            parameter.type.name
            for operation in operations
            for parameter in operation.parameters
            if isinstance(parameter.type, Ref)
        }
        | {"ErrorAnswer"}
    )
    missing = [name for name in roots if name not in schemas]
    if missing:
        msg = f"the description names {missing}, which no checked-in schema declares"
        raise ContractError(msg)

    known = _reachable(roots, schemas)
    declarations = tuple(declaration_of(name, schemas[name], known) for name in sorted(known))
    return Contract(
        version=workspace_version(root),
        version_prefix=str(description["version_prefix"]),
        media_type=str(description["media_type"]),
        declarations=declarations,
        operations=operations,
        by_name={declaration.name: declaration for declaration in declarations},
    )


def workspace_version(root: Path) -> str:
    """The version the type crate declares in the tree the clients come from.

    It is the workspace's, which release automation owns and every crate of
    this repository inherits — including the type crate whose schemas these
    are. A client records it so that a consumer of an installed package can
    tell which contract it was generated against.

    Raises:
        ContractError: If the workspace declares no version.
    """
    with (root / "Cargo.toml").open("rb") as handle:
        manifest = tomllib.load(handle)
    version = manifest.get("workspace", {}).get("package", {}).get("version")
    if not isinstance(version, str) or not version:
        msg = "the workspace declares no version for the contract to be recorded against"
        raise ContractError(msg)
    return version
