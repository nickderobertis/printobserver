"""The gate is strict, proven by running it over a tree carrying each defect.

Formatting, linting, type checking and tests each fail the build on an issue,
coverage is measured on the test run, and the build fails below the recorded
floor. Every assertion below comes from a real `just check` over a real copy.
"""

# `assert` is how pytest states an assertion and how it produces the failure
# message a reader acts on; suppressions.toml carries the reason.

from __future__ import annotations

from collections.abc import Callable
from math import ceil
from pathlib import Path

import pytest
from journey import REPO_ROOT, GateCopy, tracked_files
from repo_checks.expect import contains, failing, passing
from repo_checks.model import Repo

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

# The floor the gate enforces, read from the file that records it so that this
# journey cannot assert on a number the gate no longer uses.
RUST_FLOOR: int = Repo(REPO_ROOT).policy["gate"]["coverage"]["rust"]
FLOOR_REFUSAL = f"coverage is below the {RUST_FLOOR}% floor"
# Uncovered lines beyond the arithmetic below, so that rounding in the report
# cannot leave a tree sitting exactly on the floor rather than under it.
SINKING_MARGIN = 10
# Helpers the grown case adds. Each is driven by the block's own test, so the
# case is a tree several times the weight of the committed one that is still at
# full coverage when the uncovered block lands on it.
WELL_COVERED_HELPERS = 200
# Lines of generated Rust per generated function. Both blocks below grow with
# the tree, and clippy's pedantic `too_many_lines` refuses a function over a
# hundred lines, so both are emitted as several functions rather than one.
LINES_PER_FUNCTION = 50


def rust_source_lines(root: Path) -> int:
    """Every line of Rust a tree carries, committed or newly written."""
    return sum(
        len((root / name).read_text(encoding="utf-8").splitlines())
        for name in tracked_files(root)
        if name.endswith(".rs")
    )


def well_covered_rust(helpers: int) -> str:
    """Helper functions, one per line of `helpers`, that a test can drive."""
    return "".join(
        "\n/// A helper the block's own test drives.\n"
        "#[must_use]\n"
        f"pub fn covered_{n}(value: u32) -> u32 {{\n"
        f"    value.saturating_add({n + 1})\n"
        "}\n"
        for n in range(helpers)
    )


def driving_test(helpers: int) -> str:
    """A test module driving every line of `well_covered_rust(helpers)`."""
    if helpers == 0:
        return ""
    functions = "\n".join(
        "    #[test]\n"
        f"    fn helpers_from_{start}_are_driven() {{\n"
        + "\n".join(
            f"        assert_eq!(super::covered_{n}(0), {n + 1});"
            for n in range(start, min(start + LINES_PER_FUNCTION, helpers))
        )
        + "\n    }\n"
        for start in range(0, helpers, LINES_PER_FUNCTION)
    )
    return f"\n#[cfg(test)]\nmod well_covered {{\n{functions}}}\n"


def undriven_rust(lines: int) -> str:
    """Functions nothing calls, carrying at least `lines` lines between them."""
    body = "\n".join("    total = total.saturating_add(1);" for _ in range(LINES_PER_FUNCTION))
    return "".join(
        "\n/// A function no test drives, one of several sinking the tree's coverage.\n"
        "#[must_use]\n"
        f"pub fn undriven_{n}(value: u32) -> u32 {{\n"
        "    let mut total = value;\n"
        f"{body}\n"
        "    total\n"
        "}\n"
        for n in range(ceil(lines / LINES_PER_FUNCTION))
    )


def coverage_defect(root: Path, helpers: int) -> str:
    """A block that sinks a tree's Rust coverage, however much Rust the tree carries.

    A tree at full coverage carrying `covered` coverable lines drops under a
    floor of `RUST_FLOOR` percent once enough uncovered lines join it: the ratio
    `covered / (covered + added)` falls below the floor for every
    `added > covered * (100 - RUST_FLOOR) / RUST_FLOOR`. No file carries more
    coverable lines than it carries lines at all, so counting the Rust the tree
    would carry once this block lands bounds `covered` from above and sizes a
    block that sinks a tree of any weight.

    That is the whole point of computing the block rather than writing it down.
    A fixed one was worth a coverage defect while the crates were empty and
    worth nothing once several well-covered crates had landed — at which point
    the gate rightly passed and this journey stopped proving anything about the
    floor at all.

    The well-covered helpers come first and the test driving them last, with the
    uncovered functions between: clippy refuses an item declared after a test
    module, so the block is emitted whole rather than appended in pieces.
    """
    covered = well_covered_rust(helpers)
    driving = driving_test(helpers)
    ceiling = rust_source_lines(root) + covered.count("\n") + driving.count("\n")
    added = ceil(ceiling * (100 - RUST_FLOOR) / RUST_FLOOR) + SINKING_MARGIN
    return covered + undriven_rust(added) + driving


# journey: the-gate-refuses-a-defective-tree
def test_the_gate_accepts_the_committed_tree(gate_copy: Callable[[], GateCopy]) -> None:
    """The clean tree the defect copies are made from passes the whole gate."""
    clean = gate_copy()

    result = clean.just("check")

    passing(result)


def test_the_gate_fails_on_a_badly_formatted_file(
    gate_copy: Callable[[], GateCopy],
) -> None:
    """Formatting is a build blocker, not a suggestion."""
    broken = gate_copy()
    broken.append(TYPES, BADLY_FORMATTED)

    result = broken.just("check")

    failing(result, naming="badly_formatted")


def test_the_gate_fails_on_a_lint_finding(gate_copy: Callable[[], GateCopy]) -> None:
    """Linting is a build blocker: there is no warnings-only mode."""
    broken = gate_copy()
    broken.append(TYPES, USELESS_CONVERSION)

    result = broken.just("check")

    failing(result, naming="useless_conversion")


def test_the_gate_fails_on_a_type_error(gate_copy: Callable[[], GateCopy]) -> None:
    """Type checking is a build blocker, in every language the repository carries."""
    broken = gate_copy()
    broken.append(SDK, WRONG_RETURN_TYPE)

    result = broken.just("check")

    failing(result, naming="invalid-return-type")


def test_the_gate_fails_on_a_failing_test(gate_copy: Callable[[], GateCopy]) -> None:
    """A red test stops the build."""
    broken = gate_copy()
    broken.append(TYPES, FAILING_TEST)

    result = broken.just("check")

    failing(result, naming="the_gate_must_fail_on_a_failing_test")


@pytest.mark.parametrize(
    "helpers",
    [0, WELL_COVERED_HELPERS],
    ids=["the-committed-tree", "a-tree-grown-by-well-covered-code"],
)
def test_the_gate_fails_below_the_recorded_coverage_floor(
    gate_copy: Callable[[], GateCopy], helpers: int
) -> None:
    """Coverage is measured on the test run and the floor is enforced, at any weight.

    The second case is the property the first one alone cannot carry: it grows
    the copy by a substantial block of Rust the copy's own tests drive every
    line of, and the gate still has to refuse it. What makes that hold is that
    the uncovered block is sized from what the grown tree measures rather than
    written down once against a nearly empty workspace.
    """
    broken = gate_copy()
    broken.append(TYPES, coverage_defect(broken.root, helpers))

    result = broken.just("check")

    failing(result, naming=FLOOR_REFUSAL)


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

    contains(body, f"just {tier}")
