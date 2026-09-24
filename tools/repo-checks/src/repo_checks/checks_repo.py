"""Checks over the repository's own layer: agent files, allowlist, recipes, crates."""

from __future__ import annotations

import ast
import json
import shlex
import tomllib
from collections.abc import Iterable, Sequence
from dataclasses import dataclass
from typing import Any

from repo_checks.model import Repo, toolchain_tools
from repo_checks.parsing import (
    MarkerBlockMissingError,
    marker_block,
    programs_in,
    recipes,
)

PLACEHOLDERS = ("TODO", "TBD", "FIXME", "...", "…", "<placeholder>", "XXX")
# The Nx entry point every fan-out tier goes through, the recipe that installs
# what it needs, and the locked install that recipe must run.
NX_INVOCATION = "bunx nx"
NODE_INSTALL_RECIPE = "node-modules"
LOCKED_NODE_INSTALL = "bun install --frozen-lockfile"
NO_OP_COMMANDS = ("echo", "true", ":", "printf")
# The runner a `typecheck` command reaches its type checker through, and the
# program a pass is: `uv run -q ty check <root>` runs `ty`, and the pair is what
# an invocation has to execute rather than merely carry.
TY_RUNNER = ("uv", "run")
TY_PROGRAM = ("ty", "check")
PLATFORM_OPTION = "--python-platform"
DISPOSITIONS = ("included", "excluded")


def powershell_providers(repo: Repo) -> list[str]:
    """The PowerShell commands bootstrap accepts are the ones installer journeys run."""
    source = "tools/release-artifacts/src/release_artifacts/installing.py"
    if not repo.exists(source):
        return [f"{source} is absent: it declares the PowerShell commands installer journeys run"]
    try:
        syntax = ast.parse(repo.read(source), filename=source)
    except SyntaxError as error:
        return [f"{source} could not be parsed: {error.msg} at line {error.lineno}"]
    assignment = next(
        (
            node
            for node in syntax.body
            if isinstance(node, ast.Assign)
            and any(
                isinstance(target, ast.Name) and target.id == "POWERSHELLS"
                for target in node.targets
            )
        ),
        None,
    )
    if assignment is None:
        return [f"{source} declares no literal POWERSHELLS tuple"]
    try:
        declared = ast.literal_eval(assignment.value)
    except ValueError, TypeError, SyntaxError:
        return [f"{source} declares no literal POWERSHELLS tuple"]
    if not isinstance(declared, tuple) or not all(isinstance(name, str) for name in declared):
        return [f"{source} declares no literal POWERSHELLS tuple"]
    held = next((tool for tool in toolchain_tools(repo) if tool.command == "pwsh"), None)
    if held is None:
        return ["repo-policy.toml declares no pwsh toolchain tool"]
    if held.provided_by != declared:
        return [
            f"repo-policy.toml's pwsh provided_by {held.provided_by!r} differs from "
            f"{source}'s POWERSHELLS {declared!r}"
        ]
    return []


def agent_layer(repo: Repo) -> list[str]:
    """AGENTS.md, the CLAUDE.md symlink, and the composition record."""
    findings: list[str] = []
    if not repo.exists("AGENTS.md"):
        return ["AGENTS.md is absent"]

    claude = repo.path("CLAUDE.md")
    if not claude.is_symlink():
        findings.append("CLAUDE.md is not a symbolic link to AGENTS.md")
    elif claude.readlink().name != "AGENTS.md":
        findings.append(f"CLAUDE.md points at {claude.readlink()}, not AGENTS.md")

    try:
        entries = marker_block(repo.agents_md, "composition-record")
    except MarkerBlockMissingError as error:
        findings.append(str(error))
        return findings

    if not entries:
        findings.append("the composition record is an empty block")
    for entry in entries:
        if not entry.startswith("- "):
            continue
        name, _, rest = entry[2:].partition(" — ")
        if not rest:
            findings.append(f"composition entry {name!r} carries no disposition")
            continue
        disposition, _, reason = rest.partition(":")
        disposition = disposition.strip()
        if disposition not in DISPOSITIONS:
            findings.append(
                f"composition entry {name!r} carries no disposition "
                f"(expected one of {', '.join(DISPOSITIONS)}, found {disposition!r})"
            )
            continue
        reason = reason.strip()
        if not reason:
            findings.append(f"composition entry {name!r} carries no reason")
        elif any(marker in reason for marker in PLACEHOLDERS):
            findings.append(f"composition entry {name!r} is a placeholder: {reason!r}")
    return findings


def _derived_programs(repo: Repo) -> dict[str, list[str]]:
    """Every program this repository's own recipes and graph targets invoke."""
    derived: dict[str, list[str]] = {}
    for recipe in recipes(repo.justfile).values():
        for line in recipe.body:
            for program in programs_in(line):
                derived.setdefault(program, []).append(f"justfile recipe `{recipe.name}`")
    for project in repo.project_paths:
        data = json.loads(project.read_text(encoding="utf-8"))
        for target, spec in (data.get("targets") or {}).items():
            command = spec.get("command")
            if isinstance(command, str):
                for program in programs_in(command):
                    derived.setdefault(program, []).append(
                        f"{data.get('name', project.parent.name)}:{target}"
                    )
    return derived


def _allowlist_entries(repo: Repo) -> dict[str, str]:
    """The allowlist's entries, keyed by the program each one names."""
    settings = json.loads(repo.read(repo.policy["agent"]["allowlist"]))
    entries: dict[str, str] = {}
    for entry in settings.get("permissions", {}).get("allow", []):
        if not (entry.startswith("Bash(") and entry.endswith(")")):
            continue
        inner = entry[len("Bash(") : -1]
        program = inner.split(":")[0].split()[0]
        entries[program] = entry
    return entries


def command_allowlist(repo: Repo) -> list[str]:
    """The allowlist names exactly the commands the recipes and targets invoke."""
    derived = _derived_programs(repo)
    allowed = _allowlist_entries(repo)
    findings = [
        f"`{program}` is invoked by {sorted(set(sources))[0]} but the allowlist "
        f"{repo.policy['agent']['allowlist']} does not name it"
        for program, sources in sorted(derived.items())
        if program not in allowed
    ]
    findings.extend(
        f"the allowlist names `{program}` ({allowed[program]}), which no recipe "
        f"or graph target invokes"
        for program in sorted(allowed)
        if program not in derived
    )
    return findings


@dataclass(frozen=True)
class PythonProject:
    """One `lang:python` project of the Nx graph, as its own `project.json` declares it."""

    name: str
    root: str | None
    typecheck: str | None


def _declared_text(value: object) -> str | None:
    """The value where it is a non-empty string, and `None` where it declares nothing.

    A declaration of another shape is not read as text: coercing it would
    compose a root or a name out of whatever JSON happened to be there, and the
    check below would then hold a target to a path nobody wrote.
    """
    return value if isinstance(value, str) and value else None


def _python_projects(repo: Repo) -> list[PythonProject]:
    """Every Python project of the graph, in the graph's own order.

    A declaration of another shape declares nothing a caller can read, so each
    member is taken only where it is the shape it is meant to be: a project
    whose `root` or `typecheck` command is absent or is not a non-empty string
    carries `None`, and is a finding of the check below rather than a traceback
    out of this. The name alone falls back, to the directory the declaration
    was read from, because it identifies a project in a finding rather than
    deciding anything — and a project whose own name is unreadable is one a
    reader still has to be able to find.
    """
    projects: list[PythonProject] = []
    for path in repo.project_paths:
        declared = json.loads(path.read_text(encoding="utf-8"))
        tags = declared.get("tags") if isinstance(declared, dict) else None
        if not isinstance(tags, list) or "lang:python" not in tags:
            continue
        targets = declared.get("targets")
        target = targets.get("typecheck") if isinstance(targets, dict) else None
        command = target.get("command") if isinstance(target, dict) else None
        projects.append(
            PythonProject(
                name=_declared_text(declared.get("name")) or path.parent.name,
                root=_declared_text(declared.get("root")),
                typecheck=_declared_text(command),
            )
        )
    return projects


@dataclass(frozen=True)
class TyPass:
    """One `ty check` invocation of a `typecheck` command: what it checks, and for what."""

    platform: str | None
    roots: tuple[str, ...]


def _executed(tokens: Sequence[str]) -> Sequence[str]:
    """The program one invocation runs, and its arguments, past any runner prefix.

    `uv run -q ty check <root>` runs `ty`; `uv run -q echo ty check <root>` runs
    `echo`, carries every word the first one does, and type-checks nothing. The
    two differ only in a token that is neither the first nor the last, so the
    program is found by stepping over the one runner this repository's targets
    use and over its flags, rather than by looking for `ty check` anywhere in
    the line.

    Only that runner is stepped over. An invocation reaching `ty` some other way
    reads here as running no pass, which is a finding about a target rather than
    a silent acceptance — the safe direction for a gate.
    """
    index = len(TY_RUNNER) if tuple(tokens[: len(TY_RUNNER)]) == TY_RUNNER else 0
    while index < len(tokens) and tokens[index].startswith("-"):
        index += 1
    return tokens[index:]


def _ty_passes(command: str) -> list[TyPass]:
    """Every `ty check` a `typecheck` command actually runs, read as invocations.

    Read as text instead, the contract below is satisfied by things that check
    nothing: an `echo` of the invocation being looked for — `uv run -q echo ty
    check <root>` as much as a bare one — and a pass over
    `python/printobserver-sdk-extra`, which any test for the root
    `python/printobserver-sdk` finds inside it. So the command is split into the
    invocations the shell would run, each is tokenized, a pass counts only where
    `ty check` is the program it executes, and a root only where it is a whole
    argument of one.
    """
    passes: list[TyPass] = []
    for invocation in command.split("&&"):
        try:
            tokens = shlex.split(invocation)
        except ValueError:
            continue
        run = _executed(tokens)
        if tuple(run[: len(TY_PROGRAM)]) != TY_PROGRAM:
            continue
        platform: str | None = None
        roots: list[str] = []
        rest = run[len(TY_PROGRAM) :]
        index = 0
        while index < len(rest):
            token = rest[index]
            if token == PLATFORM_OPTION and index + 1 < len(rest):
                platform = rest[index + 1]
                index += 2
                continue
            if token.startswith(f"{PLATFORM_OPTION}="):
                platform = token.partition("=")[2]
            elif not token.startswith("-"):
                roots.append(token)
            index += 1
        passes.append(TyPass(platform=platform, roots=tuple(roots)))
    return passes


def python_typecheck_platforms(repo: Repo) -> list[str]:
    """Every Python project type-checks for the host's platform and for each declared one.

    A type checker reads `sys.platform` and `os.name` to decide which members a
    module has, so a pass on this host alone sees `os.getuid` and never sees
    `os.startfile`: an attribute reached outside a platform guard is reported
    first by a runner of the platform it is missing from. The platforms are
    `repo-policy.toml`'s rather than each project's, so nine targets cannot
    drift into carrying eight.
    """
    toolchain = repo.policy.get("toolchain")
    section = toolchain.get("python_typecheck") if isinstance(toolchain, dict) else None
    declared = section.get("platforms") if isinstance(section, dict) else None
    if not (
        isinstance(declared, list)
        and declared
        and all(isinstance(platform, str) and platform for platform in declared)
    ):
        return [
            "`repo-policy.toml` declares no `toolchain.python_typecheck.platforms` list of "
            f"platform names: found {declared!r}"
        ]
    platforms: tuple[str, ...] = tuple(declared)
    findings: list[str] = []
    for project in _python_projects(repo):
        if project.root is None:
            findings.append(f"{project.name} is a Python project declaring no `root` path")
            continue
        if project.typecheck is None:
            findings.append(f"{project.name} is a Python project declaring no `typecheck` command")
            continue
        checked = {
            (found.platform, root)
            for found in _ty_passes(project.typecheck)
            for root in found.roots
        }
        if (None, project.root) not in checked:
            findings.append(
                f"{project.name}:typecheck runs no `ty check {project.root}` pass for this host"
            )
        findings.extend(
            f"{project.name}:typecheck runs no `{platform}` pass over {project.root}: a defect "
            f"in code `sys.platform` hides from this host would be reported first by a "
            f"{platform} runner"
            for platform in platforms
            if (platform, project.root) not in checked
        )
    return findings


def recipe_set(repo: Repo) -> list[str]:
    """The check recipe invokes every declared tier and no recipe runs nothing."""
    tiers: list[str] = repo.policy["gate"]["tiers"]
    parsed = recipes(repo.justfile)
    findings: list[str] = []

    if "check" not in parsed:
        return ["the justfile declares no `check` recipe"]
    if "bootstrap" not in parsed:
        findings.append("the justfile declares no `bootstrap` recipe")

    invoked = {
        line.split()[1]
        for line in parsed["check"].body
        if line.split()[:1] == ["just"] and len(line.split()) > 1
    }
    invoked |= set(parsed["check"].dependencies)
    findings.extend(
        f"the `check` recipe does not invoke the declared tier `{tier}`"
        for tier in tiers
        if tier not in invoked
    )

    for name in [*tiers, "check", "bootstrap"]:
        recipe = parsed.get(name)
        if recipe is None:
            findings.append(f"`repo-policy.toml` declares tier `{name}` but no recipe defines it")
            continue
        if not recipe.body and not recipe.dependencies:
            findings.append(f"the `{name}` recipe has an empty body: it runs nothing")
            continue
        does_something = any(
            program not in NO_OP_COMMANDS for line in recipe.body for program in programs_in(line)
        )
        if recipe.body and not does_something:
            findings.append(f"the `{name}` recipe is a placeholder: its body runs nothing")

    coverage_profiles: dict[str, str] = {}
    for project in repo.project_paths:
        data = json.loads(project.read_text(encoding="utf-8"))
        command = (data.get("targets") or {}).get("test", {}).get("command")
        if not isinstance(command, str) or "cargo llvm-cov" not in command:
            continue
        name = str(data.get("name", project.parent.name))
        profile = next(
            (
                word.partition("=")[2]
                for word in command.split()
                if word.startswith("LLVM_PROFILE_FILE_NAME=")
            ),
            None,
        )
        if profile is None or '"${OS:-}" = Windows_NT' not in command:
            findings.append(f"{name}:test carries no Windows-only coverage profile name")
        elif profile in coverage_profiles:
            findings.append(
                f"{name}:test shares Windows coverage profile {profile!r} with "
                f"{coverage_profiles[profile]}:test"
            )
        elif "%p" not in profile or "%m" not in profile:
            # The test targets run in parallel on Windows as everywhere else,
            # and Windows reuses process identifiers freely. `%p` gives each
            # process its own file; `%m` makes a process handed a reused
            # identifier merge into that file rather than truncate it. A
            # name missing either is coverage a later process silently drops.
            findings.append(
                f"{name}:test names Windows coverage profile {profile!r}, which a reused "
                f"process identifier could overwrite: it needs both `%p` and `%m`"
            )
        else:
            coverage_profiles[profile] = name
    return findings


def _first_index(body: tuple[str, ...], *, startswith: str) -> int | None:
    """The position of the first body line starting with `startswith`."""
    return next((index for index, line in enumerate(body) if line.startswith(startswith)), None)


def node_install(repo: Repo) -> list[str]:
    """Every recipe that reaches Nx installs the JavaScript dependencies first.

    `bunx nx` fails outright in a clone whose dependencies have never been
    installed, and every publication of this repository is made from exactly
    such a clone, cut fresh for it. So a recipe reaching Nx heals that state
    before it gets there, and heals it from the committed lockfile rather than
    from whatever the registry offers today.
    """
    parsed = recipes(repo.justfile)
    install = parsed.get(NODE_INSTALL_RECIPE)
    if install is None:
        return [
            f"the justfile declares no `{NODE_INSTALL_RECIPE}` recipe: nothing "
            f"installs the JavaScript dependencies `bunx nx` needs"
        ]

    findings: list[str] = []
    if LOCKED_NODE_INSTALL not in install.body:
        findings.append(
            f"the `{NODE_INSTALL_RECIPE}` recipe does not run `{LOCKED_NODE_INSTALL}`: "
            f"an unlocked install can resolve what the committed lockfile does not describe"
        )

    invocation = f"just {NODE_INSTALL_RECIPE}"
    for recipe in parsed.values():
        reaches_nx = _first_index(recipe.body, startswith=NX_INVOCATION)
        if reaches_nx is None:
            continue
        installs = _first_index(recipe.body, startswith=invocation)
        if installs is None or installs > reaches_nx:
            findings.append(
                f"the `{recipe.name}` recipe reaches Nx without running `{invocation}` "
                f"first: it fails in a clone that has never been bootstrapped"
            )
    return findings


def _crate_manifest(repo: Repo, crate: str) -> dict[str, Any]:
    """Parse one crate's Cargo manifest."""
    with repo.path(f"crates/{crate}/Cargo.toml").open("rb") as handle:
        return tomllib.load(handle)


#: The dependency tables a crate's shipped code is built from.
SHIPPED_TABLES = ("dependencies", "build-dependencies")

#: The one dependency table only a crate's tests are built from.
TEST_TABLE = "dev-dependencies"


def _in_workspace_dependencies(
    manifest: dict[str, Any],
    names: set[str],
    tables: tuple[str, ...] = (*SHIPPED_TABLES, TEST_TABLE),
) -> set[str]:
    """The workspace crates one manifest depends on, across the tables named."""
    found: set[str] = set()
    for table in tables:
        found |= set(manifest.get(table, {})) & names
    for target in (manifest.get("target") or {}).values():
        if isinstance(target, dict):
            found |= _in_workspace_dependencies(target, names, tables)
    return found


def workspace(repo: Repo) -> list[str]:
    """The crate set, each crate's module comment, and the dependency rule.

    The rule is the edge table `repo-policy.toml`'s `crates.may_depend_on`
    declares: for every workspace crate, the workspace crates it may depend on
    across every dependency table, plus the few `crates.may_depend_on_in_tests`
    admits under `dev-dependencies` alone. The table and the workspace are held
    to each other in both directions — a row naming a crate the workspace does
    not hold, and a crate the table has no row for, are each refused — and
    every manifest edge between workspace crates is held to its row: one a
    crate's tests may add is refused the moment it appears in a table the
    shipped code is built from.
    """
    crates = repo.policy["crates"]
    table = crates.get("may_depend_on")
    if not isinstance(table, dict):
        return ["`repo-policy.toml` declares no `crates.may_depend_on` table"]
    in_tests = crates.get("may_depend_on_in_tests", {})
    if not isinstance(in_tests, dict):
        return ["`repo-policy.toml`'s `crates.may_depend_on_in_tests` is not a table"]
    declared = set(table)
    present = set(repo.crate_names)
    findings = [
        f"`repo-policy.toml` declares crate `{name}`, which the workspace does not hold"
        for name in sorted(declared - present)
    ]
    findings.extend(
        f"the workspace holds crate `{name}`, which `repo-policy.toml` does not declare"
        for name in sorted(present - declared)
    )
    for name in sorted(declared):
        admitted = table[name]
        if not isinstance(admitted, list) or any(not isinstance(edge, str) for edge in admitted):
            findings.append(f"`repo-policy.toml`'s row for `{name}` is not a list of crate names")
            continue
        findings.extend(
            f"`repo-policy.toml`'s row for `{name}` admits `{edge}`, which the workspace "
            f"does not hold"
            for edge in sorted(set(admitted) - present)
        )
    for name, admitted in in_tests.items():
        if name not in declared:
            findings.append(
                f"`repo-policy.toml`'s `may_depend_on_in_tests` names `{name}`, which "
                f"`may_depend_on` has no row for"
            )
        if not isinstance(admitted, list) or any(not isinstance(edge, str) for edge in admitted):
            findings.append(
                f"`repo-policy.toml`'s `may_depend_on_in_tests` row for `{name}` is not a "
                f"list of crate names"
            )
            continue
        findings.extend(
            f"`repo-policy.toml`'s `may_depend_on_in_tests` row for `{name}` admits "
            f"`{edge}`, which the workspace does not hold"
            for edge in sorted(set(admitted) - present)
        )

    for directory in repo.crate_dirs:
        source = directory / "src" / "lib.rs"
        if not source.is_file():
            source = directory / "src" / "main.rs"
        if not source.is_file():
            findings.append(f"crate `{directory.name}` has no `src/lib.rs` or `src/main.rs`")
            continue
        comment = "\n".join(
            line
            for line in source.read_text(encoding="utf-8").splitlines()
            if line.startswith("//!")
        )
        if "Owns:" not in comment:
            findings.append(f"crate `{directory.name}`'s module comment does not say what it owns")
        if "May depend on:" not in comment:
            findings.append(
                f"crate `{directory.name}`'s module comment does not say what it may depend on"
            )

    for name in sorted(present):
        manifest = _crate_manifest(repo, name)
        edges = _in_workspace_dependencies(manifest, present)
        admitted = table.get(name)
        if isinstance(admitted, list):
            shipped = {str(entry) for entry in admitted}
            tested = in_tests.get(name)
            in_tests_only = {str(entry) for entry in tested} if isinstance(tested, list) else set()
            findings.extend(
                f"`{name}` depends on `{edge}`, which the dependency table does not admit: "
                f"`repo-policy.toml`'s row for `{name}` names {_named(shipped)}"
                for edge in sorted(edges - shipped - in_tests_only)
            )
            shipped_edges = _in_workspace_dependencies(manifest, present, SHIPPED_TABLES)
            findings.extend(
                f"`{name}` depends on `{edge}` outside `{TEST_TABLE}`, and the dependency "
                f"table admits that edge for its tests alone"
                for edge in sorted((shipped_edges & in_tests_only) - shipped)
            )
        if name == crates.get("cli"):
            findings.extend(_cli_edges(crates, name, edges))
    return findings


def _named(entries: Iterable[str]) -> str:
    """A row's admitted crates, rendered for a finding."""
    named = sorted(entries)
    return ", ".join(f"`{entry}`" for entry in named) if named else "no crate at all"


def _cli_edges(crates: dict[str, Any], name: str, edges: set[str]) -> list[str]:
    """The command-line program's own two rules about what it may name.

    It is a composition root, so the rule above permits it any implementation.
    What it must not have is either vendor adapter: for everything but starting
    the server it is a client of an already-running one, and a client that could
    name the printer's adapter or the failure detector's could reach a printer
    without the supervisor in between.
    """
    forbidden = set(crates.get("cli_forbids", []))
    required = set(crates.get("cli_requires", []))
    findings = [
        f"`{name}` depends on `{edge}`: the command-line program is a client of a running "
        f"server for everything but starting one, and a client that can name a vendor's "
        f"adapter can reach that vendor without the supervisor in between"
        for edge in sorted(edges & forbidden)
    ]
    findings.extend(
        f"`{name}` does not depend on `{edge}`, which it is the client of"
        for edge in sorted(required - edges)
    )
    return findings


def octoprint_client(repo: Repo) -> list[str]:
    """Only one crate constructs an OctoPrint request."""
    policy = repo.policy.get("octoprint")
    if not policy:
        return ["`repo-policy.toml` declares no `[octoprint]` section"]
    permitted = str(policy["crate"])
    markers = [str(marker) for marker in policy["request_markers"]]
    if not markers:
        return ["`repo-policy.toml` names no marker of an OctoPrint request"]

    findings: list[str] = []
    if permitted not in repo.crate_names:
        return [
            f"`repo-policy.toml` permits crate `{permitted}` to construct an OctoPrint "
            f"request, which the workspace does not hold"
        ]

    found_in_permitted = False
    for directory in repo.crate_dirs:
        for path in sorted(directory.rglob("*.rs")):
            for number, line in enumerate(path.read_text(encoding="utf-8").splitlines(), start=1):
                if line.lstrip().startswith("//"):
                    continue
                for marker in markers:
                    if marker not in line:
                        continue
                    if directory.name == permitted:
                        found_in_permitted = True
                        continue
                    findings.append(
                        f"{path.relative_to(repo.root).as_posix()}:{number} carries `{marker}`, so "
                        f"crate `{directory.name}` constructs an OctoPrint request: only "
                        f"`{permitted}` may"
                    )
    if not found_in_permitted:
        findings.append(
            f"crate `{permitted}` constructs no OctoPrint request, so this rule guards a "
            f"boundary nothing is on: either the adapter is gone or the markers "
            f"`repo-policy.toml` names no longer describe one"
        )
    return findings


def _importable_names(repo: Repo, root: str) -> dict[str, str]:
    """The top-level modules and packages one search root offers, by name.

    `conftest` and `test_*` are excluded because pytest places those by path —
    it prepends each test file's own directory — so nothing imports them by a
    bare name and two roots may carry one without either becoming ambiguous.

    Args:
        repo: The tree to read.
        root: A repository-relative search root.

    Returns:
        Each importable top-level name, mapped to where it comes from.
    """
    directory = repo.path(root)
    if not directory.is_dir():
        return {}
    offered: dict[str, str] = {}
    for entry in sorted(directory.iterdir()):
        if entry.name.startswith("test_") or entry.stem == "conftest":
            continue
        if entry.is_file() and entry.suffix == ".py":
            offered[entry.stem] = f"{root}/{entry.name}"
        elif entry.is_dir() and (entry / "__init__.py").exists():
            offered[entry.name] = f"{root}/{entry.name}"
    return offered


def module_names(repo: Repo) -> list[str]:
    """No two of the type checker's search roots offer one top-level name.

    Every root in `[tool.ty.environment]` is a bare directory on the search
    path, so `import world` resolves by the order those roots are listed rather
    than to the module beside the file importing it. Two roots offering one name
    is therefore not a tie: the one listed second loses, and its own importers
    silently resolve to a module declaring none of the members they name.

    It cost this repository a gate: `python/printobserver-sdk/integration` is
    listed ahead of `tools/printer-smoke/tests`, and each carried a `world`, so
    printer-smoke's tests type-checked against the clients' supervisor and
    reported thirteen unresolved imports against a module that was never theirs.

    Args:
        repo: The tree to read.

    Returns:
        One finding per name more than one root offers.
    """
    roots = repo.read_toml("pyproject.toml")["tool"]["ty"]["environment"]["root"]
    offered: dict[str, list[str]] = {}
    for root in roots:
        for name, where in _importable_names(repo, str(root)).items():
            offered.setdefault(name, []).append(where)
    return [
        f"`{name}` is offered by more than one search root ({', '.join(sorted(places))}): "
        f"a bare import of it resolves by root order rather than to the module beside "
        f"whichever file imports it"
        for name, places in sorted(offered.items())
        if len(places) > 1
    ]
