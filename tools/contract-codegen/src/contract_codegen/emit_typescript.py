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
from contract_codegen.walk import LiveStep

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


#: How each token the real-supervisor walk supplies a value with is written in
#: TypeScript. Anything not here is written out as it stands.
LIVE_TOKENS = {
    "world.print_id": "world.print_id",
    "world.image_id": "world.image_id",
    "world.event_id": "world.event_id",
    "world.file_name": "world.file_name",
    "manifest": "manifest(world)",
    "reason": "REASON",
    "disposition:continue": '"continue"',
}


def _live_argument(token: str) -> str:
    """One value the real walk calls a Node method with."""
    return LIVE_TOKENS.get(token, token)


def _live_target(step: LiveStep) -> str:
    """The request target one real call must reach, as TypeScript builds it."""
    path = step.operation.path
    for parameter in step.operation.parameters:
        if parameter.located == "path":
            path = path.replace(f"{{{parameter.name}}}", f"${{world.{parameter.name}}}")
    asked = [
        f"{parameter.name}=${{{_live_argument(token)}}}"
        for parameter, token in step.arguments
        if parameter.located == "query"
    ]
    whole = f"{path}?{'&'.join(asked)}" if asked else path
    return f"`{whole}`"


def _live_body(step: LiveStep) -> list[str]:
    """What the supervisor must have received in the body of one real call."""
    body = [parameter for parameter in step.operation.parameters if parameter.located == "body"]
    if not body:
        return ['  expect(seen.body).toBe("");']
    supplied = {parameter.name: given for parameter, given in step.arguments}
    lines = ["  const sent = JSON.parse(seen.body);"]
    for parameter in body:
        if parameter.name == "actor":
            lines.append('  expect(sent.actor).toBe("operator");')
            continue
        token = supplied.get(parameter.name)
        if token is None:
            continue
        lines.append(
            f"  expect(sent{typescript_member(parameter.name)}).toEqual({_live_argument(token)});"
        )
    return lines


def emit_live(contract: Contract) -> str:
    """The whole generated walk of the Node client, against a real supervisor."""
    from contract_codegen.walk import REASON, plan_live, rejected_live

    steps = plan_live(contract)
    lines = [
        "/**",
        *(
            f" *{f' {line}' if line else ''}"
            for line in banner(
                "", "the Node client's walk over every operation, against a real supervisor"
            ).split("\n")
        ),
        " *",
        *(
            f" *{f' {line}' if line else ''}"
            for line in wrapped(
                "Every method this client exposes is driven against the **real** "
                "printobserver supervisor — the program this repository builds, over the "
                "OctoPrint `just octoprint-up` started, holding a print and a stored "
                "image the supervisor's own ingress opened. A recording proxy sits in "
                "front of it, so what each assertion is against is the request that "
                "supervisor received and the answer it actually sent."
            )
        ),
        " *",
        *(
            f" *{f' {line}' if line else ''}"
            for line in wrapped(
                "The order is the one a real machine admits: a printer is a state "
                "machine and the policy refuses an action that is not valid from where "
                "it is. It ends with the machine printing, where the bring-up left it."
            )
        ),
        " */",
        "",
        'import { mkdtempSync } from "node:fs";',
        'import { tmpdir } from "node:os";',
        'import { join } from "node:path";',
        'import { afterAll, beforeAll, expect, test } from "bun:test";',
        'import { Client } from "../src/client.ts";',
        'import type { JobManifest } from "../src/contract.ts";',
        'import { Rejected } from "../src/surface.ts";',
        'import { Recording, ready, same } from "./live.ts";',
        'import { Standing, type Supervisor } from "./world.ts";',
        "",
        "/** The reason every mutating call of this walk carries. */",
        f'const REASON = "{REASON}";',
        "",
        "let standing: Standing;",
        "let world: Supervisor;",
        "",
        "beforeAll(async () => {",
        "  standing = await Standing.standing(",
        '    mkdtempSync(join(tmpdir(), "printobserver-live-")),',
        "  );",
        "  world = standing.at;",
        "});",
        "",
        "afterAll(async () => {",
        "  await standing.stop();",
        "});",
        "",
        "/**",
        " * The manifest this walk writes and starts a print under.",
        " *",
        " * It narrows nothing: what the walk needs is every adjustment the envelope",
        " * allows to be answered, and a narrowing here would refuse one for a reason",
        " * that is not what this walk is about.",
        " */",
        "function manifest(at: Supervisor): JobManifest {",
        "  return {",
        "    file_name: at.file_name,",
        '    material: "PLA",',
        "    nozzle_diameter_mm: 0.4,",
        '    slicer_profile: "the walk over every operation",',
        "    allowed: {},",
        "    metadata: {},",
        "  };",
        "}",
        "",
    ]

    for step in steps:
        call = ", ".join(_live_argument(token) for _, token in step.arguments)
        spelled = method_name(step.name, "typescript")
        lines += [
            f"/** `{step.name}`, answered by a real supervisor. */",
            f"async function step{pascal(step.name)}(client: Client, proxy: Recording) {{",
            f'  await ready(client, world.print_id, "{step.state}");',
            "",
            f"  const answered = await client.{spelled}({call});",
            "",
            "  const seen = proxy.last();",
            f'  expect(seen.method).toBe("{step.method}");',
            f"  expect(seen.target).toBe({_live_target(step)});",
            "  expect(seen.status).toBe(200);",
            *_live_body(step),
            f'  same("{step.name}", answered, seen.answer);',
            "}",
            "",
        ]

    lines += [
        'test("every method is answered by a real supervisor", async () => {',
        "  await using proxy = new Recording(world.server);",
        '  const client = new Client({ server: proxy.url, actor: "operator" });',
        "",
        *(f"  await step{pascal(step.name)}(client, proxy);" for step in steps),
        "",
        f"  expect(proxy.calls()).toBeGreaterThanOrEqual({len(steps)});",
        "}, 900_000);",
        "",
    ]

    for step in rejected_live(contract):
        call = ", ".join(_live_argument(token) for _, token in step.arguments)
        spelled = method_name(step.name, "typescript")
        lines += [
            f"/** `{step.name}`, refused by a real supervisor's own policy. */",
            f"async function refused{pascal(step.name)}(client: Client, proxy: Recording) {{",
            f"  const refused = await client.{spelled}({call}).catch((raised: unknown) => raised);",
            "",
            "  expect(refused).toBeInstanceOf(Rejected);",
            '  expect((refused as Rejected).reason).toHaveProperty("actor_may_not_request");',
            "  const seen = proxy.last();",
            "  expect(seen.status).toBe(409);",
            f'  same("{step.name}", (refused as Rejected).answer, seen.answer);',
            "}",
            "",
        ]

    lines += [
        "/**",
        " * Every action, refused by a real supervisor's own policy, as a typed rejection.",
        " *",
        " * One client acting as an actor class the envelope grants nothing. The policy",
        " * takes that decision before it looks at the state, the interval or the bounds,",
        " * so every action is refused from wherever the machine happens to be.",
        " */",
        'test("every action is refused as a typed rejection by a real supervisor", async () => {',
        "  await using proxy = new Recording(world.server);",
        "  const client = new Client({",
        "    server: proxy.url,",
        '    actor: { agent: { session_name: "an actor this envelope grants nothing" } },',
        "  });",
        "",
        *(
            f"  await refused{pascal(step.name)}(client, proxy);"
            for step in rejected_live(contract)
        ),
        "}, 900_000);",
    ]
    return "\n".join(lines).rstrip("\n") + "\n"
