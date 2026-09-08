"""Which jobs carry a platform matrix, proven by running the real gate tier.

`just check-repo` is the tier that rules on it. Each journey below runs that
recipe over a real copy of the tree carrying one defect, and asserts on what the
run said — the rule has teeth in both directions, so both directions are driven:
a platform-dependent job narrowed away from the supported-platform list, and a
matrix on the judged-lint tier, which reads a text diff and is judged once.
"""

# `assert` is how pytest states an assertion and how it produces the failure
# message a reader acts on; suppressions.toml carries the reason.

from __future__ import annotations

from collections.abc import Callable

from journey import GateCopy
from repo_checks.expect import failing, passing

CI = ".github/workflows/ci.yml"
AARCH64 = "          - id: linux-aarch64\n            runner: ubuntu-24.04-arm\n"
MATRIX = """    strategy:
      fail-fast: false
      matrix:
        platform:
          - id: linux-x86_64
            runner: ubuntu-24.04
"""


def test_the_tier_accepts_the_committed_configuration(
    gate_copy: Callable[[], GateCopy],
) -> None:
    """The gate builds everywhere the list names and the judge runs once."""
    clean = gate_copy()

    result = clean.just("check-repo")

    passing(result)


def test_a_platform_dependent_job_narrowed_away_from_the_list_is_refused(
    gate_copy: Callable[[], GateCopy],
) -> None:
    """`gate` compiles the artifact, so dropping a platform drops what proves it."""
    broken = gate_copy()
    broken.edit(CI, AARCH64, "")

    result = broken.just("check-repo")

    failing(result, naming="job `gate`'s matrix omits platform `linux-aarch64`")


def test_a_platform_matrix_on_the_judged_lint_job_is_refused(
    gate_copy: Callable[[], GateCopy],
) -> None:
    """Two cells over one text diff are two verdicts, not two platforms."""
    broken = gate_copy()
    broken.edit(CI, "  llmlint:\n    name: llmlint\n", "  llmlint:\n    name: llmlint\n" + MATRIX)

    result = broken.just("check-repo")

    failing(result, naming="job `llmlint` declares a platform matrix")
