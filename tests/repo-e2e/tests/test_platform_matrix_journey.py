"""Which jobs carry a platform matrix, proven by running the real gate tier.

`just check-repo` is the tier that rules on it. Each journey below runs that
recipe over a real copy of the tree carrying one defect, and asserts on what the
run said — the rule has teeth in every direction, so every direction is driven:
a platform-dependent job narrowed away from the supported-platform list, a
matrix on the judged-lint tier, which reads a text diff and is judged once, and
a policy declaration that would leave the rule reaching no job at all.
"""

from __future__ import annotations

from collections.abc import Callable

import pytest
from journey import GateCopy
from repo_checks.expect import failing, passing

CI = ".github/workflows/ci.yml"
INSTALL = ".github/workflows/install-path.yml"
POLICY = "repo-policy.toml"
DECLARED_KINDS = 'platform_dependent_kinds = ["gate", "integration", "install"]'
AARCH64 = "          - id: linux-aarch64\n            runner: ubuntu-24.04-arm\n"
# The integration job's own copy of that entry: the one that is followed by a
# checkout taking no `with:` block, which is what tells it apart from the gate's.
INTEGRATION_AARCH64 = AARCH64 + (
    "    runs-on: ${{ matrix.platform.runner }}\n"
    "    steps:\n"
    "      - uses: actions/checkout@v5\n"
    "      - uses: extractions/setup-just@v3\n"
)
# The gate's whole matrix, down to the line that reads a cell out of it.
GATE_MATRIX = (
    """    strategy:
      fail-fast: false
      matrix:
        platform:
          - id: linux-x86_64
            runner: ubuntu-24.04
"""
    + AARCH64
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


def test_a_platform_dependent_job_that_dropped_its_matrix_is_refused(
    gate_copy: Callable[[], GateCopy],
) -> None:
    """Narrowing a build to one platform by deleting its matrix is still narrowing."""
    broken = gate_copy()
    broken.edit(CI, GATE_MATRIX, "    runs-on: ubuntu-24.04\n")

    result = broken.just("check-repo")

    failing(result, naming="job `gate` is a gate job but declares no platform matrix")


def test_the_integration_jobs_matrix_is_still_held_to_the_list(
    gate_copy: Callable[[], GateCopy],
) -> None:
    """Exempting it from this rule hands it to its own check, not to nobody."""
    broken = gate_copy()
    broken.edit(CI, INTEGRATION_AARCH64, INTEGRATION_AARCH64.replace(AARCH64, ""))

    result = broken.just("check-repo")

    failing(result, naming="integration job `integration`'s matrix omits platform")


def test_a_platform_matrix_on_the_judged_lint_job_is_refused(
    gate_copy: Callable[[], GateCopy],
) -> None:
    """Two cells over one text diff are two verdicts, not two platforms."""
    broken = gate_copy()
    broken.edit(CI, "  llmlint:\n    name: llmlint\n", "  llmlint:\n    name: llmlint\n" + MATRIX)

    result = broken.just("check-repo")

    failing(result, naming="job `llmlint` declares a platform matrix")


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
    broken.edit(INSTALL, AARCH64, "")

    result = broken.just("check-repo")

    failing(result, naming="job `install-route-pypi`'s matrix omits platform `linux-aarch64`")
