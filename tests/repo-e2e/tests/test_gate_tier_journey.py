"""The gate's tier parameter, driven through the real recipes and the real Nx.

Each journey commits a change to a copy of the committed tree on a branch off
its `main`, as a contributor would, and runs `just format-check` — the cheapest
graph tier, and one every project declares — at the tier under test. What ran is
read off what Nx itself printed: one `nx run <project>:format-check` per project.
"""

from __future__ import annotations

import json
import re
from collections.abc import Callable

from journey import GateCopy, plain
from repo_checks.expect import contains, equal, failing, passing, truth

#: Every journey here runs Nx in a copy nobody will clean up after, so it runs
#: without the daemon a local Nx would otherwise leave behind per copy.
QUIET = {"NX_DAEMON": "false", "NX_NO_CLOUD": "true"}
RAN = re.compile(r"^> nx run (?P<project>[^:\s]+):format-check", re.MULTILINE)


def as_a_clone(copy: GateCopy) -> GateCopy:
    """The copy with its linked `node_modules` ignored, as a clone's installed one is.

    `.gitignore` names `node_modules/`, which matches the directory an install
    writes and not the symbolic link a copy is handed in its place: left
    untracked, the link is a change no project owns, and every run would fall
    back to the whole graph for a reason no clone has.
    """
    with (copy.root / ".git" / "info" / "exclude").open("a", encoding="utf-8") as exclude:
        exclude.write("node_modules\n")
    return copy


def ran(output: str) -> set[str]:
    """The projects Nx ran `format-check` for, read off what it printed."""
    return {found["project"] for found in RAN.finditer(plain(output))}


def branch_with_a_change(copy: GateCopy, relative: str) -> None:
    """Commit a formatted change to `relative` on a branch off the copy's `main`."""
    copy.git("checkout", "-q", "-b", "change")
    comment = "#" if relative.endswith(".py") else "//"
    copy.write(relative, f"{copy.read(relative)}\n{comment} A change a contributor made.\n")
    copy.git("add", "-A")
    copy.git("commit", "-q", "-m", "feat: a change to one crate")


def gate_eligible(copy: GateCopy) -> set[str]:
    """Every project declaring the target, read off the project definitions."""
    found: set[str] = set()
    for path in copy.root.glob("**/project.json"):
        if "node_modules" in path.parts:
            continue
        data = json.loads(path.read_text(encoding="utf-8"))
        if "format-check" in data.get("targets", {}):
            found.add(data["name"])
    return found


def test_the_default_tier_runs_the_changed_project_and_its_dependents_alone(
    gate_copy: Callable[..., GateCopy],
) -> None:
    """A change to the vision port reaches what is built from it, and stops there."""
    copy = as_a_clone(gate_copy())
    branch_with_a_change(copy, "crates/printobserver-vision-api/src/lib.rs")

    result = copy.just("format-check", environment=QUIET)

    passing(result, describing="`just format-check` over a one-crate change")
    contains(plain(result.stderr), "gate-tier: affected: the projects the change since")
    selected = ran(result.stdout)
    for reached in (
        "printobserver-vision-api",
        "printobserver-core",
        "printobserver-obico",
        "printobserver-server",
        "printobserver",
    ):
        contains(selected, reached, describing="what a change to the vision port runs")
    for unrelated in ("printobserver-octoprint", "printobserver-types", "printobserver-sdk-node"):
        truth(unrelated not in selected, describing=f"{unrelated} left out of {sorted(selected)}")


def test_the_sweep_runs_every_gate_eligible_project(gate_copy: Callable[..., GateCopy]) -> None:
    """`all` is the whole graph whatever the change, and whatever base the environment names."""
    copy = as_a_clone(gate_copy())
    branch_with_a_change(copy, "crates/printobserver-vision-api/src/lib.rs")

    result = copy.just("format-check", "all", environment={**QUIET, "NX_BASE": "$(id)"})

    passing(result, describing="`just format-check all`")
    equal(ran(result.stdout), gate_eligible(copy), describing="what the sweep runs")


def test_a_base_that_is_not_a_ref_or_a_sha_is_refused_before_anything_runs(
    gate_copy: Callable[..., GateCopy],
) -> None:
    """The environment is a boundary: a base carrying shell syntax never reaches git or nx."""
    copy = as_a_clone(gate_copy())

    result = copy.just("format-check", environment={**QUIET, "NX_BASE": "main;touch pwned"})

    failing(result, naming="is neither a plain ref name nor a commit SHA")
    equal(ran(result.stdout), set(), describing="what ran after the refusal")
    truth(not (copy.root / "pwned").exists(), describing="the base's payload never executing")


def test_with_no_base_to_derive_the_whole_graph_runs(gate_copy: Callable[..., GateCopy]) -> None:
    """A history with no `main` to fork from has no change to scope by: it fails closed."""
    copy = as_a_clone(gate_copy())
    copy.git("branch", "-q", "-m", "main", "elsewhere")

    result = copy.just("format-check", environment=QUIET)

    passing(result, describing="`just format-check` with nothing to fork from")
    contains(plain(result.stderr), "no merge base of HEAD with origin/main or main")
    equal(ran(result.stdout), gate_eligible(copy), describing="what a base-less run runs")


def test_a_change_no_project_owns_runs_the_whole_graph(gate_copy: Callable[..., GateCopy]) -> None:
    """The justfile is read by suites the graph cannot name, so every project runs."""
    copy = as_a_clone(gate_copy())
    copy.git("checkout", "-q", "-b", "change")
    copy.write("justfile", copy.read("justfile") + "\n# A change to the command surface.\n")
    copy.git("commit", "-q", "-am", "chore: a change no project owns")

    result = copy.just("format-check", environment=QUIET)

    passing(result, describing="`just format-check` over a justfile change")
    contains(plain(result.stderr), "touches justfile, which no project owns")
    equal(ran(result.stdout), gate_eligible(copy), describing="what a root-file change runs")


def test_the_affected_coverage_report_rules_on_no_code_its_run_did_not_measure(
    gate_copy: Callable[..., GateCopy],
) -> None:
    """A change whose projects carry no measured tests leaves both floors unruled, not failed."""
    copy = as_a_clone(gate_copy())
    branch_with_a_change(copy, "tools/obico-env/obico_env.py")

    result = copy.just("coverage", environment=QUIET)

    passing(result, describing="`just coverage` over a change no measured suite covers")
    contains(
        plain(result.stdout),
        "coverage: rust lines not measured: this run's tests reached no crate (floor 95%), "
        "python lines not measured: this run's tests reached no Python source (floor 95%)",
    )
