"""The three jobs that carry no platform matrix each record why they carry none.

A job running once per change rather than once per platform is a decision, and
one a reader of a workflow cannot tell from an omission. So it is recorded in
`AGENTS.md` beside the platform list, and this holds that record to the
committed workflows in both directions.
"""

from __future__ import annotations

from collections.abc import Callable

from repo_checks.checks_platforms import unmatrixed_jobs
from repo_checks.expect import accepted, contains, refused, refused_naming
from repo_checks.model import Repo
from treecopy import Tree

BEGIN = "[//]: # (BEGIN unmatrixed-jobs)"
END = "[//]: # (END unmatrixed-jobs)"

#: The matrix every platform-dependent job of this repository carries, which is
#: what a fixture gives to a job recorded as carrying none.
MATRIX = """    strategy:
      fail-fast: false
      matrix:
        platform:
          - id: linux-x86_64
            runner: ubuntu-24.04
          - id: linux-aarch64
            runner: ubuntu-24.04-arm
"""


def record(copy: Tree, *lines: str) -> None:
    """Put exactly `lines` into the unmatrixed-jobs block of a real tree."""
    text = copy.read("AGENTS.md")
    start, end = text.index(BEGIN), text.index(END)
    body = "".join(f"{line}\n" for line in lines)
    copy.write("AGENTS.md", f"{text[:start]}{BEGIN}\n{body}{text[end:]}")


def test_the_committed_record_names_the_three_jobs_and_their_reasons(
    committed: Repo,
) -> None:
    """The judged tier, the title lint and the scheduled Obico tier, each with its reason."""
    accepted(unmatrixed_jobs(committed))

    recorded = committed.agents_md[
        committed.agents_md.index(BEGIN) : committed.agents_md.index(END)
    ]
    for job in ("`llmlint`", "`pr-title`", "`obico`"):
        contains(recorded, job, describing="the recorded unmatrixed jobs")
    contains(recorded, "not a printer-host tier", describing="the Obico tier's own reason")
    contains(recorded, "Linux container", describing="the Obico tier's own reason")


def test_an_entry_with_no_reason_is_refused(tree: Callable[[], Tree]) -> None:
    """A job listed and unexplained is the omission this record exists to rule out."""
    broken = tree()
    record(broken, "- `llmlint`", "- `pr-title` — a title is one string", "- `obico` — a stack")

    findings = unmatrixed_jobs(broken.repo)

    refused_naming(findings, "`llmlint`", "no reason it carries none")


def test_an_entry_naming_a_job_nothing_declares_is_refused(tree: Callable[[], Tree]) -> None:
    """A job renamed under its own record leaves a reason for something that is not there."""
    broken = tree()
    record(broken, "- `llm-lint` — a job this repository does not declare")

    findings = unmatrixed_jobs(broken.repo)

    refused_naming(findings, "`llm-lint`", "declare no job by that name")


def test_an_entry_for_a_job_that_carries_a_matrix_after_all_is_refused(
    tree: Callable[[], Tree],
) -> None:
    """The record says it carries none; a job that carries one disagrees with it."""
    broken = tree()
    broken.edit(
        ".github/workflows/ci.yml",
        "  pr-title:\n    name: pr-title\n",
        "  pr-title:\n    name: pr-title (${{ matrix.platform.id }})\n" + MATRIX,
    )

    findings = unmatrixed_jobs(broken.repo)

    refused_naming(findings, "`pr-title`", "declare one on it")


def test_a_line_that_is_not_of_the_recorded_shape_is_refused(tree: Callable[[], Tree]) -> None:
    """A line that starts like an entry and is not one is refused rather than skipped."""
    broken = tree()
    record(broken, "- llmlint reads one text diff")

    findings = unmatrixed_jobs(broken.repo)

    refused(findings, "which is not of the form")


def test_a_required_check_with_no_matrix_and_no_reason_is_refused(
    tree: Callable[[], Tree],
) -> None:
    """The record is derived from what a merge waits on, so it cannot go stale against it."""
    broken = tree()
    record(
        broken,
        "- `pr-title` — a title is one string",
        "- `obico` — an external producer's payload, over HTTP",
    )

    findings = unmatrixed_jobs(broken.repo)

    refused_naming(findings, "`llmlint`", "a check a merge of this repository waits on")


def test_the_scheduled_obico_tiers_job_with_no_reason_is_refused(
    tree: Callable[[], Tree],
) -> None:
    """It is the other job a reader expects a matrix on, and it owes the reason it has none."""
    broken = tree()
    record(
        broken,
        "- `llmlint` — one text diff, one non-deterministic verdict",
        "- `pr-title` — a title is one string",
    )

    findings = unmatrixed_jobs(broken.repo)

    refused_naming(findings, "`obico`", "the scheduled Obico tier")
