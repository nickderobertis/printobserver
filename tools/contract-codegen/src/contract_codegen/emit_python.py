"""The Python client's request and response types, and its one method per operation.

The types are `TypedDict`s and unions of them rather than classes with
constructors, and that is what makes the pass-through property true by
construction: what a method answers **is** the document the server sent, parsed
and handed on, so there is nothing between the two for a field to be
transformed in.
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
from contract_codegen.naming import method_name, pascal, python_identifier
from contract_codegen.walk import LiveStep

#: What each scalar of the contracts is in Python.
SCALARS = {"string": "str", "number": "float", "integer": "int", "boolean": "bool"}


def type_name(shape: TypeExpr) -> str:
    """One shape, as Python spells it."""
    match shape:
        case Ref(name):
            return name
        case Scalar(kind):
            return SCALARS[kind]
        case ListOf(item):
            return f"list[{type_name(item)}]"
        case MapOf(value):
            return f"dict[str, {type_name(value)}]"
        case Nullable(inner):
            return f"{type_name(inner)} | None"
    msg = f"{shape!r} is not a shape Python is generated for"
    raise TypeError(msg)


def docstring(summary: str, description: str, indent: str) -> list[str]:
    """One description, as a Python docstring.

    The summary is generated and the contracts' own words follow it after a
    blank line. That shape rather than the description alone, because a
    description whose first sentence wraps would put two lines where the lint
    this repository runs requires a summary and a blank line.
    """
    lines = [f'{indent}"""{summary}']
    body = wrapped(description, indent)
    if body:
        lines.append("")
        lines.extend(f"{indent}{line}".rstrip() for line in body)
    lines.append(f'{indent}"""')
    return lines


def comment(text: str, indent: str) -> list[str]:
    """One description, as the comment above a field."""
    return [f"{indent}#{f' {line}' if line else ''}" for line in wrapped(text, indent)]


def _annotation(entry: Field) -> str:
    """One property's own annotation, marked absent-able where it may be absent."""
    declared = type_name(entry.type)
    return declared if entry.required else f"NotRequired[{declared}]"


def _fields(fields: tuple[Field, ...], indent: str) -> list[str]:
    """Every property of an object, as the body of a `TypedDict`.

    Raises:
        TypeError: If a property is spelled as something Python reserves, which
            no `TypedDict` written this way could carry.
    """
    lines: list[str] = []
    for entry in fields:
        if not python_identifier(entry.name):
            msg = f"`{entry.name}` is not a name a generated `TypedDict` can carry"
            raise TypeError(msg)
        lines.extend(comment(entry.doc, indent))
        lines.append(f"{indent}{entry.name}: {_annotation(entry)}")
    if not lines:
        lines.append(f"{indent}pass")
    return lines


def _literal(values: list[str]) -> str:
    """One closed set of strings, as Python spells it."""
    return "Literal[" + ", ".join(f'"{value}"' for value in values) + "]"


def _variant_class(union: str, variant: Variant, base: str, tag_field: str) -> list[str]:
    """One arm of a union that carries something, as a `TypedDict` of its own."""
    name = f"{union}{pascal(variant.tag)}"
    lines = [f"class {name}({base}):"]
    lines.extend(docstring(f"The `{variant.tag}` arm of `{union}`.", variant.doc, "    "))
    if tag_field:
        lines.append(f"    {tag_field}: {_literal([variant.tag])}")
        lines.extend(_fields(variant.fields, "    "))
    elif variant.payload is not None:
        lines.append(f"    {variant.tag}: {type_name(variant.payload)}")
    else:
        lines.append(f"    {variant.tag}: {name}Payload")
    return lines


def _payload_class(union: str, variant: Variant) -> list[str]:
    """What one externally tagged arm carries, as a `TypedDict` of its own."""
    name = f"{union}{pascal(variant.tag)}Payload"
    lines = [f"class {name}(TypedDict):"]
    lines.extend(
        docstring(f"What the `{variant.tag}` arm of `{union}` carries.", variant.doc, "    ")
    )
    lines.extend(_fields(variant.fields, "    "))
    return lines


def _union_alias(name: str, variants: tuple[Variant, ...]) -> str:
    """One union, as the alias a caller annotates with."""
    units = [variant.tag for variant in variants if variant.unit]
    arms = [f"{name}{pascal(variant.tag)}" for variant in variants if not variant.unit]
    if units:
        arms.append(_literal(units))
    return f"type {name} = " + " | ".join(arms)


def _declaration(declaration: Declaration) -> list[str]:
    """One named type of the contracts, as Python declares it."""
    match declaration:
        case Alias(name, target, doc):
            return [
                *comment(doc, ""),
                f"type {name} = {type_name(target)}",
            ]
        case Enumeration(name, doc, values):
            return [
                *comment(doc, ""),
                f"type {name} = {_literal([value for value, _ in values])}",
            ]
        case Struct(name, doc, fields, variants, tag_field) if tag_field:
            lines = [f"class {name}Common(TypedDict):"]
            lines.extend(
                docstring(
                    f"What every arm of `{name}` carries beside its own payload.",
                    doc,
                    "    ",
                )
            )
            lines.extend(_fields(fields, "    "))
            lines.append("")
            lines.append("")
            for variant in variants:
                lines.extend(_variant_class(name, variant, f"{name}Common", tag_field))
                lines.append("")
                lines.append("")
            lines.append(f"# {name}, as the contracts declare it.")
            lines.append(_union_alias(name, variants))
            return lines
        case Struct(name, doc, fields, _, _):
            lines = [f"class {name}(TypedDict):"]
            lines.extend(docstring(f"`{name}`, as the contracts declare it.", doc, "    "))
            lines.extend(_fields(fields, "    "))
            return lines
        case Union(name, doc, variants, tag_field):
            lines: list[str] = []
            for variant in variants:
                if variant.unit:
                    continue
                if not tag_field and variant.payload is None:
                    lines.extend(_payload_class(name, variant))
                    lines.append("")
                    lines.append("")
                lines.extend(_variant_class(name, variant, "TypedDict", tag_field))
                lines.append("")
                lines.append("")
            lines.extend(comment(doc, ""))
            lines.append(_union_alias(name, variants))
            return lines
        case _:  # pragma: no cover - the model declares no fifth kind
            msg = f"{declaration!r} is not a declaration Python is generated for"
            raise TypeError(msg)


def _argument(parameter: Parameter) -> str:
    """One value a generated method takes, as Python declares it."""
    if parameter.optional:
        return f"{parameter.name}: {type_name(parameter.carried)} | None = None"
    return f"{parameter.name}: {type_name(parameter.type)}"


def _target(operation: Operation) -> str:
    """The request target one call is made to, as Python builds it."""
    path = operation.path
    for parameter in operation.parameters:
        if parameter.located == "path":
            path = path.replace(f"{{{parameter.name}}}", f"{{{parameter.name}}}")
    return path


def _method(operation: Operation) -> list[str]:
    """One operation, as the Python client's own method."""
    supplied = operation.supplied()
    arguments = ", ".join(["self", *(_argument(parameter) for parameter in supplied)])
    lines = [f"    def {method_name(operation.name, 'python')}({arguments}) -> {operation.answer}:"]
    raises = [
        "UnreachableError: If nothing answered at the configured address.",
        "UnreadableError: If the supervisor answered something this client cannot read.",
        "RefusedError: If the supervisor will not do what it was asked.",
    ]
    if operation.mutating:
        raises.insert(0, "NoReasonError: If the reason is absent or is nothing but whitespace.")
        raises.append(
            "RejectedError: If the policy refused the action. It carries the reason, "
            "the value asked for and the range allowed."
        )
    lines.append(f'        """Call `{operation.name}` on the configured supervisor.')
    lines.append("")
    lines.append("        Raises:")
    for said in raises:
        for index, line in enumerate(wrapped(said, "            ")):
            lines.append(f"            {line}" if index == 0 else f"                {line}")
    lines.append('        """')
    if operation.mutating:
        lines.append("        reason_given(reason)")
    lines.append(f'        target = f"{_target(operation)}"')

    query = [parameter for parameter in supplied if parameter.located == "query"]
    lines.append("        asked: list[tuple[str, str]] = []")
    for parameter in query:
        lines.append(f"        if {parameter.name} is not None:")
        lines.append(f'            asked.append(("{parameter.name}", str({parameter.name})))')

    body = [parameter for parameter in operation.parameters if parameter.located == "body"]
    if body:
        lines.append("        sending: dict[str, object] = {}")
        for parameter in body:
            if parameter.name == ACTOR_PARAMETER:
                lines.append(f'        sending["{parameter.name}"] = self.actor')
            elif parameter.optional:
                lines.append(f"        if {parameter.name} is not None:")
                lines.append(f'            sending["{parameter.name}"] = {parameter.name}')
            else:
                lines.append(f'        sending["{parameter.name}"] = {parameter.name}')
    else:
        lines.append("        sending = None")
    lines.append(f'        answered = self.call("{operation.method}", target, asked, sending)')
    lines.append(f"        return cast({operation.answer}, answered)")
    return lines


def emit(contract: Contract) -> str:
    """The whole generated module of the Python client."""
    lines = [
        '"""' + banner("", "the Python client's request and response types").lstrip(),
        '"""',
        "",
        "from __future__ import annotations",
        "",
        "from typing import Literal, NotRequired, TypedDict, cast",
        "",
        "from printobserver_sdk._surface import GeneratedSurface, reason_given",
        "",
        "# The version of the server contract these types were generated from, which",
        "# is the version the type crate declares in the tree they came from.",
        f'CONTRACT_VERSION = "{contract.version}"',
        "",
        "# Every operation this client exposes a method for, which is every operation",
        "# the server declares and no other.",
        "OPERATION_NAMES: tuple[str, ...] = (",
        *(f'    "{operation.name}",' for operation in contract.operations),
        ")",
        "",
        "",
    ]
    for declaration in contract.declarations:
        lines.extend(_declaration(declaration))
        lines.append("")
        lines.append("")

    lines.append("class GeneratedClient(GeneratedSurface):")
    lines.extend(
        docstring(
            "One method per operation the server declares.",
            "Every method here is generated from the checked-in description of "
            "the server's own operations. The transport, the configuration and "
            "the error vocabulary are hand-written, and this class is written "
            "against them.",
            "    ",
        )
    )
    lines.append("")
    for operation in contract.operations:
        lines.extend(_method(operation))
        lines.append("")
    return "\n".join(lines) + "\n"


#: How wide a piece of one embedded document is. The formatter lays adjacent
#: string pieces out to fit the line, and it will not split one that does not —
#: so a document that arrived as one long string would be the one line in the
#: generated walk the line-length lint refuses.
DOCUMENT_PIECE = 60


def literal(document: str) -> str:
    """One embedded document, as a Python expression the formatter can lay out."""
    pieces = [
        document[start : start + DOCUMENT_PIECE]
        for start in range(0, len(document), DOCUMENT_PIECE)
    ]
    return " ".join(f"'{piece}'" for piece in pieces) if pieces else "''"


def _python_argument(parameter: Parameter, value: object) -> str:
    """One value the walk calls a Python method with, as Python writes it."""
    from contract_codegen.walk import document

    shape = parameter.carried
    match shape:
        case Scalar("string"):
            return f'"{value}"'
        case Scalar("boolean"):
            return str(value)
        case Scalar("number") | Scalar("integer"):
            return str(value)
        case Ref(name):
            return f"cast({name}, json.loads({literal(document(value))}))"

    msg = f"{parameter!r} is not a value a Python walk can supply"
    raise TypeError(msg)


def emit_walk(contract: Contract) -> str:
    """The whole generated walk of the Python client."""
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
        '"""'
        + banner("", "the Python client's walk over every operation the server declares").lstrip(),
        "",
        *wrapped(
            "Every method this client exposes is driven here against a stub host on a "
            "real socket: what the host received is asserted against what the operation "
            "declares, and what the method answered is asserted to carry, field for "
            "field, exactly the document the host sent. A client that transformed a "
            "value on the way out fails that equality rather than needing a probe that "
            "guesses how it transformed it."
        ),
        '"""',
        "",
        "from __future__ import annotations",
        "",
        "import json",
        "from typing import cast",
        "",
        "from host import Host",
        "from printobserver_sdk import Client, NoReasonError, RejectedError",
        f"from printobserver_sdk.contract import {', '.join(named)}",
        "from repo_checks.expect import equal, truth",
        "",
        "#: The actor every client of this walk acts as.",
        f"ACTOR = cast(Actor, json.loads({literal(actor(contract))}))",
        "",
        "",
    ]

    for step in plan(contract):
        call = ", ".join(_python_argument(parameter, value) for parameter, value in step.arguments)
        lines += [
            f"def test_{step.name}_sends_what_it_declares_and_answers_what_was_sent() -> None:",
            f'    """`{step.name}` sends what it declares and answers what the server sent."""',
            f"    answer = json.loads({literal(document(step.answer))})",
            "",
            "    with Host(200, answer) as host:",
            "        client = Client(host.address, ACTOR)",
            f"        answered = client.{method_name(step.name, 'python')}({call})",
            "        received = host.received()",
            "",
            f'    equal(received.method, "{step.method}")',
            f'    equal(received.target, "{step.target}")',
        ]
        if step.body is None:
            lines.append('    equal(received.body, "")')
        else:
            lines += [
                "    equal(",
                "        json.loads(received.body),",
                f"        json.loads({literal(document(step.body))}),",
                '        describing="what this call sent",',
                "    )",
            ]
        lines += [
            '    equal(answered, answer, describing="what this call answered")',
            "",
            "",
        ]

        if step.rejection is not None:
            lines += [
                f"def test_{step.name}_surfaces_the_policys_own_refusal() -> None:",
                f'    """`{step.name}` surfaces the policy\'s own refusal, typed."""',
                f"    refusal = json.loads({literal(document(step.rejection))})",
                "",
                "    with Host(409, refusal) as host:",
                "        client = Client(host.address, ACTOR)",
                "        try:",
                f"            client.{method_name(step.name, 'python')}({call})",
                "        except RejectedError as refused:",
                "            equal(refused.requested, 1.5)",
                '            equal(refused.allowed, {"min": 0.5, "max": 1.25})',
                "            truth(",
                "                isinstance(refused.reason, dict)",
                '                and "out_of_bounds" in refused.reason,',
                '                describing="the refusal itself to be carried",',
                "            )",
                "        else:",
                '            truth(False, describing="the policy\'s refusal to arrive as one")',
                "",
                "",
            ]

        if step.operation.mutating:
            lines += [
                f"def test_{step.name}_makes_no_request_when_the_reason_is_empty() -> None:",
                f'    """`{step.name}` refuses a call with no reason before it is made."""',
            ]
            for spelled, said in (('""', "an absent reason"), ('"   "', "a blank reason")):
                blank = ", ".join(
                    spelled if parameter.name == REASON_NAME else _python_argument(parameter, value)
                    for parameter, value in step.arguments
                )
                lines += [
                    "    with Host(200, {}) as host:",
                    "        client = Client(host.address, ACTOR)",
                    "        try:",
                    f"            client.{method_name(step.name, 'python')}({blank})",
                    "        except NoReasonError:",
                    "            pass",
                    "        else:",
                    f'            truth(False, describing="{said} to be refused")',
                    "",
                    f'        equal(host.requests(), 0, describing="what {said} sent")',
                ]
            lines += ["", ""]
    return "\n".join(lines).rstrip("\n") + "\n"


#: How each token the real-supervisor walk supplies a value with is written in
#: Python. Anything not here is written out as it stands.
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
    """One value the real walk calls a Python method with."""
    return LIVE_TOKENS.get(token, token)


def _live_target(step: LiveStep) -> str:
    """The request target one real call must reach, as Python builds it."""
    path = step.operation.path
    for parameter in step.operation.parameters:
        if parameter.located == "path":
            path = path.replace(f"{{{parameter.name}}}", f"{{world.{parameter.name}}}")
    asked = [
        f"{parameter.name}={_live_argument(token)}"
        for parameter, token in step.arguments
        if parameter.located == "query"
    ]
    return f'f"{path}?{"&".join(asked)}"' if asked else f'f"{path}"'


def _live_body(step: LiveStep) -> list[str]:
    """What the supervisor must have received in the body of one real call."""
    body = [parameter for parameter in step.operation.parameters if parameter.located == "body"]
    if not body:
        return [f'    equal(seen.body, "", describing="what `{step.name}` sent")']
    supplied = {parameter.name: given for parameter, given in step.arguments}
    lines = ["    sent = json.loads(seen.body)"]
    for parameter in body:
        if parameter.name == "actor":
            lines.append(
                f'    equal(sent["actor"], "operator", describing="who `{step.name}` acted as")'
            )
            continue
        token = supplied.get(parameter.name)
        if token is None:
            continue
        lines.append(
            f"    equal(\n"
            f'        sent["{parameter.name}"],\n'
            f"        {_live_argument(token)},\n"
            f'        describing="the `{parameter.name}` `{step.name}` sent",\n'
            f"    )"
        )
    return lines


def emit_live(contract: Contract) -> str:
    """The whole generated walk of the Python client, against a real supervisor."""
    from contract_codegen.walk import REASON, plan_live, rejected_live

    steps = plan_live(contract)
    lines = [
        '"""'
        + banner(
            "", "the Python client's walk over every operation, against a real supervisor"
        ).lstrip(),
        "",
        *wrapped(
            "Every method this client exposes is driven against the **real** "
            "printobserver supervisor — the program this repository builds, over the "
            "OctoPrint `just octoprint-up` started, holding a print and a stored image "
            "the supervisor's own ingress opened. A recording proxy sits in front of it, "
            "so what each assertion is against is the request that supervisor received "
            "and the answer it actually sent."
        ),
        "",
        *wrapped(
            "The order is the one a real machine admits: a printer is a state machine "
            "and the policy refuses an action that is not valid from where it is. It "
            "ends with the machine printing, where the bring-up left it."
        ),
        '"""',
        "",
        "from __future__ import annotations",
        "",
        "import json",
        "import time",
        "from collections.abc import Iterator",
        "from typing import cast",
        "",
        "import pytest",
        "from live import PATIENCE_SECONDS, Proxy, same",
        "from printobserver_sdk import Client, RejectedError",
        "from printobserver_sdk.contract import JobManifest",
        "from repo_checks.expect import equal, truth",
        "from supervisor_world import Standing, Supervisor",
        "",
        "#: The reason every mutating call of this walk carries.",
        f'REASON = "{REASON}"',
        "",
        "",
        "def manifest(world: Supervisor) -> JobManifest:",
        '    """The manifest this walk writes and starts a print under.',
        "",
        "    It narrows nothing: what the walk needs is every adjustment the envelope",
        "    allows to be answered, and a narrowing here would refuse one for a reason",
        "    that is not what this walk is about.",
        '    """',
        "    return cast(",
        "        JobManifest,",
        "        {",
        '            "file_name": world.file_name,',
        '            "material": "PLA",',
        '            "nozzle_diameter_mm": 0.4,',
        '            "slicer_profile": "the walk over every operation",',
        '            "allowed": {},',
        '            "metadata": {},',
        "        },",
        "    )",
        "",
        "",
        "def ready(client: Client, print_id: str, wanted: str) -> None:",
        '    """Wait until the machine reports the state one step needs.',
        "",
        "    A real printer is a state machine and the policy refuses an action that is",
        "    not valid from where it is, so a walk that went on regardless would assert",
        "    against a machine that was somewhere else.",
        "",
        "    Raises:",
        "        AssertionError: If it does not, saying what it reported instead.",
        '    """',
        "    if not wanted:",
        "        return",
        "    deadline = time.monotonic() + PATIENCE_SECONDS",
        '    last: object = "nothing was reported"',
        "    while time.monotonic() < deadline:",
        '        printer = client.status(print_id).get("printer")',
        "        if printer is not None:",
        '            last = printer["connection"]',
        "            if last == wanted:",
        "                return",
        "        time.sleep(0.5)",
        '    message = f"the machine reported {last!r} and the next step needs `{wanted}`"',
        "    raise AssertionError(message)",
        "",
        "",
        "@pytest.fixture(scope='module')",
        "def world(tmp_path_factory: pytest.TempPathFactory) -> Iterator[Supervisor]:",
        '    """A real supervisor over the scripted `OctoPrint`, held up for this module."""',
        '    with Standing(tmp_path_factory.mktemp("live")) as supervisor:',
        "        yield supervisor",
        "",
        "",
    ]

    for step in steps:
        call = ", ".join(_live_argument(token) for _, token in step.arguments)
        spelled = method_name(step.name, "python")
        lines += [
            f"def step_{step.name}(client: Client, world: Supervisor, proxy: Proxy) -> None:",
            f'    """`{step.name}`, answered by a real supervisor."""',
            f'    ready(client, world.print_id, "{step.state}")',
            "",
            f"    answered = client.{spelled}({call})",
            "",
            "    seen = proxy.last()",
            f'    equal(seen.method, "{step.method}", describing="`{step.name}`")',
            f'    equal(seen.target, {_live_target(step)}, describing="`{step.name}`")',
            f'    equal(seen.status, 200, describing="`{step.name}`")',
            *_live_body(step),
            f'    same("{step.name}", answered, seen.answer)',
            "",
            "",
        ]

    lines += [
        "def test_every_method_is_answered_by_a_real_supervisor(world: Supervisor) -> None:",
        '    """Every method, answered by a real supervisor, in the one order it admits."""',
        "    with Proxy(world.server) as proxy:",
        '        client = Client(proxy.url, "operator")',
        *(f"        step_{step.name}(client, world, proxy)" for step in steps),
        "",
        "        truth(",
        f"            proxy.calls() >= {len(steps)},",
        '            describing="every call to have gone through the proxy",',
        "        )",
        "",
        "",
    ]

    for step in rejected_live(contract):
        call = ", ".join(_live_argument(token) for _, token in step.arguments)
        spelled = method_name(step.name, "python")
        lines += [
            f"def refused_{step.name}(client: Client, world: Supervisor, proxy: Proxy) -> None:",
            f'    """`{step.name}`, refused by a real supervisor\'s own policy."""',
            "    try:",
            f"        client.{spelled}({call})",
            "    except RejectedError as refused:",
            "        truth(",
            "            isinstance(refused.reason, dict)",
            '            and "actor_may_not_request" in refused.reason,',
            f'            describing="`{step.name}` to be refused for the grant",',
            "        )",
            "        seen = proxy.last()",
            f'        equal(seen.status, 409, describing="`{step.name}`")',
            f'        same("{step.name}", refused.answer, seen.answer)',
            "    else:",
            f'        truth(False, describing="`{step.name}` to be refused")',
            "",
            "",
        ]

    lines += [
        "def test_every_action_is_refused_as_a_typed_rejection(world: Supervisor) -> None:",
        '    """Every action, refused by a real supervisor\'s own policy, typed.',
        "",
        "    One client acting as an actor class the envelope grants nothing. The policy",
        "    takes that decision before it looks at the state, the interval or the",
        "    bounds, so every action is refused from wherever the machine happens to be.",
        '    """',
        "    with Proxy(world.server) as proxy:",
        "        client = Client(",
        "            proxy.url,",
        '            {"agent": {"session_name": "an actor this envelope grants nothing"}},',
        "        )",
        *(f"        refused_{step.name}(client, world, proxy)" for step in rejected_live(contract)),
    ]
    return "\n".join(lines).rstrip("\n") + "\n"
