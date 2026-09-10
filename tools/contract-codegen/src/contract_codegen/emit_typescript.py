"""The Node client's request and response types, and its one method per operation.

The types are interfaces and unions rather than classes, and that is what makes
the pass-through property true by construction: what a method answers **is**
the document the server sent, parsed and handed on, so there is nothing between
the two for a field to be transformed in.
"""

from __future__ import annotations

from contract_codegen.banner import banner
from contract_codegen.doc import wrapped
from contract_codegen.model import (
    ACTOR_PARAMETER,
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
from contract_codegen.naming import (
    camel,
    method_name,
    pascal,
    typescript_member,
    typescript_property,
)

#: What each scalar of the contracts is in TypeScript.
SCALARS = {"string": "string", "number": "number", "integer": "number", "boolean": "boolean"}


def type_name(shape: TypeExpr) -> str:
    """One shape, as TypeScript spells it."""
    match shape:
        case Ref(name):
            return name
        case Scalar(kind):
            return SCALARS[kind]
        case ListOf(item):
            return f"Array<{type_name(item)}>"
        case MapOf(value):
            return f"Record<string, {type_name(value)}>"
        case Nullable(inner):
            return f"{type_name(inner)} | null"
    msg = f"{shape!r} is not a shape TypeScript is generated for"
    raise TypeError(msg)


def doc_lines(text: str, indent: str = "") -> list[str]:
    """One description, as a documentation comment."""
    body = wrapped(text, indent)
    if len(body) == 1:
        return [f"{indent}/** {body[0]} */"]
    return [
        f"{indent}/**",
        *(f"{indent} *{f' {line}' if line else ''}" for line in body),
        f"{indent} */",
    ]


def _members(fields: tuple[Field, ...], indent: str) -> list[str]:
    """Every property of an object, as the body of an interface."""
    lines: list[str] = []
    for entry in fields:
        lines.extend(doc_lines(entry.doc, indent))
        optional = "" if entry.required else "?"
        spelled = typescript_property(entry.name)
        lines.append(f"{indent}{spelled}{optional}: {type_name(entry.type)};")
    return lines


def _inline(fields: tuple[Field, ...]) -> str:
    """One arm's own payload, as an object type written where it is used."""
    written = "; ".join(
        f"{typescript_property(entry.name)}{'' if entry.required else '?'}: {type_name(entry.type)}"
        for entry in fields
    )
    return "{ " + written + " }"


def _arm(union: str, variant: Variant, tag_field: str) -> str:
    """One arm of a union, as TypeScript spells it."""
    if tag_field:
        tagged = f'{typescript_property(tag_field)}: "{variant.tag}"; ' + "; ".join(
            f"{typescript_property(entry.name)}{'' if entry.required else '?'}: "
            f"{type_name(entry.type)}"
            for entry in variant.fields
        )
        return "{ " + tagged + " }"
    if variant.unit:
        return f'"{variant.tag}"'
    carried = type_name(variant.payload) if variant.payload is not None else _inline(variant.fields)
    return "{ " + f"{typescript_property(variant.tag)}: {carried}" + " }"


def _declaration(declaration: Declaration) -> list[str]:
    """One named type of the contracts, as TypeScript declares it."""
    match declaration:
        case Alias(name, target, doc):
            return [*doc_lines(doc), f"export type {name} = {type_name(target)};"]
        case Enumeration(name, doc, values):
            arms = " | ".join(f'"{value}"' for value, _ in values)
            return [*doc_lines(doc), f"export type {name} = {arms};"]
        case Struct(name, doc, fields, variants, tag_field) if tag_field:
            lines = [
                *doc_lines(f"What every arm of `{name}` carries beside its own payload.\n\n{doc}"),
                f"export interface {name}Common {{",
                *_members(fields, "  "),
                "}",
                "",
                *doc_lines(doc),
                f"export type {name} =",
            ]
            lines.extend(
                f"  | ({name}Common & {_arm(name, variant, tag_field)})" for variant in variants
            )
            lines[-1] += ";"
            return lines
        case Struct(name, doc, fields, _, _):
            return [
                *doc_lines(doc),
                f"export interface {name} {{",
                *_members(fields, "  "),
                "}",
            ]
        case Union(name, doc, variants, tag_field):
            lines = [*doc_lines(doc), f"export type {name} ="]
            lines.extend(f"  | {_arm(name, variant, tag_field)}" for variant in variants)
            lines[-1] += ";"
            return lines
        case _:  # pragma: no cover - the model declares no fifth kind
            msg = f"{declaration!r} is not a declaration TypeScript is generated for"
            raise TypeError(msg)


def _argument(parameter: Parameter) -> str:
    """One value a generated method takes, as TypeScript declares it."""
    if parameter.optional:
        return f"{camel(parameter.name)}?: {type_name(parameter.carried)}"
    return f"{camel(parameter.name)}: {type_name(parameter.type)}"


def _target(operation: Operation) -> str:
    """The request target one call is made to, as TypeScript builds it."""
    path = operation.path
    for parameter in operation.parameters:
        if parameter.located == "path":
            path = path.replace(f"{{{parameter.name}}}", f"${{{camel(parameter.name)}}}")
    return path


def _method(operation: Operation) -> list[str]:
    """One operation, as the Node client's own method."""
    supplied = operation.supplied()
    arguments = ", ".join(_argument(parameter) for parameter in supplied)
    said = (
        f"Call `{operation.name}` on the configured supervisor.\n\n"
        f"Rejects with `Unreachable` when nothing answered, with `Unreadable` "
        f"when the answer could not be read, and with `Refused` when the "
        f"supervisor said no"
    )
    if operation.mutating:
        said += (
            ". A call whose reason is empty is refused here, before a request is "
            "made, and the policy's own refusal arrives as `Rejected`, carrying "
            "the reason, the value asked for and the range allowed"
        )
    lines = doc_lines(f"{said}.", "  ")
    lines.append(
        f"  async {method_name(operation.name, 'typescript')}({arguments}): "
        f"Promise<{operation.answer}> {{"
    )
    if operation.mutating:
        lines.append("    reasonGiven(reason);")
    lines.append(f"    const target = `{_target(operation)}`;")

    query = [parameter for parameter in supplied if parameter.located == "query"]
    lines.append("    const asked: Array<[string, string]> = [];")
    for parameter in query:
        spelled = camel(parameter.name)
        lines.append(f"    if ({spelled} !== undefined) {{")
        lines.append(f'      asked.push(["{parameter.name}", String({spelled})]);')
        lines.append("    }")

    body = [parameter for parameter in operation.parameters if parameter.located == "body"]
    if body:
        lines.append("    const sending: Record<string, unknown> = {};")
        for parameter in body:
            spelled = camel(parameter.name)
            if parameter.name == ACTOR_PARAMETER:
                lines.append(f"    sending{typescript_member(parameter.name)} = this.actor;")
            elif parameter.optional:
                lines.append(f"    if ({spelled} !== undefined) {{")
                lines.append(f"      sending{typescript_member(parameter.name)} = {spelled};")
                lines.append("    }")
            else:
                lines.append(f"    sending{typescript_member(parameter.name)} = {spelled};")
    else:
        lines.append("    const sending = undefined;")
    lines.append(
        f'    return await this.call<{operation.answer}>("{operation.method}", '
        f"target, asked, sending);"
    )
    lines.append("  }")
    return lines


def emit(contract: Contract) -> str:
    """The whole generated module of the Node client."""
    lines = [
        "/**",
        *(
            f" *{f' {line}' if line else ''}"
            for line in banner("", "the Node client's request and response types").split("\n")
        ),
        " */",
        "",
        'import { GeneratedSurface, reasonGiven } from "./surface.ts";',
        "",
        *doc_lines(
            "The version of the server contract these types were generated from, which "
            "is the version the type crate declares in the tree they came from."
        ),
        f'export const CONTRACT_VERSION = "{contract.version}";',
        "",
        *doc_lines(
            "Every operation this client exposes a method for, which is every operation "
            "the server declares and no other."
        ),
        "export const OPERATION_NAMES = [",
        *(f'  "{operation.name}",' for operation in contract.operations),
        "] as const;",
        "",
    ]
    for declaration in contract.declarations:
        lines.extend(_declaration(declaration))
        lines.append("")

    lines.extend(
        doc_lines(
            "One method per operation the server declares.\n\nEvery method here is "
            "generated from the checked-in description of the server's own "
            "operations. The transport, the configuration and the error vocabulary "
            "are hand-written, and this class is written against them."
        )
    )
    lines.append("export class GeneratedClient extends GeneratedSurface {")
    for operation in contract.operations:
        lines.extend(_method(operation))
        lines.append("")
    lines.append("}")
    return "\n".join(lines) + "\n"


__all__ = ["emit", "pascal", "type_name"]


def _typescript_argument(parameter: Parameter, value: object) -> str:
    """One value the walk calls a Node method with, as TypeScript writes it."""
    from contract_codegen.walk import document

    shape = parameter.carried
    match shape:
        case Scalar("string"):
            return f'"{value}"'
        case Scalar("boolean"):
            return str(value).lower()
        case Scalar("number") | Scalar("integer"):
            return str(value)
        case Ref(name):
            return f"JSON.parse('{document(value)}') as {name}"
    msg = f"{parameter!r} is not a value a Node walk can supply"
    raise TypeError(msg)


def emit_walk(contract: Contract) -> str:
    """The whole generated walk of the Node client."""
    from contract_codegen.walk import REASON_PARAMETER as REASON_NAME
    from contract_codegen.walk import actor, document, plan

    named = sorted(
        {
            parameter.carried.name
            for operation in contract.operations
            for parameter in operation.supplied()
            if isinstance(parameter.carried, Ref)
        }
        | {"Actor"}
    )
    lines = [
        "/**",
        *(
            f" *{f' {line}' if line else ''}"
            for line in banner(
                "", "the Node client's walk over every operation the server declares"
            ).split("\n")
        ),
        " *",
        *(
            f" *{f' {line}' if line else ''}"
            for line in wrapped(
                "Every method this client exposes is driven here against a stub host on "
                "a real socket: what the host received is asserted against what the "
                "operation declares, and what the method answered is asserted to carry, "
                "field for field, exactly the document the host sent. A client that "
                "transformed a value on the way out fails that equality rather than "
                "needing a probe that guesses how it transformed it."
            )
        ),
        " */",
        "",
        'import { expect, test } from "bun:test";',
        'import { Client } from "../src/client.ts";',
        f'import type {{ {", ".join(named)} }} from "../src/contract.ts";',
        'import { NoReason, Rejected } from "../src/surface.ts";',
        'import { Host } from "./host.ts";',
        "",
        "/** The actor every client of this walk acts as. */",
        f"const ACTOR = JSON.parse('{actor(contract)}') as Actor;",
        "",
    ]

    for step in plan(contract):
        call = ", ".join(
            _typescript_argument(parameter, value) for parameter, value in step.arguments
        )
        spelled = method_name(step.name, "typescript")
        lines += [
            f'test("{step.name} sends what it declares and answers what the server sent", '
            "async () => {",
            f"  const answer = JSON.parse('{document(step.answer)}');",
            "  await using host = Host.answering(200, answer);",
            "  const client = new Client({ server: host.address, actor: ACTOR });",
            "",
            f"  const answered = await client.{spelled}({call});",
            "",
            "  const received = host.received();",
            f'  expect(received.method).toBe("{step.method}");',
            f'  expect(received.target).toBe("{step.target}");',
        ]
        if step.body is None:
            lines.append('  expect(received.body).toBe("");')
        else:
            lines.append(
                f"  expect(JSON.parse(received.body)).toEqual(JSON.parse('{document(step.body)}'));"
            )
        lines += ["  expect(answered).toEqual(answer);", "});", ""]

        if step.rejection is not None:
            lines += [
                f'test("{step.name} surfaces the policy\'s own refusal", async () => {{',
                f"  const refusal = JSON.parse('{document(step.rejection)}');",
                "  await using host = Host.answering(409, refusal);",
                "  const client = new Client({ server: host.address, actor: ACTOR });",
                "",
                "  let refused: unknown;",
                "  try {",
                f"    await client.{spelled}({call});",
                "  } catch (raised) {",
                "    refused = raised;",
                "  }",
                "",
                "  expect(refused).toBeInstanceOf(Rejected);",
                "  const rejection = refused as Rejected;",
                "  expect(rejection.requested).toBe(1.5);",
                "  expect(rejection.allowed).toEqual({ min: 0.5, max: 1.25 });",
                '  expect(rejection.reason).toHaveProperty("out_of_bounds");',
                "});",
                "",
            ]

        if step.operation.mutating:
            lines += [
                f'test("{step.name} makes no request when the reason is empty", async () => {{',
            ]
            for blank in ('""', '"   "'):
                call_without = ", ".join(
                    blank
                    if parameter.name == REASON_NAME
                    else _typescript_argument(parameter, value)
                    for parameter, value in step.arguments
                )
                lines += [
                    "  {",
                    "    await using host = Host.answering(200, {});",
                    "    const client = new Client({ server: host.address, actor: ACTOR });",
                    "    let refused: unknown;",
                    "    try {",
                    f"      await client.{spelled}({call_without});",
                    "    } catch (raised) {",
                    "      refused = raised;",
                    "    }",
                    "    expect(refused).toBeInstanceOf(NoReason);",
                    "    expect(host.requests()).toBe(0);",
                    "  }",
                ]
            lines += ["});", ""]
    return "\n".join(lines).rstrip("\n") + "\n"
