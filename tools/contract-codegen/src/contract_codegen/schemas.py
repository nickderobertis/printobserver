"""Reading the checked-in schemas into the model the emitters are written from.

The schemas are the one source. The set is keyed by type name across every
declaring crate: `schemas/<crate>/` holds one file per type that crate
declares — the contract crate's shared vocabulary, each port's shapes, and each
domain's event payloads, marked with the kind they are written under — and
`schemas/printobserver-server/` holds the shapes a route answers beside the
description of the operations themselves. A type moving crates moves its file
between directories and changes no generated line. Every `$ref` resolves to a
*file* rather than to the `$defs` copy beside it, so altering a type's own
file moves every client type built from it — which is what makes the drift
gate over the generated clients a gate over the contracts rather than over a
checksum.
"""

from __future__ import annotations

import json
import re
import tomllib
from pathlib import Path
from typing import Any

from contract_codegen.model import (
    ENVELOPE,
    KIND_FIELD,
    PAYLOAD_FIELD,
    Alias,
    AnyValue,
    Contract,
    Declaration,
    Enumeration,
    EventKind,
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

#: Where every crate's schemas are checked in, one directory per crate.
SCHEMAS_DIR = "schemas"

#: Where the shapes this server answers, and its operation list, are checked in.
SERVER_DIR = "schemas/printobserver-server"

#: The member a payload's schema carries naming the event kind it is under.
EVENT_KIND_MARKER = "x-event-kind"

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


#: The members a schema may carry and still constrain a value's form not at all.
UNCONSTRAINING = {"description", "title"}


def _any_value(schema: object) -> bool:
    """Whether one schema admits a JSON value of any form.

    `schemars` writes the boolean schema `true` for such a value, and the
    object carrying nothing but a description where the value is documented.
    """
    if schema is True:
        return True
    return isinstance(schema, dict) and set(schema) <= UNCONSTRAINING


def type_of(schema: dict[str, Any] | bool) -> TypeExpr:
    """The shape one property or item declares.

    Raises:
        ContractError: If the schema declares a shape no client can be
            generated for, which is a contract that has grown a form this
            generator has never been taught.
    """
    if _any_value(schema):
        return AnyValue()
    if not isinstance(schema, dict):
        msg = f"the schema {schema!r} is not one a property may take"
        raise ContractError(msg)
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
            doc=_doc(property_schema, known) if isinstance(property_schema, dict) else "",
        )
        for name, property_schema in sorted(properties.items())
        if name != skip and (isinstance(property_schema, dict) or property_schema is True)
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


def schema_files(root: Path) -> list[Path]:
    """Every checked-in schema, under every crate's directory, in a stable order.

    The operation description is not one: it describes what the server serves
    rather than declaring a shape.
    """
    return [
        path
        for directory in sorted(entry for entry in (root / SCHEMAS_DIR).iterdir() if entry.is_dir())
        for path in sorted(directory.glob("*.json"))
        if path.name != OPERATIONS_FILE
    ]


def read_schemas(root: Path) -> dict[str, dict[str, Any]]:
    """Every checked-in schema by type name, across every declaring crate.

    Raises:
        ContractError: If one type name is declared under two directories —
            two crates each claiming the type — or a file is not a schema.
    """
    schemas: dict[str, dict[str, Any]] = {}
    declared_by: dict[str, Path] = {}
    for path in schema_files(root):
        if path.stem in schemas:
            msg = (
                f"`{path.stem}` is declared under both {declared_by[path.stem].parent.name} "
                f"and {path.parent.name}; a type has one owner"
            )
            raise ContractError(msg)
        schemas[path.stem] = _read(path)
        declared_by[path.stem] = path
    return schemas


def event_kinds_of(schemas: dict[str, dict[str, Any]]) -> tuple[EventKind, ...]:
    """Every event kind the schema set declares, read off each payload's marker.

    Raises:
        ContractError: If two schemas carry the same kind — two domains each
            claiming one name — or a marker is not a string.
    """
    owners: dict[str, str] = {}
    for name in sorted(schemas):
        marked = schemas[name].get(EVENT_KIND_MARKER)
        if marked is None:
            continue
        if not isinstance(marked, str) or not marked:
            msg = f"`{name}` carries a `{EVENT_KIND_MARKER}` that is not a kind name"
            raise ContractError(msg)
        if marked in owners:
            msg = (
                f"the kind `{marked}` is declared by both `{owners[marked]}` and `{name}`; "
                f"a kind has one owner"
            )
            raise ContractError(msg)
        owners[marked] = name
    return tuple(EventKind(name=kind, payload=owners[kind]) for kind in sorted(owners))


def _envelope_findings(by_name: dict[str, Declaration]) -> str:
    """Why the envelope is not one a kind table can be generated against, if it is not."""
    envelope = by_name.get(ENVELOPE)
    if not isinstance(envelope, Struct):
        return f"`{ENVELOPE}` is not an object shape for the kind table to read"
    fields = {entry.name: entry.type for entry in envelope.fields}
    if not isinstance(fields.get(KIND_FIELD), Ref):
        return f"`{ENVELOPE}.{KIND_FIELD}` is not a named type for the kind table to read"
    if not isinstance(fields.get(PAYLOAD_FIELD), AnyValue):
        return f"`{ENVELOPE}.{PAYLOAD_FIELD}` is not a value of any form for the kind table to read"
    return ""


def load(root: Path) -> Contract:
    """Read every checked-in schema a client is generated from.

    The root set is the closure of the operations the server describes plus
    every payload type marked with an event kind — a payload is emitted whether
    or not an operation reaches it by reference, because the envelope carries
    every kind opaquely and a client reads a payload out through the table.

    Raises:
        ContractError: If the description or a schema is one no client can be
            generated from.
    """
    schemas = read_schemas(root)
    event_kinds = event_kinds_of(schemas)

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
        | {kind.payload for kind in event_kinds}
    )
    missing = [name for name in roots if name not in schemas]
    if missing:
        msg = f"the description names {missing}, which no checked-in schema declares"
        raise ContractError(msg)

    known = _reachable(roots, schemas)
    declarations = tuple(declaration_of(name, schemas[name], known) for name in sorted(known))
    by_name = {declaration.name: declaration for declaration in declarations}
    for kind in event_kinds:
        if not isinstance(by_name[kind.payload], Struct):
            msg = f"`{kind.payload}` is marked as the `{kind.name}` payload and is not an object"
            raise ContractError(msg)
    if event_kinds and (finding := _envelope_findings(by_name)):
        raise ContractError(finding)
    return Contract(
        version=workspace_version(root),
        version_prefix=str(description["version_prefix"]),
        media_type=str(description["media_type"]),
        declarations=declarations,
        operations=operations,
        by_name=by_name,
        event_kinds=event_kinds,
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
