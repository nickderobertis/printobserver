"""The project graph's edges are the ones the code actually has.

`nx affected` selects a project when a file under it changed or when a project
it depends on was selected, and nothing else. Nx reads no Cargo manifest and no
Python import here, so an edge either language draws is one the graph does not
know about unless a `project.json` declares it under `implicitDependencies` —
and an edge the graph does not know about is a suite the affected tier skips
over the change that broke it. This check holds those declarations to what the
code says, so they cannot drift from it:

* every crate's edges to other crates are exactly the workspace crates its
  manifest depends on, across every dependency table — `dev-dependencies`
  included, because a crate's tests are built from them;
* every Python project's edges include each project whose package or module
  one of its files imports, resolved through the same search roots the type
  checker reads, so a name means here what it means to `ty`;
* every target that depends on another project's target names that project as
  an edge, because a task dependency orders work and selects nothing.

The Python and task rules are a floor rather than an equality: a suite may also
depend on what it *runs* rather than imports — a built program, a script it
drives — and that edge is declared by hand, with no import to derive it from.
"""

from __future__ import annotations

import ast
import json
from dataclasses import dataclass
from pathlib import Path
from typing import Any

from repo_checks.checks_repo import _crate_manifest, _importable_names, _in_workspace_dependencies
from repo_checks.model import UNCOMMITTED_DIRECTORIES, Repo

RUST = "lang:rust"
PYTHON = "lang:python"
EDGES = "implicitDependencies"


class ProjectFileError(ValueError):
    """A file the graph is read from that does not hold what the graph needs."""


@dataclass(frozen=True, slots=True)
class Project:
    """One `project.json`: what it is called, where it is, and what it declares."""

    name: str
    root: str
    tags: frozenset[str]
    edges: frozenset[str]
    targets: dict[str, Any]
    task_edges: dict[str, str]


def projects(repo: Repo) -> list[Project]:
    """Every project of the graph, read off its own `project.json`.

    The file is the deserialization boundary, so every field is narrowed here.
    A field left out takes the default Nx gives it — the root for a name, and
    nothing for tags, edges and targets — and one present in the wrong shape is
    refused rather than replaced, because a graph read past a malformed field is
    not the graph Nx selects by.

    Raises:
        ProjectFileError: If a `project.json` decodes to anything but an object,
            or carries a name that is not a non-empty string, tags or edges that
            are not a list of strings, targets that are not an object of objects,
            or a `dependsOn` that is not a list of target names and objects whose
            `projects` is a list of strings.
    """
    found: list[Project] = []
    for path in repo.project_paths:
        data = json.loads(path.read_text(encoding="utf-8"))
        root = path.parent.relative_to(repo.root).as_posix()
        if not isinstance(data, dict):
            msg = f"{root}/project.json holds a JSON {type(data).__name__}, not a project object"
            raise ProjectFileError(msg)
        name = data.get("name", root)
        if not isinstance(name, str) or not name:
            msg = f"{root}/project.json's `name` is {name!r}, not a non-empty string"
            raise ProjectFileError(msg)
        targets = data.get("targets", {})
        if not isinstance(targets, dict):
            msg = (
                f"{root}/project.json's `targets` is a JSON {type(targets).__name__}, not an object"
            )
            raise ProjectFileError(msg)
        found.append(
            Project(
                name=name,
                root=root,
                tags=frozenset(_strings(data.get("tags", []), f"{root}/project.json's `tags`")),
                edges=frozenset(_strings(data.get(EDGES, []), f"{root}/project.json's `{EDGES}`")),
                targets=targets,
                task_edges=_task_edges(name, root, targets),
            )
        )
    return found


def _strings(value: object, field: str) -> list[str]:
    """The strings `value` lists, `field` naming where it was read for a refusal.

    Raises:
        ProjectFileError: If `value` is not a list of strings.
    """
    if not isinstance(value, list) or not all(isinstance(entry, str) for entry in value):
        msg = f"{field} is {value!r}, not a list of strings"
        raise ProjectFileError(msg)
    return value


def _task_edges(name: str, root: str, targets: dict[str, Any]) -> dict[str, str]:
    """The projects one project's targets depend on a target of, and which target.

    Raises:
        ProjectFileError: If a target is not an object, or its `dependsOn` is not
            a list of target names and objects whose `projects` lists strings.
    """
    found: dict[str, str] = {}
    for target, declared in targets.items():
        where = f"{root}/project.json's `{target}`"
        if not isinstance(declared, dict):
            msg = f"{where} is a JSON {type(declared).__name__}, not a target object"
            raise ProjectFileError(msg)
        depends = declared.get("dependsOn", [])
        if not isinstance(depends, list) or not all(isinstance(e, str | dict) for e in depends):
            msg = f"{where}.dependsOn is {depends!r}, not a list of target names and objects"
            raise ProjectFileError(msg)
        for entry in depends:
            if isinstance(entry, dict):
                for other in _strings(entry.get("projects", []), f"{where}.dependsOn projects"):
                    if other != name:
                        found.setdefault(other, target)
    return found


def _owner(path: str, by_root: dict[str, str]) -> str | None:
    """The project whose root holds `path`, the deepest one where roots nest."""
    owners = [root for root in by_root if path == root or path.startswith(f"{root}/")]
    return by_root[max(owners, key=len)] if owners else None


def _python_files(repo: Repo, root: str) -> list[Path]:
    """Every Python source under one project root, skipping what is not committed."""
    base = repo.path(root)
    return [
        path
        for path in sorted(base.rglob("*.py"))
        if not UNCOMMITTED_DIRECTORIES & set(path.relative_to(base).parts)
    ]


def _imported(path: Path) -> set[str]:
    """The top-level names one file imports absolutely."""
    syntax = ast.parse(path.read_text(encoding="utf-8"), filename=str(path))
    names: set[str] = set()
    for node in ast.walk(syntax):
        match node:
            case ast.Import(names=aliases):
                names.update(alias.name.partition(".")[0] for alias in aliases)
            case ast.ImportFrom(module=str(module), level=0):
                names.add(module.partition(".")[0])
            case _:
                pass
    return names


def python_edges(repo: Repo, graph: list[Project]) -> dict[str, dict[str, str]]:
    """Each Python project's import edges: the project imported, and one file importing it.

    A name is resolved through the type checker's own search roots — the ones
    `module_names` already holds to offering each name once — and belongs to the
    project whose root holds where it is offered.

    Raises:
        ProjectFileError: If the type checker's search roots are not a list of paths.
    """
    by_root = {project.root: project.name for project in graph}
    roots = repo.read_toml("pyproject.toml")["tool"]["ty"]["environment"]["root"]
    if not isinstance(roots, list) or not all(isinstance(r, str) and r for r in roots):
        msg = "pyproject.toml's `tool.ty.environment.root` is not a list of search-root paths"
        raise ProjectFileError(msg)
    offered: dict[str, str] = {}
    for search_root in roots:
        for name, where in _importable_names(repo, search_root).items():
            owner = _owner(where, by_root)
            if owner is not None:
                offered[name] = owner

    edges: dict[str, dict[str, str]] = {}
    for project in graph:
        if PYTHON not in project.tags:
            continue
        found: dict[str, str] = {}
        for path in _python_files(repo, project.root):
            for name in sorted(_imported(path)):
                owner = offered.get(name)
                if owner is not None and owner != project.name:
                    found.setdefault(owner, path.relative_to(repo.root).as_posix())
        edges[project.name] = found
    return edges


def graph_edges(repo: Repo) -> list[str]:
    """Every edge the code draws between projects is one the graph declares."""
    try:
        graph = projects(repo)
    except ProjectFileError as malformed:
        return [str(malformed)]
    names = {project.name for project in graph}
    findings = [
        f"{project.root}/project.json declares an edge to `{edge}`, which is no project"
        for project in graph
        for edge in sorted(project.edges - names)
    ]

    crates = set(repo.crate_names)
    by_root = {project.root: project.name for project in graph}
    crate_projects = {by_root[f"crates/{c}"] for c in crates if f"crates/{c}" in by_root}
    for project in graph:
        if RUST not in project.tags:
            continue
        crate = Path(project.root).name
        if project.root != f"crates/{crate}" or crate not in crates:
            findings.append(f"{project.root}/project.json is tagged `{RUST}` and is no crate")
            continue
        manifest = _in_workspace_dependencies(_crate_manifest(repo, crate), crates)
        drawn = {by_root[f"crates/{name}"] for name in manifest if f"crates/{name}" in by_root}
        declared = project.edges & crate_projects
        findings.extend(
            f"`{project.name}` depends on crate `{edge}` in its Cargo manifest, and its "
            f"project.json does not declare that edge: a change to `{edge}` would not "
            f"select `{project.name}`"
            for edge in sorted(drawn - declared)
        )
        findings.extend(
            f"`{project.name}` declares an edge to crate `{edge}`, which its Cargo "
            f"manifest does not depend on"
            for edge in sorted(declared - drawn)
        )

    try:
        imports = python_edges(repo, graph)
    except ProjectFileError as malformed:
        return [*findings, str(malformed)]
    for name, imported in imports.items():
        project = next(project for project in graph if project.name == name)
        findings.extend(
            f"`{name}` imports `{edge}`'s code ({where}), and its project.json does not "
            f"declare that edge: a change to `{edge}` would not select `{name}`"
            for edge, where in sorted(imported.items())
            if edge not in project.edges
        )

    for project in graph:
        findings.extend(
            f"`{project.name}:{target}` depends on a target of `{edge}`, and its "
            f"project.json does not declare that edge: a change to `{edge}` would not "
            f"select `{project.name}`"
            for edge, target in sorted(project.task_edges.items())
            if edge not in project.edges
        )
    return findings
