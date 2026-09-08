"""The gate is strict, proven by running it over a tree carrying each defect.

Formatting, linting, type checking and tests each fail the build on an issue,
coverage is measured on the test run, and the build fails below the recorded
floor. Every assertion below comes from a real `just check` over a real copy.
"""

from __future__ import annotations

from collections.abc import Callable

import pytest
from journey import REPO_ROOT, GateCopy, output

TYPES = "crates/printobserver-types/src/lib.rs"
SDK = "python/printobserver-sdk/src/printobserver_sdk/__init__.py"

BADLY_FORMATTED = "\nfn   badly_formatted()   {}\n"
USELESS_CONVERSION = """
/// A conversion `clippy` refuses.
#[must_use]
pub fn useless(value: u32) -> u32 {
    let converted: u32 = value.into();
    converted
}
"""
WRONG_RETURN_TYPE = '''

def protocol_version() -> int:
    """Return the protocol version this client speaks."""
    return "not an integer"
'''
FAILING_TEST = """
#[cfg(test)]
mod tests {
    #[test]
    fn the_gate_must_fail_on_a_failing_test() {
        assert_eq!(1 + 1, 3, "arithmetic still works");
    }
}
"""
UNCOVERED = """
/// A function no test drives.
#[must_use]
pub fn undriven(value: u32) -> u32 {
    value.saturating_add(1)
}
"""


def test_the_gate_accepts_the_committed_tree(gate_copy: Callable[[], GateCopy]) -> None:
    """The clean tree the defect copies are made from passes the whole gate."""
    clean = gate_copy()

    result = clean.just("check")

    assert result.returncode == 0, output(result)


def test_the_gate_fails_on_a_badly_formatted_file(
    gate_copy: Callable[[], GateCopy],
) -> None:
    """Formatting is a build blocker, not a suggestion."""
    broken = gate_copy()
    broken.append(TYPES, BADLY_FORMATTED)

    result = broken.just("check")

    assert result.returncode != 0
    assert "badly_formatted" in output(result), output(result)


def test_the_gate_fails_on_a_lint_finding(gate_copy: Callable[[], GateCopy]) -> None:
    """Linting is a build blocker: there is no warnings-only mode."""
    broken = gate_copy()
    broken.append(TYPES, USELESS_CONVERSION)

    result = broken.just("check")

    assert result.returncode != 0
    assert "useless_conversion" in output(result), output(result)


def test_the_gate_fails_on_a_type_error(gate_copy: Callable[[], GateCopy]) -> None:
    """Type checking is a build blocker, in every language the repository carries."""
    broken = gate_copy()
    broken.append(SDK, WRONG_RETURN_TYPE)

    result = broken.just("check")

    assert result.returncode != 0
    assert "invalid-return-type" in output(result), output(result)


def test_the_gate_fails_on_a_failing_test(gate_copy: Callable[[], GateCopy]) -> None:
    """A red test stops the build."""
    broken = gate_copy()
    broken.append(TYPES, FAILING_TEST)

    result = broken.just("check")

    assert result.returncode != 0
    assert "the_gate_must_fail_on_a_failing_test" in output(result), output(result)


def test_the_gate_fails_below_the_recorded_coverage_floor(
    gate_copy: Callable[[], GateCopy],
) -> None:
    """Coverage is measured on the test run and the floor is enforced."""
    broken = gate_copy()
    broken.append(TYPES, UNCOVERED)

    result = broken.just("check")

    assert result.returncode != 0
    assert "coverage is below the 95% floor" in output(result), output(result)


@pytest.mark.parametrize(
    "tier",
    [
        "format-check",
        "lint",
        "typecheck",
        "test",
        "coverage",
        "build",
        "lint-workflows",
        "check-repo",
        "test-e2e",
    ],
)
def test_the_check_recipe_invokes_every_declared_tier(tier: str) -> None:
    """The gate the journeys above run is the whole tier list, end-to-end included."""
    justfile = (REPO_ROOT / "justfile").read_text(encoding="utf-8")
    body = justfile[justfile.index("\ncheck:\n") : justfile.index("# Rewrite every project")]

    assert f"just {tier}" in body
