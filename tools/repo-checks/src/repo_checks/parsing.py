"""Deterministic readers for the files the checks compare against each other.

The formats here are the ones this repository chose so that a check never has to
guess: HTML-comment-delimited blocks in `AGENTS.md`, `just` recipe bodies, and
YAML workflows.
"""

from __future__ import annotations

import re
from dataclasses import dataclass
from pathlib import Path
from typing import Any

import yaml

BLOCK = "<!-- BEGIN: {name} -->"
BLOCK_END = "<!-- END: {name} -->"

# Shell keywords and builtins a recipe body may use that are not programs the
# agent allowlist has anything to say about.
SHELL_WORDS = frozenset(
    {
        "cd",
        "echo",
        "set",
        "export",
        "if",
        "then",
        "else",
        "elif",
        "fi",
        "for",
        "while",
        "do",
        "done",
        "case",
        "esac",
        "test",
        "[",
        "[[",
        "exit",
        "true",
        "false",
        ":",
        "read",
        "printf",
        "return",
        "local",
        "shift",
        "source",
        ".",
    }
)


class MarkerBlockMissingError(LookupError):
    """A marker block `AGENTS.md` is required to carry is absent."""


def marker_block(text: str, name: str) -> list[str]:
    """Return the non-empty lines inside `<!-- BEGIN: name -->`..`<!-- END: name -->`.

    Raises:
        MarkerBlockMissingError: if either marker is absent.
    """
    start = text.find(BLOCK.format(name=name))
    end = text.find(BLOCK_END.format(name=name))
    if start < 0 or end < 0 or end < start:
        msg = f"AGENTS.md carries no `{name}` marker block"
        raise MarkerBlockMissingError(msg)
    inner = text[start + len(BLOCK.format(name=name)) : end]
    return [line.strip() for line in inner.splitlines() if line.strip()]


def section(text: str, heading: str) -> str:
    """Return the body of the `## heading` section, up to the next `## `."""
    lines = text.splitlines()
    wanted = f"## {heading}"
    try:
        start = lines.index(wanted)
    except ValueError:
        return ""
    body: list[str] = []
    for line in lines[start + 1 :]:
        if line.startswith("## "):
            break
        body.append(line)
    return "\n".join(body)


@dataclass(frozen=True, slots=True)
class Recipe:
    """One `just` recipe: its name, its dependencies and its body lines."""

    name: str
    dependencies: tuple[str, ...]
    body: tuple[str, ...]


# `just` puts a recipe's dependencies after the colon: `check: lint test`.
RECIPE_HEADER = re.compile(r"^(?P<name>[a-zA-Z0-9_-]+):(?P<deps>(?: +[a-zA-Z0-9_-]+)*)\s*$")


def recipes(justfile: str) -> dict[str, Recipe]:
    """Parse the recipes out of a justfile."""
    found: dict[str, Recipe] = {}
    current: str | None = None
    deps: tuple[str, ...] = ()
    body: list[str] = []
    for raw in justfile.splitlines():
        if raw.startswith((" ", "\t")) and current is not None:
            stripped = raw.strip()
            if stripped and not stripped.startswith("#"):
                body.append(stripped)
            continue
        if current is not None:
            found[current] = Recipe(current, deps, tuple(body))
            current, deps, body = None, (), []
        match = RECIPE_HEADER.match(raw)
        if match:
            current = match.group("name")
            deps = tuple(match.group("deps").split())
            body = []
    if current is not None:
        found[current] = Recipe(current, deps, tuple(body))
    return found


def programs_in(command: str) -> list[str]:
    """The program names a shell command line invokes.

    Deterministic and deliberately simple: the first word of the command and the
    first word after each pipe, minus shell keywords and builtins, minus leading
    `@`/`-` recipe prefixes and `VAR=value` assignments.
    """
    names: list[str] = []
    for segment in command.split("|"):
        words = segment.strip().split()
        index = 0
        while index < len(words) and re.fullmatch(r"[A-Za-z_][A-Za-z0-9_]*=.*", words[index]):
            index += 1
        if index >= len(words):
            continue
        word = words[index].lstrip("@-")
        if not word or word in SHELL_WORDS:
            continue
        names.append(word)
    return names


def load_workflow(path: Path) -> dict[str, Any]:
    """Parse a workflow file, normalizing YAML's `on:`-is-`True` surprise."""
    data = yaml.safe_load(path.read_text(encoding="utf-8"))
    if not isinstance(data, dict):
        return {}
    if True in data and "on" not in data:
        data["on"] = data.pop(True)
    return data


def jobs_of(workflow: dict[str, Any]) -> dict[str, dict[str, Any]]:
    """The jobs a workflow declares."""
    jobs = workflow.get("jobs")
    if not isinstance(jobs, dict):
        return {}
    return {name: job for name, job in jobs.items() if isinstance(job, dict)}


def steps_of(job: dict[str, Any]) -> list[dict[str, Any]]:
    """The steps a job declares."""
    steps = job.get("steps")
    if not isinstance(steps, list):
        return []
    return [step for step in steps if isinstance(step, dict)]


def run_commands(job: dict[str, Any]) -> list[str]:
    """Every command line a job's `run:` steps execute, in order."""
    commands: list[str] = []
    for step in steps_of(job):
        run = step.get("run")
        if isinstance(run, str):
            commands.extend(line.strip() for line in run.strip().splitlines() if line.strip())
    return commands


def fenced_commands(markdown: str) -> list[str]:
    """Every command line inside a fenced code block, in order."""
    commands: list[str] = []
    inside = False
    for line in markdown.splitlines():
        if line.startswith("```"):
            inside = not inside
            continue
        if inside and line.strip():
            commands.append(line.strip())
    return commands
