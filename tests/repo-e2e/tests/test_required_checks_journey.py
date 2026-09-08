"""The recorded required checks name what CI reports, proven through the gate.

A branch-protection rule names a check by the name GitHub reports it under, so a
record naming a job key a matrix has moved past is a rule nothing can ever
satisfy. `just check-repo` derives the reported contexts and rules on that; each
journey below runs the real recipe over a real copy and asserts on what it said.
"""

from __future__ import annotations

from collections.abc import Callable

from journey import GateCopy
from repo_checks.expect import failing

AGENTS = "AGENTS.md"
CI = ".github/workflows/ci.yml"
QUALIFIED = "    name: gate (${{ matrix.platform.id }})\n"
X86 = "- `gate (linux-x86_64)`\n"
AARCH64 = "- `gate (linux-aarch64)`\n"


def test_a_stale_bare_matrix_job_name_is_refused(
    gate_copy: Callable[[], GateCopy],
) -> None:
    """`gate` is the job's key; its cells report under names carrying the platform."""
    broken = gate_copy()
    broken.edit(AGENTS, X86 + AARCH64, "- `gate`\n")

    result = broken.just("check-repo")

    failing(result, naming="but no committed workflow reports a status context by that name")


def test_a_record_naming_only_one_of_the_gates_cells_is_refused(
    gate_copy: Callable[[], GateCopy],
) -> None:
    """A gate required on one platform is a merge path the other never blocked."""
    broken = gate_copy()
    broken.edit(AGENTS, AARCH64, "")

    result = broken.just("check-repo")

    failing(result, naming="some but not all of job `gate`'s status contexts as required")


def test_a_required_name_several_cells_report_under_is_refused(
    gate_copy: Callable[[], GateCopy],
) -> None:
    """Two check runs under one name are two a rule requiring it cannot tell apart."""
    broken = gate_copy()
    broken.edit(CI, QUALIFIED, "    name: gate\n")
    broken.edit(AGENTS, X86 + AARCH64, "- `gate`\n")

    result = broken.just("check-repo")

    failing(result, naming="2 matrix cells of job `gate` report under that one name")


def test_a_name_interpolating_a_field_the_cells_do_not_carry_is_refused(
    gate_copy: Callable[[], GateCopy],
) -> None:
    """A typo there would derive a context nothing reports, silently."""
    broken = gate_copy()
    broken.edit(CI, QUALIFIED, "    name: gate (${{ matrix.platform.arch }})\n")

    result = broken.just("check-repo")

    failing(result, naming="but its matrix cells carry `id`, `runner`")


def test_a_name_interpolating_a_field_no_name_can_be_built_from_is_refused(
    gate_copy: Callable[[], GateCopy],
) -> None:
    """A cell field that is not a scalar cannot name a check run either."""
    broken = gate_copy()
    broken.edit(
        CI,
        "          - id: linux-x86_64\n            runner: ubuntu-24.04\n",
        "          - id: [linux, x86_64]\n            runner: ubuntu-24.04\n",
    )

    result = broken.just("check-repo")

    failing(result, naming="whose value is not something a status context can be named after")
