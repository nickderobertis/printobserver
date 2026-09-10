"""Documentation recipe suppressions must be visible to the allowlist scanner."""

from pathlib import Path

import pytest
from repo_checks.checks_suppressions import scan
from repo_checks.expect import equal


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
