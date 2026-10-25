"""What this repository publishes, read from its own release declaration.

`release-targets.toml` is the one place a target is named, and
`repo-policy.toml`'s `[release.targets]` the one place its route and the tool
assembling it are; `repo_checks.release_declaration` joins the two. Nothing
here restates a name, a registry or a route: a builder that carried its own
list would be a second answer to the question those files exist to answer.
"""

from __future__ import annotations

import tomllib
from dataclasses import dataclass
from pathlib import Path

from repo_checks import release_declaration

#: The file the declaration lives in.
DECLARATION = release_declaration.DECLARATION


class TargetError(ValueError):
    """The release declaration says something no builder can act on."""


@dataclass(frozen=True, slots=True)
class Target:
    """One artifact this repository publishes."""

    #: `<registry>:<name>`, where the name is exactly what that registry serves.
    id: str
    #: What it is, in one line, as its own registry shows it: the declaration's
    #: `what`.
    description: str
    #: The heading of the install-path route it backs, for a target that backs
    #: one. Empty for a client library, which is a dependency rather than a
    #: route to the program.
    route: str
    #: The manifest that backs it, for a target one backs.
    manifest: str
    #: Which tool assembles it. `release-plz` publishes a crate straight from
    #: the workspace; `release-artifacts` assembles everything beside them.
    built_by: str

    @property
    def registry(self) -> str:
        """Which registry serves it."""
        return self.id.partition(":")[0]

    @property
    def name(self) -> str:
        """What that registry serves it under."""
        return self.id.partition(":")[2]


def declared(root: Path) -> list[Target]:
    """Every target the declaration names, in the order it names them.

    Raises:
        TargetError: If a declared target carries no identifier of the form
            every consumer reads, or the declaration and `[release.targets]`
            disagree about which targets there are — a target nothing says
            which tool assembles is one no builder can tell is its own.
    """
    targets: list[Target] = []
    for entry in release_declaration.joined(root):
        identifier = entry.id
        if ":" not in identifier:
            msg = (
                f"{DECLARATION} declares `{identifier or 'a nameless target'}`, which is "
                f"not `<registry>:<name>`"
            )
            raise TargetError(msg)
        targets.append(
            Target(
                id=identifier,
                description=entry.what,
                route=entry.route,
                manifest=entry.manifest,
                built_by=entry.built_by.value if entry.built_by else "",
            )
        )
    drift = release_declaration.drift(root)
    if drift:
        raise TargetError("; ".join(drift))
    return targets


def named(root: Path, identifier: str) -> Target:
    """One declared target.

    Raises:
        TargetError: If the declaration names no such target.
    """
    for target in declared(root):
        if target.id == identifier:
            return target
    known = ", ".join(target.id for target in declared(root))
    msg = f"`{identifier}` is not a target {DECLARATION} declares. It declares {known}"
    raise TargetError(msg)


def workspace(root: Path) -> dict[str, str]:
    """The version, licence and repository every artifact inherits.

    Release automation owns the version and writes it into the workspace
    manifest, so every artifact this repository publishes takes it from there
    rather than from a manifest somebody keeps in step by hand.

    Raises:
        TargetError: If the workspace declares none of them.
    """
    return workspace_of((root / "Cargo.toml").read_bytes())


def workspace_of(manifest: bytes) -> dict[str, str]:
    """What one workspace manifest's bytes declare, wherever the bytes came from.

    The tree's own manifest above, or the one at a release tag as `git show`
    reads it: a dispatched publish verifies that a tag's tree carries the
    version the tag names, and it reads that through this one reader rather
    than through a second parser that could disagree with it.

    Raises:
        TargetError: If the manifest declares none of the inherited fields.
    """
    package = tomllib.loads(manifest.decode("utf-8")).get("workspace", {}).get("package", {})
    found = {key: str(package.get(key, "")) for key in ("version", "license", "repository")}
    missing = sorted(key for key, value in found.items() if not value)
    if missing:
        msg = f"the workspace declares no {', '.join(missing)} for an artifact to carry"
        raise TargetError(msg)
    return found
