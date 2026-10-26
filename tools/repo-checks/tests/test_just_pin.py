"""The `just` release the recipes are held to, and bootstrap's parse of the `justfile`.

The pin is held together by the `just-pin` check, over copies of the committed
files. The parse is driven the way a host meets it: `check-justfile` handed a
`just` on PATH — the host's real one, and stand-ins for a release that refuses
the committed `justfile` and for a newer one that reads it — and `just
bootstrap` itself, run by the real `just` over the committed recipe with the
refusing stand-in being the `just` on PATH.
"""

from __future__ import annotations

import os
import shutil
import sys
from collections.abc import Callable
from pathlib import Path

import pytest
from repo_checks.__main__ import main
from repo_checks.expect import absent, accepted, contains, equal, refused
from repo_checks.just_pin import check_justfile, held, just_pin
from repo_checks.model import Repo
from repo_checks.shell import run
from treecopy import REPO_ROOT, Closure, Tree

CLOSURE = Closure(("repo-policy.toml", ".tool-versions", "scripts/session-setup.sh", "justfile"))

#: The release the committed policy holds `just` at, read rather than restated.
HELD = held(Repo(REPO_ROOT))

#: The release this repository pinned before its `justfile` outgrew it.
REFUSING = "1.40.0"

#: What just 1.40.0 says of the committed `justfile`, as it said it.
PARSE_ERROR = """error: Unknown start of token '-'
  ——▶ justfile:65:12
   │
65 │     uv run -q python -m repo_checks install-tools {{TOOL}}
   │            ^"""

# A `just` that answers `--version` with JUST_STANDIN_RELEASE — exiting 1 instead
# where that is FAILS, as a build that cannot report itself does — and, asked for
# anything else, either says PARSE_ERROR and exits 1, as a release that cannot
# read the file does, or lists nothing and exits 0. Each invocation is recorded
# to JUST_STANDIN_RECORD.
JUST_STANDIN = """
import os
import sys

with open(os.environ["JUST_STANDIN_RECORD"], "a", encoding="utf-8") as record:
    record.write("just " + " ".join(sys.argv[1:]) + "\\n")
if sys.argv[1:] == ["--version"]:
    if os.environ["JUST_STANDIN_RELEASE"] == "FAILS":
        raise SystemExit(1)
    print("just " + os.environ["JUST_STANDIN_RELEASE"])
    raise SystemExit(0)
if os.environ.get("JUST_STANDIN_ERROR"):
    sys.stderr.write(os.environ["JUST_STANDIN_ERROR"] + "\\n")
    raise SystemExit(1)
"""

# The `uv` `just bootstrap` reaches first: its `run -q python -m repo_checks
# <verb>` is the real verb, run here in-process over the copy; anything else it
# is asked is recorded and nothing more.
UV_STANDIN = """
import os
import sys

sys.path[:0] = os.environ["STANDIN_IMPORT_PATH"].split(os.pathsep)
with open(os.environ["JUST_STANDIN_RECORD"], "a", encoding="utf-8") as record:
    record.write("uv " + " ".join(sys.argv[1:]) + "\\n")
if "repo_checks" in sys.argv:
    from repo_checks.__main__ import main

    raise SystemExit(main(sys.argv[sys.argv.index("repo_checks") + 1 :]))
"""

RECORDER = """
import os
import sys

with open(os.environ["JUST_STANDIN_RECORD"], "a", encoding="utf-8") as record:
    name = os.path.basename(sys.argv[0]).removesuffix(".py")
    record.write(name + " " + " ".join(sys.argv[1:]) + "\\n")
"""


def program(directory: Path, name: str, code: str) -> None:
    """Write a stand-in program this interpreter runs, findable on PATH as `name`.

    A POSIX host runs it by its interpreter line; a Windows host finds a program
    by its suffix, so there the code sits beside a `.cmd` handing it over.
    """
    if sys.platform == "win32":
        (directory / f"{name}.py").write_text(code, encoding="utf-8")
        (directory / f"{name}.cmd").write_text(
            f'@"{sys.executable}" "%~dp0{name}.py" %*\r\n', encoding="utf-8"
        )
        return
    written = directory / name
    written.write_text(f"#!{sys.executable}\n{code}", encoding="utf-8")
    written.chmod(0o755)


def standin_just(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch, *, release: str, refuses: bool
) -> tuple[Path, Path]:
    """A PATH whose `just` is a stand-in at `release`; answers its directory and its record."""
    directory = tmp_path / "bin"
    directory.mkdir()
    program(directory, "just", JUST_STANDIN)
    record = tmp_path / "invocations"
    record.touch()
    monkeypatch.setenv("PATH", str(directory))
    monkeypatch.setenv("JUST_STANDIN_RECORD", str(record))
    monkeypatch.setenv("JUST_STANDIN_RELEASE", release)
    if refuses:
        monkeypatch.setenv("JUST_STANDIN_ERROR", PARSE_ERROR)
    else:
        monkeypatch.delenv("JUST_STANDIN_ERROR", raising=False)
    return directory, record


def test_the_committed_pins_are_one_release() -> None:
    """`.tool-versions`, the session installer and the policy name one `just`."""
    accepted(just_pin(Repo(REPO_ROOT)))
    contains((REPO_ROOT / ".tool-versions").read_text(encoding="utf-8"), f"just {HELD}\n")


@pytest.mark.parametrize(
    ("relative", "old", "new", "naming"),
    [
        (".tool-versions", f"just {HELD}", f"just {REFUSING}", f"pins `just` at {REFUSING}"),
        (".tool-versions", f"just {HELD}\n", "", "pins `just` at no release"),
        (
            "scripts/session-setup.sh",
            f'JUST_MIN="{HELD}"',
            f'JUST_MIN="{REFUSING}"',
            f"no older than {REFUSING}, not the {HELD}",
        ),
        ("repo-policy.toml", f'[toolchain.just]\nversion = "{HELD}"', "", "at no release"),
    ],
)
def test_a_pin_drifting_from_the_policy_is_refused(
    tree: Callable[[], Tree], relative: str, old: str, new: str, naming: str
) -> None:
    """Each copy of the release is held to the policy's, and the finding says where to fix it."""
    drifted = tree()
    drifted.edit(relative, old, new)

    refused(just_pin(drifted.repo), naming)


def test_the_hosts_own_just_parses_the_committed_justfile(tree: Callable[[], Tree]) -> None:
    """The `just` that bootstrapped this host reads the file, so bootstrap gets past the check."""
    if shutil.which("just") is None:
        pytest.fail("`just` is not on PATH; `just bootstrap` needs it")

    equal(check_justfile(tree().repo), 0)


def test_a_release_that_cannot_parse_the_justfile_is_refused_by_the_pinned_release(
    tree: Callable[[], Tree],
    tmp_path: Path,
    monkeypatch: pytest.MonkeyPatch,
    capsys: pytest.CaptureFixture[str],
) -> None:
    """The refusal names the release found, the release pinned, and what the parser said."""
    copy = tree()
    _, record = standin_just(tmp_path, monkeypatch, release=REFUSING, refuses=True)

    equal(main(["check-justfile", "--root", str(copy.root)]), 1)

    said = capsys.readouterr().err
    contains(said, f"just {REFUSING} at ")
    contains(said, f"`.tool-versions` pins just {HELD}")
    contains(said, f"uv tool install rust-just=={HELD}")
    contains(said, PARSE_ERROR)
    contains(record.read_text(encoding="utf-8"), f"--justfile {copy.root / 'justfile'}")


@pytest.mark.parametrize("answer", ["FAILS", "a build naming no release"])
def test_a_refusing_just_that_cannot_say_its_release_is_still_refused_by_the_pinned_one(
    tree: Callable[[], Tree],
    tmp_path: Path,
    monkeypatch: pytest.MonkeyPatch,
    capsys: pytest.CaptureFixture[str],
    answer: str,
) -> None:
    """A `just` whose `--version` fails or names nothing is refused for its parse all the same."""
    copy = tree()
    standin_just(tmp_path, monkeypatch, release=answer, refuses=True)

    equal(main(["check-justfile", "--root", str(copy.root)]), 1)

    said = capsys.readouterr().err
    contains(said, "just (no release it names) at ")
    contains(said, f"`.tool-versions` pins just {HELD}")
    contains(said, PARSE_ERROR)


def test_a_newer_release_that_parses_the_justfile_is_accepted(
    tree: Callable[[], Tree],
    tmp_path: Path,
    monkeypatch: pytest.MonkeyPatch,
    capsys: pytest.CaptureFixture[str],
) -> None:
    """What is refused is the parse, never the release: a newer `just` reading the file passes."""
    copy = tree()
    standin_just(tmp_path, monkeypatch, release="99.0.0", refuses=False)

    equal(main(["check-justfile", "--root", str(copy.root)]), 0)
    equal(capsys.readouterr().err, "")


def test_a_host_with_no_just_is_told_which_release_to_install(
    tree: Callable[[], Tree],
    tmp_path: Path,
    monkeypatch: pytest.MonkeyPatch,
    capsys: pytest.CaptureFixture[str],
) -> None:
    """With nothing to parse with, the refusal names the pinned release rather than failing bare."""
    copy = tree()
    monkeypatch.setenv("PATH", str(tmp_path))

    equal(main(["check-justfile", "--root", str(copy.root)]), 1)
    contains(capsys.readouterr().err, f"Install just {HELD}")


def test_a_policy_holding_just_at_no_release_refuses_before_any_parse(
    tree: Callable[[], Tree],
    tmp_path: Path,
    monkeypatch: pytest.MonkeyPatch,
    capsys: pytest.CaptureFixture[str],
) -> None:
    """A parse checked against no pinned release has nothing to name in its refusal."""
    copy = tree()
    copy.edit("repo-policy.toml", f'version = "{HELD}"\n\n# The Windows', "\n# The Windows")
    _, record = standin_just(tmp_path, monkeypatch, release=REFUSING, refuses=True)

    equal(main(["check-justfile", "--root", str(copy.root)]), 1)
    contains(capsys.readouterr().err, "holds `just` at no release")
    equal(record.read_text(encoding="utf-8"), "", describing="what the stand-in `just` was asked")


@pytest.mark.skipif(sys.platform == "win32", reason="the recipe's stand-ins are POSIX scripts")
def test_bootstrap_stops_at_its_parse_check_before_it_does_anything_else(
    tree: Callable[[], Tree], tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    """`just bootstrap`, with a `just` on PATH that cannot read the file, refuses first.

    The real `just` runs the committed recipe; the `just` on PATH is the one the
    pin would select, and it is the one whose parse decides. Nothing after the
    check — the toolchain, the fetch, the installs — is reached.
    """
    real = shutil.which("just")
    shell = shutil.which("bash")
    if real is None or shell is None:
        pytest.fail("`just` and `bash` must be on PATH to run the recipe")
    copy = tree()
    directory, record = standin_just(tmp_path, monkeypatch, release=REFUSING, refuses=True)
    program(directory, "uv", UV_STANDIN)
    for name in ("rustup", "cargo"):
        program(directory, name, RECORDER)
    monkeypatch.setenv("PATH", os.pathsep.join([str(directory), str(Path(shell).parent)]))
    monkeypatch.setenv("STANDIN_IMPORT_PATH", os.pathsep.join(sys.path))

    ran = run([real, "bootstrap"], cwd=copy.root)

    equal(ran.returncode, 1, describing=f"`just bootstrap`: {ran.stdout}{ran.stderr}")
    contains(ran.stderr, f"just {REFUSING} at ")
    contains(ran.stderr, f"`.tool-versions` pins just {HELD}")
    contains(ran.stderr, "Unknown start of token")
    asked = record.read_text(encoding="utf-8")
    contains(asked, "uv run -q python -m repo_checks check-justfile")
    absent(asked, "rustup", describing="the toolchain step after the check")
    absent(asked, "cargo", describing="the fetch after the check")
