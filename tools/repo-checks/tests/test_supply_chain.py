"""The Rust supply-chain gate, read off the committed tree.

What `cargo deny` and `cargo machete` refuse is theirs to decide and is proven
by running `just supply-chain`; what this file holds is the wiring a forge runs
and no local run can: that the policy enables every check without an allow-all,
that both tools are held at a release like every other pinned tool, and that it
runs once, on Linux, on every pull request, as steps of the `lint` job rather
than as a job — and so a context — of its own.
"""

from __future__ import annotations

import tomllib
from typing import Any

import pytest
from repo_checks.checks_ci import status_contexts
from repo_checks.expect import contains, equal, truth
from repo_checks.model import Repo, toolchain_tools
from repo_checks.parsing import (
    jobs_of,
    load_workflow,
    marker_block,
    recipes,
    run_commands,
    steps_of,
)
from treecopy import REPO_ROOT

#: The contexts a pull request must always report, read off AGENTS.md's record —
#: which `just check-repo` holds to the workflows — rather than restated here.
FIXED_CONTEXTS = frozenset(
    line[2:].strip("`")
    for line in marker_block(
        (REPO_ROOT / "AGENTS.md").read_text(encoding="utf-8"), "required-checks"
    )
    if line.startswith("- ")
)


def deny_table(name: str) -> dict[str, Any]:
    """One table of the committed `deny.toml`, failing the test where it is not one."""
    policy = tomllib.loads((REPO_ROOT / "deny.toml").read_text(encoding="utf-8"))
    table = policy.get(name)
    if not isinstance(table, dict):
        pytest.fail(f"`deny.toml` configures no `[{name}]` table")
    return table


def test_the_policy_enables_every_check_and_allows_nothing_wholesale() -> None:
    """Advisories, licences, bans and sources each carry a policy, none of them open."""
    allowed = deny_table("licenses")["allow"]
    truth(bool(allowed), describing="a licence allow-list")
    for licence in allowed:
        truth("*" not in licence, describing=f"the allowed licence {licence!r} naming one")
    sources = deny_table("sources")
    equal(sources["unknown-registry"], "deny")
    equal(sources["unknown-git"], "deny")
    equal(sources["allow-git"], [])
    equal(deny_table("advisories")["ignore"], [], describing="the advisories it ignores")
    bans = deny_table("bans")
    equal(bans["wildcards"], "deny")
    truth(bool(bans["deny"]), describing="the crates the ban check refuses")


def test_both_tools_are_held_at_a_release_and_installed_by_the_recipe() -> None:
    """Pinned like every other tool, and not paid for by every bootstrapped host."""
    held = {tool.command: tool for tool in toolchain_tools(Repo(REPO_ROOT))}
    body = recipes((REPO_ROOT / "justfile").read_text(encoding="utf-8"))["supply-chain"].body
    for command in ("cargo-deny", "cargo-machete"):
        contains(set(held), command, describing="the tools `repo-policy.toml` declares")
        truth(held[command].version is not None, describing=f"{command} held at a release")
        truth(not held[command].bootstrap, describing=f"{command} left out of bootstrap")
        contains(body, f"uv run -q python -m repo_checks install-tools {command}")
    truth(
        any(line.startswith("cargo deny") and " check" in line for line in body),
        describing=f"the recipe running every `cargo deny` check: {body}",
    )
    contains(body, "cargo machete")


def test_it_runs_once_on_linux_on_every_pull_request_as_steps_of_the_lint_job() -> None:
    """Steps of `lint`, last, one cell, unconditional, gating nothing, and no job of its own.

    The lockfile's graph is the same on every platform, so the supply chain runs
    in the one job that runs once: `lint`, which every pull request runs. It is
    no job of its own, so no context names it and no job's `needs` can wait on
    it, and no other job runs it a second time.
    """
    workflow = load_workflow(REPO_ROOT / ".github" / "workflows" / "ci.yml")
    jobs = jobs_of(workflow)
    running = [name for name, job in jobs.items() if "just supply-chain" in run_commands(job)]
    equal(running, ["lint"], describing="the ci.yml jobs running `just supply-chain`")

    job = jobs["lint"]
    truth(str(job["runs-on"]).startswith("ubuntu-"), describing=f"runs-on {job['runs-on']!r}")
    equal(job.get("strategy"), None, describing="the lint job's matrix")
    equal(
        job.get("if"),
        "github.event_name == 'pull_request'",
        describing="the lint job's condition",
    )
    triggers = workflow.get("on", workflow.get(True))
    truth(isinstance(triggers, dict) and "pull_request" in triggers, describing="ci.yml's events")
    commands = run_commands(job)
    truth(
        commands.index("just bootstrap") < commands.index("just supply-chain"),
        describing="the supply chain running in the bootstrapped environment",
    )
    truth(
        commands.index("just lint-llm-diff") < commands.index("just supply-chain"),
        describing="the supply chain after the judged lint, hiding no verdict before it",
    )
    (step,) = [one for one in steps_of(job) if one.get("run") == "just supply-chain"]
    equal(step.get("if"), None, describing="the supply-chain step's condition")
    equal(
        step.get("continue-on-error"),
        True,
        describing="the supply-chain step reporting and gating nothing, as its job did before",
    )

    reported = {context.name for context in status_contexts(Repo(REPO_ROOT))}
    truth(
        not any("supply-chain" in name for name in reported | FIXED_CONTEXTS),
        describing="no status context named for the supply chain",
    )
    truth("supply-chain" not in jobs, describing="no `supply-chain` job for another to need")
