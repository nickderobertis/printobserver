"""What this suite builds under the repository root, processes beside each other build safely.

xdist runs this suite in several processes at once, and Nx runs it beside the
end-to-end tier, whose PowerShell install journey compiles the same stand-in
program. Both used to write one fixed path under `target` — the stand-in's
source and program, and the packaged Rust client — so two builds beside each
other raced `rustc` and the linker for one file. These start real interpreters
beside each other, as those do, and read what each built.
"""

from __future__ import annotations

import os
import secrets
import subprocess
import sys
import tarfile
from pathlib import Path

from release_artifacts.stand_in import (
    RUST_STAND_IN,
    program_name,
    published,
    stand_in_program,
)
from repo_checks.expect import equal, truth
from repo_checks.model import Repo
from repo_checks.shell import run, start

REPO_ROOT = Path(__file__).resolve().parents[3]

#: How many processes build at once, as xdist workers of two suites would.
BUILDERS = 4

#: Where the processes import the artifact tool and the checks it is built on from.
IMPORTS = os.pathsep.join(
    str(REPO_ROOT / package / "src")
    for package in ("tools/release-artifacts", "tools/repo-checks", "tools/contract-codegen")
)

#: One process asking for one compiled stand-in, which it then runs.
COMPILE = """
import sys
from pathlib import Path
from release_artifacts.stand_in import stand_in_program
from repo_checks.model import Repo
from repo_checks.shell import run

program = stand_in_program(Repo(Path(sys.argv[1])), Path(sys.argv[2]), sys.argv[3], compiled=True)
answered = run([str(program), "--version"], cwd=program.parent, timeout=60)
print(answered.stdout.strip())
sys.exit(answered.returncode)
"""


def _launch(*argv: str) -> subprocess.Popen[str]:
    """Start one interpreter importing this repository's tools."""
    return start([sys.executable, *argv], env={**os.environ, "PYTHONPATH": IMPORTS})


def _finished(process: subprocess.Popen[str], describing: str) -> str:
    """What one process printed, once it has exited zero."""
    out, err = process.communicate(timeout=900)
    equal(process.returncode, 0, describing=f"{describing}'s exit:\n{out}{err}")
    return out


def test_processes_compiling_one_new_stand_in_at_once_each_get_the_whole_program(
    tmp_path: Path,
) -> None:
    """Built beside each other, the stand-in is published once, whole, and every copy runs."""
    answer = f"printobserver 0.0.0-{secrets.token_hex(6)}"
    shared = REPO_ROOT / "target" / "stand-in"
    before = set(shared.iterdir()) if shared.is_dir() else set()
    copies = [tmp_path / f"builder{n}" / "printobserver.exe" for n in range(BUILDERS)]

    builders = [_launch("-c", COMPILE, str(REPO_ROOT), str(copy), answer) for copy in copies]
    said = [_finished(builder, f"builder {n}") for n, builder in enumerate(builders)]

    equal(said, [f"{answer}\n"] * BUILDERS, describing="what each builder's copy answered")
    equal(
        len({copy.read_bytes() for copy in copies}),
        1,
        describing="the distinct programs the builders were handed",
    )
    added = set(shared.iterdir()) - before
    equal(len(added), 1, describing=f"what the builders left under target/stand-in: {added}")
    published_program = added.pop()
    truth(published_program.is_file(), describing="the one thing left to be the program")
    equal(
        published_program.read_bytes(),
        copies[0].read_bytes(),
        describing="the published program",
    )
    published_program.unlink()


def test_publishing_over_a_program_another_process_published_keeps_that_one(
    tmp_path: Path,
) -> None:
    """The second process to finish finds the name taken and leaves the first's program be."""
    first = tmp_path / "first"
    second = tmp_path / "second"
    first.write_bytes(b"the first process's program")
    second.write_bytes(b"the second process's program")
    program = tmp_path / "stand-in"

    published(first, program)
    published(second, program)

    equal(program.read_bytes(), b"the first process's program", describing="the program kept")


def test_a_stand_in_is_named_for_its_source_and_its_toolchain() -> None:
    """Change either and the program has a new name; change neither and it has the old one."""
    source = RUST_STAND_IN.format(answer="printobserver 1.0.0")
    toolchain = "rustc 1.97.1 (abc 2026-01-01)\nhost: x86_64-unknown-linux-gnu\n"

    named = program_name(source, toolchain)

    equal(program_name(source, toolchain), named, describing="the name asked for twice")
    truth(
        program_name(RUST_STAND_IN.format(answer="printobserver 1.0.1"), toolchain) != named,
        describing="a changed source to name a new program",
    )
    truth(
        program_name(source, toolchain.replace("1.97.1", "1.98.0")) != named,
        describing="a changed toolchain to name a new program",
    )


def test_a_program_already_published_for_this_source_and_toolchain_is_reused(
    tmp_path: Path,
) -> None:
    """A process finding the name taken copies what is there rather than compiling again."""
    answer = f"printobserver 0.0.0-{secrets.token_hex(6)}"
    toolchain = run(["rustc", "-vV"], cwd=REPO_ROOT, timeout=300).stdout
    shared = REPO_ROOT / "target" / "stand-in"
    shared.mkdir(parents=True, exist_ok=True)
    already = shared / program_name(RUST_STAND_IN.format(answer=answer), toolchain)
    already.write_bytes(b"what an earlier process published")
    try:
        handed = stand_in_program(
            Repo(REPO_ROOT), tmp_path / "printobserver.exe", answer, compiled=True
        )

        equal(handed.read_bytes(), b"what an earlier process published", describing="handed")
    finally:
        already.unlink()


def test_processes_packaging_the_rust_client_at_once_each_get_the_whole_crate(
    tmp_path: Path,
) -> None:
    """Packaged beside each other from one tree, every process gets the same crate."""
    intos = [tmp_path / f"packager{n}" for n in range(BUILDERS)]

    packagers = [
        _launch(
            "-m",
            "release_artifacts",
            "build",
            "--target",
            "crate:printobserver-sdk",
            "--root",
            str(REPO_ROOT),
            "--into",
            str(into),
        )
        for into in intos
    ]
    for n, packager in enumerate(packagers):
        _finished(packager, f"packager {n}")

    crates = [sorted(into.glob("*.crate")) for into in intos]
    equal([len(found) for found in crates], [1] * BUILDERS, describing="the crates each made")
    equal(
        len({found[0].read_bytes() for found in crates}),
        1,
        describing="the distinct crates the packagers made",
    )
    with tarfile.open(crates[0][0]) as crate:
        names = crate.getnames()
    truth(
        any(name.endswith("/Cargo.toml") for name in names),
        describing=f"the crate to carry its manifest: {names}",
    )
