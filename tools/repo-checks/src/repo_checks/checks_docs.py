"""The agent-facing documentation, held to the things it documents.

Three checks, and each of them reads the tree rather than the document's own
claims about itself.

`skill` holds the committed skill to the two bounds `repo-policy.toml` declares,
to the nine things it owes in its own text, and to carrying no reference
material of its own — an argument list, an output description, a schema fragment
or a worked example, in whatever form. The skill is sent as the system prompt of
every supervision turn, so its length is paid on every turn of every print; a
skill that restates the reference documentation crowds out the picture of the
print it is supposed to be looking at.

`reference` holds each declared document to its declared headings, to an opening
statement that says what it covers and is true of the document itself, and to
inventories that are complete against **the thing they inventory** — the command
surface the program declares, the operations the server serves, the crates the
workspace declares, the tiers the configuration declares, the fields the
contracts' rejection type declares. A document cannot be complete by listing
less.

`schema_document` holds the generated schema document to the schema set the
contracts' own generation target maintains, in both directions.
"""

from __future__ import annotations

import json
import re
import tomllib
from dataclasses import dataclass
from pathlib import Path, PurePosixPath
from typing import Any

from repo_checks.docs import (
    DocsPolicy,
    Document,
    docs_policy,
    entries_of,
    entry_body,
    headings_of,
    links_of,
    opening_statement,
    schema_document_text,
    schema_members,
    section_of,
)
from repo_checks.model import (
    UNCOMMITTED_DIRECTORIES,
    PolicyValueError,
    Repo,
    policy_string_list,
    policy_table,
)

#: A markdown table's cell separator, which needs two of them to be a row.
TABLE_ROW = re.compile(r"\|.*\|")

#: A command-line option as a document writes one.
OPTION = re.compile(r"(?<![\w-])--[a-z][a-z0-9-]*")

#: A backticked token, which is how a document names something in the tree.
BACKTICKED = re.compile(r"`([^`]+)`")

#: A word that could be the name of an option or of a value a request carries.
#:
#: Compound rather than any word: `reason` is a field of every mutating request
#: and is also the ordinary English word the skill has to use, while `print_id`,
#: `duration-s` and `image_path` are spellings nothing but this surface has.
COMPOUND = re.compile(r"(?<![\w-])[A-Za-z][A-Za-z0-9]*(?:[_-][A-Za-z0-9]+)+(?![\w-])")

#: Punctuation that makes a line a fragment of a JSON document or schema.
SCHEMA_PUNCTUATION = ('":', '{"', "$ref", "oneOf", "$defs")

#: What an entry of the command-surface document owes beside its arguments.
COMMAND_ENTRY_LABELS = ("**Output.**", "**Failures.**")

#: The shape a document writes a test function's name in.
TEST_NAME = re.compile(r"^[a-z][a-z0-9_]{19,}$")

#: How the source that ships the skill names an asset it carries into the artifact.
BUNDLED = re.compile(r'include_str!\("\.\./assets/([^"]+)"\)')


@dataclass(frozen=True, slots=True)
class Surface:
    """The command surface, read off the artifact the program generates."""

    #: Every command, by the name a caller types.
    commands: dict[str, dict[str, Any]]
    #: Every public operation, by the name the server declares it under.
    operations: tuple[str, ...]
    #: Every option every command takes of its own, and the four global ones.
    options: frozenset[str]
    #: Every value name a request carries.
    fields: frozenset[str]


def _read_surface(repo: Repo, policy: DocsPolicy) -> Surface | str:
    """The generated surface manifest, or why it could not be read."""
    if not repo.exists(policy.surface_manifest):
        return (
            f"the generated surface manifest `{policy.surface_manifest}` is absent. "
            f"Run `just docs-generate`: the documentation checks read this program's "
            f"command surface off it."
        )
    try:
        document = json.loads(repo.read(policy.surface_manifest))
    except json.JSONDecodeError as error:
        return f"the generated surface manifest `{policy.surface_manifest}` is not JSON: {error}"
    entries = document.get("commands") if isinstance(document, dict) else None
    if not isinstance(entries, list) or not entries:
        return f"the generated surface manifest `{policy.surface_manifest}` names no command"
    commands = {entry["command"]: entry for entry in entries if isinstance(entry, dict)}
    options = {
        option["option"] for entry in commands.values() for option in entry.get("options", [])
    }
    options.update(document.get("global_options", []))
    return Surface(
        commands=commands,
        operations=tuple(
            entry["operation"] for entry in commands.values() if entry.get("operation")
        ),
        options=frozenset(options),
        fields=frozenset(
            option["field"] for entry in commands.values() for option in entry.get("options", [])
        ),
    )


def _reference_vocabulary(repo: Repo, policy: DocsPolicy, surface: Surface) -> set[str]:
    """Every token that names something the reference documents own.

    A skill carrying one of these in backticks is carrying reference material:
    an option, a value a request takes, a command, or a field of something this
    system answers. The set is read off the generated surface manifest and the
    generated schema set rather than written here, so a vocabulary that grows
    grows this with it.
    """
    vocabulary = set(surface.options) | set(surface.fields) | set(surface.commands)
    vocabulary |= {option.lstrip("-") for option in surface.options}
    for _, _, path in schema_members(repo, policy.schema_directory):
        try:
            schema = json.loads(path.read_text(encoding="utf-8"))
        except json.JSONDecodeError:
            continue
        if isinstance(schema, dict) and isinstance(schema.get("properties"), dict):
            vocabulary |= set(schema["properties"])
    return vocabulary


def skill(repo: Repo) -> list[str]:
    """The committed skill is short, carries what it owes, and restates nothing."""
    try:
        policy = docs_policy(repo)
    except PolicyValueError as error:
        return [str(error)]
    if not repo.exists(policy.skill):
        return [f"the committed skill `{policy.skill}` is absent"]

    if not repo.exists(policy.bundle_source):
        return [
            f"the source that bundles the reference documents, `{policy.bundle_source}`, is absent"
        ]

    text = repo.read(policy.skill)
    return [
        *_bounds(policy, text),
        *_reference_material(repo, policy, text),
        *_skill_links(repo, policy, text),
        *_bundled_references(repo, policy, repo.read(policy.bundle_source)),
        *_missing_elements(policy, text),
    ]


def _bounds(policy: DocsPolicy, text: str) -> list[str]:
    """The two bounds, which are two because either alone is satisfiable by shape."""
    findings: list[str] = []
    characters = len(text)
    lines = text.splitlines()
    if characters > policy.skill_max_characters:
        findings.append(
            f"`{policy.skill}` is {characters} characters, and the bound is "
            f"{policy.skill_max_characters}. It is sent as the system prompt of every "
            f"supervision turn, so every character of it is paid on every turn: move "
            f"what it says into a reference document and link to it."
        )
    if len(lines) > policy.skill_max_lines:
        findings.append(
            f"`{policy.skill}` is {len(lines)} lines, and the bound is {policy.skill_max_lines}"
        )
    return findings


def _missing_elements(policy: DocsPolicy, text: str) -> list[str]:
    """Everything the skill owes in its own text and does not say."""
    # Markers are matched over whitespace-normalized text, so a passage that
    # wraps at a different column is the same passage. What removing the passage
    # removes is the marker, which is what this is about.
    flowed = " ".join(text.split())
    return [
        f"`{policy.skill}` carries nothing saying {element.name}. That is the whole of "
        f"what the agent is given before it starts reading, so it cannot be left to a "
        f"document the skill links to. The passage carrying it is the one whose text "
        f"reads `{element.marker}`."
        for element in policy.elements
        if " ".join(element.marker.split()) not in flowed
    ]


def _reference_material(repo: Repo, policy: DocsPolicy, text: str) -> list[str]:
    """Every piece of reference material the skill carries, whatever its form."""
    surface = _read_surface(repo, policy)
    if isinstance(surface, str):
        return [surface]
    vocabulary = _reference_vocabulary(repo, policy, surface)

    findings: list[str] = []
    for number, line in enumerate(text.splitlines(), start=1):
        where = f"`{policy.skill}`:{number}"
        if line.startswith("```"):
            findings.append(
                f"{where} opens a fenced block. A worked example belongs in a reference "
                f"document, not in the skill."
            )
        if re.match(r"^(\t| {4,})\S", line):
            findings.append(
                f"{where} is an indented block, which is a worked example in another "
                f"form. It belongs in a reference document."
            )
        if len(TABLE_ROW.findall(line)) >= 1 and line.count("|") >= 2:
            findings.append(
                f"{where} is a table row. An argument table belongs in a reference document."
            )
        found = OPTION.findall(line)
        if found:
            findings.append(
                f"{where} names the option `{found[0]}`. An argument list belongs in a "
                f"reference document, whether it is written as a table, as bullets or as "
                f"prose."
            )
        if any(marker in line for marker in SCHEMA_PUNCTUATION) or ("{" in line and "}" in line):
            findings.append(
                f"{where} carries a schema or document fragment. The schemas are "
                f"generated into a reference document of their own."
            )
        named = [token for token in BACKTICKED.findall(line) if token in vocabulary]
        named.extend(token for token in COMPOUND.findall(line) if token in vocabulary)
        if named:
            findings.append(
                f"{where} names `{named[0]}`, which is part of the command surface or of "
                f"a shape this system answers. An argument list or an output description "
                f"belongs in a reference document."
            )
    return findings


def _skill_links(repo: Repo, policy: DocsPolicy, text: str) -> list[str]:
    """The skill links to every declared document, by a path an install can keep.

    Links are resolved **relative to the skill's own file** rather than to the
    repository root, because that is the anchor an installed program can
    reproduce: the composition root writes the reference documents beside the
    skill it materialized, and runs the agent with that directory as its working
    directory. A link that escaped that directory would resolve in a checkout and
    nowhere else, so one is refused here rather than discovered by an agent.
    """
    beside = repo.path(policy.skill).parent
    targets = links_of(text)
    findings: list[str] = []
    reached: set[Path] = set()
    for target in sorted(set(targets)):
        if target.startswith(("/", "#")) or "://" in target:
            findings.append(
                f"`{policy.skill}` links to `{target}`, which is not a path beside the "
                f"skill. An installed program materializes the documents beside the "
                f"skill it wrote, so a link it cannot reproduce is a dead link there."
            )
            continue
        if ".." in PurePosixPath(target).parts:
            findings.append(
                f"`{policy.skill}` links to `{target}`, which climbs out of the skill's "
                f"own directory. An installed program can only carry what sits beside "
                f"the skill it materialized."
            )
            continue
        resolved = (beside / target).resolve()
        if not resolved.exists():
            findings.append(f"`{policy.skill}` links to `{target}`, and there is no such document")
            continue
        reached.add(resolved)
    findings.extend(
        f"`{policy.skill}` links to no declared reference document `{document.path}`. "
        f"Everything the skill does not say itself has to be reachable from it."
        for document in policy.documents
        if repo.path(document.path).resolve() not in reached
    )
    return findings


def _bundled_references(repo: Repo, policy: DocsPolicy, source: str) -> list[str]:
    """The built artifact carries every document the skill is allowed to link to.

    Read off the `include_str!` calls of the source that ships the skill and
    compared with the declared documents **on the filesystem**, so a document is
    bundled by being that document rather than by being spelled the same way.
    """
    beside = repo.path(policy.bundle_assets)
    carried = {
        (beside / captured).resolve()
        for captured in BUNDLED.findall(source)
        if captured.startswith(f"{policy.bundle_directory}/")
    }
    declared = {repo.path(document.path).resolve(): document.path for document in policy.documents}
    findings = [
        f"`{policy.bundle_source}` bundles no reference document for `{path}`. The skill "
        f"links to it, and an install that carried the skill and not the document would "
        f"hand the agent a dead link."
        for resolved, path in sorted(declared.items(), key=lambda entry: entry[1])
        if resolved not in carried
    ]
    findings.extend(
        f"`{policy.bundle_source}` bundles `{resolved}`, which this repository declares no "
        f"reference document for"
        for resolved in sorted(carried - set(declared))
    )
    return findings


def _crate_dependencies(repo: Repo, crate: str) -> set[str]:
    """Every crate of this workspace one crate's manifest names as a dependency."""
    manifest = repo.path("crates") / crate / "Cargo.toml"
    if not manifest.is_file():
        return set()
    with manifest.open("rb") as handle:
        parsed = tomllib.load(handle)
    named: set[str] = set()
    for section in ("dependencies", "dev-dependencies", "build-dependencies"):
        table = parsed.get(section)
        if isinstance(table, dict):
            named |= {name for name in table if name in repo.crate_names}
    return named


def reference(repo: Repo) -> list[str]:
    """Every declared document is there, says what it covers, and inventories the tree."""
    try:
        policy = docs_policy(repo)
    except PolicyValueError as error:
        return [str(error)]
    surface = _read_surface(repo, policy)
    if isinstance(surface, str):
        return [surface]

    declared_headings = {heading for document in policy.documents for heading in document.headings}
    findings: list[str] = []
    inventories = {
        "command-surface.md": _command_surface,
        "api-and-clients.md": _api_and_clients,
        "architecture.md": _architecture,
        "common-operations.md": _common_operations,
        "testing.md": _testing,
        "intervention-policy.md": _intervention_policy,
    }
    for document in policy.documents:
        if not repo.exists(document.path):
            findings.append(
                f"the declared reference document `{document.path}` is absent, and the "
                f"skill links to it"
            )
            continue
        text = repo.read(document.path)
        findings.extend(_document_frame(document, text, declared_headings))
        inventory = inventories.get(Path(document.path).name)
        if inventory is not None:
            findings.extend(inventory(repo, document.path, text, surface))
    return findings


def _document_frame(document: Document, text: str, declared_headings: set[str]) -> list[str]:
    """The headings a document owes, and an opening statement true of it."""
    findings: list[str] = []
    carried = headings_of(text)
    findings.extend(
        f"`{document.path}` does not carry the declared heading `## {heading}`"
        for heading in document.headings
        if heading not in carried
    )

    findings.extend(
        f"`{document.path}`'s `{entry}` entry says nothing. A complete inventory with "
        f"empty prose beside it is a list rather than a document."
        for entry in entries_of(text)
        if not entry_body(text, entry).strip()
    )

    said = opening_statement(text)
    if not said:
        return [
            *findings,
            f"`{document.path}` carries no opening coverage statement: it begins with its "
            f"first heading or with content rather than by saying what it covers",
        ]
    lowered = " ".join(said.split()).lower()
    findings.extend(
        f"`{document.path}` carries `## {heading}` and its opening statement does not say "
        f"it covers it"
        for heading in carried
        if " ".join(heading.split()).lower() not in lowered
    )
    findings.extend(
        f"`{document.path}` says it covers `{heading}`, and it carries no such section"
        for heading in sorted(declared_headings)
        if " ".join(heading.split()).lower() in lowered and heading not in carried
    )
    return findings


def _inventory(named: list[str], expected: list[str], where: str, thing: str) -> list[str]:
    """Hold one document's entries to the thing they inventory, in both directions."""
    findings = [
        f"`{where}` carries no entry for the {thing} `{entry}`"
        for entry in expected
        if entry not in named
    ]
    findings.extend(
        f"`{where}` carries an entry `{entry}`, and there is no such {thing}"
        for entry in named
        if entry not in expected
    )
    return findings


def _command_surface(repo: Repo, where: str, text: str, surface: Surface) -> list[str]:
    """One entry per command, each naming its arguments, its output and its failures."""
    section = section_of(text, "The commands")
    named = entries_of(section)
    findings = _inventory(named, list(surface.commands), where, "command")
    for name, command in surface.commands.items():
        if name not in named:
            continue
        body = entry_body(text, name)
        findings.extend(
            f"`{where}`'s `{name}` entry does not name the argument `{option['option']}`, "
            f"which that command declares"
            for option in command.get("options", [])
            if f"`{option['option']}`" not in body
        )
        findings.extend(
            f"`{where}`'s `{name}` entry states no {label.strip('*.').lower()}"
            for label in COMMAND_ENTRY_LABELS
            if label not in body
        )
    return findings


def _exported_methods(repo: Repo, client: str) -> list[str]:
    """Every method one Rust client crate exports."""
    sources = repo.path("crates") / client / "src"
    if not sources.is_dir():
        return []
    found: list[str] = []
    for path in sorted(sources.rglob("*.rs")):
        found.extend(
            re.findall(
                r"^\s*pub (?:const |async )*fn ([a-z_][a-z0-9_]*)",
                path.read_text(encoding="utf-8"),
                re.MULTILINE,
            )
        )
    return sorted(set(found))


def _api_and_clients(repo: Repo, where: str, text: str, surface: Surface) -> list[str]:
    """One entry per operation, and one entry per method every declared client exports."""
    findings = _inventory(
        entries_of(section_of(text, "The operations")),
        list(surface.operations),
        where,
        "operation",
    )
    clients = list(policy_string_list(policy_table(repo, "crates"), "clients", "crates"))
    section = section_of(text, "The clients")
    findings.extend(_inventory(entries_of(section), clients, where, "client"))
    for client in clients:
        body = entry_body(text, client)
        named = [line[5:].strip() for line in body.splitlines() if line.startswith("#### ")]
        findings.extend(
            _inventory(named, _exported_methods(repo, client), where, f"method of `{client}`")
        )
    return findings


def _architecture(repo: Repo, where: str, text: str, surface: Surface) -> list[str]:
    """One entry per crate, and a dependency direction the manifests agree with."""
    findings = _inventory(
        entries_of(section_of(text, "The crates")), repo.crate_names, where, "crate"
    )
    # The surface it says the agent works through is the one the command-surface
    # document inventories, rather than a surface it describes for itself.
    inventoried = next(
        (
            document.path
            for document in docs_policy(repo).documents
            if Path(document.path).name == "command-surface.md"
        ),
        "",
    )
    reaching = section_of(text, "Why the agent reaches this system the way an operator does")
    if inventoried and Path(inventoried).name not in reaching:
        findings.append(
            f"`{where}` says the agent reaches this system the way an operator does and "
            f"does not point at `{inventoried}`, which is the document that inventories "
            f"that surface"
        )
    core = policy_table(repo, "crates").get("core")
    if not isinstance(core, str):
        return [*findings, "`repo-policy.toml` declares no `crates.core`"]
    section = section_of(text, "Why core names no implementation crate")
    stated: set[str] | None = None
    for line in section.splitlines():
        if f"`{core}` depends" in line:
            stated = set()
        if stated is not None:
            stated |= {token for token in BACKTICKED.findall(line) if token in repo.crate_names}
            if line.strip().endswith("."):
                break
    if stated is None:
        return [
            *findings,
            f"`{where}` states no dependency direction for `{core}`: the section says why "
            f"the rule holds and never says what the rule is",
        ]
    stated.discard(core)
    actual = _crate_dependencies(repo, core)
    findings.extend(
        f"`{where}` says `{core}` depends on `{crate}`, and its manifest does not"
        for crate in sorted(stated - actual)
    )
    findings.extend(
        f"`{where}` omits `{crate}` from what `{core}` depends on, and its manifest names it"
        for crate in sorted(actual - stated)
    )
    return findings


def _common_operations(repo: Repo, where: str, text: str, surface: Surface) -> list[str]:
    """One worked example per operation, each a sequence a reader can follow."""
    section = section_of(text, "The worked examples")
    named = entries_of(section)
    findings = _inventory(named, list(surface.operations), where, "operation")
    findings.extend(
        f"`{where}`'s `{operation}` entry carries no worked example a reader can follow"
        for operation in surface.operations
        if operation in named and "$ printobserver " not in entry_body(text, operation)
    )
    return findings


def _tier_schedules(repo: Repo) -> tuple[list[str], dict[str, str]]:
    """Every tier the configuration declares, and the cron each scheduled one runs on."""
    gate = policy_table(repo, "gate")
    tiers = list(policy_string_list(gate, "tiers", "gate"))
    schedules: dict[str, str] = {}
    # Every tier outside the gate is a table of `repo-policy.toml` declaring a
    # `tier` recipe, and they are found by that rather than by being named here.
    # Naming them was how the judged-lint tier came to be undocumented: it is
    # neither a gate tier nor one of the two this loop used to ask for, so the
    # inventory was complete without it and nothing said so.
    for _, table in sorted(repo.policy.items()):
        if not isinstance(table, dict):
            continue
        tier = table.get("tier")
        if not isinstance(tier, str):
            continue
        tiers.append(tier)
        workflow = table.get("workflow")
        if not isinstance(workflow, str):
            continue
        from repo_checks.parsing import load_workflow

        triggers = load_workflow(repo.path(".github/workflows") / workflow).get("on")
        if not isinstance(triggers, dict) or "schedule" not in triggers:
            continue
        for entry in triggers["schedule"] or []:
            if isinstance(entry, dict) and isinstance(entry.get("cron"), str):
                schedules[tier] = entry["cron"]
    return tiers, schedules


def _testing(repo: Repo, where: str, text: str, surface: Surface) -> list[str]:
    """One entry per tier, and the committed schedule for every scheduled one."""
    tiers, schedules = _tier_schedules(repo)
    named = entries_of(text)
    findings = _inventory(named, tiers, where, "tier")
    for tier, cron in schedules.items():
        if tier not in named:
            continue
        body = entry_body(text, tier)
        if f"`{cron}`" not in body:
            findings.append(
                f"`{where}`'s `{tier}` entry does not give the schedule the committed "
                f"configuration gives that tier, which is the cron `{cron}`"
            )
    return findings


def _rejection_vocabulary(repo: Repo, policy: DocsPolicy) -> set[str] | str:
    """Every variant and field the contracts' rejection type declares."""
    path = repo.path(policy.schema_directory) / "printobserver-types" / "RejectionReason.json"
    if not path.is_file():
        return "the contracts generate no `RejectionReason` schema to read a rejection off"
    schema = json.loads(path.read_text(encoding="utf-8"))
    named: set[str] = set()
    for arm in schema.get("oneOf", []):
        if not isinstance(arm, dict):
            continue
        if isinstance(arm.get("const"), str):
            named.add(arm["const"])
        for tag, body in (arm.get("properties") or {}).items():
            named.add(tag)
            if isinstance(body, dict) and isinstance(body.get("properties"), dict):
                named |= set(body["properties"])
    return named


def _intervention_policy(repo: Repo, where: str, text: str, surface: Surface) -> list[str]:
    """Four statements, each held to the thing it describes rather than read as prose."""
    policy = docs_policy(repo)
    findings: list[str] = []

    bounds = section_of(text, "Where the bounds come from")
    named = [token for token in BACKTICKED.findall(bounds) if token.endswith(".rs")]
    reading = [
        source for source in named if repo.exists(source) and "SafetyEnvelope" in repo.read(source)
    ]
    if not reading:
        findings.append(
            f"`{where}` names no source the server reads the safety envelope from. Name "
            f"the file that reads it, so the claim is held to the tree."
        )

    vocabulary = _rejection_vocabulary(repo, policy)
    if isinstance(vocabulary, str):
        findings.append(vocabulary)
    else:
        section = section_of(text, "What a rejection carries")
        stated = set(BACKTICKED.findall(section))
        findings.extend(
            f"`{where}` says a rejection carries `{token}`, and the contracts' rejection "
            f"type declares no such thing"
            for token in sorted(stated - vocabulary)
        )
        findings.extend(
            f"`{where}` omits `{token}`, which the contracts' rejection type declares"
            for token in sorted(vocabulary - stated)
        )

    for heading in (
        "How a bounded intervention is restored",
        "What expiry does when no prior value was available",
    ):
        findings.extend(_asserted_by_a_test(repo, where, heading, section_of(text, heading)))
    return findings


def _test_names(repo: Repo) -> set[str]:
    """Every test function this repository's suites declare."""
    found: set[str] = set()
    for path in sorted(repo.root.rglob("*.rs")):
        if UNCOMMITTED_DIRECTORIES & set(path.relative_to(repo.root).parts):
            continue
        found |= set(
            re.findall(r"^\s*fn ([a-z_][a-z0-9_]*)\(", path.read_text(encoding="utf-8"), re.M)
        )
    return found


def _asserted_by_a_test(repo: Repo, where: str, heading: str, section: str) -> list[str]:
    """One stated behaviour names a journey of this repository that asserts it."""
    claimed = [token for token in BACKTICKED.findall(section) if TEST_NAME.fullmatch(token)]
    if not claimed:
        return [f"`{where}`'s `{heading}` states a behaviour and names no test asserting it"]
    declared = _test_names(repo)
    return [
        f"`{where}`'s `{heading}` names `{name}` as the test asserting it, and this "
        f"repository carries no such test"
        for name in claimed
        if name not in declared
    ]


def schema_document(repo: Repo) -> list[str]:
    """The generated schema document is what the types generate, over the whole set."""
    try:
        policy = docs_policy(repo)
    except PolicyValueError as error:
        return [str(error)]
    if not repo.exists(policy.schema_document):
        return [
            f"the generated schema document `{policy.schema_document}` is absent. Run "
            f"`just docs-generate`."
        ]

    members = schema_members(repo, policy.schema_directory)
    if not members:
        return [
            f"`{policy.schema_directory}` declares no schema at all, so the schema "
            f"document inventories nothing"
        ]
    return _schema_findings(
        policy,
        repo.read(policy.schema_document),
        schema_document_text(repo, policy),
        [name for _, name, _ in members],
    )


def _schema_findings(
    policy: DocsPolicy, text: str, generated: str, declared: list[str]
) -> list[str]:
    """The document is what the types generate, over the whole declared set.

    Two rules rather than one, because comparing the document with a generator's
    output catches only what the generator was handed: a generator whose input
    omitted a declared type writes a document that matches its own output exactly
    and is missing that type. The declared set is read beside it for that reason.
    """
    findings = _inventory(
        entries_of(text), declared, policy.schema_document, "schema-emitting type"
    )
    if text != generated:
        findings.append(
            f"`{policy.schema_document}` is not what the types generate. It is generated "
            f"rather than written: run `just docs-generate`."
        )
    return findings
