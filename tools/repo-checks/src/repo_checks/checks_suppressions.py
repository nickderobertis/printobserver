"""The suppression allowlist, and the check that makes it the only way to suppress.

This check refuses on shape rather than on judgment. Nothing here asks whether a
reason is a *good* reason: that is a question for whoever reviews the change, and
a check that tried to answer it would be exactly the non-deterministic thing the
allowlist exists to replace.

The directive patterns below are written so that this module's own source does
not match any of them — an escaped regex is not the directive it recognizes — so
the scanner needs no exemption for the file that defines it.
"""

from __future__ import annotations

import re
import subprocess
from dataclasses import dataclass
from pathlib import Path

from repo_checks.model import Repo

SCANNED_SUFFIXES = frozenset(
    {".rs", ".py", ".ts", ".tsx", ".js", ".mjs", ".sh", ".toml", ".yml", ".yaml", ".json", ".md"}
)
SKIPPED_DIRECTORIES = frozenset(
    {".git", "target", "node_modules", ".venv", ".nx", "dist", ".ruff_cache", ".pytest_cache"}
)

# (pattern, whether group 1 is a comma-separated rule list)
DIRECTIVE_PATTERNS: tuple[tuple[re.Pattern[str], bool], ...] = (
    (re.compile(r"#\s*noqa:\s*([A-Z]+[0-9]+(?:\s*,\s*[A-Z]+[0-9]+)*)"), True),
    (re.compile(r"#\s*type:\s*ignore\[([^\]]+)\]"), True),
    (re.compile(r"#\s*ty:\s*ignore\[([^\]]+)\]"), True),
    (re.compile(r"#\[(?:allow|expect)\(([^)]+)\)\]"), True),
    (re.compile(r"//\s*biome-ignore\s+([\w/]+)"), False),
    (re.compile(r"//\s*@ts-(expect-error|ignore)\b"), False),
    (re.compile(r"#\s*shellcheck\s+disable=([A-Z]+[0-9]+(?:\s*,\s*[A-Z]+[0-9]+)*)"), True),
    (re.compile(r"llmlint:\s*ignore(?:-file)?\[([^\]]+)\]"), True),
)


@dataclass(frozen=True, slots=True)
class Directive:
    """One suppression standing in the tree."""

    file: str
    line: int
    rule: str
    text: str
    following: tuple[str, ...]

    def matched_by(self, site: str) -> bool:
        """Whether an allowlist entry's site anchors to this directive."""
        return site in self.text or any(site in line for line in self.following)


def directives_in(text: str, relative: str) -> list[Directive]:
    """Every suppression directive in one file."""
    lines = text.splitlines()
    found: list[Directive] = []
    for index, line in enumerate(lines):
        following = tuple(
            candidate for candidate in lines[index + 1 : index + 8] if candidate.strip()
        )[:3]
        for pattern, is_list in DIRECTIVE_PATTERNS:
            for match in pattern.finditer(line):
                captured = match.group(1)
                rules = [r.strip() for r in captured.split(",")] if is_list else [captured.strip()]
                found.extend(
                    Directive(relative, index + 1, rule, line.strip(), following)
                    for rule in rules
                    if rule
                )
    return found


def scan(root: Path) -> list[Directive]:
    """Every suppression directive in a tree."""
    found: list[Directive] = []
    for path in sorted(root.rglob("*")):
        if not path.is_file() or path.suffix not in SCANNED_SUFFIXES:
            continue
        relative = path.relative_to(root)
        if SKIPPED_DIRECTORIES & set(relative.parts):
            continue
        found.extend(
            directives_in(path.read_text(encoding="utf-8", errors="replace"), str(relative))
        )
    return found


def _allowlist(repo: Repo) -> tuple[str, list[dict[str, str]]]:
    """The allowlist path this repository declares, and the entries it holds."""
    relative = repo.policy["suppressions"]["allowlist"]
    entries = repo.read_toml(relative).get("suppression", [])
    return relative, entries


def suppressions(repo: Repo, base: str | None = None) -> list[str]:
    """Every directive has an entry, every entry has a reason, and every entry matches."""
    relative, entries = _allowlist(repo)
    found = [d for d in scan(repo.root) if d.file != relative]
    findings: list[str] = []

    for index, entry in enumerate(entries):
        for field in ("rule", "file", "site"):
            if not str(entry.get(field, "")).strip():
                findings.append(f"{relative}: entry {index} names no {field}")
        if not str(entry.get("reason", "")).strip():
            findings.append(
                f"{relative}: the entry for `{entry.get('rule')}` in "
                f"{entry.get('file')} carries no reason"
            )

    def matches(entry: dict[str, str], directive: Directive) -> bool:
        return (
            entry.get("rule") == directive.rule
            and entry.get("file") == directive.file
            and directive.matched_by(str(entry.get("site", "")))
        )

    for entry in entries:
        if not any(matches(entry, directive) for directive in found):
            findings.append(
                f"{relative}: the entry for `{entry.get('rule')}` in "
                f"{entry.get('file')} matches no suppression directive in the tree"
            )
    for directive in found:
        if not any(matches(entry, directive) for entry in entries):
            findings.append(
                f"{directive.file}:{directive.line} suppresses `{directive.rule}` with no "
                f"entry in {relative}"
            )

    if base is not None:
        findings.extend(_introduced_without_entry(repo, base, relative, entries, found))
    return findings


def _introduced_without_entry(
    repo: Repo,
    base: str,
    relative: str,
    entries: list[dict[str, str]],
    found: list[Directive],
) -> list[str]:
    """A change that adds a directive adds its entry in the same change."""
    import tomllib

    try:
        before = subprocess.run(
            ["git", "show", f"{base}:{relative}"],
            cwd=repo.root,
            capture_output=True,
            text=True,
            check=True,
        ).stdout
        base_entries = tomllib.loads(before).get("suppression", [])
    except subprocess.CalledProcessError, tomllib.TOMLDecodeError:
        base_entries = []

    changed = subprocess.run(
        ["git", "diff", "--name-only", f"{base}...HEAD"],
        cwd=repo.root,
        capture_output=True,
        text=True,
        check=False,
    ).stdout.split()

    findings: list[str] = []
    for directive in found:
        if directive.file not in changed:
            continue
        was_there = subprocess.run(
            ["git", "show", f"{base}:{directive.file}"],
            cwd=repo.root,
            capture_output=True,
            text=True,
            check=False,
        )
        prior = directives_in(was_there.stdout, directive.file) if was_there.returncode == 0 else []
        if any(p.rule == directive.rule and p.text == directive.text for p in prior):
            continue
        added = [
            entry
            for entry in entries
            if entry not in base_entries
            and entry.get("rule") == directive.rule
            and entry.get("file") == directive.file
        ]
        if not added:
            findings.append(
                f"{directive.file}:{directive.line} introduces a suppression of "
                f"`{directive.rule}` without adding its entry to {relative} in the "
                f"same change"
            )
    return findings
