"""The agent-facing documentation, as the checks over it read it.

`repo-policy.toml`'s `[docs]` section is the one declaration of what this
repository documents: the skill, the bounds it is held to, the nine things it
owes, and the reference documents it links to with the headings each of them
carries. Everything here narrows that section once, so a malformed declaration
is one finding naming the key rather than an attribute error out of whichever
check happened to read it first.

The skill is an Agent Skill, so it opens with a YAML frontmatter block and what
a turn sends is the prose after it; `skill_prose` is the one split of the two,
and it is the Rust adapter's `skill_prose` read the same way.

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

    #: The skill whose prose OneHarness sends as every turn's system prompt.
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
    #: The nine things the skill owes.
    elements: tuple[SkillElement, ...]
    #: Every reference document, in the order they are declared.
    documents: tuple[Document, ...]


def _entries(table: dict[str, object], key: str) -> list[dict[str, object]]:
    """Read every table, refusing malformed entries instead of dropping them.

    Raises:
        PolicyValueError: If a declared value is not an array of tables.
    """
    value = table.get(key, [])
    if not isinstance(value, list):
        msg = f"`repo-policy.toml`'s `docs.{key}` must be an array of tables"
        raise PolicyValueError(msg)
    entries: list[dict[str, object]] = []
    for index, entry in enumerate(value):
        if not isinstance(entry, dict):
            msg = f"`repo-policy.toml`'s `docs.{key}[{index}]` must be a table"
            raise PolicyValueError(msg)
        entries.append(entry)
    return entries


def _repository_path(repo: Repo, value: str, key: str) -> None:
    """Keep declared documentation inputs and outputs inside the repository.

    Raises:
        PolicyValueError: If a path is absolute, traverses parents, or resolves outside.
    """
    path = Path(value)
    msg = f"`repo-policy.toml`'s `{key}` must be a relative path inside the repository"
    if path.is_absolute() or ".." in path.parts:
        raise PolicyValueError(msg)
    try:
        contained = (repo.root / path).resolve().is_relative_to(repo.root.resolve())
    except (OSError, RuntimeError) as error:
        raise PolicyValueError(f"{msg}: {error}") from error
    if not contained:
        raise PolicyValueError(msg)


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
    for key, value in named.items():
        if key != "example_fence":
            _repository_path(repo, value, f"docs.{key}")
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
        _repository_path(repo, path, "docs.document.path")
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
    prunes — every type any crate declares under `schemas/<crate>/`: the
    contract crate's shared vocabulary, each port's shapes, each domain's event
    payloads marked with the kind they are written under, and the server's
    answer shapes. Reading it off that tree rather than off a list here is what
    makes a type entering or leaving the set move what the schema document
    owes, with nothing to update by hand.
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
        f"{len(members)} types. The set is every type any crate declares under",
        "`schemas/<crate>/`, keyed by type name across every declaring crate: the",
        "contract crate's shared vocabulary, the request and answer shapes the port",
        "crates own, each domain's event payloads — marked with `x-event-kind`, the",
        "kind each is written under — and the shapes the server answers. A port's own",
        "error vocabulary is deliberately not in it: it reaches no process boundary, so",
        "it emits no schema.",
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


class UnclosedFrontmatterError(ValueError):
    """A skill that opens a `---` frontmatter block and never closes it."""


@dataclass(frozen=True, slots=True)
class SkillText:
    """A skill split into its frontmatter and the prose a turn sends."""

    #: The text between the two `---` lines, or `None` when no block opens.
    frontmatter: str | None
    #: Every character after the closing `---` line, or the whole text.
    prose: str
    #: How many lines of the file come before the prose, so a line number
    #: counted in the prose is that number plus this one in the file.
    offset: int


def _is_fence(line: str) -> bool:
    """Whether one line, with its terminator, is exactly `---`."""
    bare = line.removesuffix("\n").removesuffix("\r") if line.endswith("\n") else line
    return bare == "---"


def skill_prose(text: str) -> SkillText:
    r"""Split a `SKILL.md` into its frontmatter and its prose.

    Exactly the adapter's own split: when the first line — its `\n` or
    `\r\n` terminator stripped — is not exactly `---` there is no
    frontmatter and the prose is the whole text; otherwise the prose is every
    character after the next line that is exactly `---`.

    Raises:
        UnclosedFrontmatterError: If the first line opens a block no later
            line closes.
    """
    pieces = text.split("\n")
    lines = [piece + "\n" for piece in pieces[:-1]]
    if pieces[-1]:
        lines.append(pieces[-1])
    if not lines or not _is_fence(lines[0]):
        return SkillText(frontmatter=None, prose=text, offset=0)
    consumed = len(lines[0])
    for index, line in enumerate(lines[1:], start=1):
        if _is_fence(line):
            return SkillText(
                frontmatter="".join(lines[1:index]),
                prose=text[consumed + len(line) :],
                offset=index + 1,
            )
        consumed += len(line)
    msg = "the frontmatter block the first line opens is never closed by a `---` line"
    raise UnclosedFrontmatterError(msg)
