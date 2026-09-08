"""A rule silenced in a configuration file is a suppression the allowlist never sees.

`suppressions.toml` is the only way to suppress a diagnostic here, and a glob in
a linter's configuration defeats that: it is invisible in the diff, it names no
site, it carries no reason, and it keeps silencing findings long after whatever
motivated it is gone. Each journey below drives the committed check against a
tree whose configuration silences a rule and asserts it is refused.
"""

# `assert` is how pytest states an assertion and how it produces the failure
# message a reader acts on; suppressions.toml carries the reason.

from __future__ import annotations

from collections.abc import Callable

from repo_checks.checks_suppressions import suppressions
from repo_checks.expect import accepted, refused, refused_naming
from repo_checks.model import Repo
from treecopy import Tree


def test_the_committed_configuration_silences_nothing(committed: Repo) -> None:
    """No linter or type-checker configuration in this repository turns a rule off."""
    accepted(suppressions(committed))


def test_a_repository_wide_rule_ignore_is_refused(tree: Callable[[], Tree]) -> None:
    """A blanket `ignore` silences every site of a rule and names none of them."""
    broken = tree()
    broken.edit(
        "pyproject.toml",
        "[tool.ruff.lint.pydocstyle]",
        'ignore = ["S603", "S607"]\n\n[tool.ruff.lint.pydocstyle]',
    )

    findings = suppressions(broken.repo)

    refused_naming(findings, "pyproject.toml", "S603")


def test_a_per_file_rule_ignore_is_refused(tree: Callable[[], Tree]) -> None:
    """A per-file glob is the same blanket with a path attached."""
    broken = tree()
    broken.append(
        "pyproject.toml",
        '\n[tool.ruff.lint.per-file-ignores]\n"**/tests/**" = ["S101"]\n',
    )

    findings = suppressions(broken.repo)

    refused_naming(findings, "per-file-ignores", "S101")


def test_a_crate_level_lint_allow_is_refused(tree: Callable[[], Tree]) -> None:
    """A workspace lint set to `allow` silences a rule across every crate."""
    broken = tree()
    broken.edit(
        "Cargo.toml",
        'pedantic = { level = "deny", priority = -1 }',
        'pedantic = { level = "allow", priority = -1 }',
    )

    findings = suppressions(broken.repo)

    refused_naming(findings, "Cargo.toml", "pedantic")


def test_a_linter_rule_turned_off_is_refused(tree: Callable[[], Tree]) -> None:
    """A rule set to `off` in the JavaScript linter's configuration is the same hole."""
    broken = tree()
    broken.edit("biome.json", '"noExplicitAny": "error"', '"noExplicitAny": "off"')

    findings = suppressions(broken.repo)

    refused_naming(findings, "biome.json", "noExplicitAny")


def test_a_type_check_rule_set_to_ignore_is_refused(tree: Callable[[], Tree]) -> None:
    """The type checker's configuration is no more exempt than the linter's."""
    broken = tree()
    broken.append("pyproject.toml", '\n[tool.ty.rules]\ninvalid-return-type = "ignore"\n')

    findings = suppressions(broken.repo)

    refused(findings, "invalid-return-type")


def test_a_disabled_linter_is_refused(tree: Callable[[], Tree]) -> None:
    """Turning the linter off entirely is every rule silenced at once."""
    broken = tree()
    broken.edit(
        "biome.json",
        '"linter": {\n    "enabled": true,',
        '"linter": {\n    "enabled": false,',
    )

    findings = suppressions(broken.repo)

    refused_naming(findings, "biome.json", "linter")


def test_a_standalone_ruff_configuration_is_read_too(tree: Callable[[], Tree]) -> None:
    """Moving the glob into `ruff.toml` does not put it out of reach."""
    broken = tree()
    broken.write("ruff.toml", '[lint]\nignore = ["S101"]\n')

    findings = suppressions(broken.repo)

    refused_naming(findings, "ruff.toml", "S101")


def test_a_single_ignored_rule_written_as_a_string_is_refused(
    tree: Callable[[], Tree],
) -> None:
    """A scalar is the same silence with one fewer bracket."""
    broken = tree()
    broken.write("ruff.toml", 'lint = { "extend-ignore" = "S603" }\n')

    findings = suppressions(broken.repo)

    refused(findings, "S603")


def test_a_crate_local_lint_allow_is_refused(tree: Callable[[], Tree]) -> None:
    """A `[lints]` table in one crate is as invisible as a workspace-wide one."""
    broken = tree()
    broken.append(
        "crates/printobserver-core/Cargo.toml",
        '\n[lints.clippy]\nmissing_panics_doc = "allow"\n',
    )

    findings = suppressions(broken.repo)

    refused(findings, "missing_panics_doc")
