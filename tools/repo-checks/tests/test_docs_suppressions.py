"""Documentation recipe suppressions must be visible to the allowlist scanner."""

from pathlib import Path

import pytest
from repo_checks.checks_suppressions import configured_suppressions, scan
from repo_checks.expect import equal, truth


@pytest.mark.parametrize("name", ["justfile", "Justfile", ".justfile"])
def test_recipe_suppression_is_scanned(tmp_path: Path, name: str) -> None:
    """An extensionless recipe file carries the same scoped directives as source files."""
    directive = "# llmlint: " + "ignore[changed_behavior_has_e2e] A fixture reason.\n"
    (tmp_path / name).write_text(directive + "docs-generate:\n    generator\n", encoding="utf-8")

    equal(
        [(item.file, item.rule) for item in scan(tmp_path)],
        [(name, "changed_behavior_has_e2e")],
        describing="the recipe suppression",
    )


def test_an_uncommitted_directory_is_neither_scanned_nor_read_for_configuration(
    tmp_path: Path,
) -> None:
    """A vendored tree's directives and linter settings are not this repository's."""
    directive = "# llmlint: " + "ignore[changed_behavior_has_e2e] A fixture reason.\n"
    silencing = '[tool.ruff.lint]\nignore = ["E501"]\n'
    for place in ("src", "node_modules/package"):
        (tmp_path / place).mkdir(parents=True)
        (tmp_path / place / "module.py").write_text(directive, encoding="utf-8")
        (tmp_path / place / "pyproject.toml").write_text(silencing, encoding="utf-8")

    equal(
        [item.file for item in scan(tmp_path)],
        ["src/module.py"],
        describing="the files the scanner read a directive from",
    )
    findings = configured_suppressions(tmp_path)
    equal(len(findings), 1, describing=f"the configurations refused: {findings}")
    truth("src/pyproject.toml" in findings[0], describing=f"the finding names it: {findings}")
