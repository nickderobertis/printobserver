"""The suppression allowlist, and the check that makes it the only way to suppress.

This check refuses on shape rather than on judgment. Nothing here asks whether a
reason is a *good* reason: that is a question for whoever reviews the change, and
a check that tried to answer it would be exactly the non-deterministic thing the
allowlist exists to replace.

A rule silenced for a whole file — in a *configuration* file, or by a
file-level directive inside the file itself — is refused outright rather than
allowlisted. A blanket `ignore`, a per-file glob, a crate lint set to `allow` or
a linter rule set to `off` names no site, carries no reason, is invisible in a
diff, and goes on silencing findings long after whatever motivated it is gone —
which is every property the allowlist exists to remove. Selecting which rules
run is not suppression and is left alone; so is a docstring `convention`, which
picks which of two mutually contradictory styles is checked rather than
silencing a finding.

The directive patterns below are written so that this module's own source does
not match any of them — an escaped regex is not the directive it recognizes — so
the scanner needs no exemption for the file that defines it.
"""

from __future__ import annotations

import json
import re
import tomllib
from collections.abc import Iterator, Sequence
from dataclasses import dataclass
from pathlib import Path

from repo_checks.model import Repo
from repo_checks.shell import run

SCANNED_SUFFIXES = frozenset(
    {".rs", ".py", ".ts", ".tsx", ".js", ".mjs", ".sh", ".toml", ".yml", ".yaml", ".json", ".md"}
)
# Build products and provisioned environments: nothing under them is committed,
# so a directive inside one is somebody else's source rather than a suppression
# this repository made. `.octoprint-env` is the scripted OctoPrint environment,
# which carries an OctoPrint install of its own, and `.obico-env` is the
# self-hosted Obico environment, which carries a clone of Obico's own sources.
SKIPPED_DIRECTORIES = frozenset(
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

# (pattern, whether group 1 is a comma-separated rule list)
DIRECTIVE_PATTERNS: tuple[tuple[re.Pattern[str], bool], ...] = (
    (re.compile(r"#\s*noqa:\s*([A-Z]+[0-9]+(?:\s*,\s*[A-Z]+[0-9]+)*)"), True),
    (re.compile(r"#\s*type:\s*ignore\[([^\]]+)\]"), True),
    (re.compile(r"#\s*ty:\s*ignore\[([^\]]+)\]"), True),
    (re.compile(r"#\[(?:allow|expect)\(([^)]+)\)\]"), True),
    (re.compile(r"//\s*biome-ignore\s+([\w/]+)"), False),
    (re.compile(r"//\s*@ts-(expect-error|ignore)\b"), False),
    (re.compile(r"#\s*shellcheck\s+disable=([A-Z]+[0-9]+(?:\s*,\s*[A-Z]+[0-9]+)*)"), True),
    (re.compile(r"llmlint:\s*ignore\[([^\]]+)\]"), True),
    (WHOLE_FILE_LLMLINT := re.compile(r"llmlint:\s*ignore-file\[([^\]]+)\]"), True),
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


# Keys whose presence in a linter's configuration silences a rule, rather than
# choosing which rules run.
RUFF_SUPPRESSION_KEYS = (
    "ignore",
    "extend-ignore",
    "per-file-ignores",
    "extend-per-file-ignores",
)
SILENCED_LEVELS = frozenset({"allow", "ignore", "off", "none"})
CONFIG_DIRECTORIES = frozenset(
    {".git", "target", "node_modules", ".venv", ".nx", "dist", ".octoprint-env", ".obico-env"}
)


def _load_toml(path: Path) -> dict[str, object]:
    """Parse a TOML configuration file."""
    with path.open("rb") as handle:
        return tomllib.load(handle)


def _config_files(root: Path, names: tuple[str, ...]) -> list[Path]:
    """Every configuration file of the given names, ignoring vendored trees."""
    found: list[Path] = []
    for path in sorted(root.rglob("*")):
        if not path.is_file() or path.name not in names:
            continue
        if CONFIG_DIRECTORIES & set(path.relative_to(root).parts):
            continue
        found.append(path)
    return found


def _ruff_findings(where: str, ruff: dict[str, object]) -> list[str]:
    """Refuse every key of a ruff configuration that silences a rule."""
    findings: list[str] = []
    tables: list[tuple[str, object]] = [(where, ruff), (f"{where}.lint", ruff.get("lint", {}))]
    for label, table in tables:
        if not isinstance(table, dict):
            continue
        for key in RUFF_SUPPRESSION_KEYS:
            value = table.get(key)
            if not value:
                continue
            if isinstance(value, dict):
                silenced = [
                    f"{', '.join(str(r) for r in rules)} under {glob}"
                    for glob, rules in sorted(value.items())
                ]
            elif isinstance(value, list):
                silenced = [str(rule) for rule in value]
            else:
                silenced = [str(value)]
            findings.append(
                f"{label}.{key} silences {'; '.join(silenced)} in configuration; "
                f"a suppression belongs at its site, with an entry naming the rule, "
                f"the file, the site and a reason"
            )
    return findings


def _levels_set_to_silence(prefix: str, table: object) -> list[str]:
    """Every rule a level-keyed table turns off, at any depth."""
    findings: list[str] = []
    if not isinstance(table, dict):
        return findings
    for name, value in table.items():
        level = value.get("level") if isinstance(value, dict) else value
        if isinstance(level, str) and level.lower() in SILENCED_LEVELS:
            findings.append(f"{prefix}.{name} is set to `{level}`")
        elif isinstance(value, dict):
            findings.extend(_levels_set_to_silence(f"{prefix}.{name}", value))
    return findings


def configured_suppressions(root: Path) -> list[str]:
    """Refuse a rule silenced in a configuration file rather than at its site."""
    findings: list[str] = []

    for path in _config_files(root, ("pyproject.toml", "ruff.toml", ".ruff.toml")):
        relative = path.relative_to(root)
        data = _load_toml(path)
        tools = data.get("tool")
        tools = tools if isinstance(tools, dict) else {}
        if path.name == "pyproject.toml":
            ruff = tools.get("ruff", {})
            where = "[tool.ruff]"
        else:
            ruff = data
            where = "the ruff configuration"
        if isinstance(ruff, dict):
            findings.extend(f"{relative}: {f}" for f in _ruff_findings(where, ruff))
        ty = tools.get("ty")
        if isinstance(ty, dict):
            findings.extend(
                f"{relative}: {f}"
                for f in _levels_set_to_silence("[tool.ty.rules]", ty.get("rules"))
            )

    for path in _config_files(root, ("Cargo.toml",)):
        relative = path.relative_to(root)
        data = _load_toml(path)
        workspace = data.get("workspace")
        lints = workspace.get("lints") if isinstance(workspace, dict) else None
        findings.extend(
            f"{relative}: {f}" for f in _levels_set_to_silence("[workspace.lints]", lints)
        )
        findings.extend(
            f"{relative}: {f}" for f in _levels_set_to_silence("[lints]", data.get("lints"))
        )

    for path in _config_files(root, ("biome.json", "biome.jsonc")):
        relative = path.relative_to(root)
        data = json.loads(path.read_text(encoding="utf-8"))
        for section in ("linter", "formatter", "assist"):
            block = data.get(section)
            if isinstance(block, dict) and block.get("enabled") is False:
                findings.append(f"{relative}: the {section} is disabled, silencing every rule")
            if isinstance(block, dict):
                findings.extend(
                    f"{relative}: {f}"
                    for f in _levels_set_to_silence(f"{section}.rules", block.get("rules"))
                )

    return findings


# Directives that silence a rule for a WHOLE FILE. Each is refused rather than
# allowlisted: it covers every line, including lines written long after the
# reason was true, so it is a blanket suppression whatever file it sits in.
#
# Every tool named here attributes each of its findings to a line, so a
# file-level directive is always strictly broader than the finding it answers,
# and there is no exception for any of them. llmlint is absent because it is the
# one tool with rules that are not attributable to a line; `whole_file_directives`
# above is where its narrow, enumerated permission is enforced.
FILE_LEVEL_PATTERNS: tuple[tuple[re.Pattern[str], str], ...] = (
    (re.compile(r"#\s*ruff:\s*noqa\b"), "ruff"),
    (re.compile(r"#\s*flake8:\s*noqa\b"), "flake8"),
    (re.compile(r"#\s*mypy:\s*ignore-errors\b"), "mypy"),
    (re.compile(r"//\s*biome-ignore-all\b"), "biome"),
    (re.compile(r"//\s*eslint-disable\s*$"), "eslint"),
    (re.compile(r"/\*\s*eslint-disable\s*\*/"), "eslint"),
)


def _scannable(root: Path) -> Iterator[tuple[Path, Path, list[str]]]:
    """Every file the directive scanners read, with its lines."""
    for path in sorted(root.rglob("*")):
        if not path.is_file() or path.suffix not in SCANNED_SUFFIXES:
            continue
        relative = path.relative_to(root)
        if SKIPPED_DIRECTORIES & set(relative.parts):
            continue
        yield path, relative, path.read_text(encoding="utf-8", errors="replace").splitlines()


def whole_file_directives(root: Path, admitted: Sequence[str]) -> list[str]:
    """Refuse a whole-file llmlint directive naming a rule that is not intrinsically one.

    llmlint is the one tool here with rules that are about a file as a whole
    rather than a line — whether a script's output is signal, whether its inputs
    are validated. For those, a whole-file directive is the only shape that
    fits, so `repo-policy.toml` names them and they are permitted here. Every
    other rule llmlint attributes to a line, and silencing one of those for a
    whole file is the blanket suppression the policy refuses.

    The permission is narrow in three ways: it is llmlint's alone, it reaches
    only the rules the policy names, and the directive still needs its own
    allowlist entry with a reason like any other.
    """
    findings: list[str] = []
    for path, relative, lines in _scannable(root):
        del path
        for number, line in enumerate(lines, start=1):
            for match in WHOLE_FILE_LLMLINT.finditer(line):
                findings.extend(
                    f"{relative}:{number} silences `{rule}` for the whole file, and "
                    f"`repo-policy.toml`'s suppressions.whole_file_rules does not name "
                    f"it as a rule that is intrinsically about a whole file. Suppress "
                    f"it at the line it answers instead."
                    for rule in (r.strip() for r in match.group(1).split(","))
                    if rule and rule not in admitted
                )
    return findings


def file_level_directives(root: Path) -> list[str]:
    """Refuse every directive that silences a rule for a whole file."""
    findings: list[str] = []
    for path, relative, lines in _scannable(root):
        del path
        for number, line in enumerate(lines, start=1):
            findings.extend(
                f"{relative}:{number} carries a file-level {tool} directive, which "
                f"silences every line of the file. Suppress at the site that needs "
                f"it, with an entry naming the rule, the file, the site and a "
                f"reason — or change the code so the rule has nothing to say."
                for pattern, tool in FILE_LEVEL_PATTERNS
                if pattern.search(line)
            )
    return findings


def _allowlist(repo: Repo) -> tuple[str, list[dict[str, str]]]:
    """The allowlist path this repository declares, and the entries it holds."""
    relative = repo.policy["suppressions"]["allowlist"]
    entries = repo.read_toml(relative).get("suppression", [])
    return relative, entries


def suppressions(repo: Repo, base: str | None = None) -> list[str]:
    """Every directive has an entry, every entry has a reason, and every entry matches."""
    relative, entries = _allowlist(repo)
    found = [d for d in scan(repo.root) if d.file != relative]
    findings: list[str] = configured_suppressions(repo.root)
    findings.extend(file_level_directives(repo.root))
    findings.extend(
        whole_file_directives(repo.root, repo.policy["suppressions"].get("whole_file_rules", []))
    )

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
    before = run(["git", "show", f"{base}:{relative}"], cwd=repo.root)
    try:
        base_entries = (
            tomllib.loads(before.stdout).get("suppression", []) if before.returncode == 0 else []
        )
    except tomllib.TOMLDecodeError:
        base_entries = []

    changed = run(["git", "diff", "--name-only", f"{base}...HEAD"], cwd=repo.root).stdout.split()

    findings: list[str] = []
    for directive in found:
        if directive.file not in changed:
            continue
        was_there = run(["git", "show", f"{base}:{directive.file}"], cwd=repo.root)
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
