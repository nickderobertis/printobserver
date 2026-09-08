"""The tree a check reads, and what a check says when it refuses."""

from __future__ import annotations

import tomllib
from dataclasses import dataclass
from functools import cached_property
from pathlib import Path
from typing import Any


@dataclass(frozen=True, slots=True)
class Finding:
    """One refusal: which check, and what disagreed."""

    check: str
    message: str

    def render(self) -> str:
        """Format the finding as the one line a reader acts on."""
        return f"{self.check}: {self.message}"


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
