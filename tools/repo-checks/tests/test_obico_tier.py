"""The scheduled Obico tier: its workflow's triggers, its schedule, and the gate's silence.

Every journey below drives the committed check against a real copy of the
committed tree with one defect in it, save the last, which runs the ordinary
gate's own project selection and asks what it selects.

The trigger journeys are the point of the file. This tier builds Obico's own
images, starts four containers and waits a real failure alert out, so a copy of
its workflow that also fired on a change would be the gate again under another
name — and the way that mistake is actually made is by *adding* a change-firing
trigger beside the schedule rather than by removing the schedule. So that is the
copy the check is driven against.
"""

from __future__ import annotations

import json
from collections.abc import Callable

from repo_checks.checks_obico import obico_tier
from repo_checks.expect import accepted, contains, equal, refused, truth
from repo_checks.model import Repo
from repo_checks.parsing import recipes
from repo_checks.shell import run
from treecopy import REPO_ROOT, Tree

WORKFLOW = ".github/workflows/obico.yml"
POLICY = "repo-policy.toml"
JUSTFILE = "justfile"
AGENTS = "AGENTS.md"

SCHEDULE = """on:
  schedule:
    # Weekly, on Monday. Obico releases far less often than this repository
    # changes, so a weekly reconciliation dates the claim to within a week
    # without spending an hour of runner time on every change.
    - cron: "17 4 * * 1"
  workflow_dispatch:
"""

# The Nx entry point the gate's fan-out tiers go through, and the flag that
# names the target each one selects on.
NX_RUN_MANY = "bunx nx run-many -t "


def test_the_committed_configuration_is_accepted(committed: Repo) -> None:
    """The workflow this repository ships runs the tier, on a schedule and nothing else."""
    accepted(obico_tier(committed))


def test_a_configuration_with_no_obico_workflow_is_refused(tree: Callable[[], Tree]) -> None:
    """A tier no workflow runs is a tier nobody runs."""
    broken = tree()
    broken.remove(WORKFLOW)

    findings = obico_tier(broken.repo)

    refused(findings, "declares no scheduled Obico tier workflow")


def test_a_workflow_carrying_no_schedule_is_refused(tree: Callable[[], Tree]) -> None:
    """Out of the gate and on no schedule is out of everything."""
    broken = tree()
    broken.edit(WORKFLOW, SCHEDULE, "on:\n  workflow_dispatch:\n")

    findings = obico_tier(broken.repo)

    refused(findings, "declares no `schedule` trigger")


def test_a_schedule_naming_no_cron_expression_is_refused(tree: Callable[[], Tree]) -> None:
    """A `schedule:` key with nothing under it fires on nothing at all."""
    broken = tree()
    broken.edit(WORKFLOW, SCHEDULE, "on:\n  schedule: []\n  workflow_dispatch:\n")

    findings = obico_tier(broken.repo)

    refused(findings, "names no cron expression")


def test_a_change_firing_trigger_beside_the_schedule_is_refused(
    tree: Callable[[], Tree],
) -> None:
    """The mistake this check exists for: a schedule *and* a push, so it fires on changes."""
    broken = tree()
    broken.edit(
        WORKFLOW, "  workflow_dispatch:\n", "  workflow_dispatch:\n  push:\n    branches: [main]\n"
    )

    findings = obico_tier(broken.repo)

    refused(findings, "fires on `push`")
    refused(findings, "this tier must not fire on a change")


def test_a_change_request_trigger_beside_the_schedule_is_refused(
    tree: Callable[[], Tree],
) -> None:
    """A change-request event is a change event too, however cheap it looks."""
    broken = tree()
    broken.edit(WORKFLOW, "  workflow_dispatch:\n", "  workflow_dispatch:\n  pull_request:\n")

    findings = obico_tier(broken.repo)

    refused(findings, "fires on `pull_request`")


def test_a_workflow_that_does_not_run_the_tier_is_refused(tree: Callable[[], Tree]) -> None:
    """A scheduled workflow that runs something else is not this tier."""
    broken = tree()
    broken.edit(WORKFLOW, "      - run: just test-obico\n", "      - run: just check-repo\n")

    findings = obico_tier(broken.repo)

    refused(findings, "declares no job that runs `just test-obico`")


def test_a_workflow_that_omits_the_bring_down_is_refused(tree: Callable[[], Tree]) -> None:
    """A tier that failed with no teardown is four containers nobody stopped."""
    broken = tree()
    broken.edit(WORKFLOW, "      - if: always()\n        run: just obico-down\n", "")

    findings = obico_tier(broken.repo)

    refused(findings, "does not run `just obico-down`")


def test_a_bring_down_that_does_not_run_on_failure_is_refused(
    tree: Callable[[], Tree],
) -> None:
    """`if: always()` is the whole point of the teardown step."""
    broken = tree()
    broken.edit(
        WORKFLOW,
        "      - if: always()\n        run: just obico-down\n",
        "      - run: just obico-down\n",
    )

    findings = obico_tier(broken.repo)

    refused(findings, "unconditionally rather than under `if: always()`")


def test_a_gate_that_declares_the_tier_is_refused(tree: Callable[[], Tree]) -> None:
    """The gate must not select this tier: that is the cost it exists to keep out."""
    broken = tree()
    broken.edit(POLICY, '    "test-e2e",\n]', '    "test-e2e",\n    "test-obico",\n]')

    findings = obico_tier(broken.repo)

    refused(findings, "gate.tiers names `test-obico`")


def test_a_check_recipe_that_invokes_the_tier_is_refused(tree: Callable[[], Tree]) -> None:
    """Not in the declared tiers, and not smuggled into the `check` recipe either."""
    broken = tree()
    broken.edit(JUSTFILE, "    just test-e2e\n", "    just test-e2e\n    just test-obico\n")

    findings = obico_tier(broken.repo)

    refused(findings, "the `check` recipe invokes `just test-obico`")


def test_a_recorded_schedule_that_differs_from_the_workflows_is_refused(
    tree: Callable[[], Tree],
) -> None:
    """A schedule stated in prose that is not the one that fires is worse than none."""
    broken = tree()
    broken.edit(AGENTS, "- cron: `17 4 * * 1`", "- cron: `0 3 * * *`")

    findings = obico_tier(broken.repo)

    refused(findings, "AGENTS.md records the schedule `0 3 * * *`")
    refused(findings, "which AGENTS.md's `obico-tier-schedule` block does not record")


def test_a_recorded_schedule_that_is_absent_is_refused(tree: Callable[[], Tree]) -> None:
    """The block is where the prose and the workflow are held to each other."""
    broken = tree()
    broken.edit(AGENTS, "- cron: `17 4 * * 1`", "the schedule is written down somewhere else")

    findings = obico_tier(broken.repo)

    refused(findings, "records no schedule")


def test_a_section_that_does_not_say_how_to_run_the_tier_by_hand_is_refused(
    tree: Callable[[], Tree],
) -> None:
    """A developer runs it with one recipe, and this is where that recipe is pasteable from."""
    broken = tree()
    broken.edit(AGENTS, "just obico-up\njust test-obico\n", "just test-obico\n")

    findings = obico_tier(broken.repo)

    refused(findings, "states no `just obico-up` command")


def _gate_targets() -> list[str]:
    """Every Nx target the recipes `just check` invokes fan out over."""
    justfile = (REPO_ROOT / "justfile").read_text(encoding="utf-8")
    parsed = recipes(justfile)
    tiers = [
        line.split()[1]
        for line in parsed["check"].body
        if line.split()[:1] == ["just"] and len(line.split()) > 1
    ]
    targets: list[str] = []
    for tier in tiers:
        for line in parsed[tier].body:
            if line.startswith(NX_RUN_MANY):
                targets.append(line[len(NX_RUN_MANY) :].split()[0])
    return targets


def _selects(target: str) -> list[str]:
    """The projects Nx itself selects for one target.

    Raises:
        AssertionError: If Nx will not answer, so a silent empty answer cannot
            be read as "the gate selects nothing".
    """
    answered = run(
        ["bunx", "nx", "show", "projects", "--with-target", target],
        cwd=REPO_ROOT,
        timeout=600,
    )
    if answered.returncode != 0:
        message = (
            f"expected Nx to answer for `{target}`; it exited "
            f"{answered.returncode}:\n{answered.stdout}{answered.stderr}"
        )
        raise AssertionError(message)
    return list(json.loads(answered.stdout.strip().splitlines()[-1]))


def test_the_ordinary_gates_own_selection_does_not_include_this_tier() -> None:
    """Run the gate's own fan-out selection, and ask whether the tier is in it.

    This is the gate's real selection rather than a reading of the justfile:
    every target `just check` fans out over is put to Nx, and Nx is asked what it
    selects. The tier's own target must not be one of them — and must still be a
    target Nx knows, so that "not selected" means "reachable only by its own
    recipe" rather than "does not exist".
    """
    targets = _gate_targets()
    truth(targets, describing="at least one Nx target the `check` recipe fans out over")
    for target in targets:
        truth(
            target != "test-obico",
            describing=(
                f"the gate's tier `{target}` not to be the scheduled Obico tier's own "
                f"target: every gate run would then build Obico's images"
            ),
        )
        # Asked of Nx rather than assumed: a target the gate names that selects
        # nothing would make this journey vacuous.
        truth(_selects(target), describing=f"the gate's target `{target}` to select projects")

    contains(_selects("test-obico"), "obico-env", describing="what Nx selects for `test-obico`")
    equal(
        [t for t in targets if t == "test-obico"],
        [],
        describing="the gate's targets that are the scheduled Obico tier",
    )
