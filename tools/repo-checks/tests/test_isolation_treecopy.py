"""Copies taken by processes beside each other neither tear nor leak.

xdist runs this suite's tests in several processes at once, and Nx runs this
suite beside every other one, the adapter's suite among them, which rewrites a
checked-in schema on disk. So these journeys start real interpreters beside
each other, as those do, and read what each saw: `copying` is what they run.
"""

from __future__ import annotations

import json
import os
import subprocess
import sys
from pathlib import Path

import copying
from repo_checks.expect import accepted, contains, equal, refused, truth
from repo_checks.shell import run, start
from treecopy import REPO_ROOT

#: Where the processes import `copying`, `treecopy` and the checks from.
IMPORTS = os.pathsep.join(
    [str(Path(__file__).parent), str(REPO_ROOT / "tools" / "repo-checks" / "src")]
)

#: How long the rewriting and copying processes run beside each other.
SECONDS = 4.0

#: How many processes copy at once, as xdist workers would.
COPIERS = 3


def _launch(*argv: str) -> subprocess.Popen[str]:
    """Start one `copying` process, importing what this suite imports."""
    return start(
        [sys.executable, copying.__file__, *argv],
        env={**os.environ, "PYTHONPATH": IMPORTS},
    )


def _said(process: subprocess.Popen[str]) -> dict[str, object]:
    """What one process printed, once it has exited zero."""
    out, err = process.communicate(timeout=300)
    equal(process.returncode, 0, describing=f"a copying process's exit:\n{err}")
    said = json.loads(out.strip().splitlines()[-1])
    truth(isinstance(said, dict), describing=f"a copying process's report: {said!r}")
    return said


def _findings(value: object) -> list[str]:
    """A check's findings as one process reported them."""
    truth(
        isinstance(value, list) and all(isinstance(entry, str) for entry in value),
        describing=f"findings, not {value!r}",
    )
    return [str(entry) for entry in value] if isinstance(value, list) else []


def _source(root: Path) -> Path:
    """A repository holding one whole version of the rewritten schema, committed."""
    (root / copying.SCHEMA).parent.mkdir(parents=True)
    (root / copying.SCHEMA).write_text(copying.version(1), encoding="utf-8")
    (root / ".gitignore").write_text("target/\n", encoding="utf-8")
    run(["git", "init", "-q"], cwd=root, check=True)
    run(["git", "add", "-A"], cwd=root, check=True)
    return root


def test_copies_taken_while_another_process_rewrites_a_schema_are_never_torn(
    tmp_path: Path,
) -> None:
    """Every copy carries one whole version of a schema rewritten beside it in halves."""
    source = _source(tmp_path / "source")
    rewriting = _launch("rewrite", str(source), str(SECONDS))
    copiers = [
        _launch("copy", str(source), str(tmp_path / f"copier{n}"), str(SECONDS))
        for n in range(COPIERS)
    ]

    rewrites = _said(rewriting)["rewrites"]
    copied = [_said(copier) for copier in copiers]

    truth(isinstance(rewrites, int) and rewrites > 10, describing=f"{rewrites} rewrites")
    for n, said in enumerate(copied):
        copies = said["copies"]
        truth(isinstance(copies, int) and copies > 1, describing=f"copier {n}'s {copies} copies")
        equal(said["torn"], 0, describing=f"the torn schemas copier {n} carried: {said['first']}")
    final = (source / copying.SCHEMA).read_text(encoding="utf-8")
    truth(
        final in (copying.version(1), copying.version(2)),
        describing="the source schema to be whole once the rewriting is done",
    )


def test_a_defect_one_process_puts_in_its_copy_reaches_no_other(tmp_path: Path) -> None:
    """Each process's real check refuses its own defect, and only its own."""
    names = [f"worker{n}" for n in range(COPIERS)]
    workflow = (REPO_ROOT / copying.CI).read_text(encoding="utf-8")
    processes = {name: _launch("break", str(tmp_path / name), name) for name in names}

    said = {name: _said(process) for name, process in processes.items()}

    for name in names:
        broken = _findings(said[name]["broken"])
        refused(broken, f"verify-{name}")
        accepted(_findings(said[name]["unbroken"]), describing=f"{name}'s unbroken copy")
        contains(str(said[name]["workflow"]), f"just verify-{name}\n")
        for other in names:
            if other != name:
                truth(
                    f"verify-{other}" not in str(said[name]["workflow"])
                    and not any(f"verify-{other}" in finding for finding in broken),
                    describing=f"{other}'s defect to be absent from {name}'s copy",
                )
    equal(
        (REPO_ROOT / copying.CI).read_text(encoding="utf-8"),
        workflow,
        describing="the committed workflow, after every process broke its own copy",
    )
