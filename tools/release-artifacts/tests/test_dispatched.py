"""A hand-dispatched run publishes an existing release, and only one that exists.

The release workflow dispatched with a tag builds that tag's tree and publishes
it with the publisher at the dispatched ref. What stands between the dispatcher
and a build is `dispatched`: the tag is one release automation writes, the
checkout carries it, and the workspace at the tag declares the version the tag
names. Each is driven here over a real git repository through the command line
the workflow runs — nothing is mocked, and a refused tag is proven to print
nothing a job could read and to write no record.

The record it writes is what crosses to the install-path proof, and `recorded`
is the proof's reader of it: one line, refused rather than read as an empty
field where it is anything else.
"""

from __future__ import annotations

from dataclasses import dataclass
from pathlib import Path

import pytest
from release_artifacts import targets
from release_artifacts.__main__ import main
from release_artifacts.registries import (
    RELEASED_FIELD,
    VERSION_FIELD,
    RegistryError,
    dispatched,
    recorded,
)
from repo_checks.expect import contains, equal, truth
from repo_checks.model import Repo
from repo_checks.shell import run

#: A version no tree of this repository declares, tagged at the same commit as
#: the real one: the tag exists, and its tree builds another release.
MISMATCHED = "9.9.9"


@dataclass(frozen=True, slots=True)
class Tagged:
    """A real repository carrying the committed workspace manifest and its tags."""

    path: Path
    #: The version that manifest declares, and so the one `v<version>` names.
    version: str

    def git(self, *argv: str) -> str:
        """Run one git command here, or fail the test saying what it said."""
        done = run(["git", *argv], cwd=self.path, timeout=60)
        truth(done.returncode == 0, describing=f"`git {' '.join(argv)}`:\n{done.stderr}")
        return done.stdout.strip()


@pytest.fixture
def tagged(repo: Repo, tmp_path: Path) -> Tagged:
    """The committed tree's own manifest, committed and tagged as release automation tags it.

    Tagged twice at one commit: `v<version>` as the release was, and
    `v9.9.9`, which exists and whose tree declares another version. A third
    tag names an earlier commit carrying no manifest at all.
    """
    root = tmp_path / "checkout"
    root.mkdir()
    checkout = Tagged(root, targets.workspace(repo.root)["version"])
    checkout.git("init", "--initial-branch", "main")
    checkout.git("config", "user.email", "release@example.invalid")
    checkout.git("config", "user.name", "release automation")
    checkout.git("commit", "--allow-empty", "-m", "chore: before any manifest")
    checkout.git("tag", "v0.0.1")
    (root / "Cargo.toml").write_bytes((repo.root / "Cargo.toml").read_bytes())
    checkout.git("add", "Cargo.toml")
    checkout.git("commit", "-m", f"chore: release v{checkout.version}")
    checkout.git("tag", f"v{checkout.version}")
    checkout.git("tag", f"v{MISMATCHED}")
    return checkout


def dispatch(
    tag: str, root: Path, record: Path, capsys: pytest.CaptureFixture[str]
) -> tuple[int, str, str]:
    """Drive the command the workflow's dispatched step runs, as it runs it."""
    code = main(["dispatched", "--tag", tag, "--root", str(root), "--record", str(record)])
    captured = capsys.readouterr()
    return code, captured.out, captured.err


def test_an_existing_tag_whose_tree_agrees_is_answered_and_recorded(
    tagged: Tagged, tmp_path: Path, capsys: pytest.CaptureFixture[str]
) -> None:
    """The one line the artifact jobs are gated on, and the record the proof reads."""
    record = tmp_path / "dispatched-release" / "version"

    code, out, err = dispatch(f"v{tagged.version}", tagged.path, record, capsys)

    equal(code, 0, describing=f"the exit a dispatch of an existing release gets:\n{err}")
    equal(out, f"{RELEASED_FIELD}=v{tagged.version}\n", describing="the one line a job reads")
    equal(err, "", describing="what a passing dispatch said beside its one line")
    equal(
        record.read_text(encoding="utf-8"),
        f"{VERSION_FIELD}={tagged.version}\n",
        describing="the record, its parent created",
    )
    equal(recorded(record), tagged.version, describing="what the proof reads back off it")


def test_a_tag_the_checkout_does_not_carry_is_refused_naming_it(
    tagged: Tagged, tmp_path: Path, capsys: pytest.CaptureFixture[str]
) -> None:
    """A dispatch names an existing release, and one nothing tagged is not that."""
    record = tmp_path / "record"

    code, out, err = dispatch("v0.3.0", tagged.path, record, capsys)

    equal(code, 1, describing="the exit a tag nothing carries gets")
    equal(out, "", describing="the output a job would have read a field from")
    contains(err, "v0.3.0", describing="the tag named")
    contains(err, str(tagged.path), describing="the checkout named")
    contains(err, "existing release tag", describing="what a dispatch names")
    truth(not record.exists(), describing="no record for a refused dispatch")


def test_a_shallow_tagless_checkout_refuses_a_tag_its_origin_carries(
    tagged: Tagged, tmp_path: Path, capsys: pytest.CaptureFixture[str]
) -> None:
    """The checkout the job must not have: the tag exists, and this clone cannot see it.

    `actions/checkout` without `fetch-depth: 0` is exactly this clone, and it
    would refuse every dispatch there is — so the refusal says the checkout
    carries no tags and what fetches them, rather than that the tag does not
    exist.
    """
    shallow = tmp_path / "shallow"
    cloned = run(
        ["git", "clone", "--depth", "1", "--no-tags", f"file://{tagged.path}", str(shallow)],
        timeout=60,
    )
    truth(cloned.returncode == 0, describing=cloned.stderr)
    equal(tagged.git("tag", "--list").split() != [], True, describing="the origin's tags")

    code, out, err = dispatch(f"v{tagged.version}", shallow, tmp_path / "record", capsys)

    equal(code, 1, describing="the exit a shallow, tagless checkout gets")
    equal(out, "", describing="the output a job would have read a field from")
    contains(err, f"v{tagged.version}", describing="the tag named")
    contains(err, "carries no tags at all", describing="what is wrong with the checkout")
    contains(err, "fetch-depth: 0", describing="what repairs the job's checkout")


def test_a_tag_whose_tree_declares_another_version_is_refused_naming_both(
    tagged: Tagged, tmp_path: Path, capsys: pytest.CaptureFixture[str]
) -> None:
    """An existing tag over the wrong tree would put one release's artifacts on another's."""
    record = tmp_path / "record"

    code, out, err = dispatch(f"v{MISMATCHED}", tagged.path, record, capsys)

    equal(code, 1, describing="the exit a tag over the wrong tree gets")
    equal(out, "", describing="the output a job would have read a field from")
    contains(err, MISMATCHED, describing="the version the tag names")
    contains(err, tagged.version, describing="the version the tree declares")
    truth(not record.exists(), describing="no record for a refused dispatch")


@pytest.mark.parametrize("tag", ["0.2.0", "v1.2", "release-1", "v0.2.0-rc1", ""])
def test_a_name_that_is_not_a_release_tag_is_refused_before_the_checkout_is_asked(
    tag: str, tagged: Tagged, tmp_path: Path, capsys: pytest.CaptureFixture[str]
) -> None:
    """What arrives from a dispatch input reaches `git` as an argument, so it stops here."""
    record = tmp_path / "record"

    code, out, err = dispatch(tag, tagged.path, record, capsys)

    equal(code, 2 if not tag else 1, describing=f"the exit `{tag}` gets")
    equal(out, "", describing="the output a job would have read a field from")
    contains(err, "v<major>.<minor>.<patch>" if tag else "takes --tag", describing=err)
    truth(not record.exists(), describing="no record for a refused dispatch")


def test_a_tag_whose_tree_carries_no_readable_manifest_is_refused(tagged: Tagged) -> None:
    """A tree with no workspace manifest, and one whose manifest declares no version."""
    with pytest.raises(RegistryError) as unreadable:
        dispatched(tagged.path, "v0.0.1")
    contains(str(unreadable.value), "could not read the workspace manifest", describing="a tree")

    (tagged.path / "Cargo.toml").write_text("[workspace]\nmembers = []\n", encoding="utf-8")
    tagged.git("commit", "-am", "chore: a manifest declaring no version")
    tagged.git("tag", "v0.0.2")
    with pytest.raises(RegistryError) as undeclared:
        dispatched(tagged.path, "v0.0.2")
    contains(str(undeclared.value), "declares no version", describing="a versionless manifest")

    (tagged.path / "Cargo.toml").write_text("this is not = [toml\n", encoding="utf-8")
    tagged.git("commit", "-am", "chore: a manifest nothing can parse")
    tagged.git("tag", "v0.0.3")
    with pytest.raises(RegistryError) as unparsed:
        dispatched(tagged.path, "v0.0.3")
    contains(str(unparsed.value), "declares no version", describing="an unparseable manifest")


def test_a_directory_that_is_no_repository_cannot_be_asked(tmp_path: Path) -> None:
    """The question is git's, and a directory git refuses is refused naming the question."""
    with pytest.raises(RegistryError) as refused:
        dispatched(tmp_path, "v0.2.0")

    contains(str(refused.value), "could not be asked which tags", describing="what it said")


def test_dispatching_without_a_record_path_is_refused(
    tagged: Tagged, capsys: pytest.CaptureFixture[str]
) -> None:
    """The record is what the proof reads, so a dispatch that writes none is no dispatch."""
    equal(
        main(["dispatched", "--tag", f"v{tagged.version}", "--root", str(tagged.path)]),
        2,
        describing="the exit naming no record gets",
    )

    contains(capsys.readouterr().err, "takes --record", describing="what it said")


def test_the_recorded_version_is_answered_as_the_field_the_proof_reads(
    tmp_path: Path, capsys: pytest.CaptureFixture[str]
) -> None:
    """One line in, the same line out, through the command the resolving job runs."""
    record = tmp_path / "version"
    record.write_text(f"{VERSION_FIELD}=0.2.0\n", encoding="utf-8")

    code = main(["recorded", "--record", str(record)])

    captured = capsys.readouterr()
    equal(code, 0, describing=f"the exit a good record gets:\n{captured.err}")
    equal(captured.out, f"{VERSION_FIELD}=0.2.0\n", describing="the one line a job reads")
    equal(captured.err, "", describing="what a passing read said beside its one line")


@pytest.mark.parametrize(
    "written",
    [
        # A second line would be a second output nothing named.
        f"{VERSION_FIELD}=0.2.0\nextra=1\n",
        # Another field is not the one the proof reads.
        "released=v0.2.0\n",
        # The tag rather than the version: not what the proof is handed.
        f"{VERSION_FIELD}=v0.2.0\n",
        f"{VERSION_FIELD}=not a version\n",
        f"{VERSION_FIELD}=\n",
        "",
    ],
)
def test_a_record_holding_anything_but_the_one_line_is_refused_rather_than_read_as_none(
    written: str, tmp_path: Path, capsys: pytest.CaptureFixture[str]
) -> None:
    """Read as no version, the route proofs would skip over a publish nobody checked."""
    record = tmp_path / "version"
    record.write_text(written, encoding="utf-8")

    code = main(["recorded", "--record", str(record)])

    captured = capsys.readouterr()
    equal(code, 1, describing=f"the exit {written!r} gets")
    equal(captured.out, "", describing="the output a job would have read a field from")
    contains(captured.err, str(record), describing="the record named")
    contains(captured.err, f"`{VERSION_FIELD}=<version>`", describing="the line it wanted")


def test_a_record_that_is_not_there_or_not_text_is_refused_naming_it(
    tmp_path: Path, capsys: pytest.CaptureFixture[str]
) -> None:
    """A dispatch refused at `release` leaves no record, and the proof after it must say so."""
    equal(main(["recorded"]), 2, describing="the exit naming no record gets")
    contains(capsys.readouterr().err, "takes --record", describing="what it said")

    never_written = tmp_path / "never-written"
    equal(main(["recorded", "--record", str(never_written)]), 1, describing="a missing record")
    captured = capsys.readouterr()
    equal(captured.out, "", describing="the output a job would have read a field from")
    contains(captured.err, str(never_written), describing="the record named")
    contains(captured.err, "is not there", describing="what is wrong with it")

    undecodable = tmp_path / "not-text"
    undecodable.write_bytes(b"\xff\xfe=")
    equal(main(["recorded", "--record", str(undecodable)]), 1, describing="a record not in text")
    captured = capsys.readouterr()
    equal(captured.out, "", describing="the output a job would have read a field from")
    contains(captured.err, str(undecodable), describing="the record named")
    contains(captured.err, "could not be read", describing="what is wrong with it")
