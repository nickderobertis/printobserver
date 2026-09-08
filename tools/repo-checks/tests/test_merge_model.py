"""The recorded merge model names jobs the configuration actually declares."""

# `assert` is how pytest states an assertion and how it produces the failure
# message a reader acts on; suppressions.toml carries the reason.

from __future__ import annotations

from collections.abc import Callable

from repo_checks.checks_ci import merge_model
from repo_checks.expect import absent, accepted, contains, equal, refused, truth
from repo_checks.model import Repo
from repo_checks.parsing import jobs_of, load_workflow, marker_block
from treecopy import Tree

BLOCK_START = "[//]: # (BEGIN required-checks)"
BLOCK_END = "[//]: # (END required-checks)"


def _required(repo: Repo) -> list[str]:
    """The checks AGENTS.md records as required, as it spells them."""
    return [line[2:].strip().strip("`") for line in marker_block(repo.agents_md, "required-checks")]


def _declared_jobs(repo: Repo) -> dict[str, dict[str, object]]:
    """Every job every committed workflow declares, keyed by the name it reports under."""
    found: dict[str, dict[str, object]] = {}
    for path in repo.workflow_paths:
        found.update(jobs_of(load_workflow(path)))
    return found


def test_the_committed_record_is_accepted(committed: Repo) -> None:
    """Every required name has a job behind it."""
    accepted(merge_model(committed))


def test_every_required_name_is_a_job_the_workflows_declare(committed: Repo) -> None:
    """A required context nothing reports would block every change forever."""
    declared = _declared_jobs(committed)
    required = _required(committed)

    truth(required, describing="a non-empty record of required checks")
    for name in required:
        contains(declared, name, describing="the jobs the committed workflows declare")


def test_the_judged_tier_is_required_once_under_a_name_carrying_no_platform(
    committed: Repo,
) -> None:
    """One roll of the judge, one status context, and the record names exactly it."""
    required = _required(committed)
    judged = [name for name in required if name.startswith("llmlint")]

    equal(judged, ["llmlint"], describing="the judged-lint entries of the required record")
    for platform in ("linux-x86_64", "linux-aarch64", "ubuntu-24.04"):
        absent(judged[0], platform, describing="the judged-lint required check's name")
    # A job with a platform matrix reports one context per cell, each suffixed
    # with that cell, so the bare name would be a context nothing reports.
    absent(
        _declared_jobs(committed)["llmlint"],
        "strategy",
        describing="the judged-lint job",
    )


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
    _replace_required(broken, ["gate", "llmlint", "pr-title", "smoke"])

    findings = merge_model(broken.repo)

    refused(findings, "no committed workflow declares a job")


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
    _replace_required(broken, ["gate", "pr-title"])

    findings = merge_model(broken.repo)

    refused(findings, "omit the judged-lint job")


def test_the_record_states_that_the_base_branch_takes_no_direct_push(
    committed: Repo,
) -> None:
    """How a change reaches the base branch is written down."""
    contains(committed.agents_md, "takes no direct push")
    contains(committed.agents_md, "squash-merged")
