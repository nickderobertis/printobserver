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
import sys
from pathlib import Path
from typing import NamedTuple

import pytest
from release_artifacts.__main__ import main
from release_artifacts.reused import DIGEST_FILE, DIRECTORY, IGNORE_FILE, KEY_FIELD
from repo_checks import platforms
from repo_checks.expect import contains, equal, truth
from repo_checks.model import Repo
from repo_checks.parsing import jobs_of, load_workflow, steps_of
from repo_checks.shell import run

REPO_ROOT = Path(__file__).resolve().parents[3]

#: The crate the program is built from, as `release_artifacts.build` names it.
CRATE = "crates/printobserver"

#: What the program prints when nothing about its source has changed.
FIRST = "built from the first source"

#: The line a compiler of this suite's own adds to the real one's `-vV` answer.
OTHER_COMPILER = "printobserver-suite: the same compiler, built again"

#: Every input a preparation is keyed on, each changed alone by one journey.
CHANGES = [
    "source",
    "toolchain",
    "compiler",
    "manifest-profile",
    "config-profile",
    "config-build-flags",
    "config-target-flags",
    "RUSTFLAGS",
    "CARGO_ENCODED_RUSTFLAGS",
    "CARGO_BUILD_RUSTFLAGS",
    "CARGO_PROFILE_RELEASE_OPT_LEVEL",
    "target-flags",
]

#: The variable cases above, and what each is set to: the same flag in each
#: one's own spelling, and a profile setting.
VARIABLES = {
    "RUSTFLAGS": "-C overflow-checks=on",
    "CARGO_ENCODED_RUSTFLAGS": "-C\x1foverflow-checks=on",
    "CARGO_BUILD_RUSTFLAGS": "-C overflow-checks=on",
    "CARGO_PROFILE_RELEASE_OPT_LEVEL": "1",
}


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
    (root / CRATE / "src" / "main.rs").write_text(
        f'fn main() {{\n    println!("{says}");\n    eprintln!("{says}");\n}}\n', encoding="utf-8"
    )


def _other_compiler(root: Path, into: Path) -> Path:
    """A compiler that is the real one, built again under another identity.

    It hands every invocation to the toolchain's own `rustc` and adds one line
    to its `-vV` answer, which is what a compiler built from other sources says
    about itself: cargo builds with it, and its version answer is not the one
    the kept program was built under.
    """
    sysroot = run(["rustc", "--print", "sysroot"], cwd=root, timeout=300)
    equal(sysroot.returncode, 0, describing=f"the toolchain's sysroot: {sysroot.stderr}")
    real = Path(sysroot.stdout.strip()) / "bin" / f"rustc{_suffix()}"
    (into / "src").mkdir(parents=True)
    (into / "Cargo.toml").write_text(
        '[package]\nname = "compiler"\nversion = "0.0.0"\nedition = "2021"\n\n[workspace]\n',
        encoding="utf-8",
    )
    (into / "src" / "main.rs").write_text(
        f'const REAL: &str = r#"{real}"#;\n'
        "fn main() {\n"
        "    let args: Vec<String> = std::env::args().skip(1).collect();\n"
        '    if args == ["-vV"] {\n'
        "        let out = std::process::Command::new(REAL).args(&args).output().unwrap();\n"
        '        print!("{}", String::from_utf8_lossy(&out.stdout));\n'
        f'        println!("{OTHER_COMPILER}");\n'
        "        std::process::exit(out.status.code().unwrap_or(1));\n"
        "    }\n"
        "    let status = std::process::Command::new(REAL).args(&args).status().unwrap();\n"
        "    std::process::exit(status.code().unwrap_or(1));\n"
        "}\n",
        encoding="utf-8",
    )
    built = run(["cargo", "build", "--quiet"], cwd=into, timeout=600)
    equal(built.returncode, 0, describing=f"the other compiler's build: {built.stderr}")
    return into / "target" / "debug" / f"compiler{_suffix()}"


def _host(root: Path) -> str:
    """This host's own target, as the compiler names it."""
    answered = run(["rustc", "-vV"], cwd=root, timeout=300)
    equal(answered.returncode, 0, describing=f"the compiler's answer: {answered.stderr}")
    hosts = [
        line.removeprefix("host: ")
        for line in answered.stdout.splitlines()
        if line.startswith("host: ")
    ]
    equal(len(hosts), 1, describing=f"the host line of the compiler's answer: {answered.stdout}")
    truth(bool(hosts[0].strip()), describing=f"a host the compiler names: {answered.stdout}")
    return hosts[0].strip()


def _target_flags(root: Path) -> str:
    """The variable cargo reads this host's own target's compiler flags from."""
    return f"CARGO_TARGET_{_host(root).upper().replace('-', '_')}_RUSTFLAGS"


def _suffix() -> str:
    """What this host's programs are called after their name."""
    return ".exe" if sys.platform == "win32" else ""


class Preparation(NamedTuple):
    """What one `release-program` run answered."""

    #: The program it named.
    program: Path
    #: How it got it: `built` or `reused`.
    how: str


def _prepare(root: Path, capsys: pytest.CaptureFixture[str]) -> Preparation:
    """Run `release-program` over `root`, reading the one line it answers."""
    capsys.readouterr()
    equal(main(["release-program", "--root", str(root)]), 0, describing="release-program")
    said = capsys.readouterr()
    equal(said.err, "", describing="what a successful preparation says on standard error")
    lines = said.out.splitlines()
    equal(len(lines), 1, describing=f"one line from a successful preparation, not {lines!r}")
    how, _, path = lines[0].partition(" ")
    contains({"built", "reused"}, how, describing=f"how the program was got, in {lines[0]!r}")
    return Preparation(Path(path), how)


def _answer(program: Path) -> str:
    ran = run([str(program)], timeout=60)
    equal(ran.returncode, 0, describing=f"{program} running: {ran.stderr}")
    return ran.stdout.strip()


def _forget_the_build(root: Path) -> None:
    """Remove cargo's own output, so only a kept program can answer without a build."""
    shutil.rmtree(root / "target")


def _key(root: Path, capsys: pytest.CaptureFixture[str]) -> str:
    """The key `release-program-key` prints for `root`, as its one line."""
    capsys.readouterr()
    equal(main(["release-program-key", "--root", str(root)]), 0, describing="the key")
    lines = capsys.readouterr().out.splitlines()
    equal(len(lines), 1, describing=f"one `key=` line, not {lines!r}")
    truth(lines[0].startswith(f"{KEY_FIELD}="), describing=f"a `key=` line, not {lines[0]!r}")
    return lines[0].removeprefix(f"{KEY_FIELD}=")


@pytest.fixture(autouse=True)
def _cargo_builds_into_the_workspace(monkeypatch: pytest.MonkeyPatch) -> None:
    """Every build lands in the workspace's own `target`, under no inherited profile."""
    for name in ("CARGO_TARGET_DIR", "RUSTC", *VARIABLES):
        monkeypatch.delenv(name, raising=False)


def test_an_unchanged_key_reuses_the_kept_program(
    tmp_path: Path, capsys: pytest.CaptureFixture[str]
) -> None:
    """Prepared twice over one tree, the second preparation builds nothing."""
    root = _workspace(tmp_path / "workspace", says=FIRST)

    first, said_first = _prepare(root, capsys)
    equal(said_first, "built", describing="the first preparation")
    _forget_the_build(root)
    # A change to a crate's own tests is no change to what a release build reads.
    (root / CRATE / "tests" / "walk.rs").write_text(
        "#[test]\nfn walks_further() {}\n", encoding="utf-8"
    )
    again, said_again = _prepare(root, capsys)

    equal(said_again, "reused", describing="the second preparation")
    equal(again, first, describing="the program the second preparation named")
    truth(not (root / "target").exists(), describing="no build made by the second preparation")
    equal(_answer(again), FIRST, describing="the reused program")
    equal(
        again.parent.name,
        _key(root, capsys),
        describing="the key the program is kept under, as release-program-key prints it",
    )
    equal(again.name, platforms.host(Repo(root)).program, describing="the kept program's name")
    equal(
        (root / DIRECTORY / IGNORE_FILE).read_text(encoding="utf-8"),
        "*\n",
        describing="the kept directory's own ignore file, which ignores all of it",
    )


@pytest.mark.parametrize("change", CHANGES)
def test_a_changed_key_builds_again_rather_than_reusing(
    change: str,
    tmp_path: Path,
    capsys: pytest.CaptureFixture[str],
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    """Each input, changed alone, makes the next preparation build."""
    root = _workspace(tmp_path / "workspace", says=FIRST)
    first, _ = _prepare(root, capsys)
    key = _key(root, capsys)
    _forget_the_build(root)

    expected = FIRST
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
        case "compiler":
            monkeypatch.setenv("RUSTC", str(_other_compiler(root, tmp_path / "compiler")))
        case "manifest-profile":
            with (root / "Cargo.toml").open("a", encoding="utf-8") as manifest:
                manifest.write("\n[profile.release]\nopt-level = 1\n")
        case "config-profile":
            (root / ".cargo").mkdir()
            (root / ".cargo" / "config.toml").write_text(
                "[profile.release]\nopt-level = 1\n", encoding="utf-8"
            )
        case "config-build-flags":
            (root / ".cargo").mkdir()
            (root / ".cargo" / "config.toml").write_text(
                '[build]\nrustflags = ["-C", "overflow-checks=on"]\n', encoding="utf-8"
            )
        case "config-target-flags":
            (root / ".cargo").mkdir()
            (root / ".cargo" / "config.toml").write_text(
                f'[target.{_host(root)}]\nrustflags = ["-C", "overflow-checks=on"]\n',
                encoding="utf-8",
            )
        case "target-flags":
            monkeypatch.setenv(_target_flags(root), "-C overflow-checks=on")
        case variable:
            monkeypatch.setenv(variable, VARIABLES[variable])
    second, said = _prepare(root, capsys)

    equal(said, "built", describing=f"the preparation after a {change}")
    truth((root / "target").is_dir(), describing=f"a build made after a {change} change")
    truth(_key(root, capsys) != key, describing=f"a new key after a {change} change")
    truth(second != first, describing="a program kept under the new key")
    equal(_answer(second), expected, describing=f"the program built after a {change} change")
    equal(
        sorted(entry.name for entry in (root / DIRECTORY).iterdir()),
        sorted([IGNORE_FILE, second.parent.name]),
        describing="what the kept directory holds: only the newest program, and its ignore file",
    )


@pytest.mark.parametrize("damage", ["missing", "truncated", "unrecorded"])
def test_a_kept_program_that_is_not_what_was_kept_is_built_over(
    damage: str, tmp_path: Path, capsys: pytest.CaptureFixture[str]
) -> None:
    """A restored program whose bytes are not the ones kept is never run.

    The kept directory comes back from a cache rather than from this tool, so
    a program gone from beside its record, one cut short, or one with no record
    of what it was kept as is not the program the key names: the key still
    holds and it is built over anyway.
    """
    root = _workspace(tmp_path / "workspace", says=FIRST)
    first, _ = _prepare(root, capsys)
    _forget_the_build(root)
    match damage:
        case "missing":
            first.unlink()
        case "truncated":
            first.write_bytes(first.read_bytes()[:64])
        case _:
            (first.parent / DIGEST_FILE).unlink()

    second, said = _prepare(root, capsys)

    equal(said, "built", describing=f"the preparation over a {damage} one")
    truth((root / "target").is_dir(), describing=f"a build made over a {damage} program")
    equal(second, first, describing="the program kept again under the same key")
    equal(_answer(second), FIRST, describing="the program built over the damaged one")
    truth((second.parent / DIGEST_FILE).is_file(), describing="the digest kept beside it again")


def test_a_build_whose_inputs_move_under_it_is_used_once_and_kept_under_no_key(
    tmp_path: Path, capsys: pytest.CaptureFixture[str]
) -> None:
    """A source the build itself changes is never one a kept program is reused for.

    The crate's build script writes a file into the crate on every build, as a
    generator writing into its own sources does: what was built is of neither
    the key taken before the build nor the one after it.
    """
    root = _workspace(tmp_path / "workspace", says=FIRST)
    (root / CRATE / "build.rs").write_text(
        "fn main() {\n"
        '    let crate_dir = std::env::var("CARGO_MANIFEST_DIR").unwrap();\n'
        "    let now = std::time::SystemTime::now()\n"
        "        .duration_since(std::time::UNIX_EPOCH)\n"
        "        .unwrap()\n"
        "        .as_nanos();\n"
        '    std::fs::write(format!("{crate_dir}/stamp.txt"), now.to_string()).unwrap();\n'
        "}\n",
        encoding="utf-8",
    )

    first, said_first = _prepare(root, capsys)
    second, said_second = _prepare(root, capsys)

    for program, said in ((first, said_first), (second, said_second)):
        equal(said, "built", describing="a preparation over moving inputs")
        equal(program.parent, root / "target" / "release", describing="where that program is")
        equal(_answer(program), FIRST, describing="the program built over moving inputs")
    truth(not (root / DIRECTORY).exists(), describing="no program kept for moving inputs")


def test_a_build_that_fails_is_refused_and_the_next_one_recovers(
    tmp_path: Path, capsys: pytest.CaptureFixture[str]
) -> None:
    """A source that does not compile is refused, and a fixed one is built and kept."""
    root = _workspace(tmp_path / "workspace", says=FIRST)
    _prepare(root, capsys)
    (root / CRATE / "src" / "main.rs").write_text("fn main() { not rust }\n", encoding="utf-8")

    refused = main(["release-program", "--root", str(root)])
    said = capsys.readouterr()

    equal(refused, 1, describing="release-program over a source that does not compile")
    contains(said.err, "cargo build --release` failed", describing="what the refusal says")
    contains(said.err, "run `just release-program` again", describing="the next action it names")
    equal(said.out, "", describing="the program a failed preparation named")

    _says(root, "built from the mended source")
    mended, said_mended = _prepare(root, capsys)

    equal(said_mended, "built", describing="the mended preparation")
    equal(_answer(mended), "built from the mended source", describing="the mended program")


@pytest.mark.parametrize(
    ("compiler", "naming"),
    [("absent", "`rustc -vV` failed"), ("no-version", "answered no compiler version")],
)
def test_a_toolchain_that_cannot_say_what_it_is_is_refused(
    compiler: str,
    naming: str,
    tmp_path: Path,
    capsys: pytest.CaptureFixture[str],
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    """No key is guessed for a compiler that does not answer `rustc -vV` with a version."""
    root = tmp_path / "workspace"
    root.mkdir()
    # Cargo answers `-vV` and exits zero, with a version that is not a compiler's.
    asked = tmp_path / "no-compiler-here" if compiler == "absent" else shutil.which("cargo")
    monkeypatch.setenv("RUSTC", str(asked))

    refused = main(["release-program-key", "--root", str(root)])
    said = capsys.readouterr()

    equal(refused, 1, describing=f"release-program-key with a compiler that is {compiler}")
    contains(said.err, naming, describing="what the refusal says")
    contains(said.err, "toolchain cannot be named", describing="what the refusal says")
    contains(said.err, "rustup toolchain install", describing="the next action it names")
    equal(said.out, "", describing="the key a refused run printed")


def test_the_integration_job_caches_the_directory_under_the_key_the_command_prints(
    capsys: pytest.CaptureFixture[str],
) -> None:
    """The committed workflow caches the kept directory under the key the command prints.

    The job appends the command's line to `GITHUB_OUTPUT`, caches the directory
    this tool keeps the program in under that key, then prepares the program
    before the tier. A directory or field renamed on either side, or a
    preparation moved after the tier, would leave the tier building it again.
    """
    workflow = load_workflow(REPO_ROOT / ".github" / "workflows" / "ci.yml")
    steps = steps_of(jobs_of(workflow)["integration"])
    runs = [str(step.get("run", "")).strip() for step in steps]
    keyed = runs.index('just release-program-key >> "$GITHUB_OUTPUT"')
    outputs = f"steps.{steps[keyed]['id']}.outputs"
    cached = next(step for step in steps if str(step.get("uses", "")).startswith("actions/cache@"))

    equal(cached["with"]["path"], DIRECTORY, describing="the directory the job caches")
    contains(
        cached["with"]["key"],
        f"${{{{ {outputs}.{KEY_FIELD} }}}}",
        describing="the key the job caches it under",
    )
    truth(
        keyed
        < steps.index(cached)
        < runs.index("just release-program")
        < runs.index("just test-integration"),
        describing=f"the key, the cache, the preparation and the tier in that order: {runs}",
    )
    _key(REPO_ROOT, capsys)


def test_the_world_command_carries_the_kept_program_without_building(
    tmp_path: Path, capsys: pytest.CaptureFixture[str]
) -> None:
    """A world asked for no particular program runs the kept one, and builds nothing.

    This suite's program is no supervisor, so the world refuses it — and its
    refusal quotes what the program said, which is the source the kept program
    was built from. With cargo's own output gone first, a world that built
    rather than reused would have left a `target` behind.
    """
    root = _workspace(tmp_path / "workspace", says=FIRST)
    _prepare(root, capsys)
    _forget_the_build(root)

    refused = main(["world", "--root", str(root), "--into", str(tmp_path / "world")])
    said = capsys.readouterr()

    equal(refused, 1, describing="a world whose program is no supervisor")
    contains(said.err, "stopped before it answered", describing="what the world says of it")
    contains(said.err, FIRST, describing="what the program the world ran said")
    truth(not (root / "target").exists(), describing="no build made for the world")
