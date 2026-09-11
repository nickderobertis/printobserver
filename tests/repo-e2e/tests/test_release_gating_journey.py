"""The artifacts follow a release that was cut, and only one that was cut.

`release-plz release` exits zero having released nothing — every ordinary push,
under `release_always` — and the Python and JavaScript registries refuse a
version they already serve. So the two jobs after it are gated on what it
ANSWERED rather than on its exit status: the workflow runs it with `--output
json`, `just release-answer` reads that answer into a job output, and the artifact
build and the artifact publish run on that output being non-empty.

These journeys drive that recipe for both answers — a release having been cut
and none having been cut — and for every answer it must refuse, and drive `just
check-repo` over the committed tree and over copies carrying each defect the
`release-gating` rule refuses: a publishing job that waits on the drafting job,
and an artifact build that no longer reads the answer.
"""

# `assert` is how pytest states an assertion and how it produces the failure
# message a reader acts on; suppressions.toml carries the reason.

from __future__ import annotations

import json
from collections.abc import Callable
from pathlib import Path

import pytest
from journey import REPO_ROOT, GateCopy, capture, clean_environment, output, pythonpath
from release_artifacts.registries import RELEASE_ANSWER_SAMPLE
from repo_checks.expect import equal, failing, passing, truth
from repo_checks.model import Repo

WORKFLOW = ".github/workflows/release-plz.yml"
RECIPE_TIMEOUT_SECONDS = 300

#: What `repo-policy.toml` declares about the gating: the recipe that reads the
#: release program's answer, the output it publishes it under, and the two
#: recipes gated on it. Read rather than restated, because these are what
#: `just check-repo` holds the workflow to.
DECLARED = Repo(REPO_ROOT).policy["release"]


def answer(*versions: str) -> str:
    """What `release-plz release --output json` answers having released `versions`.

    Built from the one recorded sample rather than written here, so a fixture
    cannot drift from the reader on its own.
    """
    recorded = json.loads((REPO_ROOT / RELEASE_ANSWER_SAMPLE).read_text(encoding="utf-8"))
    entry = recorded["releases"][0]
    return json.dumps(
        {
            "releases": [
                {**entry, "package_name": f"printobserver-{index}", "tag": f"v{v}", "version": v}
                for index, v in enumerate(versions)
            ]
        }
    )


def read(answered: str, into: Path) -> tuple[int, str]:
    """Drive the recipe the publishing job reads its answer with, over `answered`."""
    written = into / "released.json"
    written.write_text(answered, encoding="utf-8")
    return read_file(written)


def read_file(written: Path) -> tuple[int, str]:
    """Drive the recipe over the file the release step would have written."""
    result = capture(
        ["just", str(DECLARED["answer_recipe"]), str(written)],
        REPO_ROOT,
        timeout=RECIPE_TIMEOUT_SECONDS,
        env=clean_environment(PYTHONPATH=pythonpath()),
    )
    return result.returncode, output(result)


@pytest.mark.parametrize(
    ("versions", "answered"),
    [
        # Thirteen crates under one version name one tag, answered once.
        (("0.4.0", "0.4.0"), "v0.4.0"),
        # Two packages released at two versions name two, in the order released.
        (("0.4.0", "0.5.0"), "v0.4.0 v0.5.0"),
    ],
)
def test_a_run_that_cut_a_release_answers_a_field_the_artifact_jobs_run_on(
    versions: tuple[str, ...], answered: str, tmp_path: Path
) -> None:
    """A run that released something answers a non-empty field, one line, nothing beside it."""
    code, said = read(answer(*versions), tmp_path)

    passing((code, said), describing=f"`just {DECLARED['answer_recipe']}` over a cut release")
    equal(
        said.strip(),
        f"{DECLARED['answer_output']}={answered}",
        describing="the one line a job reads",
    )


def test_a_run_that_cut_nothing_answers_the_empty_field_the_artifact_jobs_skip_on(
    tmp_path: Path,
) -> None:
    """Every ordinary push finishes a release run that released nothing, exiting zero."""
    code, said = read(answer(), tmp_path)

    passing((code, said), describing=f"`just {DECLARED['answer_recipe']}` over no release")
    equal(
        said.strip(),
        f"{DECLARED['answer_output']}=",
        describing="the empty field such a run publishes",
    )


@pytest.mark.parametrize(
    ("answered", "named"),
    [
        ("release-plz wrote something else here", "not JSON"),
        (json.dumps({"something": "else"}), "no `releases` list"),
        (answer("0.4.0").replace('"tag": "v0.4.0", ', ""), "carries no"),
        # A tag with a newline in it would write a second output nothing named.
        (answer("0.4.0\nextra=1"), "is not that"),
        # A version is not a tag: release automation writes `v` before it.
        (answer("0.4.0").replace("v0.4.0", "0.4.0"), "is not that"),
    ],
)
def test_an_answer_the_release_program_does_not_write_fails_the_job_rather_than_skipping(
    answered: str, named: str, tmp_path: Path
) -> None:
    """Read as "released nothing", an unreadable answer would skip the publish of a cut release."""
    code, said = read(answered, tmp_path)

    failing((code, said), naming=named)
    truth(
        not any(line.startswith(f"{DECLARED['answer_output']}=") for line in said.splitlines()),
        describing=f"no field for a job to read off a refused answer: {said!r}",
    )


def test_an_answer_the_release_program_never_wrote_fails_the_job(tmp_path: Path) -> None:
    """A release step that wrote no answer file is a step whose answer nothing can read."""
    never_written = tmp_path / "never-written.json"

    result = read_file(never_written)

    failing(result, naming=str(never_written))


def test_the_committed_release_path_is_accepted_by_the_gate(
    gate_copy: Callable[[], GateCopy],
) -> None:
    """Publishing waits on no drafting and the artifacts follow the answer, as committed."""
    clean = gate_copy()

    result = clean.just("check-repo")

    passing(result)


def test_a_publishing_job_that_waits_on_the_drafting_job_is_refused_by_the_gate(
    gate_copy: Callable[[], GateCopy],
) -> None:
    """The state this repair undid — publishing chained behind drafting — cannot come back."""
    broken = gate_copy()
    broken.edit(
        WORKFLOW,
        "  release:\n    name: release\n",
        "  release:\n    name: release\n    needs: release-pr\n",
    )

    result = broken.just("check-repo")

    failing(result, naming="waits on `release-pr`, which drafts the next one")


def test_an_artifact_build_that_no_longer_reads_the_answer_is_refused_by_the_gate(
    gate_copy: Callable[[], GateCopy],
) -> None:
    """A build chained behind the release unconditionally runs on every push."""
    broken = gate_copy()
    broken.edit(
        WORKFLOW,
        f"    if: needs.release.outputs.{DECLARED['answer_output']} != ''\n    strategy:\n",
        "    strategy:\n",
    )

    result = broken.just("check-repo")

    failing(result, naming=f"runs `just {DECLARED['build_recipe']}` and is not gated on")
