"""The release pull request the drafting step names is armed, and nothing else.

Driven through the command line the workflow's `just release-pr-arm` runs, over
the drafting program's own answer. The forge's CLI is the one boundary: a
stand-in `gh` first on the PATH records every call and refuses where told to,
because the real one would arm a real pull request.
"""

from __future__ import annotations

import json
import os
import sys
from pathlib import Path

import pytest
from conftest import REPO_ROOT
from release_artifacts.__main__ import main
from release_artifacts.arming import DRAFTED_SAMPLE
from repo_checks.expect import contains, equal

# llmlint: ignore[e2e_not_mocked] suppressions.toml has the reason.
GH = """import json
import os
import sys

with open(os.environ["GH_STANDIN_RECORD"], "a", encoding="utf-8") as record:
    record.write(json.dumps(sys.argv[1:]) + "\\n")
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


def arming(answer: Path, capsys: pytest.CaptureFixture[str]) -> tuple[int, str, str]:
    """Run the command `just release-pr-arm` runs, as it runs it."""
    code = main(["arm-release-pr", "--answer", str(answer)])
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
    contains(err, "armed #42", describing="what the refusal says was armed")


@pytest.mark.parametrize(
    ("answer", "naming"),
    [
        ("not json", "is not the JSON"),
        ('{"releases": []}', "carries no `prs` list"),
        ('{"prs": [{"number": 41}]}', "is not a pull request"),
        ('{"prs": [{"number": true, "html_url": "https://x/pull/1"}]}', "is not a pull request"),
        ('{"prs": [{"number": 41, "html_url": "https://x/pull/40"}]}', "is not a pull request"),
        ('{"prs": [{"number": 41, "html_url": "http://x/pull/41"}]}', "is not a pull request"),
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
    equal(forge.asked, [], describing="what the forge was asked")


def test_an_answer_that_is_not_there_is_refused(
    forge: Forge, tmp_path: Path, capsys: pytest.CaptureFixture[str]
) -> None:
    """A drafting step that wrote nothing is a failure, naming the file it did not write."""
    missing = tmp_path / "never-written.json"

    code, _, err = arming(missing, capsys)

    equal(code, 1, describing="arming an answer that is not there")
    contains(err, str(missing), describing="the refusal")
    equal(forge.asked, [], describing="what the forge was asked")


def test_arming_without_an_answer_is_a_usage_error(capsys: pytest.CaptureFixture[str]) -> None:
    """The command says what it takes rather than arming a guess."""
    code = main(["arm-release-pr"])

    equal(code, 2, describing="arming with no answer named")
    contains(capsys.readouterr().err, "--answer", describing="what it said")
