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


#: The annotation a schema carries when a string is a byte sequence rather than
#: text. The contracts' generator writes it for a type holding bytes.
BYTE_ENCODING = "contentEncoding"

#: The annotation a schema carries when a string is content of a media type.
#: Anything under `image/` is image content, and no answer of this server may
#: carry one: an image is a path on the server's own filesystem, and a client
#: that could be handed the bytes would be a second way to move them.
CONTENT_TYPE = "contentMediaType"

#: The prefix an image's own media type carries.
IMAGE_MEDIA = "image/"

#: The types the contracts declare for an image, neither of which may declare
#: a field carrying its bytes. The walk asserts it reaches them, so the rule
#: guards a boundary something is actually on.
IMAGE_TYPES = ("ImageRecord", "ImageRef")


def response_shapes(repo: Repo) -> list[str]:
    """No shape this server answers carries an image, at any depth.

    Every response shape the server declares is walked transitively — each
    declared field and each variant payload in turn — and refused if it carries
    a byte-sequence field or a string declared as carrying image content. The
    walk is over the schemas themselves rather than over the generated clients,
    and it holds for all three at once, because each client's response types
    correspond to these schemas under `generated_clients` above.

    A byte-sequence field is refused unless `repo-policy.toml` records it by
    type and field with a reason. One entry stands there today — the external
    producer's own body, recorded verbatim so an alert this system cannot read
    is written down rather than dropped — and a *new* byte-sequence field
    cannot appear in a response shape unnoticed, which is the whole point of
    walking rather than looking where an image would be.
    """
    import json

    from contract_codegen.schemas import OPERATIONS_FILE, SERVER_DIR, TYPES_DIR

    schemas: dict[str, dict[str, object]] = {}
    for directory in (TYPES_DIR, SERVER_DIR):
        for path in sorted((repo.root / directory).glob("*.json")):
            if path.name != OPERATIONS_FILE:
                schemas[path.stem] = json.loads(path.read_text(encoding="utf-8"))

    described = json.loads(repo.read(f"{SERVER_DIR}/{OPERATIONS_FILE}"))
    roots = sorted(
        {
            str(answer["type"])
            for operation in described["operations"]
            for answer in operation["responses"]
        }
    )
    if not roots:
        return ["the server describes no answer shape at all, so this walk reads nothing"]

    permitted = _permitted_byte_fields(repo)
    findings: list[str] = []
    reached: set[str] = set()
    seen_sites: set[tuple[str, str]] = set()
    # One finding per site rather than one per answer shape that reaches it: a
    # field two answers both carry is one thing to fix.
    for root in roots:
        if root not in schemas:
            findings.append(
                f"the server describes `{root}` as an answer shape, and no checked-in "
                f"schema declares it"
            )
            continue
        reached.add(root)
        findings.extend(
            _walk(
                schemas,
                schemas[root],
                (root, ""),
                root,
                frozenset({root}),
                reached,
                permitted,
                seen_sites,
            )
        )

    findings = list(dict.fromkeys(findings))
    findings.extend(
        f"`repo-policy.toml` records `{owner}.{field}` as a byte-sequence field a "
        f"response shape may carry, and this walk reaches no such field: an "
        f"exception guarding nothing has stopped being one"
        for owner, field in sorted(permitted)
        if (owner, field) not in seen_sites
    )
    findings.extend(
        f"no shape this server answers reaches `{named}`, so this walk says nothing "
        f"about the one type an image travels as"
        for named in IMAGE_TYPES
        if named not in reached
    )
    return findings


def _permitted_byte_fields(repo: Repo) -> dict[tuple[str, str], str]:
    """Every byte-sequence field a response shape may carry, with its reason."""
    from repo_checks.model import policy_table

    recorded: dict[tuple[str, str], str] = {}
    declared = policy_table(repo, "clients").get("byte_sequence")
    for entry in declared if isinstance(declared, list) else []:
        if not isinstance(entry, dict):
            continue
        owner = str(entry.get("type", ""))
        field = str(entry.get("field", ""))
        reason = str(entry.get("reason", "")).strip()
        if owner and field and reason:
            recorded[owner, field] = reason
    return recorded


def _walk(
    schemas: dict[str, dict[str, object]],
    node: object,
    site: tuple[str, str],
    current: str,
    entered: frozenset[str],
    reached: set[str],
    permitted: dict[tuple[str, str], str],
    seen_sites: set[tuple[str, str]],
) -> list[str]:
    """Every finding one shape carries, following each field and each payload.

    `site` is the type and field a finding is reported against — the type that
    *declares* the field rather than whichever type the field's own reference
    resolves to, because a reader answering the finding edits the declaration.
    `current` is the type whose properties are being descended into, which is
    what a nested field's site is taken from.
    """
    from contract_codegen.schemas import REF_PREFIX

    if isinstance(node, list):
        return [
            finding
            for branch in node
            for finding in _walk(
                schemas, branch, site, current, entered, reached, permitted, seen_sites
            )
        ]
    if not isinstance(node, dict):
        return []

    reference = node.get("$ref")
    if isinstance(reference, str) and reference.startswith(REF_PREFIX):
        named = reference.removeprefix(REF_PREFIX)
        reached.add(named)
        if named in entered or named not in schemas:
            return []
        return _walk(
            schemas, schemas[named], site, named, entered | {named}, reached, permitted, seen_sites
        )

    owner, field = site
    findings: list[str] = []
    if BYTE_ENCODING in node:
        seen_sites.add(site)
        if site not in permitted:
            findings.append(
                f"the shape this server answers carries `{owner}.{field}`, a "
                f"byte-sequence field ({BYTE_ENCODING}: {node[BYTE_ENCODING]!r}). No "
                f"answer of this server may carry bytes unless "
                f"`repo-policy.toml`'s `clients.byte_sequence` records it with a reason"
            )
    declared_media = node.get(CONTENT_TYPE)
    if isinstance(declared_media, str) and declared_media.startswith(IMAGE_MEDIA):
        findings.append(
            f"the shape this server answers carries `{owner}.{field}`, a string "
            f"declared as carrying image content ({CONTENT_TYPE}: {declared_media!r}). "
            f"An image is a path on this server's own filesystem, and no answer of it "
            f"carries the bytes"
        )

    for key, value in node.items():
        if key == "$defs":
            # The copy of the referenced types the generator inlined. This walk
            # follows the reference to that type's own file, so reading these
            # would be reading every shape whether an answer reaches it or not.
            continue
        if key == "properties" and isinstance(value, dict):
            for name, shape in value.items():
                findings.extend(
                    _walk(
                        schemas,
                        shape,
                        (current, str(name)),
                        current,
                        entered,
                        reached,
                        permitted,
                        seen_sites,
                    )
                )
            continue
        findings.extend(
            _walk(schemas, value, site, current, entered, reached, permitted, seen_sites)
        )
    return findings
