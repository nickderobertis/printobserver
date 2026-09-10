"""The Rust client's request and response types, and its one method per operation."""

from __future__ import annotations

from contract_codegen.banner import banner
from contract_codegen.doc import wrapped
from contract_codegen.model import (
    ACTOR_PARAMETER,
    REASON_PARAMETER,
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
from contract_codegen.naming import method_name, pascal, rust_identifier
from contract_codegen.walk import LiveStep

#: What each scalar of the contracts is in Rust.
SCALARS = {
    "string": "String",
    "number": "f64",
    "integer": "i64",
    "boolean": "bool",
}

#: What every generated shape derives. `Eq` is not among them: the contracts
#: carry temperatures and factors, and a float has no total equality.
DERIVES = "#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]"


def type_name(shape: TypeExpr) -> str:
    """One shape, as Rust spells it."""
    match shape:
        case Ref(name):
            return name
        case Scalar(kind):
            return SCALARS[kind]
        case ListOf(item):
            return f"Vec<{type_name(item)}>"
        case MapOf(value):
            return f"BTreeMap<String, {type_name(value)}>"
        case Nullable(inner):
            return f"Option<{type_name(inner)}>"
    msg = f"{shape!r} is not a shape Rust is generated for"
    raise TypeError(msg)


def doc_lines(text: str, indent: str = "") -> list[str]:
    """One description, as a Rust documentation comment.

    Wrapped here rather than left to the formatter: `rustfmt` reflows code and
    leaves comments exactly as it found them, so a description the contracts
    write as one long sentence would be one long line in three clients.
    """
    return [f"{indent}///{f' {line}' if line else ''}" for line in wrapped(text, indent)]


def _field(entry: Field, indent: str, *, public: bool = True) -> list[str]:
    """One property of an object, as a Rust field.

    A field of an enum's own arm is not public: an arm's fields take the
    enum's visibility, and Rust refuses a visibility spelled on one.
    """
    declared = type_name(entry.type)
    lines = doc_lines(entry.doc, indent)
    if not entry.required:
        # A value the answer did not carry has to come back out absent rather
        # than as null: a client whose re-serialization differs from what the
        # server sent is one that changed the answer on the way through.
        if not declared.startswith("Option<"):
            declared = f"Option<{declared}>"
        lines.append(f'{indent}#[serde(default, skip_serializing_if = "Option::is_none")]')
    visibility = "pub " if public else ""
    lines.append(f"{indent}{visibility}{rust_identifier(entry.name)}: {declared},")
    return lines


def _variant(variant: Variant, indent: str) -> list[str]:
    """One arm of a union, as a Rust variant."""
    lines = doc_lines(variant.doc, indent)
    lines.append(f'{indent}#[serde(rename = "{variant.tag}")]')
    name = pascal(variant.tag)
    if variant.unit:
        lines.append(f"{indent}{name},")
        return lines
    if variant.payload is not None:
        lines.append(f"{indent}{name}({type_name(variant.payload)}),")
        return lines
    lines.append(f"{indent}{name} {{")
    for entry in variant.fields:
        lines.extend(_field(entry, f"{indent}    ", public=False))
    lines.append(f"{indent}}},")
    return lines


def _declaration(declaration: Declaration) -> list[str]:
    """One named type of the contracts, as Rust declares it."""
    lines = doc_lines(declaration.doc)
    match declaration:
        case Alias(name, target, _):
            lines.append(f"pub type {name} = {type_name(target)};")
        case Enumeration(name, _, values):
            lines.append("#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]")
            lines.append(f"pub enum {name} {{")
            for value, said in values:
                lines.extend(doc_lines(said, "    "))
                lines.append(f'    #[serde(rename = "{value}")]')
                lines.append(f"    {pascal(value)},")
            lines.append("}")
        case Struct(name, _, fields, variants, tag_field) if tag_field:
            lines.append(DERIVES)
            lines.append(f"pub struct {name} {{")
            for entry in fields:
                lines.extend(_field(entry, "    "))
            lines.extend(
                doc_lines(
                    f"Which arm this is, and what that arm carries. It travels "
                    f"flattened into this object, under `{tag_field}`.",
                    "    ",
                )
            )
            lines.append("    #[serde(flatten)]")
            lines.append(f"    pub {rust_identifier(tag_field)}: {name}{pascal(tag_field)},")
            lines.append("}")
            lines.append("")
            lines.extend(doc_lines(f"Which arm one `{name}` is, and what that arm carries."))
            lines.append(DERIVES)
            lines.append(f'#[serde(tag = "{tag_field}")]')
            lines.append(f"pub enum {name}{pascal(tag_field)} {{")
            for variant in variants:
                lines.extend(_variant(variant, "    "))
            lines.append("}")
        case Struct(name, _, fields, _, _):
            lines.append(DERIVES)
            lines.append(f"pub struct {name} {{")
            for entry in fields:
                lines.extend(_field(entry, "    "))
            lines.append("}")
        case Union(name, _, variants, tag_field):
            lines.append(DERIVES)
            if tag_field:
                lines.append(f'#[serde(tag = "{tag_field}")]')
            lines.append(f"pub enum {name} {{")
            for variant in variants:
                lines.extend(_variant(variant, "    "))
            lines.append("}")
        case _:  # pragma: no cover - the model declares no fifth kind
            msg = f"{declaration!r} is not a declaration Rust is generated for"
            raise TypeError(msg)
    return lines


def _argument(parameter: Parameter) -> str:
    """One value a generated method takes, as Rust declares it."""
    spelled = rust_identifier(parameter.name)
    if parameter.optional:
        return f"{spelled}: Option<{type_name(parameter.carried)}>"
    match parameter.carried:
        case Scalar("string"):
            return f"{spelled}: &str"
        case Scalar(kind):
            return f"{spelled}: {SCALARS[kind]}"
        case Ref(name):
            return f"{spelled}: &{name}"
    msg = f"{parameter!r} is not a value a Rust method takes"
    raise TypeError(msg)


def _target(operation: Operation) -> str:
    """The request target one call is made to, as Rust builds it."""
    path = operation.path
    for parameter in operation.parameters:
        if parameter.located == "path":
            path = path.replace(f"{{{parameter.name}}}", f"{{{rust_identifier(parameter.name)}}}")
    return path


def _method(operation: Operation) -> list[str]:
    """One operation, as the Rust client's own method."""
    supplied = operation.supplied()
    arguments = ", ".join(["&self", *(_argument(parameter) for parameter in supplied)])
    lines = doc_lines(
        f"Call `{operation.name}` on the configured supervisor.\n\n"
        f"# Errors\n\n"
        f"Returns `ClientError` when the supervisor could not be reached, when "
        f"it answered something this client cannot read"
        + (
            ", when this client refuses the call before making it because the "
            "reason is empty, and when the policy refused the action — which "
            "arrives as `ClientError::Rejected`, carrying the reason, the value "
            "asked for and the range allowed."
            if operation.mutating
            else "."
        ),
        "    ",
    )
    lines.append(
        f"    pub fn {method_name(operation.name, 'rust')}({arguments}) "
        f"-> Result<{operation.answer}, ClientError> {{"
    )
    if operation.mutating:
        lines.append("        crate::reason_given(reason)?;")
    lines.append(f'        let target = format!("{_target(operation)}");')

    query = [parameter for parameter in supplied if parameter.located == "query"]
    if query:
        lines.append("        let mut asked: Vec<(String, String)> = Vec::new();")
        for parameter in query:
            spelled = rust_identifier(parameter.name)
            lines.append(f"        if let Some(value) = {spelled} {{")
            lines.append(
                f'            asked.push(("{parameter.name}".to_owned(), value.to_string()));'
            )
            lines.append("        }")
    else:
        lines.append("        let asked: Vec<(String, String)> = Vec::new();")

    body = [parameter for parameter in operation.parameters if parameter.located == "body"]
    if body:
        lines.append("        let mut sending = serde_json::Map::new();")
        for parameter in body:
            spelled = rust_identifier(parameter.name)
            if parameter.name == ACTOR_PARAMETER:
                given = "self.actor()"
            elif parameter.optional:
                lines.append(f"        if let Some(value) = {spelled} {{")
                lines.append(
                    f'            sending.insert("{parameter.name}".to_owned(), '
                    f"crate::as_value(&value)?);"
                )
                lines.append("        }")
                continue
            else:
                given = spelled
            lines.append(
                f'        sending.insert("{parameter.name}".to_owned(), '
                f"crate::as_value(&{given})?);"
            )
        lines.append("        let sending = Some(serde_json::Value::Object(sending));")
    else:
        lines.append("        let sending: Option<serde_json::Value> = None;")

    lines.append(f'        self.call("{operation.method}", &target, &asked, sending.as_ref())')
    lines.append("    }")
    return lines


def emit(contract: Contract) -> str:
    """The whole generated module of the Rust client."""
    lines = [
        line.rstrip()
        for line in banner("//!", "the Rust client's request and response types").split("\n")
    ]
    lines += [
        "",
        "use std::collections::BTreeMap;",
        "",
        "use serde::{Deserialize, Serialize};",
        "",
        "use crate::{Client, ClientError};",
        "",
        *doc_lines(
            "The version of the server contract these types were generated from, which "
            "is the version the type crate declares in the tree they came from."
        ),
        f'pub const CONTRACT_VERSION: &str = "{contract.version}";',
        "",
        *doc_lines(
            "Every operation this client exposes a method for, which is every operation "
            "the server declares and no other."
        ),
        f"pub const OPERATION_NAMES: [&str; {len(contract.operations)}] = [",
        *(f'    "{operation.name}",' for operation in contract.operations),
        "];",
        "",
    ]
    for declaration in contract.declarations:
        lines.extend(_declaration(declaration))
        lines.append("")

    lines.append("impl Client {")
    for operation in contract.operations:
        lines.extend(_method(operation))
        lines.append("")
    lines.append("}")
    return "\n".join(lines) + "\n"


__all__ = ["ACTOR_PARAMETER", "REASON_PARAMETER", "emit", "type_name"]


def _rust_argument(parameter: Parameter, value: object, contract: Contract) -> str:
    """One value the walk calls a Rust method with, as Rust writes it."""
    from contract_codegen.walk import document

    shape = parameter.carried
    match shape:
        case Scalar("string"):
            given = f'"{value}"'
        case Scalar("number") | Scalar("integer") | Scalar("boolean"):
            given = str(value).lower() if isinstance(value, bool) else str(value)
        case Ref(name):
            given = f'&parsed::<{name}>(r#"{document(value)}"#)'
        case _:
            msg = f"{parameter!r} is not a value a Rust walk can supply"
            raise TypeError(msg)
    if not parameter.optional:
        return given
    if isinstance(shape, Ref):
        return f"Some({given.removeprefix('&')})"
    return f"Some({given})"


def emit_walk(contract: Contract) -> str:
    """The whole generated walk of the Rust client."""
    from contract_codegen.walk import REASON_PARAMETER as REASON_NAME
    from contract_codegen.walk import actor, document, plan

    named = sorted(
        {
            parameter.carried.name
            for operation in contract.operations
            for parameter in operation.supplied()
            if isinstance(parameter.carried, Ref)
        }
        | {"Actor", "Client", "ClientError", "RejectionReason"}
    )
    lines = [
        line.rstrip()
        for line in banner(
            "//!", "the Rust client's walk over every operation the server declares"
        ).split("\n")
    ]
    lines += ["//!"]
    lines += [
        f"//!{f' {line}' if line else ''}"
        for line in wrapped(
            "Every method this client exposes is driven here against a stub host on a "
            "real socket: what the host received is asserted against what the operation "
            "declares, and what the method answered is asserted to carry, field for "
            "field, exactly the document the host sent. A client that transformed a "
            "value on the way out fails that equality rather than needing a probe that "
            "guesses how it transformed it."
        )
    ]
    lines += [
        "",
        '#[path = "support/host.rs"]',
        "mod host;",
        "",
        "use host::Host;",
        "use printobserver_sdk::{" + ", ".join(named) + "};",
        "use serde::de::DeserializeOwned;",
        "use serde_json::Value;",
        "",
        "/// One document of the contracts, as the type it is a document of.",
        "///",
        "/// # Panics",
        "///",
        "/// Panics when the document is not one of that type, which is a walk and a",
        "/// client generated from two different descriptions.",
        "fn parsed<T: DeserializeOwned>(document: &str) -> T {",
        '    serde_json::from_str(document).expect("a generated document is of its own type")',
        "}",
        "",
        "/// The actor every client of this walk acts as.",
        "fn actor() -> Actor {",
        f'    parsed::<Actor>(r#"{actor(contract)}"#)',
        "}",
        "",
    ]

    for step in plan(contract):
        call = ", ".join(
            _rust_argument(parameter, value, contract) for parameter, value in step.arguments
        )
        lines += [
            f"/// `{step.name}` sends what it declares and answers what the server sent.",
            "#[test]",
            f"fn {step.name}_sends_what_it_declares_and_answers_what_the_server_sent() {{",
            f'    let answer: Value = serde_json::from_str(r#"{document(step.answer)}"#)',
            '        .expect("a generated answer is a document");',
            "    let host = Host::answering(200, &answer.to_string());",
            "    let client = Client::new(host.address(), actor());",
            "",
            f"    let answered = client.{method_name(step.name, 'rust')}({call})",
            f'        .expect("`{step.name}` is answered");',
            "",
            "    let received = host.received();",
            f'    assert_eq!(received.method, "{step.method}");',
            f'    assert_eq!(received.target, "{step.target}");',
        ]
        if step.body is None:
            lines.append('    assert_eq!(received.body, "");')
        else:
            lines += [
                f'    let sent: Value = serde_json::from_str(r#"{document(step.body)}"#)',
                '        .expect("a generated body is a document");',
                "    assert_eq!(",
                '        serde_json::from_str::<Value>(&received.body).expect("a body"),',
                "        sent,",
                '        "this call sent something other than what its operation declares"',
                "    );",
            ]
        lines += [
            "    assert_eq!(",
            '        serde_json::to_value(&answered).expect("an answer is a document"),',
            "        answer,",
            '        "this call answered something other than what the server sent"',
            "    );",
            "}",
            "",
        ]

        if step.rejection is not None:
            lines += [
                f"/// `{step.name}` surfaces the policy's own refusal, typed.",
                "#[test]",
                f"fn {step.name}_surfaces_the_policys_own_refusal() {{",
                f'    let refusal: Value = serde_json::from_str(r#"{document(step.rejection)}"#)',
                '        .expect("a generated refusal is a document");',
                "    let host = Host::answering(409, &refusal.to_string());",
                "    let client = Client::new(host.address(), actor());",
                "",
                f"    let refused = client.{method_name(step.name, 'rust')}({call})",
                f'        .expect_err("`{step.name}` is refused");',
                "",
                "    let ClientError::Rejected(rejection) = refused else {",
                '        panic!("the policy\'s refusal did not arrive as one");',
                "    };",
                "    assert_eq!(rejection.requested, Some(1.5));",
                "    assert_eq!(",
                "        rejection.allowed.as_ref().map(|range| (range.min, range.max)),",
                "        Some((0.5, 1.25))",
                "    );",
                "    assert!(matches!(rejection.reason, RejectionReason::OutOfBounds { .. }));",
                "}",
                "",
            ]

        if step.operation.mutating:
            blank = ", ".join(
                '"   "'
                if parameter.name == REASON_NAME
                else _rust_argument(parameter, value, contract)
                for parameter, value in step.arguments
            )
            lines += [
                f"/// `{step.name}` makes no request at all when the reason is empty.",
                "#[test]",
                f"fn {step.name}_makes_no_request_when_the_reason_is_empty() {{",
                '    let host = Host::answering(200, "{}");',
                "    let client = Client::new(host.address(), actor());",
                "",
                f"    let refused = client.{method_name(step.name, 'rust')}({blank})",
                '        .expect_err("a call with no reason is refused");',
                "",
                "    assert!(matches!(refused, ClientError::NoReason));",
                '    assert_eq!(host.requests(), 0, "this call reached the server");',
                "}",
                "",
            ]
    return "\n".join(lines) + "\n"


#: How each token the real-supervisor walk supplies a value with is written in
#: Rust. Anything not here is written out as it stands, which is what a literal
#: number or a quoted string is.
LIVE_TOKENS = {
    "world.print_id": "&world.print_id",
    "world.image_id": "&world.image_id",
    "world.event_id": "&world.event_id",
    "world.file_name": "&world.file_name",
    # Already a reference where a step takes one, so nothing adds another.
    "manifest": "manifest",
    "reason": "REASON",
    "disposition:continue": "&printobserver_sdk::AcknowledgementDisposition::Continue",
}

#: The same tokens as the value a body field must have carried, where that is
#: written differently from the argument — a `to_value` takes the value rather
#: than a reference to it.
LIVE_SENT = {
    "disposition:continue": "printobserver_sdk::AcknowledgementDisposition::Continue",
}


def _live_argument(parameter: Parameter, token: str) -> str:
    """One value the real walk calls a Rust method with.

    Every token below is written out as the expression Rust wants, references
    and all: a reference added here would be one the compiler immediately
    takes off again, which is a lint of its own.
    """
    written = LIVE_TOKENS.get(token, token)
    if parameter.optional:
        return f"Some({written})"
    if token not in LIVE_TOKENS and isinstance(parameter.carried, Ref):
        return f"&{written}"
    return written


def emit_live(contract: Contract) -> str:
    """The whole generated walk of the Rust client, against a real supervisor."""
    from contract_codegen.walk import REASON, plan_live, rejected_live

    lines = [
        line.rstrip()
        for line in banner(
            "//!", "the Rust client's walk over every operation, against a real supervisor"
        ).split("\n")
    ]
    lines += ["//!"]
    lines += [
        f"//!{f' {line}' if line else ''}"
        for line in wrapped(
            "Every method this client exposes is driven against the **real** "
            "printobserver supervisor — the program this repository builds, over the "
            "`OctoPrint` `just octoprint-up` started, holding a print and a stored image "
            "the supervisor's own ingress opened. A recording proxy sits in front of it, "
            "so what each assertion is against is the request that supervisor received "
            "and the answer it actually sent."
        )
    ]
    lines += ["//!"]
    lines += [
        f"//!{f' {line}' if line else ''}"
        for line in wrapped(
            "The order is the one a real machine admits: a printer is a state machine "
            "and the policy refuses an action that is not valid from where it is. It "
            "ends with the machine printing, where the bring-up left it."
        )
    ]
    lines += [
        "",
        '#[path = "support/live.rs"]',
        "mod live;",
        '#[path = "support/supervisor.rs"]',
        "mod supervisor;",
        "",
        "use std::collections::BTreeMap;",
        "",
        "use printobserver_sdk::{Actor, Client, ClientError, JobManifest, RejectionReason};",
        "",
        "/// The reason every mutating call of this walk carries.",
        f'const REASON: &str = "{REASON}";',
        "",
        "/// How long the machine is given to reach a state a step needs.",
        "const PATIENCE: std::time::Duration = std::time::Duration::from_secs(180);",
        "",
        "/// The manifest this walk writes and starts a print under.",
        "///",
        "/// It narrows nothing: what the walk needs is every adjustment the envelope",
        "/// allows to be answered, and a narrowing here would refuse one for a reason",
        "/// that is not what this walk is about.",
        "fn manifest(file_name: &str) -> JobManifest {",
        "    JobManifest {",
        "        file_name: file_name.to_owned(),",
        '        material: "PLA".to_owned(),',
        "        nozzle_diameter_mm: 0.4,",
        '        slicer_profile: "the all-operation walk\'s own profile".to_owned(),',
        "        allowed: BTreeMap::new(),",
        "        metadata: BTreeMap::new(),",
        "    }",
        "}",
        "",
        "/// Wait until the machine reports the state one step needs.",
        "///",
        "/// A real printer is a state machine and the policy refuses an action that is",
        "/// not valid from where it is, so a walk that went on regardless would assert",
        "/// against a machine that was somewhere else.",
        "///",
        "/// # Panics",
        "///",
        "/// Panics saying what it reported instead.",
        "fn ready(client: &Client, print_id: &str, wanted: &str) {",
        "    if wanted.is_empty() {",
        "        return;",
        "    }",
        "    let deadline = std::time::Instant::now() + PATIENCE;",
        '    let mut last = String::from("nothing was reported");',
        "    while std::time::Instant::now() < deadline {",
        '        let status = client.status(print_id).expect("a status read is answered");',
        "        if let Some(printer) = status.printer {",
        "            last = serde_json::to_string(&printer.connection).unwrap_or_default();",
        "            if last.trim_matches('\"') == wanted {",
        "                return;",
        "            }",
        "        }",
        "        std::thread::sleep(std::time::Duration::from_millis(500));",
        "    }",
        '    panic!("the machine reported {last} and the next step needs `{wanted}`");',
        "}",
        "",
    ]

    steps = plan_live(contract)
    for step in steps:
        call = ", ".join(_live_argument(parameter, token) for parameter, token in step.arguments)
        spelled = method_name(step.name, "rust")
        carried = _carries_manifest(step)
        lines += [
            f"/// `{step.name}`, answered by a real supervisor.",
            f"fn step_{step.name}(",
            "    client: &Client,",
            "    world: &supervisor::Supervisor,",
            "    proxy: &live::Proxy,",
            *(["    manifest: &printobserver_sdk::JobManifest,"] if carried else []),
            ") {",
            f'    ready(client, &world.print_id, "{step.state}");',
            "",
            f"    let answered = client.{spelled}({call})",
            f'        .expect("`{step.name}` is answered by a real supervisor");',
            "",
            "    let seen = proxy.last();",
            f'    assert_eq!(seen.method, "{step.method}", "`{step.name}`");',
            f'    assert_eq!(seen.target, {_live_target(step)}, "`{step.name}`");',
            f'    assert_eq!(seen.status, 200, "`{step.name}`");',
        ]
        lines.extend(_live_body_assertions(step))
        lines += [
            f'    live::same("{step.name}", &answered, &seen.answer);',
            "}",
            "",
        ]

    for step in rejected_live(contract):
        call = ", ".join(_live_argument(parameter, token) for parameter, token in step.arguments)
        spelled = method_name(step.name, "rust")
        carried = _carries_manifest(step)
        lines += [
            f"/// `{step.name}`, refused by a real supervisor's own policy.",
            f"fn refused_{step.name}(",
            "    client: &Client,",
            "    world: &supervisor::Supervisor,",
            "    proxy: &live::Proxy,",
            *(["    manifest: &printobserver_sdk::JobManifest,"] if carried else []),
            ") {",
            f"    let refused = client.{spelled}({call})",
            f'        .expect_err("`{step.name}` is refused for an actor granted nothing");',
            "",
            "    let ClientError::Rejected(rejection) = refused else {",
            f'        panic!("`{step.name}`: the refusal did not arrive as a rejection");',
            "    };",
            "    assert!(",
            "        matches!(rejection.reason, RejectionReason::ActorMayNotRequest { .. }),",
            f'        "`{step.name}` was refused for something other than the grant: {{:?}}",',
            "        rejection.reason",
            "    );",
            "    let seen = proxy.last();",
            f'    assert_eq!(seen.status, 409, "`{step.name}`");',
            f'    live::same("{step.name}", rejection.answer.as_ref(), &seen.answer);',
            "}",
            "",
        ]

    lines += [
        "/// Every method, answered by a real supervisor, in the one order it admits.",
        "#[test]",
        "fn every_method_is_answered_by_a_real_supervisor() {",
        '    let root = tempfile::tempdir().expect("this walk\'s own root");',
        "    let mut standing = supervisor::standing(root.path());",
        "    let world = standing.at.clone();",
        "    let proxy = live::Proxy::in_front_of(&world.server);",
        "    let client = Client::new(proxy.url(), Actor::Operator);",
        "    let manifest = manifest(&world.file_name);",
        "",
        *(f"    step_{step.name}(&client, &world, &proxy{_live_handed(step)});" for step in steps),
        "",
        f'    assert!(proxy.calls() >= {len(steps)}, "every call went through the proxy");',
        "    standing.stop();",
        "}",
        "",
        "/// Every action, refused by a real supervisor's own policy, as a typed rejection.",
        "///",
        "/// One client acting as an actor class the envelope grants nothing. The policy",
        "/// takes that decision before it looks at the state, the interval or the",
        "/// bounds, so every action is refused from wherever the machine happens to be.",
        "#[test]",
        "fn every_action_is_refused_as_a_typed_rejection_by_a_real_supervisor() {",
        '    let root = tempfile::tempdir().expect("this walk\'s own root");',
        "    let mut standing = supervisor::standing(root.path());",
        "    let world = standing.at.clone();",
        "    let proxy = live::Proxy::in_front_of(&world.server);",
        "    let client = Client::new(",
        "        proxy.url(),",
        "        Actor::Agent {",
        '            session_name: "an actor this envelope grants nothing".to_owned(),',
        "        },",
        "    );",
        "    let manifest = manifest(&world.file_name);",
        "",
        *(
            f"    refused_{step.name}(&client, &world, &proxy{_live_handed(step)});"
            for step in rejected_live(contract)
        ),
        "",
        "    standing.stop();",
        "}",
    ]
    return "\n".join(lines) + "\n"


def _carries_manifest(step: LiveStep) -> bool:
    """Whether one step's own call takes the manifest the walk wrote."""
    return any(given == "manifest" for _, given in step.arguments)


def _live_handed(step: LiveStep) -> str:
    """What one step is handed beside the client, the world and the proxy."""
    return ", &manifest" if _carries_manifest(step) else ""


def _live_target(step: LiveStep) -> str:
    """The request target one real call must reach, as Rust builds it."""
    operation = step.operation
    path = operation.path
    for parameter in operation.parameters:
        if parameter.located == "path":
            path = path.replace(f"{{{parameter.name}}}", f"{{{parameter.name}}}")
    asked = [
        f"{parameter.name}={token}"
        for parameter, token in step.arguments
        if parameter.located == "query"
    ]
    whole = f"{path}?{'&'.join(asked)}" if asked else path
    bindings = ", ".join(
        f"{parameter.name} = world.{parameter.name}"
        for parameter in operation.parameters
        if parameter.located == "path"
    )
    return f'format!("{whole}", {bindings})' if bindings else f'"{whole}"'


def _live_body_assertions(step: LiveStep) -> list[str]:
    """What the supervisor must have received in the body of one real call."""
    operation = step.operation
    body = [parameter for parameter in operation.parameters if parameter.located == "body"]
    if not body:
        return [f'    assert_eq!(seen.body, "", "`{operation.name}` sends no body");']
    supplied = {parameter.name: token for parameter, token in step.arguments}
    lines = [
        "    let sent: serde_json::Value = serde_json::from_str(&seen.body)",
        '        .expect("the supervisor received a document");',
        f'    assert_eq!(sent["actor"], serde_json::json!("operator"), "`{operation.name}`");'
        if any(parameter.name == "actor" for parameter in body)
        else "",
    ]
    for parameter in body:
        if parameter.name == "actor":
            continue
        token = supplied.get(parameter.name)
        if token is None:
            continue
        lines.append(
            f"    assert_eq!(\n"
            f'        sent["{parameter.name}"],\n'
            f"        serde_json::to_value({_live_sent(parameter, token)})"
            f'.expect("a value renders"),\n'
            f'        "`{operation.name}` sent another `{parameter.name}`"\n'
            f"    );"
        )
    return [line for line in lines if line]


def _live_sent(parameter: Parameter, token: str) -> str:
    """The value one body field must have carried, as Rust names it."""
    _ = parameter
    return LIVE_SENT.get(token) or LIVE_TOKENS.get(token, token)
