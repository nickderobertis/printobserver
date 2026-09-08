"""The printer integration tier's job, held to the recipe set, the platform list and every change.

Every journey below drives the committed check against a real copy of the
committed tree with one defect in it. Two of them are about the supported-
platform list rather than the job, because that list is a source this
repository's integration job is derived from and not one it may edit to fit:
a list that gained a platform refuses an unchanged job, and a list that lost one
it carried at the base revision is refused outright.
"""

from __future__ import annotations

from collections.abc import Callable
from pathlib import Path

from repo_checks.checks_integration import integration_tier
from repo_checks.expect import accepted, refused
from repo_checks.model import Repo
from repo_checks.shell import run
from treecopy import Tree

CI = ".github/workflows/ci.yml"
AARCH64_ENTRY = "          - id: linux-aarch64\n            runner: ubuntu-24.04-arm\n"
RISCV64 = (
    "- `linux-riscv64` — runner `ubuntu-24.04-riscv`, Rust target "
    "`riscv64gc-unknown-linux-gnu`, service manager `systemd`, install path: yes\n"
)
AARCH64_PLATFORM = (
    "- `linux-aarch64` — runner `ubuntu-24.04-arm`, Rust target "
    "`aarch64-unknown-linux-gnu`, service manager `systemd`, install path: yes\n"
)
NO_EXCLUSIONS = """[//]: # (BEGIN virtual-printer-exclusions)
No platform is excluded. OctoPrint's virtual printer is a bundled pure-Python
plugin that needs no hardware, so it is available on every platform the list
above names, and the integration job runs on all of them.
[//]: # (END virtual-printer-exclusions)"""


def _integration_matrix_entry(tree: Tree, old: str, new: str) -> None:
    """Edit the integration job's own matrix, which is the second one in the file."""
    text = tree.read(CI)
    start = text.index("  integration:")
    head, tail = text[:start], text[start:]
    if old not in tail:
        message = f"{CI}'s integration job does not contain {old!r}; the fixture is stale"
        raise AssertionError(message)
    tree.write(CI, head + tail.replace(old, new, 1))


def test_the_committed_configuration_is_accepted(committed: Repo) -> None:
    """The job this repository ships runs the tier, everywhere, on every change."""
    accepted(integration_tier(committed))


def test_a_configuration_with_no_integration_job_is_refused(tree: Callable[[], Tree]) -> None:
    """A tier nothing runs proves nothing — and a job with no bring-up is not one."""
    broken = tree()
    broken.edit(CI, "      - run: just octoprint-up\n", "")

    findings = integration_tier(broken.repo)

    refused(findings, "declares no printer-integration job")


def test_a_step_naming_a_recipe_the_set_does_not_declare_is_refused(
    tree: Callable[[], Tree],
) -> None:
    """The job's steps are this repository's recipes, not commands of their own."""
    broken = tree()
    broken.edit(CI, "      - run: just test-integration\n", "      - run: just test-integrations\n")

    findings = integration_tier(broken.repo)

    refused(findings, "`just test-integrations`, which the recipe set does not declare")


def test_a_step_running_something_that_is_not_a_recipe_is_refused(
    tree: Callable[[], Tree],
) -> None:
    """A job that ran the tier its own way would not be running this repository's tier."""
    broken = tree()
    broken.edit(
        CI,
        "      - run: just test-integration\n",
        "      - run: uv run -q pytest tools/octoprint-env/tests\n",
    )

    findings = integration_tier(broken.repo)

    refused(findings, "which is not one of this repository's recipes")


def test_a_job_omitting_the_bring_down_recipe_is_refused(tree: Callable[[], Tree]) -> None:
    """An environment nothing stops is a runner nobody stopped."""
    broken = tree()
    broken.edit(broken_path := CI, "        run: just octoprint-down\n", "")
    broken.edit(broken_path, "      - if: always()\n", "")

    findings = integration_tier(broken.repo)

    refused(findings, "does not run `just octoprint-down`")


def test_a_job_omitting_a_platform_no_exclusion_records_is_refused(
    tree: Callable[[], Tree],
) -> None:
    """The matrix is the list minus the exclusions, and nothing is excluded today."""
    broken = tree()
    _integration_matrix_entry(broken, AARCH64_ENTRY, "")

    findings = integration_tier(broken.repo)

    refused(findings, "omits platform `linux-aarch64`")


def test_a_job_naming_a_platform_the_list_does_not_name_is_refused(
    tree: Callable[[], Tree],
) -> None:
    """The job cannot widen past the list it is derived from either."""
    broken = tree()
    _integration_matrix_entry(
        broken,
        AARCH64_ENTRY,
        AARCH64_ENTRY + "          - id: windows-x86_64\n            runner: windows-2022\n",
    )

    findings = integration_tier(broken.repo)

    refused(findings, "`windows-x86_64`, which AGENTS.md's supported-platform list does not")


def test_an_exclusion_with_no_reason_is_refused(tree: Callable[[], Tree]) -> None:
    """A platform excluded with no reason is a narrowing nobody has to justify."""
    broken = tree()
    broken.edit(
        "AGENTS.md",
        NO_EXCLUSIONS,
        "[//]: # (BEGIN virtual-printer-exclusions)\n"
        "- `linux-aarch64`\n"
        "[//]: # (END virtual-printer-exclusions)",
    )

    findings = integration_tier(broken.repo)

    refused(findings, "with no reason it is")


def test_an_exclusion_with_a_reason_admits_a_matrix_without_that_platform(
    tree: Callable[[], Tree],
) -> None:
    """The exclusion mechanism works, so a real one is recorded rather than hidden."""
    allowed = tree()
    allowed.edit(
        "AGENTS.md",
        NO_EXCLUSIONS,
        "[//]: # (BEGIN virtual-printer-exclusions)\n"
        "- `linux-aarch64` — no OctoPrint wheel is published for this architecture in "
        "the pinned release, so the virtual printer cannot be started here.\n"
        "[//]: # (END virtual-printer-exclusions)",
    )
    _integration_matrix_entry(allowed, AARCH64_ENTRY, "")

    accepted(integration_tier(allowed.repo), describing="a job that skips an excluded platform")


def test_a_list_that_gains_a_platform_refuses_the_unchanged_job(
    tree: Callable[[], Tree],
) -> None:
    """The list is the source, not the matrix beside it."""
    broken = tree()
    broken.edit("AGENTS.md", AARCH64_PLATFORM, AARCH64_PLATFORM + RISCV64)

    findings = integration_tier(broken.repo)

    refused(findings, "omits platform `linux-riscv64`")


def _committed(root: Path, message: str) -> str:
    """Commit whatever is in a copy, and answer with the revision it made."""
    for arguments in (
        ["add", "-A"],
        [
            "-c",
            "user.email=test@example.com",
            "-c",
            "user.name=test",
            "commit",
            "-q",
            "-m",
            message,
        ],
    ):
        run(["git", *arguments], cwd=root, check=True)
    return run(["git", "rev-parse", "HEAD"], cwd=root, check=True).stdout.strip()


def test_a_list_that_loses_a_platform_it_carried_at_the_base_is_refused(
    tree: Callable[[], Tree],
) -> None:
    """Deleting a platform and narrowing the job to match is the state this refuses."""
    broken = tree()
    run(["git", "init", "-q", "-b", "main"], cwd=broken.root, check=True)
    base = _committed(broken.root, "chore: the committed tree, copied")

    broken.edit("AGENTS.md", AARCH64_PLATFORM, "")
    broken.write(CI, broken.read(CI).replace(AARCH64_ENTRY, ""))
    _committed(broken.root, "ci: narrow the platform list and every matrix with it")

    findings = integration_tier(broken.repo, base=base)

    refused(findings, "no longer names `linux-aarch64`")


def test_a_list_that_keeps_every_platform_it_carried_at_the_base_is_accepted(
    tree: Callable[[], Tree],
) -> None:
    """A change that leaves the list alone is not narrowing it."""
    allowed = tree()
    run(["git", "init", "-q", "-b", "main"], cwd=allowed.root, check=True)
    base = _committed(allowed.root, "chore: the committed tree, copied")

    accepted(integration_tier(allowed.repo, base=base), describing="a list nothing narrowed")


def test_a_workflow_restricted_to_a_branch_pattern_is_refused(tree: Callable[[], Tree]) -> None:
    """A tier that runs on some branches does not run on every change."""
    broken = tree()
    broken.edit(CI, "on:\n  pull_request:\n", "on:\n  pull_request:\n    branches: [main]\n")

    findings = integration_tier(broken.repo)

    refused(findings, "restricted to branches")


def test_a_workflow_restricted_by_changed_path_is_refused(tree: Callable[[], Tree]) -> None:
    """A change that touched no listed path is still a change."""
    broken = tree()
    broken.edit(
        CI,
        "on:\n  pull_request:\n",
        "on:\n  pull_request:\n    paths: ['crates/**']\n",
    )

    findings = integration_tier(broken.repo)

    refused(findings, "restricted by changed path")


def test_a_workflow_firing_on_a_subset_of_the_change_events_is_refused(
    tree: Callable[[], Tree],
) -> None:
    """Both events the base branch receives, or the tier misses some changes."""
    broken = tree()
    broken.edit(CI, "on:\n  pull_request:\n  push:\n    branches: [main]\n", "on:\n  push:\n")

    findings = integration_tier(broken.repo)

    refused(findings, "does not fire on `pull_request`")


def test_a_job_carrying_a_condition_that_skips_it_is_refused(tree: Callable[[], Tree]) -> None:
    """A condition on the job is a narrowing the triggers do not show."""
    broken = tree()
    _integration_matrix_entry(
        broken,
        "    runs-on: ${{ matrix.platform.runner }}\n",
        "    if: github.event_name == 'push'\n    runs-on: ${{ matrix.platform.runner }}\n",
    )

    findings = integration_tier(broken.repo)

    refused(findings, "which skips it for some changes")


def test_a_conditional_tier_step_is_refused(tree: Callable[[], Tree]) -> None:
    """The one condition this job may carry is on the teardown that must always run."""
    broken = tree()
    broken.edit(
        CI,
        "      - run: just test-integration\n",
        "      - if: github.event_name == 'push'\n        run: just test-integration\n",
    )

    findings = integration_tier(broken.repo)

    refused(findings, "just test-integration` under the condition")
