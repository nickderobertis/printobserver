"""Which jobs carry a platform matrix, proven by running the real gate tier.

`just check-repo` is the tier that rules on it. Each journey below runs that
recipe over a real copy of the tree carrying one defect, and asserts on what the
run said — the rule has teeth in every direction, so every direction is driven:
a platform-dependent job narrowed away from the supported-platform list, a
cache-warm job whose cells are not the `test` job's, a matrix on the judged-lint
tier, which reads a text diff and is judged once, and
a policy declaration that would leave the rule reaching no job at all.
"""

from __future__ import annotations

from collections.abc import Callable

import pytest
from journey import GateCopy
from repo_checks.expect import failing, passing

CI = ".github/workflows/ci.yml"
WARM = ".github/workflows/warm.yml"
INSTALL = ".github/workflows/install-path.yml"
POLICY = "repo-policy.toml"
DECLARED_KINDS = 'platform_dependent_kinds = ["gate", "integration", "install", "artifact", "warm"]'
AARCH64 = "          - id: linux-aarch64\n            runner: ubuntu-24.04-arm\n"
MACOS = "          - id: macos-aarch64\n            runner: macos-15\n"
# The two Windows cells, which follow the Linux ones in every matrix carrying
# them: the macOS cell of `test` is carried by the job `test-macos`, per
# `repo-policy.toml`'s `[[workflows.carried]]`.
WINDOWS = (
    "          - id: windows-x86_64\n            runner: windows-2025\n"
    "          - id: windows-aarch64\n            runner: windows-11-arm\n"
)
# The pypi install job's own copy of that entry: the one followed by a step that
# sets up Python, which none of the registry proofs ahead of it in that file does.
INSTALL_ROUTE_AARCH64 = (
    AARCH64
    + WINDOWS
    + (
        "    runs-on: ${{ matrix.platform.runner }}\n"
        "    steps:\n"
        "      - uses: actions/setup-python@v5\n"
    )
)
# The `test` job's whole matrix, down to the line that reads a cell out of it:
# `ci.yml`'s only one, and the four cells that are not carried elsewhere.
GATE_MATRIX = (
    """    strategy:
      fail-fast: false
      matrix:
        platform:
          - id: linux-x86_64
            runner: ubuntu-24.04
"""
    + AARCH64
    + WINDOWS
    + "    runs-on: ${{ matrix.platform.runner }}\n"
)
NO_LIST = "declares no non-empty `workflows.platform_dependent_kinds` list"
NOT_A_KIND = "which is not one of the job kinds these checks classify"
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
    """`test` builds everywhere the list names and the judge runs once."""
    clean = gate_copy()

    result = clean.just("check-repo")

    passing(result)


def test_a_platform_dependent_job_narrowed_away_from_the_list_is_refused(
    gate_copy: Callable[[], GateCopy],
) -> None:
    """`test` compiles the artifact, so dropping a platform drops what proves it.

    Its `linux-aarch64` cell is swapped for another platform's rather than
    deleted, so its matrix is narrowed rather than emptied.
    """
    broken = gate_copy()
    broken.edit(CI, AARCH64, "          - id: linux-x86_64\n            runner: ubuntu-24.04\n")

    result = broken.just("check-repo")

    failing(result, naming="job `test`'s matrix omits platform `linux-aarch64`")


def test_a_platform_dependent_job_that_dropped_its_matrix_is_refused(
    gate_copy: Callable[[], GateCopy],
) -> None:
    """Narrowing a build to one platform by deleting its matrix is still narrowing."""
    broken = gate_copy()
    broken.edit(CI, GATE_MATRIX, "    runs-on: ubuntu-24.04\n")

    result = broken.just("check-repo")

    failing(result, naming="job `test` is a gate job but declares no platform matrix")


def test_the_integration_tiers_matrix_is_still_held_to_the_list(
    gate_copy: Callable[[], GateCopy],
) -> None:
    """`test` runs the printer integration tier too, and that tier's own check holds it.

    Its cells are held twice — as a gate job's by the matrix rule, and as the
    integration job's by the tier's own check — so the same narrowing is refused
    by each, and losing one rule leaves the other.
    """
    broken = gate_copy()
    broken.edit(CI, AARCH64, "")

    result = broken.just("check-repo")

    failing(result, naming="the integration job `test`'s matrix omits platform `linux-aarch64`")


def test_a_warm_job_narrowed_away_from_the_test_cells_is_refused(
    gate_copy: Callable[[], GateCopy],
) -> None:
    """A `test` cell whose cache nothing warms compiles the world on every change."""
    broken = gate_copy()
    broken.edit(WARM, AARCH64, "")

    result = broken.just("check-repo")

    failing(result, naming="job `warm`'s matrix omits platform `linux-aarch64`")


def test_a_warm_leg_for_a_carried_cell_is_refused(
    gate_copy: Callable[[], GateCopy],
) -> None:
    """The macOS cell is carried by `test-macos`, whose cache no `warm` leg is owed.

    A leg for it would spend a macOS slot on every push to `main` filling a cache
    the policy says nothing has asked for.
    """
    broken = gate_copy()
    broken.edit(WARM, AARCH64, AARCH64 + MACOS)

    result = broken.just("check-repo")

    failing(result, naming="its matrix names `macos-aarch64`, whose `test` cell nothing restores")


def test_a_platform_matrix_on_the_judged_lint_job_is_refused(
    gate_copy: Callable[[], GateCopy],
) -> None:
    """Two cells over one text diff are two verdicts, not two platforms."""
    broken = gate_copy()
    broken.edit(CI, "  lint:\n    name: lint\n", "  lint:\n    name: lint\n" + MATRIX)

    result = broken.just("check-repo")

    failing(result, naming="job `lint` declares a platform matrix")


@pytest.mark.parametrize(
    ("declaration", "naming"),
    [
        pytest.param("", NO_LIST, id="absent"),
        pytest.param("platform_dependent_kinds = []", NO_LIST, id="empty"),
        pytest.param('platform_dependent_kinds = "gate"', NO_LIST, id="not-a-list"),
        pytest.param('platform_dependent_kinds = ["gate", 7]', NOT_A_KIND, id="not-a-string"),
        pytest.param('platform_dependent_kinds = ["gate", "smoke"]', NOT_A_KIND, id="unknown"),
    ],
)
def test_a_declaration_the_rule_cannot_act_on_is_refused(
    gate_copy: Callable[[], GateCopy], declaration: str, naming: str
) -> None:
    """Each shape is refused outright, rather than leaving the rule reaching no job."""
    broken = gate_copy()
    broken.edit(POLICY, DECLARED_KINDS, declaration)

    result = broken.just("check-repo")

    failing(result, naming=naming)


def test_an_install_jobs_matrix_is_held_to_the_list_too(
    gate_copy: Callable[[], GateCopy],
) -> None:
    """A route proves nothing on a platform it was never run on."""
    broken = gate_copy()
    broken.edit(INSTALL, INSTALL_ROUTE_AARCH64, INSTALL_ROUTE_AARCH64.replace(AARCH64, ""))

    result = broken.just("check-repo")

    failing(result, naming="job `install-route-pypi`'s matrix omits platform `linux-aarch64`")


def test_a_policy_whose_table_is_not_a_table_is_refused(
    gate_copy: Callable[[], GateCopy],
) -> None:
    """A malformed table is a finding, not an attribute error on the way to one."""
    broken = gate_copy()
    # `workflows` becomes a top-level string rather than a table, which is what a
    # reader calling `.get` on it without narrowing would trip over.
    broken.edit(POLICY, "schema_version = 1\n", 'schema_version = 1\nworkflows = "yes"\n')
    broken.edit(POLICY, "\n[workflows]\n", "\n[workflow-settings]\n")
    while "[[workflows.carried]]" in broken.read(POLICY):
        broken.edit(POLICY, "[[workflows.carried]]", "[[workflow-settings.carried]]")

    result = broken.just("check-repo")

    failing(result, naming=NO_LIST)
