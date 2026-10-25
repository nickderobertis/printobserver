"""What this repository publishes, joined from the two files that say it.

`release-targets.toml` is the producer declaration `onevcs` reads, and its
schema is `onevcs`'s rather than this repository's: it refuses a key it does not
declare. So the two facts this repository's own tooling needs and that schema
has no place for — which install-path route a target backs, and which tool
assembles it — live under `repo-policy.toml`'s `[release.targets]`, keyed by the
target's `id`, and are joined back to the declaration here. Every reader of
either fact reads it through this module rather than through a TOML read of its
own, so the join is written once.

The two homes name one set of targets. A target one names and the other does
not is a finding of `drift`, which `just check-repo`'s `release-targets` reports
and `release_artifacts.targets.declared` refuses to build from.
"""

from __future__ import annotations

import tomllib
from dataclasses import dataclass
from pathlib import Path
from typing import Any

#: The producer declaration, at the repository root under the name `onevcs` fixes.
DECLARATION = "release-targets.toml"

#: The file the moved facts live in, and the table that holds them.
POLICY = "repo-policy.toml"
FACTS = "release.targets"


@dataclass(frozen=True, slots=True)
class Target:
    """One declared artifact, with the facts this repository keeps beside it."""

    #: `<registry>:<name>`, where the name is exactly what that registry serves.
    id: str
    #: The short name `onevcs` and a consumer's plan call it by.
    name: str
    #: One sentence saying what a dependent gets — the text its registry shows.
    what: str
    #: The manifest its version is read from, for a target one backs.
    manifest: str
    #: The heading of the install-path route it backs; empty for one backing none.
    route: str
    #: Which tool assembles it; empty where `[release.targets]` names nothing.
    built_by: str

    @property
    def registry(self) -> str:
        """Which registry serves it."""
        return self.id.partition(":")[0]


def _load(path: Path) -> dict[str, Any]:
    with path.open("rb") as handle:
        return tomllib.load(handle)


def _entries(root: Path) -> list[dict[str, Any]]:
    """The declaration's `[[target]]` tables, each narrowed to a table."""
    entries = _load(root / DECLARATION).get("target", [])
    if not isinstance(entries, list):
        return []
    return [entry for entry in entries if isinstance(entry, dict)]


def facts(root: Path) -> dict[str, dict[str, Any]]:
    """`[release.targets]`, by target id, each value narrowed to a table.

    A tree with no `repo-policy.toml` records nothing, which `drift` then
    reports target by target rather than as a missing file.
    """
    if not (root / POLICY).is_file():
        return {}
    release = _load(root / POLICY).get("release", {})
    table = release.get("targets", {}) if isinstance(release, dict) else {}
    if not isinstance(table, dict):
        return {}
    return {str(key): value for key, value in table.items() if isinstance(value, dict)}


def _text(table: dict[str, Any], key: str) -> str:
    value = table.get(key, "")
    return value.strip() if isinstance(value, str) else ""


def joined(root: Path) -> list[Target]:
    """Every declared target, in declaration order, with its moved facts beside it.

    A target `[release.targets]` names nothing for is joined with an empty
    route and an empty `built_by`, which no reader mistakes for either value it
    acts on; `drift` is what reports it.
    """
    moved = facts(root)
    targets: list[Target] = []
    for entry in _entries(root):
        identifier = _text(entry, "id")
        beside = moved.get(identifier, {})
        targets.append(
            Target(
                id=identifier,
                name=_text(entry, "name"),
                what=_text(entry, "what"),
                manifest=_text(entry, "manifest"),
                route=_text(beside, "route"),
                built_by=_text(beside, "built_by"),
            )
        )
    return targets


def drift(root: Path) -> list[str]:
    """Where the declaration and `[release.targets]` disagree about which targets exist."""
    declared = [_text(entry, "id") for entry in _entries(root)]
    moved = facts(root)
    findings = [
        f"{DECLARATION} declares `{identifier}`, and {POLICY}'s `[{FACTS}]` records "
        f"nothing for it: which tool assembles it, and which route it backs, are said nowhere"
        for identifier in declared
        if identifier not in moved
    ]
    findings.extend(
        f"{POLICY}'s `[{FACTS}]` records `{identifier}`, which {DECLARATION} does not "
        f"declare: a fact about an artifact nothing publishes"
        for identifier in moved
        if identifier not in declared
    )
    findings.extend(
        f"{POLICY}'s `[{FACTS}]` gives `{identifier}` no `built_by`, so nothing says "
        f"which tool assembles it"
        for identifier, table in moved.items()
        if identifier in declared and not _text(table, "built_by")
    )
    return findings
