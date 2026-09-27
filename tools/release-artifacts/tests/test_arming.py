"""The release pull request the drafting step names is armed, and nothing else.

Driven through the command line the workflow's `just release-pr-arm` runs, over
the drafting program's own answer. The forge's CLI is the one boundary: a
stand-in `gh` first on the PATH records every call and refuses where told to,
because the real one would arm a real pull request.
"""

from __future__ import annotations

import json
import os
import shutil
import sys
from pathlib import Path

import pytest
from conftest import REPO_ROOT
from release_artifacts import arming as arming_module
from release_artifacts.__main__ import main
from release_artifacts.arming import DRAFTED_SAMPLE
from repo_checks.expect import contains, equal
from repo_checks.shell import run

# llmlint: ignore[e2e_not_mocked] suppressions.toml has the reason.
GH = """import json
import os
import shutil
import sys

with open(os.environ["GH_STANDIN_RECORD"], "a", encoding="utf-8") as record:
    record.write(json.dumps(sys.argv[1:]) + "\\n")
if os.environ.get("GH_STANDIN_SLEEPS"):
    import time

    time.sleep(float(os.environ["GH_STANDIN_SLEEPS"]))
refused = os.environ.get("GH_STANDIN_REFUSES", "")
if refused and refused in sys.argv:
    print("GraphQL: Pull request is not mergeable (enablePullRequestAutoMerge)", file=sys.stderr)
    raise SystemExit(1)
"""


class Forge:
    """The stand-in `gh`, first on the PATH, and what it was asked."""

    def __init__(self, root: Path, monkeypatch: pytest.MonkeyPatch) -> None:
        """Install it in `root` and put `root` first on the PATH."""
        self.record = root / "asked"
        self.record.touch()
        if sys.platform == "win32":
            (root / "gh.py").write_text(GH, encoding="utf-8")
            (root / "gh.cmd").write_text(
                f'@"{sys.executable}" "%~dp0gh.py" %*\r\n', encoding="utf-8"
            )
        else:
            program = root / "gh"
            program.write_text(f"#!{sys.executable}\n{GH}", encoding="utf-8")
            program.chmod(0o755)
        monkeypatch.setenv("PATH", f"{root}{os.pathsep}{os.environ['PATH']}")
        monkeypatch.setenv("GH_STANDIN_RECORD", str(self.record))

    @property
    def asked(self) -> list[list[str]]:
        """Every call it received, as its argument list."""
        return [json.loads(line) for line in self.record.read_text(encoding="utf-8").splitlines()]


@pytest.fixture
def forge(tmp_path: Path, monkeypatch: pytest.MonkeyPatch) -> Forge:
    """The stand-in forge CLI."""
    root = tmp_path / "forge"
    root.mkdir()
    return Forge(root, monkeypatch)


def arming(
    answer: Path, capsys: pytest.CaptureFixture[str], root: Path = REPO_ROOT
) -> tuple[int, str, str]:
    """Run the command `just release-pr-arm` runs, as it runs it, in the checkout `root`."""
    code = main(["arm-release-pr", "--answer", str(answer), "--root", str(root)])
    captured = capsys.readouterr()
    return code, captured.out, captured.err


def answer_of(tmp_path: Path, *numbers: int) -> Path:
    """A drafting answer naming the pull requests `numbers`, shaped as the program writes it."""
    sample = json.loads((REPO_ROOT / DRAFTED_SAMPLE).read_text(encoding="utf-8"))["prs"][0]
    prs = [
        {**sample, "number": n, "html_url": sample["html_url"].rsplit("/", 1)[0] + f"/{n}"}
        for n in numbers
    ]
    path = tmp_path / "release-pr.json"
    path.write_text(json.dumps({"prs": prs}), encoding="utf-8")
    return path


def test_the_committed_sample_is_armed_by_its_url_and_nothing_else(
    forge: Forge, capsys: pytest.CaptureFixture[str]
) -> None:
    """One pull request answered, one arming, naming it by the URL the answer gave."""
    sample = json.loads((REPO_ROOT / DRAFTED_SAMPLE).read_text(encoding="utf-8"))["prs"][0]

    code, out, err = arming(REPO_ROOT / DRAFTED_SAMPLE, capsys)

    equal((code, err), (0, ""), describing="arming the sample")
    equal(forge.asked, [["pr", "merge", "--auto", "--squash", sample["html_url"]]])
    contains(out, f"armed #{sample['number']}", describing="what arming said")


def test_an_answer_naming_no_pull_request_arms_nothing(
    forge: Forge, tmp_path: Path, capsys: pytest.CaptureFixture[str]
) -> None:
    """Nothing drafted is nothing armed, said rather than silent."""
    code, out, _ = arming(answer_of(tmp_path), capsys)

    equal(code, 0, describing="arming an answer naming none")
    equal(forge.asked, [], describing="what the forge was asked")
    contains(out, "nothing to arm", describing="what arming said")


def test_a_refusal_fails_naming_the_pull_request_after_attempting_every_one(
    forge: Forge,
    tmp_path: Path,
    monkeypatch: pytest.MonkeyPatch,
    capsys: pytest.CaptureFixture[str],
) -> None:
    """The first pull request refused does not cost the second, and the step fails naming it."""
    answer = answer_of(tmp_path, 41, 42)
    refused = json.loads(answer.read_text(encoding="utf-8"))["prs"][0]["html_url"]
    monkeypatch.setenv("GH_STANDIN_REFUSES", refused)

    code, _, err = arming(answer, capsys)

    equal(code, 1, describing="arming with the forge refusing one")
    equal(len(forge.asked), 2, describing="the pull requests the forge was asked to arm")
    contains(err, "#41", describing="the refusal")
    contains(err, refused, describing="the refusal")
    contains(err, "not mergeable", describing="the forge's own words")
    contains(err, arming_module.REFUSED_NEXT, describing="what the refusal says to do")
    contains(err, "armed #42", describing="what the refusal says was armed")


#: One pull request as the program answers it — the committed sample's — for
#: the refusals below to move one field of.
OURS = json.loads((REPO_ROOT / DRAFTED_SAMPLE).read_text(encoding="utf-8"))["prs"][0]

#: Where that pull request's URL names pull requests of its repository.
PULLS = OURS["html_url"].rsplit("/", 1)[0]


def _moved(**fields: object) -> str:
    """An answer naming one pull request, `OURS` with `fields` moved."""
    return json.dumps({"prs": [{**OURS, **fields}]})


@pytest.mark.parametrize(
    ("answer", "naming"),
    [
        ("not json", "is not the JSON"),
        ('{"releases": []}', "carries no `prs` list"),
        ('{"prs": ["41"]}', "is not a pull request of"),
        (_moved(number=True), "is not a pull request of"),
        (_moved(number=0, html_url=f"{PULLS}/0"), "is not a pull request of"),
        (_moved(html_url=f"{PULLS}/{OURS['number'] - 1}"), "is not a pull request of"),
        (
            _moved(html_url="https://github.com/somebody/else/pull/41"),
            "is not a pull request of",
        ),
        (_moved(html_url="http" + OURS["html_url"][5:]), "is not a pull request of"),
        (_moved(base_branch="develop"), "into `main`"),
        (_moved(releases=[]), "releases at least one package"),
        (_moved(releases=None), "releases at least one package"),
        (_moved(releases=[{"package_name": "left-pad", "version": "1.0.0"}]), "only crates"),
        (_moved(releases=["printobserver"]), "only crates"),
        (_moved(releases=[{"package_name": ["printobserver"]}]), "only crates"),
    ],
    ids=[
        "not-json",
        "no-prs",
        "not-an-object",
        "boolean-number",
        "number-zero",
        "url-of-another-number",
        "another-repository",
        "not-https",
        "another-base-branch",
        "releasing-nothing",
        "no-releases",
        "releasing-another-projects-package",
        "release-not-an-object",
        "package-name-not-a-string",
    ],
)
def test_an_answer_that_is_not_the_programs_is_refused_arming_nothing(
    forge: Forge,
    tmp_path: Path,
    capsys: pytest.CaptureFixture[str],
    answer: str,
    naming: str,
) -> None:
    """Read as "none", an unparsable answer would leave the release blocked with nothing said."""
    path = tmp_path / "release-pr.json"
    path.write_text(answer, encoding="utf-8")

    code, _, err = arming(path, capsys)

    equal(code, 1, describing=f"arming {answer!r}")
    contains(err, naming, describing="the refusal")
    contains(err, arming_module.ANSWER_NEXT, describing="what the refusal says to do")
    equal(forge.asked, [], describing="what the forge was asked")


def test_an_answer_that_is_not_there_is_refused(
    forge: Forge, tmp_path: Path, capsys: pytest.CaptureFixture[str]
) -> None:
    """A drafting step that wrote nothing is a failure, naming the file it did not write."""
    missing = tmp_path / "never-written.json"

    code, _, err = arming(missing, capsys)

    equal(code, 1, describing="arming an answer that is not there")
    contains(err, str(missing), describing="the refusal")
    contains(err, arming_module.ANSWER_NEXT, describing="what the refusal says to do")
    equal(forge.asked, [], describing="what the forge was asked")


def test_a_forge_that_never_answers_fails_naming_the_pull_request(
    forge: Forge,
    tmp_path: Path,
    monkeypatch: pytest.MonkeyPatch,
    capsys: pytest.CaptureFixture[str],
) -> None:
    """A hung forge is that pull request's refusal, reported, rather than a traceback."""
    monkeypatch.setattr(arming_module, "TIMEOUT_SECONDS", 1)
    monkeypatch.setenv("GH_STANDIN_SLEEPS", "10")

    code, _, err = arming(answer_of(tmp_path, 41), capsys)

    equal(code, 1, describing="arming with the forge silent")
    contains(err, "#41", describing="the refusal")
    contains(err, "no answer within 1 s", describing="the refusal")


def test_a_checkout_declaring_no_repository_arms_nothing(
    forge: Forge, tmp_path: Path, capsys: pytest.CaptureFixture[str]
) -> None:
    """Nothing says which pull request is this repository's, so none is armed."""
    elsewhere = tmp_path / "elsewhere"
    elsewhere.mkdir()
    (elsewhere / "repo-policy.toml").write_text("schema_version = 1\n", encoding="utf-8")

    code, _, err = arming(REPO_ROOT / DRAFTED_SAMPLE, capsys, elsewhere)

    equal(code, 1, describing="arming from a checkout of something else")
    contains(err, "repository.owner", describing="the refusal")
    equal(forge.asked, [], describing="what the forge was asked")


def test_arming_without_an_answer_is_a_usage_error(capsys: pytest.CaptureFixture[str]) -> None:
    """The command says what it takes rather than arming a guess."""
    code = main(["arm-release-pr"])

    equal(code, 2, describing="arming with no answer named")
    contains(capsys.readouterr().err, "--answer", describing="what it said")


def test_the_committed_sample_carries_every_key_the_reader_reads() -> None:
    """The sample and the reader name one shape, so neither can move without the other."""
    sample = json.loads((REPO_ROOT / DRAFTED_SAMPLE).read_text(encoding="utf-8"))

    contains(sample, arming_module.PRS, describing="the sample")
    for pull in sample[arming_module.PRS]:
        for key in (arming_module.NUMBER, arming_module.URL, arming_module.BASE):
            contains(pull, key, describing="a pull request of the sample")
        for release in pull[arming_module.RELEASES]:
            contains(release, arming_module.PACKAGE, describing="a release of the sample")


@pytest.mark.skipif(shutil.which("gh") is None, reason="GitHub CLI is not on this host")
def test_the_forge_cli_takes_every_option_arming_passes() -> None:
    """The options `ARM` hands `gh`, read against the installed `gh`'s own help.

    The stand-in above accepts anything, so this is what notices a `gh` that no
    longer takes one of them.
    """
    program, *subcommand = arming_module.ARM[:3]
    helped = run([program, *subcommand, "--help"], timeout=60)
    said = helped.stdout + helped.stderr

    equal(helped.returncode, 0, describing=f"`gh pr merge --help`: {said}")
    for option in arming_module.ARM[3:]:
        contains(said, option, describing="what `gh pr merge --help` lists")
