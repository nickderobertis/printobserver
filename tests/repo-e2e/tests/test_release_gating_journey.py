"""The artifacts follow a release that was cut, and only one that was cut.

`release-plz release` exits zero having released nothing — every ordinary push,
under `release_always` — and the Python and JavaScript registries refuse a
version they already serve. So the two jobs after it are gated on what it
ANSWERED rather than on its exit status: the workflow runs it with `--output
json`, `just release-cut` reads that answer into a job output, and the artifact
build and the artifact publish run on that output being non-empty.

These journeys read the committed workflow for which job publishes, which
recipe reads its answer and what the artifact jobs are gated on, and then drive
that recipe for both answers — a release having been cut and none having been
cut — and for an answer it must refuse. The check that holds the workflow to
all of it is driven over a copy carrying the defect each rule refuses.
"""

# `assert` is how pytest states an assertion and how it produces the failure
# message a reader acts on; suppressions.toml carries the reason.

from __future__ import annotations

import json
from collections.abc import Callable
from pathlib import Path
from typing import Any

import yaml
from journey import REPO_ROOT, GateCopy, capture, clean_environment, output, pythonpath
from repo_checks.checks_release import DRAFTING, PUBLISHING
from repo_checks.expect import absent, contains, equal, failing, passing, truth
from repo_checks.model import Repo

WORKFLOW = ".github/workflows/release-plz.yml"
RECIPE_TIMEOUT_SECONDS = 300

#: What `repo-policy.toml` declares about the gating: the recipe that reads the
#: release program's answer, the output it publishes it under, and the two
#: recipes gated on it. Read rather than restated, because these are what
#: `just check-repo` holds the workflow to.
DECLARED = Repo(REPO_ROOT).policy["release"]


def _release(package: str, version: str) -> dict[str, object]:
    """One package as `release-plz release --output json` reports having released it."""
    return {"package_name": package, "prs": [], "tag": f"v{version}", "version": version}


def jobs() -> dict[str, dict[str, Any]]:
    """The committed release workflow's jobs."""
    workflow = yaml.safe_load((REPO_ROOT / WORKFLOW).read_text(encoding="utf-8"))
    return dict(workflow["jobs"])


def job_running(command: str) -> str:
    """The one committed job with a step running `command`, whole or as its first words."""
    running = [
        name
        for name, job in jobs().items()
        for step in job["steps"]
        if str(step.get("run", "")).strip() == command
        or str(step.get("run", "")).strip().startswith(f"{command} ")
    ]
    equal(len(running), 1, describing=f"the jobs running `{command}`: {running}")
    return running[0]


def needs(job: dict[str, Any]) -> list[str]:
    """The jobs a job waits on, however the workflow spells them."""
    declared = job.get("needs", [])
    return [declared] if isinstance(declared, str) else list(declared)


def cut(answer: str, into: Path) -> tuple[int, str]:
    """Drive the recipe the publishing job reads its answer with, over `answer`."""
    written = into / "released.json"
    written.write_text(answer, encoding="utf-8")
    result = capture(
        ["just", str(DECLARED["cut_recipe"]), str(written)],
        REPO_ROOT,
        timeout=RECIPE_TIMEOUT_SECONDS,
        env=clean_environment(PYTHONPATH=pythonpath()),
    )
    return result.returncode, output(result)


def test_publishing_waits_on_no_drafting_and_the_artifacts_wait_on_its_answer() -> None:
    """The committed workflow's shape, read off the commands its jobs run.

    The drafting job computes each package's difference against the registry
    and can die doing it, so the publishing job does not wait on it. And both
    artifact jobs wait on the publishing job and are gated on the output it
    publishes its answer under — not on its result, which is success whether
    or not it released anything.
    """
    publishing = job_running(PUBLISHING)
    drafting = job_running(DRAFTING)
    field = str(DECLARED["cut_output"])
    declared = jobs()

    absent(needs(declared[publishing]), drafting, describing=f"what `{publishing}` waits on")
    published = " ".join(str(declared[publishing]["outputs"][field]).split())
    contains(published, f".outputs.{field}", describing=f"where `{publishing}` reads `{field}`")

    gate = f"needs.{publishing}.outputs.{field} != ''"
    for recipe in (DECLARED["build_recipe"], DECLARED["publish_recipe"]):
        name = job_running(f"just {recipe}")
        contains(needs(declared[name]), publishing, describing=f"what `{name}` waits on")
        condition = " ".join(str(declared[name].get("if", "")).split())
        contains(condition, gate, describing=f"what `{name}` is gated on")
        absent(condition, ".result", describing=f"`{name}` not to be gated on a result")


def test_a_run_that_cut_a_release_answers_a_field_the_artifact_jobs_run_on(
    tmp_path: Path,
) -> None:
    """Thirteen crates under one version name one tag, answered once and non-empty."""
    answer = json.dumps(
        {"releases": [_release("printobserver-types", "0.4.0"), _release("printobserver", "0.4.0")]}
    )

    code, said = cut(answer, tmp_path)

    passing((code, said), describing=f"`just {DECLARED['cut_recipe']}` over a cut release")
    equal(said.strip(), f"{DECLARED['cut_output']}=v0.4.0", describing="the one line a job reads")


def test_a_run_that_cut_nothing_answers_the_empty_field_the_artifact_jobs_skip_on(
    tmp_path: Path,
) -> None:
    """Every ordinary push finishes a release run that released nothing, exiting zero."""
    code, said = cut(json.dumps({"releases": []}), tmp_path)

    passing((code, said), describing=f"`just {DECLARED['cut_recipe']}` over no release")
    equal(
        said.strip(),
        f"{DECLARED['cut_output']}=",
        describing="the empty field such a run publishes",
    )


def test_an_answer_the_release_program_does_not_write_fails_the_job_rather_than_skipping(
    tmp_path: Path,
) -> None:
    """Read as "released nothing", an unreadable answer would skip the publish of a cut release."""
    code, said = cut("release-plz wrote something else here", tmp_path)

    failing((code, said), naming="not JSON")
    truth(
        not any(line.startswith(f"{DECLARED['cut_output']}=") for line in said.splitlines()),
        describing=f"no field for a job to read off a refused answer: {said!r}",
    )


def test_a_publishing_job_that_waits_on_the_drafting_job_is_refused_by_the_gate(
    gate_copy: Callable[[], GateCopy],
) -> None:
    """The state this repair undid — publishing chained behind drafting — cannot come back."""
    publishing = job_running(PUBLISHING)
    drafting = job_running(DRAFTING)
    broken = gate_copy()
    broken.edit(
        WORKFLOW,
        f"  {publishing}:\n    name: {publishing}\n",
        f"  {publishing}:\n    name: {publishing}\n    needs: {drafting}\n",
    )

    result = broken.just("check-repo")

    failing(result, naming=f"waits on `{drafting}`, which drafts the next one")


def test_an_artifact_build_that_no_longer_reads_the_answer_is_refused_by_the_gate(
    gate_copy: Callable[[], GateCopy],
) -> None:
    """A build chained behind the release unconditionally runs on every push."""
    publishing = job_running(PUBLISHING)
    field = str(DECLARED["cut_output"])
    broken = gate_copy()
    broken.edit(
        WORKFLOW,
        f"    if: needs.{publishing}.outputs.{field} != ''\n    strategy:\n",
        "    strategy:\n",
    )

    result = broken.just("check-repo")

    failing(result, naming=f"runs `just {DECLARED['build_recipe']}` and is not gated on")
