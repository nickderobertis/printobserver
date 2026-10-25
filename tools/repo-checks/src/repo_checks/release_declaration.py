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
from enum import StrEnum
from pathlib import Path
from typing import Any, NewType

#: The producer declaration, at the repository root under the name `onevcs` fixes.
DECLARATION = "release-targets.toml"

#: The file the moved facts live in, and the table that holds them.
POLICY = "repo-policy.toml"
FACTS = "release.targets"

#: A target's `<registry>:<name>`, the one key both files name it by.
TargetId = NewType("TargetId", str)


class BuiltBy(StrEnum):
    """The tools that assemble what this repository publishes."""

    #: Publishes every crate straight from the workspace.
    RELEASE_PLZ = "release-plz"
    #: Assembles everything beside the crates: the clients and the routes.
    RELEASE_ARTIFACTS = "release-artifacts"


#: The keys a `[release.targets]` entry may carry.
FACT_KEYS = ("built_by", "route")


@dataclass(frozen=True, slots=True)
class Target:
    """One declared artifact, with the facts this repository keeps beside it."""

    #: `<registry>:<name>`, where the name is exactly what that registry serves.
    id: TargetId
    #: The short name `onevcs` and a consumer's plan call it by.
    name: str
    #: One sentence saying what a dependent gets — the text its registry shows.
    what: str
    #: The manifest its version is read from, for a target one backs.
    manifest: str
    #: The heading of the install-path route it backs; empty for one backing none.
    route: str
    #: Which tool assembles it; `None` where `[release.targets]` names none, or
    #: names something that is not one of them.
    built_by: BuiltBy | None

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

    A target `[release.targets]` names nothing for — or names a malformed
    fact for — is joined with an empty route and no `built_by`, which no reader
    mistakes for a value it acts on; `drift` is what reports it.
    """
    moved = facts(root)
    targets: list[Target] = []
    for entry in _entries(root):
        identifier = TargetId(_text(entry, "id"))
        beside = moved.get(identifier, {})
        targets.append(
            Target(
                id=identifier,
                name=_text(entry, "name"),
                what=_text(entry, "what"),
                manifest=_text(entry, "manifest"),
                route=_text(beside, "route"),
                built_by=_built_by(beside),
            )
        )
    return targets


def _shape(root: Path) -> list[str]:
    """What is wrong with the shape of either table, which the narrowing above would drop."""
    findings: list[str] = []
    entries = _load(root / DECLARATION).get("target", [])
    if not isinstance(entries, list) or not entries:
        findings.append(f"{DECLARATION} declares no `[[target]]` table")
    else:
        findings.extend(
            f"{DECLARATION}'s [[target]] {position} is not a table"
            for position, entry in enumerate(entries, start=1)
            if not isinstance(entry, dict)
        )
    if (root / POLICY).is_file():
        release = _load(root / POLICY).get("release", {})
        table = release.get("targets", {}) if isinstance(release, dict) else None
        if not isinstance(table, dict):
            findings.append(f"{POLICY}'s `[{FACTS}]` is not a table")
        else:
            findings.extend(
                f"{POLICY}'s `[{FACTS}]` gives `{key}` {value!r}, which is not a table"
                for key, value in table.items()
                if not isinstance(value, dict)
            )
    return findings


def drift(root: Path) -> list[str]:
    """Where the two files disagree about which targets exist, or are not the shape read."""
    declared = [_text(entry, "id") for entry in _entries(root)]
    moved = facts(root)
    findings = _shape(root)
    findings += [
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
    for identifier, table in moved.items():
        if identifier in declared:
            findings.extend(_malformed(identifier, table))
    return findings


def _built_by(table: dict[str, Any]) -> BuiltBy | None:
    value = table.get("built_by")
    return BuiltBy(value) if value in tuple(BuiltBy) else None


def _malformed(identifier: str, table: dict[str, Any]) -> list[str]:
    """What is wrong with one `[release.targets]` entry, if anything."""
    where = f"{POLICY}'s `[{FACTS}]` gives `{identifier}`"
    findings = [
        f"{where} the key `{key}`, which is none of {', '.join(FACT_KEYS)}"
        for key in table
        if key not in FACT_KEYS
    ]
    if _built_by(table) is None:
        findings.append(
            f"{where} no `built_by`, or one naming no tool: it is one of "
            f"{', '.join(tool.value for tool in BuiltBy)}, so nothing else says which "
            f"tool assembles it"
        )
    route = table.get("route", "")
    if not isinstance(route, str) or (route and not route.strip()):
        findings.append(f"{where} a `route` that is not a route heading: {route!r}")
    return findings
