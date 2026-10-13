"""The release program the integration tier's worlds carry, reused under its key.

Every journey here drives the tool's own `release-program` command over a real
cargo workspace of this suite's own — one crate named as the program is, whose
program says which source it was built from — and real `cargo build --release`
runs. Between preparations the workspace's `target` directory is removed, so a
program that answers afterwards is either the one that was kept or one cargo
built again, and the journeys tell the two apart by whether `target` came back.
"""

from __future__ import annotations

import shutil
from pathlib import Path

import pytest
from release_artifacts.__main__ import main
from release_artifacts.reused import DIRECTORY
from repo_checks import platforms
from repo_checks.expect import contains, equal, truth
from repo_checks.model import Repo
from repo_checks.shell import run

REPO_ROOT = Path(__file__).resolve().parents[3]

#: The crate the program is built from, as `release_artifacts.build` names it.
CRATE = "crates/printobserver"


def _workspace(root: Path, says: str) -> Path:
    """A cargo workspace whose `printobserver` program prints `says`."""
    crate = root / CRATE
    (crate / "src").mkdir(parents=True)
    (crate / "tests").mkdir()
    # The host's platform is read from the supported-platform list, and the
    # toolchain is the one this repository declares, so the build is made by
    # the compiler every other build here is made by.
    shutil.copy2(REPO_ROOT / "AGENTS.md", root / "AGENTS.md")
    shutil.copy2(REPO_ROOT / "rust-toolchain.toml", root / "rust-toolchain.toml")
    (root / "Cargo.toml").write_text(
        '[workspace]\nmembers = ["crates/printobserver"]\nresolver = "2"\n', encoding="utf-8"
    )
    (crate / "Cargo.toml").write_text(
        '[package]\nname = "printobserver"\nversion = "0.0.0"\nedition = "2021"\n',
        encoding="utf-8",
    )
    _says(root, says)
    (crate / "tests" / "walk.rs").write_text("#[test]\nfn walks() {}\n", encoding="utf-8")
    locked = run(["cargo", "generate-lockfile", "--offline"], cwd=root, timeout=300)
    equal(locked.returncode, 0, describing=f"the workspace's lockfile: {locked.stderr}")
    return root


def _says(root: Path, says: str) -> None:
    """Make the workspace's program print `says`."""
    (root / CRATE / "src" / "main.rs").write_text(
        f'fn main() {{\n    println!("{says}");\n}}\n', encoding="utf-8"
    )


def _prepare(root: Path, capsys: pytest.CaptureFixture[str]) -> tuple[Path, str]:
    """Run `release-program` over `root`: the program it named, and what it said."""
    capsys.readouterr()
    equal(main(["release-program", "--root", str(root)]), 0, describing="release-program")
    said = capsys.readouterr()
    return Path(said.out.strip()), said.err


def _answer(program: Path) -> str:
    """What a program prints."""
    ran = run([str(program)], timeout=60)
    equal(ran.returncode, 0, describing=f"{program} running: {ran.stderr}")
    return ran.stdout.strip()


def _forget_the_build(root: Path) -> None:
    """Remove cargo's own output, so only a kept program can answer without a build."""
    shutil.rmtree(root / "target")


@pytest.fixture(autouse=True)
def _cargo_builds_into_the_workspace(monkeypatch: pytest.MonkeyPatch) -> None:
    """Every build lands in the workspace's own `target`, under no inherited profile."""
    monkeypatch.delenv("CARGO_TARGET_DIR", raising=False)
    monkeypatch.delenv("CARGO_PROFILE_RELEASE_OPT_LEVEL", raising=False)


def test_an_unchanged_key_reuses_the_kept_program(
    tmp_path: Path, capsys: pytest.CaptureFixture[str]
) -> None:
    """Prepared twice over one tree, the second preparation builds nothing."""
    root = _workspace(tmp_path / "workspace", says="built from the first source")

    first, said_first = _prepare(root, capsys)
    contains(said_first, "built the release program", describing="the first preparation")
    _forget_the_build(root)
    # A change to a crate's own tests is no change to what a release build reads.
    (root / CRATE / "tests" / "walk.rs").write_text(
        "#[test]\nfn walks_further() {}\n", encoding="utf-8"
    )
    again, said_again = _prepare(root, capsys)

    contains(said_again, "reused the release program", describing="the second preparation")
    equal(again, first, describing="the program the second preparation named")
    truth(not (root / "target").exists(), describing="no build made by the second preparation")
    equal(_answer(again), "built from the first source", describing="the reused program")
    equal(
        again.parent.name,
        _key(root, capsys),
        describing="the key the program is kept under, as release-program-key prints it",
    )
    equal(again.name, platforms.host(Repo(root)).program, describing="the kept program's name")


@pytest.mark.parametrize("change", ["source", "toolchain", "profile"])
def test_a_changed_key_builds_again_rather_than_reusing(
    change: str,
    tmp_path: Path,
    capsys: pytest.CaptureFixture[str],
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    """Each of the three inputs, changed alone, makes the next preparation build."""
    root = _workspace(tmp_path / "workspace", says="built from the first source")
    first, _ = _prepare(root, capsys)
    key = _key(root, capsys)
    _forget_the_build(root)

    expected = "built from the first source"
    match change:
        case "source":
            expected = "built from the second source"
            _says(root, expected)
        case "toolchain":
            # The same installed compiler, declared without a component it
            # carried: a declaration that changed is a toolchain that changed.
            declared = root / "rust-toolchain.toml"
            text = declared.read_text(encoding="utf-8")
            truth('"llvm-tools-preview"' in text, describing="a component to drop")
            declared.write_text(text.replace(', "llvm-tools-preview"', ""), encoding="utf-8")
        case _:
            monkeypatch.setenv("CARGO_PROFILE_RELEASE_OPT_LEVEL", "1")
    second, said = _prepare(root, capsys)

    contains(said, "built the release program", describing=f"the preparation after a {change}")
    truth((root / "target").is_dir(), describing=f"a build made after a {change} change")
    truth(_key(root, capsys) != key, describing=f"a new key after a {change} change")
    truth(second != first, describing="a program kept under the new key")
    equal(_answer(second), expected, describing=f"the program built after a {change} change")
    equal(
        [entry.name for entry in (root / DIRECTORY).iterdir()],
        [second.parent.name],
        describing="the kept programs, which are only the newest",
    )


def test_a_build_that_fails_keeps_nothing_and_the_next_one_recovers(
    tmp_path: Path, capsys: pytest.CaptureFixture[str]
) -> None:
    """A source that does not compile is refused, and a fixed one is built and kept."""
    root = _workspace(tmp_path / "workspace", says="built from the first source")
    _prepare(root, capsys)
    (root / CRATE / "src" / "main.rs").write_text("fn main() { not rust }\n", encoding="utf-8")

    refused = main(["release-program", "--root", str(root)])
    said = capsys.readouterr()

    equal(refused, 1, describing="release-program over a source that does not compile")
    contains(said.err, "cargo build --release` failed", describing="what the refusal says")
    equal(said.out, "", describing="the program a failed preparation named")

    _says(root, "built from the mended source")
    mended, said_mended = _prepare(root, capsys)

    contains(said_mended, "built the release program", describing="the mended preparation")
    equal(_answer(mended), "built from the mended source", describing="the mended program")


def test_a_toolchain_that_cannot_say_what_it_is_is_refused(
    tmp_path: Path, capsys: pytest.CaptureFixture[str], monkeypatch: pytest.MonkeyPatch
) -> None:
    """No key is guessed for a compiler that does not answer `rustc -vV`."""
    root = tmp_path / "workspace"
    root.mkdir()
    monkeypatch.setenv("RUSTC", str(tmp_path / "no-compiler-here"))

    refused = main(["release-program-key", "--root", str(root)])
    said = capsys.readouterr()

    equal(refused, 1, describing="release-program-key with no compiler to ask")
    contains(said.err, "toolchain cannot be named", describing="what the refusal says")
    equal(said.out, "", describing="the key a refused run printed")


def _key(root: Path, capsys: pytest.CaptureFixture[str]) -> str:
    """The key `release-program-key` prints for `root`, without its `key=`."""
    capsys.readouterr()
    equal(main(["release-program-key", "--root", str(root)]), 0, describing="the key")
    line = capsys.readouterr().out.strip()
    truth(line.startswith("key="), describing=f"a `key=` line, not {line!r}")
    return line.removeprefix("key=")
