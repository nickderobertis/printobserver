"""The two levers a platform is brought up in stages by, driven against real trees.

Growing the supported-platform list fans out instantly to every matrix derived
from it, which on its own would make adding a platform one impossible change.
The `install path` answer and the `platform-exclusions` block are what turn it
into a set of reviewable ones — and neither is allowed to be pulled quietly, so
every refusal below is driven by building a copy of this tree carrying exactly
one of the defects and reading what the committed check says about it.
"""

from __future__ import annotations

from collections.abc import Callable

from repo_checks.checks_ci import artifact_jobs, platforms
from repo_checks.checks_integration import integration_tier
from repo_checks.checks_release import release_automation
from repo_checks.expect import accepted, refused, refused_naming
from repo_checks.model import Repo
from treecopy import Tree

AARCH64 = (
    "- `linux-aarch64` — runner `ubuntu-24.04-arm`, Rust target "
    "`aarch64-unknown-linux-gnu`, service manager `systemd`, install path: yes"
)
EXCLUSIONS_BEGIN = "[//]: # (BEGIN platform-exclusions)"
INSTALL = ".github/workflows/install-path.yml"
EXCLUSIONS_END = "[//]: # (END platform-exclusions)"

#: The matrix every platform-dependent job of this repository carries.
MATRIX_AARCH64 = "          - id: linux-aarch64\n            runner: ubuntu-24.04-arm\n"

#: The macOS cell that follows it in every one of those matrices.
MATRIX_MACOS = "          - id: macos-aarch64\n            runner: macos-15\n"

#: A platform every matrix carries, whose cells the tests below take out of one
#: job at a time to drive the two levers: the last one brought up, so that a
#: copy narrowing it is the tree as it stood one step before this one.
LAST_BROUGHT_UP = "windows-x86_64"

#: The runner the supported-platform list declares for it.
LAST_BROUGHT_UP_RUNNER = "windows-2025"


#: The runner each platform's cell names, as the supported-platform list declares it.
RUNNERS = {
    "linux-aarch64": "ubuntu-24.04-arm",
    "macos-aarch64": "macos-15",
    LAST_BROUGHT_UP: LAST_BROUGHT_UP_RUNNER,
    "windows-aarch64": "windows-11-arm",
}


def matrix_cell(platform: str) -> str:
    """One cell of the matrix every platform-dependent job carries."""
    return f"          - id: {platform}\n            runner: {RUNNERS[platform]}\n"


def record(copy: Tree, *lines: str) -> None:
    """Add `lines` to the platform-exclusions block of a real tree."""
    copy.edit(
        "AGENTS.md",
        f"{EXCLUSIONS_BEGIN}\n",
        f"{EXCLUSIONS_BEGIN}\n{''.join(f'{line}\n' for line in lines)}",
    )


def narrow(copy: Tree, workflow: str, job: str, platform: str = LAST_BROUGHT_UP) -> None:
    """Take one platform's cell out of one job's matrix in a real tree.

    Anchored on the job, because every platform-dependent job of a workflow
    carries the same matrix: what this narrows is that one job's own.
    """
    text = copy.read(workflow)
    at = text.index(matrix_cell(platform), text.index(f"\n  {job}:\n"))
    copy.write(workflow, text[:at] + text[at + len(matrix_cell(platform)) :])


def answer_no(copy: Tree, platform: str, reason: str) -> None:
    """Flip one committed entry of the supported-platform list to `install path: no`."""
    text = copy.read("AGENTS.md")
    start = text.index(f"- `{platform}` — ")
    end = text.index("\n", start)
    entry = text[start:end]
    copy.write(
        "AGENTS.md",
        text[:start]
        + entry[: entry.index("install path: yes")]
        + f"install path: no — {reason}"
        + text[end:],
    )


def test_the_committed_tree_carries_every_cell_it_owes(committed: Repo) -> None:
    """Both platforms answer `yes` and every job derived from the list carries both."""
    accepted(platforms(committed))


def test_an_install_route_job_carrying_a_platform_answered_no_is_refused(
    tree: Callable[[], Tree],
) -> None:
    """`install path: no` takes a platform out of every install tier, matrices included."""
    broken = tree()
    broken.edit(
        "AGENTS.md",
        AARCH64,
        AARCH64.replace("install path: yes", "install path: no — no runner has been proven yet"),
    )

    findings = platforms(broken.repo)

    refused_naming(
        findings,
        "job `install-route-pypi`",
        "names platform `linux-aarch64`",
        "install path: no",
    )
    refused_naming(findings, "job `prove-registry-npm`", "install path: no")
    refused_naming(findings, "job `prove-registry-script`", "install path: no")


def test_a_job_that_does_not_take_a_route_still_carries_a_platform_answered_no(
    tree: Callable[[], Tree],
) -> None:
    """The lever reaches the install tiers and nothing else: the gate still owes both cells."""
    broken = tree()
    broken.edit(
        "AGENTS.md",
        AARCH64,
        AARCH64.replace("install path: yes", "install path: no — no runner has been proven yet"),
    )
    broken.edit(".github/workflows/ci.yml", MATRIX_AARCH64, "")

    findings = platforms(broken.repo)

    refused_naming(findings, "job `gate`", "omits platform `linux-aarch64`")


def test_an_install_route_job_omitting_a_platform_answered_yes_is_refused(
    tree: Callable[[], Tree],
) -> None:
    """The answer binds in both directions: a `yes` every install tier must carry."""
    broken = tree()
    narrow(broken, INSTALL, "prove-registry-npm", "linux-aarch64")

    findings = platforms(broken.repo)

    refused_naming(findings, "job `prove-registry-npm`", "omits platform `linux-aarch64`")


def test_an_install_path_answer_of_no_with_no_reason_is_refused(
    tree: Callable[[], Tree],
) -> None:
    """A platform cannot be taken out of every install tier by an unexplained opt-out."""
    broken = tree()
    broken.edit("AGENTS.md", AARCH64, AARCH64.replace("install path: yes", "install path: no"))

    findings = platforms(broken.repo)

    refused_naming(findings, "`linux-aarch64`", "states no reason")


def test_a_recorded_exclusion_lets_a_job_omit_that_cell(tree: Callable[[], Tree]) -> None:
    """The second lever: a cell recorded as not running is a cell the job no longer owes.

    `Tree.edit` replaces one occurrence, and the first in the install-path
    workflow is `prove-registry-pypi`'s own matrix — so exactly one cell of one
    job goes, which is what one entry records.
    """
    narrowed = tree()
    narrowed.edit(INSTALL, MATRIX_AARCH64, "")
    record(
        narrowed,
        "- `linux-aarch64` on `prove-registry-pypi` — the arm runner is being brought up",
    )

    accepted(platforms(narrowed.repo), describing="a tree recording the one cell it omits")


def test_a_job_omitting_a_cell_no_entry_names_is_refused(tree: Callable[[], Tree]) -> None:
    """Recording one job's cell does not excuse another job's."""
    broken = tree()
    broken.edit(INSTALL, MATRIX_AARCH64, "")
    record(broken, "- `linux-aarch64` on `prove-registry-npm` — a different job's cell")

    findings = platforms(broken.repo)

    refused_naming(findings, "job `prove-registry-pypi`", "omits platform `linux-aarch64`")


def test_a_job_carrying_a_cell_an_entry_excludes_is_refused(tree: Callable[[], Tree]) -> None:
    """An entry says that cell does not run; a matrix that runs it disagrees with it."""
    broken = tree()
    record(broken, "- `linux-aarch64` on `prove-registry-npm` — the arm runner is coming up")

    findings = platforms(broken.repo)

    refused_naming(
        findings,
        "job `prove-registry-npm`",
        "names platform `linux-aarch64`",
        "a cell that does not run",
    )


def test_an_exclusion_with_no_reason_is_refused(tree: Callable[[], Tree]) -> None:
    """A cell taken out of a matrix says why, exactly as an opt-out does."""
    broken = tree()
    broken.edit(INSTALL, MATRIX_AARCH64, "")
    record(broken, "- `linux-aarch64` on `prove-registry-pypi`")

    findings = platforms(broken.repo)

    refused_naming(findings, "`linux-aarch64`", "`prove-registry-pypi`", "no reason")


def test_an_exclusion_naming_a_platform_that_is_not_there_is_refused(
    tree: Callable[[], Tree],
) -> None:
    """An exclusion for a platform nothing supports narrows nothing and hides a typo."""
    broken = tree()
    record(broken, "- `linux-riscv64` on `prove-registry-client-rust` — a platform nothing names")

    findings = platforms(broken.repo)

    refused_naming(findings, "`linux-riscv64`", "does not name `linux-riscv64`")


def test_an_exclusion_naming_a_job_that_is_not_there_is_refused(
    tree: Callable[[], Tree],
) -> None:
    """A job renamed under an exclusion leaves a cell nothing runs and nothing records."""
    broken = tree()
    record(broken, "- `linux-aarch64` on `artifact-route-brew` — a job nothing declares")

    findings = platforms(broken.repo)

    refused_naming(findings, "`artifact-route-brew`", "declare no job")


def test_an_exclusion_that_is_not_of_the_recorded_shape_is_refused(
    tree: Callable[[], Tree],
) -> None:
    """A line that starts like an entry and is not one is refused rather than ignored."""
    broken = tree()
    record(broken, "- linux-aarch64 on prove-registry-client-rust because the runner is coming up")

    findings = platforms(broken.repo)

    refused(findings, "which is not of the form")


def test_a_list_entry_that_is_not_of_the_recorded_shape_is_refused(
    tree: Callable[[], Tree],
) -> None:
    """A line that begins like an entry and is not one is a platform silently unsupported."""
    broken = tree()
    broken.edit("AGENTS.md", AARCH64, "- `linux-aarch64` — runner `ubuntu-24.04-arm`")

    findings = platforms(broken.repo)

    refused_naming(findings, "`linux-aarch64`", "which is not of the form")


def test_an_entry_naming_a_service_manager_nothing_here_has_a_name_for_is_refused(
    tree: Callable[[], Tree],
) -> None:
    """The column is a closed vocabulary: a typo in it is a pair of commands nobody states."""
    broken = tree()
    broken.edit(
        "AGENTS.md", AARCH64, AARCH64.replace("service manager `systemd`", "service manager `sysv`")
    )

    findings = platforms(broken.repo)

    refused_naming(findings, "`linux-aarch64`", "`sysv`", "none this repository has a name for")


def test_a_platform_listed_twice_is_refused(tree: Callable[[], Tree]) -> None:
    """A mapping built from the list would take the second and drop the first silently."""
    broken = tree()
    broken.edit(
        "AGENTS.md",
        AARCH64,
        AARCH64 + "\n" + AARCH64.replace("install path: yes", "install path: no — a second answer"),
    )

    findings = platforms(broken.repo)

    refused_naming(findings, "`linux-aarch64`", "more than once")


def test_a_cell_recorded_as_not_running_twice_is_refused(tree: Callable[[], Tree]) -> None:
    """Two reasons for one cell are two a reader takes one of."""
    broken = tree()
    record(
        broken,
        "- `linux-aarch64` on `prove-registry-client-rust` — the arm runner is coming up",
        "- `linux-aarch64` on `prove-registry-client-rust` — and a second reason for it",
    )

    findings = platforms(broken.repo)

    refused_naming(findings, "`prove-registry-client-rust`", "more than once")


def test_a_matrix_naming_one_platform_in_two_cells_is_refused(
    tree: Callable[[], Tree],
) -> None:
    """The second is a check run repeating the first under the same name."""
    broken = tree()
    broken.edit(".github/workflows/ci.yml", MATRIX_AARCH64, MATRIX_AARCH64 + MATRIX_AARCH64)

    findings = platforms(broken.repo)

    refused_naming(findings, "job `gate`", "`linux-aarch64` in more than one cell")


def test_an_entry_whose_identifier_is_not_shaped_like_one_is_refused(
    tree: Callable[[], Tree],
) -> None:
    """The line's grammar admits any word; an identifier is a family and a processor."""
    broken = tree()
    broken.edit("AGENTS.md", AARCH64, AARCH64.replace("`linux-aarch64`", "`arm`"))

    findings = platforms(broken.repo)

    refused_naming(findings, "`arm`", "not shaped like a platform identifier")


def test_the_committed_tree_owes_no_cell_of_a_platform_still_being_brought_up(
    committed: Repo,
) -> None:
    """Every check a platform's cells are derived by reads both levers, not only `platforms`."""
    accepted(artifact_jobs(committed), describing="the artifact jobs")
    accepted(release_automation(committed), describing="release automation")
    accepted(integration_tier(committed), describing="the integration tier")


def test_a_route_job_owes_a_platform_once_the_install_path_targets_it(
    tree: Callable[[], Tree],
) -> None:
    """The first lever, read by the artifact-job check: `yes` makes every route's cell owed.

    One tree, two answers. With the platform answered `no`, the route jobs
    that omit its cell owe nothing for it — the lever is what let the routes
    be brought up after the gate; answered `yes`, the same omissions are what
    the answer now demands of the jobs, and a cell the block records stays
    excused, which is the second lever.
    """
    narrowed = tree()
    for job in ("prove-registry-pypi", "prove-registry-npm", "prove-registry-script"):
        narrow(narrowed, INSTALL, job)
    record(narrowed, f"- `{LAST_BROUGHT_UP}` on `prove-registry-pypi` — still being brought up")
    answer_no(narrowed, LAST_BROUGHT_UP, "the routes are still being brought up")
    accepted(
        [finding for finding in artifact_jobs(narrowed.repo) if LAST_BROUGHT_UP in finding],
        describing="the route jobs, while the install path does not target the platform",
    )

    text = narrowed.read("AGENTS.md")
    narrowed.write(
        "AGENTS.md",
        text.replace(
            "install path: no — the routes are still being brought up", "install path: yes"
        ),
    )

    findings = artifact_jobs(narrowed.repo)

    refused_naming(findings, "job `prove-registry-npm`", f"AGENTS.md names `{LAST_BROUGHT_UP}`")
    refused_naming(findings, "job `prove-registry-script`", f"AGENTS.md names `{LAST_BROUGHT_UP}`")
    accepted(
        [finding for finding in findings if "job `prove-registry-pypi`" in finding],
        describing="`prove-registry-pypi`, whose cell is recorded",
    )


def test_a_client_job_omitting_a_cell_no_entry_records_is_refused_by_the_artifact_check(
    tree: Callable[[], Tree],
) -> None:
    """The second lever, read by the artifact-job check: only a recorded cell is excused."""
    broken = tree()
    narrow(broken, INSTALL, "prove-registry-client-rust")
    narrow(broken, INSTALL, "prove-registry-client-python")
    record(broken, f"- `{LAST_BROUGHT_UP}` on `prove-registry-client-python` — coming up")

    findings = artifact_jobs(broken.repo)

    refused_naming(
        findings, "job `prove-registry-client-rust`", f"AGENTS.md names `{LAST_BROUGHT_UP}`"
    )
    for job in ("prove-registry-client-python", "prove-registry-client-node"):
        accepted(
            [finding for finding in findings if f"job `{job}`" in finding],
            describing=f"`{job}`, whose cell is recorded or carried",
        )


def test_a_release_build_omitting_a_cell_no_entry_records_is_refused(
    tree: Callable[[], Tree],
) -> None:
    """Release automation builds for every platform the list names but the cells recorded."""
    broken = tree()
    narrow(broken, ".github/workflows/release-plz.yml", "artifacts")

    findings = release_automation(broken.repo)

    refused_naming(findings, f"names `{LAST_BROUGHT_UP}`", "no committed job builds")


def test_a_release_build_omitting_a_recorded_cell_is_excused(tree: Callable[[], Tree]) -> None:
    """The second lever, read by release automation's check: a recorded cell is not owed."""
    narrowed = tree()
    narrow(narrowed, ".github/workflows/release-plz.yml", "artifacts")
    record(narrowed, f"- `{LAST_BROUGHT_UP}` on `artifacts` — its toolchain is being brought up")

    accepted(
        [finding for finding in release_automation(narrowed.repo) if LAST_BROUGHT_UP in finding],
        describing="a release build omitting the one cell the block records",
    )


def test_the_integration_job_omitting_a_cell_no_entry_records_is_refused(
    tree: Callable[[], Tree],
) -> None:
    """The integration job reads this block beside its virtual-printer one.

    The committed job carries the cell and no entry records it, so the cell is
    taken out of the job's own matrix — the second occurrence of it in the
    workflow, the gate's being the first — and nothing excuses the omission.
    """
    broken = tree()
    narrow(broken, ".github/workflows/ci.yml", "integration")

    findings = integration_tier(broken.repo)

    refused_naming(
        findings, "the integration job `integration`", f"omits platform `{LAST_BROUGHT_UP}`"
    )


def test_the_integration_job_carrying_a_cell_an_entry_excludes_is_refused(
    tree: Callable[[], Tree],
) -> None:
    """An entry says that cell does not run, for the integration job as for any other.

    The committed job carries the cell, so recording it as one that does not
    run is what makes the matrix wrong.
    """
    broken = tree()
    record(
        broken,
        f"- `{LAST_BROUGHT_UP}` on `integration` — the virtual printer is being brought up",
    )

    findings = integration_tier(broken.repo)

    refused_naming(
        findings,
        "the integration job `integration`",
        f"names platform `{LAST_BROUGHT_UP}`",
        "a cell that does not run",
    )


def test_a_release_build_whose_strategy_is_not_a_mapping_is_refused_rather_than_crashing(
    tree: Callable[[], Tree],
) -> None:
    """A malformed matrix builds for nothing, which is a finding and not a traceback."""
    broken = tree()
    text = broken.read(".github/workflows/release-plz.yml")
    job = text.index("\n  artifacts:\n")
    start = text.index("    strategy:\n", job)
    end = text.index("    runs-on:", start)
    broken.write(
        ".github/workflows/release-plz.yml",
        f"{text[:start]}    strategy: fail-fast\n{text[end:]}",
    )

    findings = release_automation(broken.repo)

    refused_naming(findings, "names `linux-x86_64`", "no committed job builds")
