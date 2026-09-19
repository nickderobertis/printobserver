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
EXCLUSIONS_END = "[//]: # (END platform-exclusions)"

#: The matrix every platform-dependent job of this repository carries.
MATRIX_AARCH64 = "          - id: linux-aarch64\n            runner: ubuntu-24.04-arm\n"


#: A platform the list carries while its bring-up is owed, answered `install path: no`.
BEING_BROUGHT_UP = "macos-aarch64"


def record(copy: Tree, *lines: str) -> None:
    """Add `lines` to the platform-exclusions block of a real tree.

    The entries the committed block already carries stay: they are what lets the
    platforms still being brought up be absent from every matrix, and a copy
    without them would be refused for that before it said anything about `lines`.
    """
    copy.edit(
        "AGENTS.md",
        f"{EXCLUSIONS_BEGIN}\n",
        f"{EXCLUSIONS_BEGIN}\n{''.join(f'{line}\n' for line in lines)}",
    )


def unrecord(copy: Tree, platform: str, job: str) -> None:
    """Take the committed entry for one cell out of a real tree's exclusions block."""
    text = copy.read("AGENTS.md")
    start = text.index(f"- `{platform}` on `{job}` — ")
    end = text.index("\n", start) + 1
    copy.write("AGENTS.md", text[:start] + text[end:])


def answer_yes(copy: Tree, platform: str) -> None:
    """Flip one committed entry of the supported-platform list to `install path: yes`."""
    text = copy.read("AGENTS.md")
    start = text.index(f"- `{platform}` — ")
    end = text.index("\n", start)
    entry = text[start:end]
    copy.write(
        "AGENTS.md",
        text[:start] + entry[: entry.index("install path: no")] + "install path: yes" + text[end:],
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
    refused_naming(findings, "job `artifact-route-script`", "install path: no")


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
    # Anchored on the whole of the npm route job after its matrix, because
    # every artifact job of that workflow carries the same matrix: what this
    # narrows is that one job's own.
    text = broken.read(".github/workflows/artifacts.yml")
    job = text.index("\n  artifact-route-npm:\n")
    start = text.index(MATRIX_AARCH64, job)
    broken.write(
        ".github/workflows/artifacts.yml",
        text[:start] + text[start + len(MATRIX_AARCH64) :],
    )

    findings = platforms(broken.repo)

    refused_naming(findings, "job `artifact-route-npm`", "omits platform `linux-aarch64`")


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

    `Tree.edit` replaces one occurrence, and the first is `artifact-client-rust`'s
    own matrix — so exactly one cell of one job goes, which is what one entry
    records.
    """
    narrowed = tree()
    narrowed.edit(".github/workflows/artifacts.yml", MATRIX_AARCH64, "")
    record(
        narrowed,
        "- `linux-aarch64` on `artifact-client-rust` — the arm runner is being brought up",
    )

    accepted(platforms(narrowed.repo), describing="a tree recording the one cell it omits")


def test_a_job_omitting_a_cell_no_entry_names_is_refused(tree: Callable[[], Tree]) -> None:
    """Recording one job's cell does not excuse another job's."""
    broken = tree()
    broken.edit(".github/workflows/artifacts.yml", MATRIX_AARCH64, "")
    record(broken, "- `linux-aarch64` on `artifact-route-npm` — a different job's cell")

    findings = platforms(broken.repo)

    refused_naming(findings, "job `artifact-client-rust`", "omits platform `linux-aarch64`")


def test_a_job_carrying_a_cell_an_entry_excludes_is_refused(tree: Callable[[], Tree]) -> None:
    """An entry says that cell does not run; a matrix that runs it disagrees with it."""
    broken = tree()
    record(broken, "- `linux-aarch64` on `artifact-route-npm` — the arm runner is coming up")

    findings = platforms(broken.repo)

    refused_naming(
        findings,
        "job `artifact-route-npm`",
        "names platform `linux-aarch64`",
        "a cell that does not run",
    )


def test_an_exclusion_with_no_reason_is_refused(tree: Callable[[], Tree]) -> None:
    """A cell taken out of a matrix says why, exactly as an opt-out does."""
    broken = tree()
    broken.edit(".github/workflows/artifacts.yml", MATRIX_AARCH64, "")
    record(broken, "- `linux-aarch64` on `artifact-client-rust`")

    findings = platforms(broken.repo)

    refused_naming(findings, "`linux-aarch64`", "`artifact-client-rust`", "no reason")


def test_an_exclusion_naming_a_platform_that_is_not_there_is_refused(
    tree: Callable[[], Tree],
) -> None:
    """An exclusion for a platform nothing supports narrows nothing and hides a typo."""
    broken = tree()
    record(broken, "- `linux-riscv64` on `artifact-client-rust` — a platform nothing names")

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
    record(broken, "- linux-aarch64 on artifact-client-rust because the runner is coming up")

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
        "- `linux-aarch64` on `artifact-client-rust` — the arm runner is coming up",
        "- `linux-aarch64` on `artifact-client-rust` — and a second reason for it",
    )

    findings = platforms(broken.repo)

    refused_naming(findings, "`artifact-client-rust`", "more than once")


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
    """The first lever, read by the artifact-job check: `yes` makes every route's cell owed."""
    broken = tree()
    answer_yes(broken, BEING_BROUGHT_UP)

    findings = artifact_jobs(broken.repo)

    refused_naming(findings, "job `artifact-route-npm`", f"AGENTS.md names `{BEING_BROUGHT_UP}`")
    refused_naming(findings, "job `prove-registry-script`", f"AGENTS.md names `{BEING_BROUGHT_UP}`")


def test_a_client_job_omitting_a_cell_no_entry_records_is_refused_by_the_artifact_check(
    tree: Callable[[], Tree],
) -> None:
    """The second lever, read by the artifact-job check: only a recorded cell is excused."""
    broken = tree()
    unrecord(broken, BEING_BROUGHT_UP, "artifact-client-rust")

    findings = artifact_jobs(broken.repo)

    refused_naming(findings, "job `artifact-client-rust`", f"AGENTS.md names `{BEING_BROUGHT_UP}`")
    for job in ("artifact-client-python", "artifact-client-node"):
        accepted(
            [finding for finding in findings if f"job `{job}`" in finding],
            describing=f"`{job}`, whose cell is still recorded",
        )


def test_a_release_build_omitting_a_cell_no_entry_records_is_refused(
    tree: Callable[[], Tree],
) -> None:
    """Release automation builds for every platform the list names but the cells recorded."""
    broken = tree()
    unrecord(broken, BEING_BROUGHT_UP, "artifacts")

    findings = release_automation(broken.repo)

    refused_naming(findings, f"names `{BEING_BROUGHT_UP}`", "no committed job builds")


def test_the_integration_job_omitting_a_cell_no_entry_records_is_refused(
    tree: Callable[[], Tree],
) -> None:
    """The integration job reads this block beside its virtual-printer one."""
    broken = tree()
    unrecord(broken, BEING_BROUGHT_UP, "integration")

    findings = integration_tier(broken.repo)

    refused_naming(
        findings, "the integration job `integration`", f"omits platform `{BEING_BROUGHT_UP}`"
    )


def test_the_integration_job_carrying_a_cell_an_entry_excludes_is_refused(
    tree: Callable[[], Tree],
) -> None:
    """An entry says that cell does not run, for the integration job as for any other."""
    broken = tree()
    text = broken.read(".github/workflows/ci.yml")
    job = text.index("\n  integration:\n")
    at = text.index(MATRIX_AARCH64, job) + len(MATRIX_AARCH64)
    broken.write(
        ".github/workflows/ci.yml",
        f"{text[:at]}          - id: {BEING_BROUGHT_UP}\n            runner: macos-15\n{text[at:]}",
    )

    findings = integration_tier(broken.repo)

    refused_naming(
        findings,
        "the integration job `integration`",
        f"names platform `{BEING_BROUGHT_UP}`",
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
