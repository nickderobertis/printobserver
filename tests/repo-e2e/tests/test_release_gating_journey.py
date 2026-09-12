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

The workflow's dispatched shape has two recipes of its own, driven here the
same way. `just release-dispatched` answers the same field for an existing tag
whose tree's workspace version the tag names, and refuses every other tag
naming why — a tag the origin carries and a shallow, tagless clone cannot see
included, proven over such a clone rather than over a copy that already
carries the tag. `just release-version-dispatched` answers the version that
recipe recorded and refuses a record it cannot read rather than answering an
empty field. And `just check-repo` refuses a copy carrying each defect the
`release-dispatch` rule names.
"""

# `assert` is how pytest states an assertion and how it produces the failure
# message a reader acts on; suppressions.toml carries the reason.

from __future__ import annotations

import json
from collections.abc import Callable
from pathlib import Path

import pytest
from journey import REPO_ROOT, GateCopy, capture, clean_environment, output, pythonpath
from release_artifacts import targets
from release_artifacts.registries import RELEASE_ANSWER_SAMPLE
from repo_checks.expect import contains, equal, failing, passing, truth
from repo_checks.model import Repo
from repo_checks.shell import run as shell_run

WORKFLOW = ".github/workflows/release-plz.yml"
RECIPE_TIMEOUT_SECONDS = 300

#: What `repo-policy.toml` declares about the gating: the recipe that reads the
#: release program's answer, the output it publishes it under, and the two
#: recipes gated on it. Read rather than restated, because these are what
#: `just check-repo` holds the workflow to.
DECLARED = Repo(REPO_ROOT).policy["release"]

#: A version no tree of this repository declares, tagged at the same commit as
#: the copy's own: the tag exists, and its tree builds another release.
MISMATCHED = "9.9.9"

#: The ref a dispatched publish is made on, as the forge names the base branch.
ON_MAIN = f"refs/heads/{Repo(REPO_ROOT).policy['repository']['base_branch']}"


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


def recipe(name: str, *arguments: str) -> tuple[int, str]:
    """Drive one recipe of the committed command surface, as its workflow step does."""
    result = capture(
        ["just", name, *arguments],
        REPO_ROOT,
        timeout=RECIPE_TIMEOUT_SECONDS,
        env=clean_environment(PYTHONPATH=pythonpath()),
    )
    return result.returncode, output(result)


def tagged(copy: GateCopy) -> str:
    """Tag the copy's commit `v<X>` at its own version, and `v9.9.9` beside it."""
    version = targets.workspace(copy.root)["version"]
    for tag in (f"v{version}", f"v{MISMATCHED}"):
        shell_run(["git", "tag", tag], cwd=copy.root, check=True)
    return version


def test_a_dispatch_of_an_existing_tag_answers_the_field_and_records_the_version(
    gate_copy: Callable[[], GateCopy], tmp_path: Path
) -> None:
    """One line for the artifact jobs' gate, and the record the proof reads afterwards."""
    copy = gate_copy()
    version = tagged(copy)
    record = tmp_path / "dispatched-release" / "version"

    code, said = recipe(
        str(DECLARED["dispatched_recipe"]), f"v{version}", str(copy.root), str(record), ON_MAIN
    )

    passing((code, said), describing=f"`just {DECLARED['dispatched_recipe']}` over an existing tag")
    equal(
        said.strip(),
        f"{DECLARED['answer_output']}=v{version}",
        describing="the one line a job reads",
    )
    equal(record.read_text(encoding="utf-8"), f"version={version}\n", describing="the record")

    code, said = recipe(str(DECLARED["record_recipe"]), str(record))

    passing((code, said), describing=f"`just {DECLARED['record_recipe']}` over that record")
    equal(said.strip(), f"version={version}", describing="the line the resolving job reads")


@pytest.mark.parametrize(
    ("tag", "named"),
    [
        # A tag nothing tagged: a dispatch names an existing release.
        ("v0.0.1", "carries no tag `v0.0.1`"),
        # A tag that exists over a tree declaring another version.
        (f"v{MISMATCHED}", f"names version {MISMATCHED}, and the workspace at that tag declares"),
        # Not a tag release automation writes.
        ("0.2.0", "v<major>.<minor>.<patch>"),
        ("latest", "v<major>.<minor>.<patch>"),
    ],
)
def test_a_dispatch_of_any_other_tag_is_refused_naming_why_and_records_nothing(
    tag: str, named: str, gate_copy: Callable[[], GateCopy], tmp_path: Path
) -> None:
    """The job fails naming which, and nothing after it is handed a field or a record."""
    copy = gate_copy()
    tagged(copy)
    record = tmp_path / "record"

    code, said = recipe(
        str(DECLARED["dispatched_recipe"]), tag, str(copy.root), str(record), ON_MAIN
    )

    failing((code, said), naming=named)
    truth(
        not any(line.startswith(f"{DECLARED['answer_output']}=") for line in said.splitlines()),
        describing=f"no field for a job to read off a refused dispatch: {said!r}",
    )
    truth(not record.exists(), describing="no record for a refused dispatch")


def test_a_dispatch_on_a_ref_other_than_the_base_branch_is_refused_naming_both(
    gate_copy: Callable[[], GateCopy], tmp_path: Path
) -> None:
    """The publisher and the secrets a release trusts are the base branch's, and no other's."""
    copy = gate_copy()
    version = tagged(copy)
    record = tmp_path / "record"

    code, said = recipe(
        str(DECLARED["dispatched_recipe"]),
        f"v{version}",
        str(copy.root),
        str(record),
        "refs/heads/a-branch",
    )

    failing((code, said), naming="`refs/heads/a-branch`")
    contains(said, f"`{ON_MAIN}`", describing="the ref a dispatch is made on")
    truth(
        not any(line.startswith(f"{DECLARED['answer_output']}=") for line in said.splitlines()),
        describing=f"no field for a job to read off a refused dispatch: {said!r}",
    )
    truth(not record.exists(), describing="no record for a refused dispatch")


def test_a_shallow_tagless_checkout_refuses_a_tag_its_origin_carries(
    gate_copy: Callable[[], GateCopy], tmp_path: Path
) -> None:
    """The checkout the job must not have, and the one the copy cannot stand in for.

    The copy carries its tags already, so over it a checkout narrowed to
    `main` reads exactly as the whole history does. This clone is the
    narrowed one — `git clone --depth 1 --no-tags` — and over it the recipe
    refuses the tag the origin carries, saying the checkout carries no tags.
    """
    copy = gate_copy()
    version = tagged(copy)
    shallow = tmp_path / "shallow"
    shell_run(
        ["git", "clone", "-q", "--depth", "1", "--no-tags", f"file://{copy.root}", str(shallow)],
        check=True,
    )
    record = tmp_path / "record"

    code, said = recipe(
        str(DECLARED["dispatched_recipe"]), f"v{version}", str(shallow), str(record), ON_MAIN
    )

    failing((code, said), naming=f"v{version}")
    contains(said, "carries no tags at all", describing=said)
    contains(said, "fetch-depth: 0", describing="what repairs the job's checkout")
    truth(not record.exists(), describing="no record for a refused dispatch")


@pytest.mark.parametrize(
    ("written", "named"),
    [
        (None, "is not there"),
        ("version=0.2.0\nextra=1\n", "does not hold the one `version=<version>` line"),
        ("released=v0.2.0\n", "does not hold the one `version=<version>` line"),
        ("version=\n", "does not hold the one `version=<version>` line"),
    ],
)
def test_a_record_that_cannot_be_read_fails_the_resolving_job_rather_than_answering_nothing(
    written: str | None, named: str, tmp_path: Path
) -> None:
    """Answered as an empty field, the route proofs would skip over a publish nobody checked."""
    record = tmp_path / "version"
    if written is not None:
        record.write_text(written, encoding="utf-8")

    code, said = recipe(str(DECLARED["record_recipe"]), str(record))

    failing((code, said), naming=named)
    contains(said, str(record), describing="the record named")
    truth(
        not any(line.startswith("version=") for line in said.splitlines()),
        describing=f"no field for a job to read off a refused record: {said!r}",
    )


#: Each defect the `release-dispatch` rule names, as an edit of the committed
#: tree, and what the check names refusing it.
DISPATCH_DEFECTS = {
    "the release program running on a dispatch": (
        WORKFLOW,
        "  release-pr:\n    name: release-pr\n    if: github.event_name == 'push'\n",
        "  release-pr:\n    name: release-pr\n",
        "runs `release-plz release-pr` without `github.event_name == 'push'`",
    ),
    "the dispatched tag reaching the artifact jobs unverified": (
        WORKFLOW,
        "released: ${{ steps.answer.outputs.released || steps.dispatched.outputs.released }}",
        "released: ${{ steps.answer.outputs.released || inputs.tag }}",
        "publishes no output `released` from `steps.dispatched.outputs.released`",
    ),
    "the build not checking out the dispatched tag": (
        WORKFLOW,
        "      - uses: actions/checkout@v5\n        with:\n          ref: ${{ inputs.tag }}\n",
        "      - uses: actions/checkout@v5\n",
        "runs `just build-artifacts` and its checkout does not take `ref:",
    ),
    "the publish not handed the dispatched version": (
        WORKFLOW,
        "          PRINTOBSERVER_PUBLISH_VERSION: ${{ inputs.tag }}\n",
        "",
        "does not hand the recipe `PRINTOBSERVER_PUBLISH_VERSION` from `inputs.tag`",
    ),
    "the two workflows naming different artifacts for the record": (
        ".github/workflows/install-path.yml",
        "          name: dispatched-release\n",
        "          name: dispatched\n",
        "downloads no artifact named `dispatched-release`",
    ),
    "the verifying job checking out less than the whole history": (
        WORKFLOW,
        "          fetch-depth: 0\n          token: ${{ secrets.RELEASE_PLZ_TOKEN }}\n"
        "      - uses: extractions/setup-just@v3\n",
        "          token: ${{ secrets.RELEASE_PLZ_TOKEN }}\n"
        "      - uses: extractions/setup-just@v3\n",
        "does not carry `fetch-depth: 0`",
    ),
    "the concurrency no longer keyed on the ref": (
        WORKFLOW,
        "  group: release-plz-${{ github.ref }}\n",
        "  group: release-plz\n",
        "does not read `github.ref`",
    ),
}


@pytest.mark.parametrize("defect", sorted(DISPATCH_DEFECTS))
def test_each_defect_of_the_dispatched_shape_is_refused_by_the_gate(
    defect: str, gate_copy: Callable[[], GateCopy]
) -> None:
    """The committed shape is accepted, and a copy carrying the defect is refused naming it."""
    relative, old, new, named = DISPATCH_DEFECTS[defect]
    broken = gate_copy()
    broken.edit(relative, old, new)

    result = broken.just("check-repo")

    failing(result, naming=named)
