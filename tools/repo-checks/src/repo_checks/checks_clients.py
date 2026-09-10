"""Checks over the three clients of the server's own API.

Two rules, and both are about the same thing: three clients of one server are
one product only for as long as something refuses a tree in which they have
come apart.

The first is **correspondence**: every generated file of every client is what
`contract-codegen` writes from the checked-in schemas. A finished tree cannot
show where its bytes came from — a hand-maintained type matching the
generator's output is, in the tree, indistinguishable from a generated one — so
what is checked is that regenerating leaves the tree unchanged. A hand-written
type agreeing today fails the moment the schema it agreed with moves.

The second is **the surface**: each client exposes one method per operation the
server declares and no method no operation backs, with the reason required and
the duration optional exactly as the operation declares them.
"""

from __future__ import annotations

import re
from dataclasses import dataclass

from contract_codegen.banner import MARK
from contract_codegen.generate import OUTPUTS, FormatterError, drifted
from contract_codegen.model import ACTOR_PARAMETER, REASON_PARAMETER, Operation
from contract_codegen.naming import method_name
from contract_codegen.schemas import ContractError, load

from repo_checks.model import Repo


@dataclass(frozen=True, slots=True)
class Client:
    """One of the three clients, and how a method is read out of its source."""

    #: Which client it is, spelled as the generator spells it.
    language: str
    #: Where its generated module lives, relative to the repository root.
    generated: str
    #: How one method's definition opens in that language.
    definition: str

    def methods(self, source: str) -> dict[str, str]:
        """Every method the generated module defines, by name, with its signature."""
        found: dict[str, str] = {}
        for match in re.finditer(
            rf"{self.definition}(?P<name>[A-Za-z_][A-Za-z0-9_]*)\((?P<taken>[^)]*)\)",
            source,
        ):
            found[match["name"]] = match["taken"]
        return found


#: The three clients, in the order the generator writes them.
CLIENTS: tuple[Client, ...] = (
    Client("rust", "crates/printobserver-sdk/src/contract.rs", r"pub fn "),
    Client(
        "python",
        "python/printobserver-sdk/src/printobserver_sdk/contract.py",
        r"    def ",
    ),
    Client("typescript", "npm/printobserver-sdk/src/contract.ts", r"  async "),
)

#: Method names the surface walk expects a generated module to carry beside the
#: operations. `call` is the transport every generated method is written
#: against and is declared by the hand-written surface, not generated.
BESIDE_THE_OPERATIONS = frozenset({"call"})


def generated_clients(repo: Repo) -> list[str]:
    """Every generated file of the three clients is what the generator writes."""
    findings = _banner_findings(repo)
    try:
        findings.extend(drifted(repo.root))
    except (ContractError, FormatterError) as refused:
        findings.append(f"the client generator could not run over this tree: {refused}")
    return findings


def _banner_findings(repo: Repo) -> list[str]:
    """Every generated file says in its own text that it is generated."""
    return [
        f"{output.path} does not say it is generated: a reader who edits it has "
        f"nothing telling them the edit will not survive"
        for output in OUTPUTS
        if not repo.exists(output.path) or MARK not in repo.read(output.path)
    ]


def client_surface(repo: Repo) -> list[str]:
    """Each client exposes one method per operation the server declares."""
    try:
        contract = load(repo.root)
    except (ContractError, KeyError, OSError) as refused:
        return [f"the server's own operation list could not be read: {refused}"]

    findings: list[str] = []
    declared = {operation.name: operation for operation in contract.operations}
    for client in CLIENTS:
        if not repo.exists(client.generated):
            findings.append(
                f"the {client.language} client carries no generated module at "
                f"{client.generated}, so it exposes no operation at all"
            )
            continue
        exposed = client.methods(repo.read(client.generated))
        expected = {
            method_name(name, client.language): operation for name, operation in declared.items()
        }
        findings.extend(
            f"the server declares operation `{declared_name}`, which the "
            f"{client.language} client exposes no method for"
            for declared_name, spelled in (
                (name, method_name(name, client.language)) for name in declared
            )
            if spelled not in exposed
        )
        findings.extend(
            f"the {client.language} client exposes `{spelled}`, which no operation "
            f"the server declares backs"
            for spelled in sorted(exposed)
            if spelled not in expected and spelled not in BESIDE_THE_OPERATIONS
        )
        for spelled, operation in expected.items():
            if spelled in exposed:
                findings.extend(_signature_findings(client, spelled, operation, exposed[spelled]))
    return findings


def _signature_findings(
    client: Client, spelled: str, operation: Operation, taken: str
) -> list[str]:
    """The reason is required and the duration optional, in every client."""
    findings: list[str] = []
    for parameter in operation.parameters:
        if parameter.name == ACTOR_PARAMETER:
            # Who a client acts as is what it was constructed with, so no
            # method takes it: one that did could act as somebody else.
            if _mentions(client, taken, parameter.name):
                findings.append(
                    f"the {client.language} client's `{spelled}` takes "
                    f"`{parameter.name}`, which is what the client was made with"
                )
            continue
        if not _mentions(client, taken, parameter.name):
            findings.append(
                f"the {client.language} client's `{spelled}` does not take "
                f"`{parameter.name}`, which the operation's own request declares"
            )
            continue
        optional = _optional(client, taken, parameter.name)
        if parameter.optional and not optional:
            findings.append(
                f"the {client.language} client's `{spelled}` requires "
                f"`{parameter.name}`, which the operation declares optional"
            )
        if not parameter.optional and optional:
            findings.append(
                f"the {client.language} client's `{spelled}` takes "
                f"`{parameter.name}` as optional, which the operation requires"
                + (
                    " — a mutating call with no reason is refused by the client "
                    "before a request is made, and that is not the same as one "
                    "a caller may leave out"
                    if parameter.name == REASON_PARAMETER
                    else ""
                )
            )
    return findings


def _spelling(client: Client, name: str) -> str:
    """How one value's name is spelled in one client's method signature."""
    if client.language == "typescript":
        from contract_codegen.naming import camel

        return camel(name)
    return name


def _mentions(client: Client, taken: str, name: str) -> bool:
    """Whether one signature takes a value of that name."""
    return re.search(rf"\b{re.escape(_spelling(client, name))}\b", taken) is not None


def _optional(client: Client, taken: str, name: str) -> bool:
    """Whether one signature takes that value as one a caller may leave out."""
    spelled = re.escape(_spelling(client, name))
    if client.language == "typescript":
        return re.search(rf"\b{spelled}\?", taken) is not None
    if client.language == "python":
        return re.search(rf"\b{spelled}: [^,]*= None", taken) is not None
    return re.search(rf"\b{spelled}: Option<", taken) is not None
