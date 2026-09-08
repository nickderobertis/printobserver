"""The recorded merge model names the status contexts the workflows report.

A branch-protection rule names a check by the name GitHub reports it under, not
by the job's key: a matrixed job reports one check run per cell, each carrying
that cell's own name. So every journey here compares the record against the
contexts derived from the committed workflows, and a record naming a job key a
matrix has moved past is refused.
"""

# `assert` is how pytest states an assertion and how it produces the failure
# message a reader acts on; suppressions.toml carries the reason.

from __future__ import annotations

from collections.abc import Callable

from repo_checks.checks_ci import merge_model, status_contexts
from repo_checks.expect import absent, accepted, contains, equal, refused, refused_naming, truth
from repo_checks.model import Repo
from repo_checks.parsing import marker_block
from treecopy import Tree

BLOCK_START = "[//]: # (BEGIN required-checks)"
BLOCK_END = "[//]: # (END required-checks)"
CI = ".github/workflows/ci.yml"
QUALIFIED = "    name: gate (${{ matrix.platform.id }})\n"
# The two contexts the gate reports, which is what the record has to name.
GATE_CELLS = ("gate (linux-x86_64)", "gate (linux-aarch64)")


def _required(repo: Repo) -> list[str]:
    """The checks AGENTS.md records as required, as it spells them."""
    return [line[2:].strip().strip("`") for line in marker_block(repo.agents_md, "required-checks")]


def test_the_committed_record_is_accepted(committed: Repo) -> None:
    """Every required name is a context the committed workflows report."""
    accepted(merge_model(committed))


def test_every_required_name_is_a_context_the_workflows_report(committed: Repo) -> None:
    """A required context nothing reports would block every change forever."""
    reported = [context.name for context in status_contexts(committed)]
    required = _required(committed)

    truth(required, describing="a non-empty record of required checks")
    for name in required:
        contains(reported, name, describing="the contexts the committed workflows report")
        equal(
            reported.count(name),
            1,
            describing=f"the number of check runs reporting under `{name}`",
        )


def test_the_gate_is_required_once_per_platform_it_runs_on(committed: Repo) -> None:
    """Two cells, two distinguishable contexts, and the record names both."""
    required = _required(committed)
    gate = [name for name in required if name.startswith("gate")]

    equal(
        sorted(gate),
        ["gate (linux-aarch64)", "gate (linux-x86_64)"],
        describing="the gate entries of the required record",
    )


def test_the_judged_tier_is_required_once_under_a_name_carrying_no_platform(
    committed: Repo,
) -> None:
    """One roll of the judge, one status context, and the record names exactly it."""
    required = _required(committed)
    judged = [name for name in required if name.startswith("llmlint")]

    equal(judged, ["llmlint"], describing="the judged-lint entries of the required record")
    for platform in ("linux-x86_64", "linux-aarch64", "ubuntu-24.04"):
        absent(judged[0], platform, describing="the judged-lint required check's name")


def test_a_stale_bare_matrix_job_name_is_refused(tree: Callable[[], Tree]) -> None:
    """`gate` is the job's key; the cells report under names carrying the platform."""
    broken = tree()
    _replace_required(broken, ["gate", "llmlint", "pr-title"])

    findings = merge_model(broken.repo)

    refused_naming(findings, "`gate` as a required check", "reports a status context")


def test_a_record_naming_only_one_of_a_jobs_cells_is_refused(
    tree: Callable[[], Tree],
) -> None:
    """A gate required on one platform is a merge path the other never blocked."""
    broken = tree()
    _replace_required(broken, ["gate (linux-x86_64)", "llmlint", "pr-title"])

    findings = merge_model(broken.repo)

    refused_naming(findings, "some but not all", "gate (linux-aarch64)")


def test_a_matrix_job_whose_cells_share_one_name_is_refused(
    tree: Callable[[], Tree],
) -> None:
    """Two check runs under one name are two a rule requiring it cannot tell apart."""
    broken = tree()
    broken.edit(CI, QUALIFIED, "    name: gate\n")
    _replace_required(broken, ["gate", "llmlint", "pr-title"])

    findings = merge_model(broken.repo)

    refused_naming(findings, "2 matrix cells of job `gate`", "which of them was green")


def test_a_name_interpolating_a_field_the_cells_do_not_carry_is_refused(
    tree: Callable[[], Tree],
) -> None:
    """An empty substitution would derive a context nothing reports."""
    broken = tree()
    broken.edit(CI, QUALIFIED, "    name: gate (${{ matrix.platform.arch }})\n")

    findings = merge_model(broken.repo)

    refused_naming(findings, "matrix.platform.arch", "its matrix cells carry `id`, `runner`")


def test_a_name_interpolating_a_field_no_name_can_be_built_from_is_refused(
    tree: Callable[[], Tree],
) -> None:
    """A cell field that is not a scalar cannot name a check run either."""
    broken = tree()
    broken.edit(
        CI,
        "          - id: linux-x86_64\n            runner: ubuntu-24.04\n",
        "          - id: [linux, x86_64]\n            runner: ubuntu-24.04\n",
    )

    findings = merge_model(broken.repo)

    refused(findings, "not something a status context can be named after")


def test_the_derived_contexts_follow_the_names_the_workflow_declares(
    tree: Callable[[], Tree],
) -> None:
    """The contexts are derived from the workflow, not restated beside it."""
    renamed = tree()
    renamed.edit(CI, QUALIFIED, "    name: build-${{ matrix.platform.id }}\n")

    reported = {context.name for context in status_contexts(renamed.repo)}

    contains(reported, "build-linux-aarch64", describing="the derived contexts")
    absent(reported, "gate (linux-aarch64)", describing="the derived contexts")


def _replace_required(tree: Tree, names: list[str]) -> None:
    text = tree.read("AGENTS.md")
    start = text.index(BLOCK_START) + len(BLOCK_START)
    end = text.index(BLOCK_END)
    listing = "".join(f"\n- `{name}`" for name in names)
    tree.write("AGENTS.md", text[:start] + listing + "\n" + text[end:])


def test_a_required_name_with_no_job_behind_it_is_refused(
    tree: Callable[[], Tree],
) -> None:
    """A required context nothing reports blocks every pull request forever."""
    broken = tree()
    _replace_required(broken, [*GATE_CELLS, "llmlint", "pr-title", "smoke"])

    findings = merge_model(broken.repo)

    refused_naming(findings, "`smoke` as a required check", "reports a status context")


def test_a_record_naming_no_required_job_is_refused(tree: Callable[[], Tree]) -> None:
    """A merge path with nothing required on it is not a gate."""
    broken = tree()
    _replace_required(broken, [])

    findings = merge_model(broken.repo)

    refused(findings, "records no required check at all")


def test_a_record_omitting_the_gate_job_is_refused(tree: Callable[[], Tree]) -> None:
    """The complete-gate job is one of the two the gate is made of."""
    broken = tree()
    _replace_required(broken, ["llmlint", "pr-title"])

    findings = merge_model(broken.repo)

    refused(findings, "omit the complete-gate job")


def test_a_record_omitting_the_judged_lint_job_is_refused(
    tree: Callable[[], Tree],
) -> None:
    """So is the judged-lint job."""
    broken = tree()
    _replace_required(broken, [*GATE_CELLS, "pr-title"])

    findings = merge_model(broken.repo)

    refused(findings, "omit the judged-lint job")


def test_the_record_states_that_the_base_branch_takes_no_direct_push(
    committed: Repo,
) -> None:
    """How a change reaches the base branch is written down."""
    contains(committed.agents_md, "takes no direct push")
    contains(committed.agents_md, "squash-merged")
