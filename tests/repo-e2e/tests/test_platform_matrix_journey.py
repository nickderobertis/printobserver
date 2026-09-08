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

from journey import GateCopy
from repo_checks.expect import failing, passing

CI = ".github/workflows/ci.yml"
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


def test_a_policy_declaring_no_platform_dependent_kinds_is_refused(
    gate_copy: Callable[[], GateCopy],
) -> None:
    """A rule left reaching no job would pass every workflow by inspecting none."""
    broken = gate_copy()
    broken.edit(POLICY, DECLARED_KINDS, "platform_dependent_kinds = []")

    result = broken.just("check-repo")

    failing(result, naming="declares no non-empty `workflows.platform_dependent_kinds` list")


def test_a_policy_naming_a_kind_no_job_can_be_is_refused(
    gate_copy: Callable[[], GateCopy],
) -> None:
    """A typo there is refused rather than quietly matching nothing."""
    broken = gate_copy()
    broken.edit(POLICY, DECLARED_KINDS, 'platform_dependent_kinds = ["gate", "smoke"]')

    result = broken.just("check-repo")

    failing(result, naming="which is not one of the job kinds these checks classify")
