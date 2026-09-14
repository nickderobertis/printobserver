"""The tree a check reads, and the narrowing of what it declares.

`repo-policy.toml` is whatever the TOML reader handed back, so every value a
check acts on starts out as `Any`. The readers at the foot of this module are
where that stops: each narrows one declaration and raises `PolicyValueError`
when it cannot, so a malformed policy is one finding naming the key rather than
an attribute error out of whichever check happened to read it first.
"""

from __future__ import annotations

import re
import tomllib
from dataclasses import dataclass
from functools import cached_property
from pathlib import Path
from typing import Any

# Directories nothing in this repository commits: build products, installed
# dependencies, and the environments the printer and Obico tiers provision. A
# manifest, a suppression directive or a project file under one of these is
# somebody else's source, and a check that read it would report a finding nobody
# here can act on.
#
# It lives here rather than in each check because it was three copies before,
# and the copy `checks_release` carried had already fallen behind the others:
# an Obico clone under `.obico-env` was refused for carrying a version field in
# Obico's own `package.json`. A new provisioned environment lands here once.
UNCOMMITTED_DIRECTORIES = frozenset(
    {
        ".git",
        "target",
        "node_modules",
        ".venv",
        ".nx",
        "dist",
        ".ruff_cache",
        ".pytest_cache",
        ".octoprint-env",
        ".obico-env",
    }
)


class Repo:
    """A committed tree, and the declarations its checks read."""

    def __init__(self, root: Path) -> None:
        """Bind the checks to the tree rooted at `root`."""
        self.root = root.resolve()

    def path(self, relative: str) -> Path:
        """Resolve a repository-relative path."""
        return self.root / relative

    def exists(self, relative: str) -> bool:
        """Whether a repository-relative path is present."""
        return self.path(relative).exists()

    def read(self, relative: str) -> str:
        """Read a repository-relative text file."""
        return self.path(relative).read_text(encoding="utf-8")

    def read_toml(self, relative: str) -> dict[str, Any]:
        """Parse a repository-relative TOML file."""
        with self.path(relative).open("rb") as handle:
            return tomllib.load(handle)

    @cached_property
    def policy(self) -> dict[str, Any]:
        """The machine-readable half of this repository's rules."""
        return self.read_toml("repo-policy.toml")

    @cached_property
    def agents_md(self) -> str:
        """The prose half of this repository's rules."""
        return self.read("AGENTS.md")

    @cached_property
    def justfile(self) -> str:
        """The command surface."""
        return self.read("justfile")

    @cached_property
    def workflow_paths(self) -> list[Path]:
        """Every committed workflow file, in a stable order."""
        directory = self.path(".github/workflows")
        if not directory.is_dir():
            return []
        return sorted(p for p in directory.iterdir() if p.suffix in {".yml", ".yaml"})

    @cached_property
    def project_paths(self) -> list[Path]:
        """Every committed `project.json` of the Nx graph, in a stable order."""
        return [
            project
            for project in sorted(self.root.glob("**/project.json"))
            if not UNCOMMITTED_DIRECTORIES & set(project.relative_to(self.root).parts)
        ]

    @cached_property
    def crate_dirs(self) -> list[Path]:
        """Every crate directory of the Cargo workspace, in a stable order."""
        crates = self.path("crates")
        if not crates.is_dir():
            return []
        return sorted(p for p in crates.iterdir() if (p / "Cargo.toml").is_file())

    @cached_property
    def crate_names(self) -> list[str]:
        """Every crate name of the Cargo workspace, in a stable order."""
        return [p.name for p in self.crate_dirs]


class PolicyValueError(ValueError):
    """`repo-policy.toml` declares a value a check cannot act on."""


def policy_table(repo: Repo, name: str) -> dict[str, Any]:
    """One table of `repo-policy.toml`, or an empty one where it declares none.

    `repo.policy` is whatever the TOML reader handed back, so a table read here
    may be absent or may not be a table at all. Both leave a reader with nothing
    to find, which is a finding of its own rather than an attribute error on a
    value nobody narrowed.
    """
    table = repo.policy.get(name)
    return table if isinstance(table, dict) else {}


def policy_strings(table: dict[str, Any], keys: tuple[str, ...], where: str) -> dict[str, str]:
    """The named values of a policy table, each of them a non-empty string.

    Raises:
        PolicyValueError: If one is absent or carries anything else. A reader
            taking them unnarrowed would abort the whole tier on a malformed file
            rather than report the one thing wrong with it.
    """
    found: dict[str, str] = {}
    for key in keys:
        value = table.get(key)
        if not isinstance(value, str) or not value.strip():
            msg = f"`repo-policy.toml` declares no `{where}.{key}` string"
            raise PolicyValueError(msg)
        found[key] = value.strip()
    return found


def policy_string_list(table: dict[str, Any], key: str, where: str) -> tuple[str, ...]:
    """One named value of a policy table, a non-empty list of non-empty strings.

    Raises:
        PolicyValueError: If it is absent, is not a list, is empty, or holds
            anything that is not a non-empty string.
    """
    value = table.get(key)
    if not isinstance(value, list) or not value:
        msg = f"`repo-policy.toml` declares no non-empty `{where}.{key}` list"
        raise PolicyValueError(msg)
    entries: list[str] = []
    for entry in value:
        if not isinstance(entry, str) or not entry.strip():
            msg = f"`repo-policy.toml`'s `{where}.{key}` names {entry!r}, which is not a name"
            raise PolicyValueError(msg)
        entries.append(entry.strip())
    return tuple(entries)


#: How `repo-policy.toml` writes the release a tool is held at, and how a tool's
#: `--version` answer names one: a dotted triple, with any pre-release or build
#: suffix.
RELEASE = re.compile(r"\d+\.\d+\.\d+(?:[-+][0-9A-Za-z.-]+)?")


@dataclass(frozen=True, slots=True)
class Tool:
    """One `[[toolchain.tool]]`: the command it puts on PATH, how, and the release it holds."""

    command: str
    install: str
    version: str | None

    @property
    def install_argv(self) -> list[str]:
        """The install command as arguments, `{version}` substituted from the held release."""
        if self.version is None:
            return self.install.split()
        return self.install.replace("{version}", self.version).split()


def toolchain_tools(repo: Repo) -> tuple[Tool, ...]:
    """Every tool `repo-policy.toml` declares, narrowed.

    A held `version` reaches an installer's arguments and a workflow's
    `GITHUB_OUTPUT`, so it is read as a release or not at all — and an `install`
    that never substitutes it would install some other release than the held one.

    Raises:
        PolicyValueError: If an entry is not a table or lacks a `command` or
            `install` string, holds a `version` that is not a release, or holds
            one its `install` never substitutes.
    """
    entries = policy_table(repo, "toolchain").get("tool")
    if not isinstance(entries, list):
        msg = "`repo-policy.toml` declares no `toolchain.tool` entries"
        raise PolicyValueError(msg)
    tools: list[Tool] = []
    for entry in entries:
        if not isinstance(entry, dict):
            msg = f"`repo-policy.toml`'s `toolchain.tool` holds {entry!r}, which is not a table"
            raise PolicyValueError(msg)
        named = policy_strings(entry, ("command", "install"), "toolchain.tool")
        version = entry.get("version")
        if version is not None and (not isinstance(version, str) or not RELEASE.fullmatch(version)):
            msg = (
                f"`repo-policy.toml` holds `{named['command']}` at {version!r}, "
                f"which is not a release"
            )
            raise PolicyValueError(msg)
        if version is not None and "{version}" not in named["install"]:
            msg = (
                f"`repo-policy.toml`'s install for `{named['command']}`, `{named['install']}`, "
                f"never substitutes `{{version}}`, so it installs a release other than {version}"
            )
            raise PolicyValueError(msg)
        tools.append(Tool(named["command"], named["install"], version))
    return tuple(tools)
