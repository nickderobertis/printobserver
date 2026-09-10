"""The agent-facing documentation, as the checks over it read it.

`repo-policy.toml`'s `[docs]` section is the one declaration of what this
repository documents: the skill, the bounds it is held to, the nine things it
owes, and the reference documents it links to with the headings each of them
carries. Everything here narrows that section once, so a malformed declaration
is one finding naming the key rather than an attribute error out of whichever
check happened to read it first.

The generator for the schema document lives here too, because the document and
the check over it must be one reading: a document generated one way and checked
another is a document that can pass a comparison with itself while missing a
type nobody noticed.
"""

from __future__ import annotations

import json
import re
from dataclasses import dataclass
from pathlib import Path

from repo_checks.model import (
    PolicyValueError,
    Repo,
    policy_string_list,
    policy_strings,
    policy_table,
)

#: The declarations of `[docs]` read as non-empty strings.
POLICY_NAMES = (
    "skill",
    "surface_manifest",
    "schema_document",
    "schema_directory",
    "example_fence",
    "bundle_source",
    "bundle_assets",
    "bundle_directory",
)

#: The declarations of `[docs]` read as whole numbers.
POLICY_NUMBERS = ("skill_max_characters", "skill_max_lines")


@dataclass(frozen=True, slots=True)
class SkillElement:
    """One thing the skill must carry in its own text."""

    #: What it is, for a finding a reader can act on.
    name: str
    #: The marker its passage carries, which removing the passage removes.
    marker: str


@dataclass(frozen=True, slots=True)
class Document:
    """One reference document, and the headings it owes."""

    #: Where it is, relative to the repository root.
    path: str
    #: The `##` headings it must carry, in the order it must carry them.
    headings: tuple[str, ...]


@dataclass(frozen=True, slots=True)
class DocsPolicy:
    """The `[docs]` section of `repo-policy.toml`, narrowed once."""

    #: The skill OneHarness sends as every turn's system prompt.
    skill: str
    #: The most characters that skill may be.
    skill_max_characters: int
    #: The most lines that skill may be.
    skill_max_lines: int
    #: The generated artifact the command surface is read off.
    surface_manifest: str
    #: The generated document every schema of the set has an entry in.
    schema_document: str
    #: The tree the contracts' generation target writes the schema set into.
    schema_directory: str
    #: The fence a runnable example is written in.
    example_fence: str
    #: The source that carries the reference documents into the artifact.
    bundle_source: str
    #: The directory that source reads them out of.
    bundle_assets: str
    #: The directory the skill links to them under, beside itself.
    bundle_directory: str
    #: The nine things the skill owes.
    elements: tuple[SkillElement, ...]
    #: Every reference document, in the order they are declared.
    documents: tuple[Document, ...]


def _entries(table: dict[str, object], key: str) -> list[dict[str, object]]:
    """The array-of-tables one key holds, or nothing where it holds none."""
    value = table.get(key)
    if not isinstance(value, list):
        return []
    return [entry for entry in value if isinstance(entry, dict)]


def docs_policy(repo: Repo) -> DocsPolicy:
    """Read the `[docs]` section, narrowing every value the checks act on.

    Raises:
        PolicyValueError: If the section is absent, or declares a key these
            checks read as something other than what they read it as.
    """
    table = policy_table(repo, "docs")
    if not table:
        msg = "`repo-policy.toml` declares no `[docs]` section"
        raise PolicyValueError(msg)
    named = policy_strings(table, POLICY_NAMES, "docs")
    numbers: dict[str, int] = {}
    for key in POLICY_NUMBERS:
        value = table.get(key)
        if not isinstance(value, int) or isinstance(value, bool) or value <= 0:
            msg = f"`repo-policy.toml` declares no positive `docs.{key}` whole number"
            raise PolicyValueError(msg)
        numbers[key] = value

    elements = []
    for entry in _entries(table, "skill_element"):
        pair = policy_strings(entry, ("name", "marker"), "docs.skill_element")
        elements.append(SkillElement(name=pair["name"], marker=pair["marker"]))
    if not elements:
        msg = "`repo-policy.toml` declares no `docs.skill_element`, so the skill owes nothing"
        raise PolicyValueError(msg)

    documents = []
    for entry in _entries(table, "document"):
        path = policy_strings(entry, ("path",), "docs.document")["path"]
        documents.append(
            Document(path=path, headings=policy_string_list(entry, "headings", "docs.document"))
        )
    if not documents:
        msg = "`repo-policy.toml` declares no `docs.document`, so the skill links to nothing"
        raise PolicyValueError(msg)

    return DocsPolicy(
        skill=named["skill"],
        skill_max_characters=numbers["skill_max_characters"],
        skill_max_lines=numbers["skill_max_lines"],
        surface_manifest=named["surface_manifest"],
        schema_document=named["schema_document"],
        schema_directory=named["schema_directory"],
        example_fence=named["example_fence"],
        bundle_source=named["bundle_source"],
        bundle_assets=named["bundle_assets"],
        bundle_directory=named["bundle_directory"],
        elements=tuple(elements),
        documents=tuple(documents),
    )


def headings_of(text: str) -> list[str]:
    """Every `##` heading one document carries, in the order it carries them."""
    return [line[3:].strip() for line in text.splitlines() if line.startswith("## ")]


def entries_of(text: str) -> list[str]:
    """Every `###` entry one document carries, in the order it carries them."""
    return [line[4:].strip() for line in text.splitlines() if line.startswith("### ")]


def section_of(text: str, heading: str) -> str:
    """The body of one `##` section, up to the next `##`."""
    lines = text.splitlines()
    try:
        start = lines.index(f"## {heading}")
    except ValueError:
        return ""
    body: list[str] = []
    for line in lines[start + 1 :]:
        if line.startswith("## "):
            break
        body.append(line)
    return "\n".join(body)


def entry_body(text: str, entry: str) -> str:
    """The body of one `###` entry, up to the next heading of any depth."""
    lines = text.splitlines()
    try:
        start = lines.index(f"### {entry}")
    except ValueError:
        return ""
    body: list[str] = []
    for line in lines[start + 1 :]:
        if line.startswith(("## ", "### ")):
            break
        body.append(line)
    return "\n".join(body)


def opening_statement(text: str) -> str:
    """What a document says it covers: the first paragraph after its title.

    The first paragraph rather than everything before the first section,
    because what follows it is the document's own preamble — which may link to
    a sibling document and would otherwise read as a claim to cover it.
    """
    said: list[str] = []
    started = False
    for line in text.splitlines():
        if line.startswith("## "):
            break
        if line.startswith("# "):
            started = True
            continue
        if not started:
            continue
        if line.strip():
            said.append(line.strip())
        elif said:
            break
    return " ".join(said)


@dataclass(frozen=True, slots=True)
class SchemaMember:
    """One type in the contracts' generated schema set."""

    crate: str
    name: str
    path: Path


def schema_members(repo: Repo, directory: str) -> list[SchemaMember]:
    """Every member of the declared schema set: its crate, its name and its file.

    The set is read off the tree the contracts' own generation target writes and
    prunes — every type the contracts crate declares as its own, together with
    the request and answer shapes the port crates own. Reading it off that tree
    rather than off a list here is what makes a type entering or leaving the set
    move what the schema document owes, with nothing to update by hand.
    """
    root = repo.path(directory)
    if not root.is_dir():
        return []
    found = [SchemaMember(path.parent.name, path.stem, path) for path in root.glob("*/*.json")]
    return sorted(found, key=lambda member: (member.name, member.crate))


def schema_document_text(repo: Repo, policy: DocsPolicy) -> str:
    """The schema document, generated from the schema set the contracts write."""
    members = schema_members(repo, policy.schema_directory)
    lines = [
        "# The schemas",
        "",
        "Every JSON Schema this system's contracts generate, one entry per type in the",
        "declared schema set. This document covers the schema set and the schemas",
        "themselves.",
        "",
        "**This document is generated.** Every entry below is exactly what that type",
        "emits — nothing here is transcribed, and `just docs-generate` is what writes it.",
        "A check refuses a tree in which an entry is not what the type generates, in which",
        "a member of the set has no entry, or in which an entry names something the",
        "contracts do not declare.",
        "",
        "## The schema set",
        "",
        f"{len(members)} types. The set is every type `printobserver-types` declares as its",
        "own, together with the six request and answer shapes the four port crates own —",
        "the shapes their methods carry across a process boundary. A port's own error",
        "vocabulary is deliberately not in it: it reaches no process boundary, so it emits",
        "no schema.",
        "",
        "The set is read off the tree the contracts' generation target writes and prunes,",
        "so a type entering or leaving it moves this document with it.",
        "",
        "## The schemas",
        "",
    ]
    for member in members:
        schema = json.loads(member.path.read_text(encoding="utf-8"))
        lines.append(f"### {member.name}")
        lines.append("")
        lines.append(f"Declared by `{member.crate}`.")
        lines.append("")
        lines.append("```json")
        lines.extend(json.dumps(schema, indent=2, sort_keys=True).splitlines())
        lines.append("```")
        lines.append("")
    return "\n".join(lines).rstrip() + "\n"


#: How a link to a reference document is written in the skill.
LINK = re.compile(r"\[[^\]]+\]\(([^)]+)\)")


def links_of(text: str) -> list[str]:
    """Every markdown link target one document carries."""
    return LINK.findall(text)
