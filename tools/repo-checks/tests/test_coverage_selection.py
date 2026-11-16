"""The coverage the CI jobs measure, together, is the whole gate's.

`just ci-lint` and `just ci-test` each run `just coverage` over one side of the
`platform:independent` tag, which `repo_checks.commands.measured_for` reads
from `PRINTOBSERVER_GATE_PROJECTS`. The base's whole-graph gate ran one report
over every project. These hold, through the real selection over the committed
graph, that the two sides' reports are over every crate and every Python source
that one report was, each on exactly one side; that a side's report differs
from the whole one only by which of those sources it includes, so the omit and
exclusion settings `pyproject.toml` declares apply alike; and that both run
under the floors `repo-policy.toml` records, which the reports enforce.
"""

from __future__ import annotations

import os
import shutil
import sys
import tomllib
from collections.abc import Callable, Iterator
from pathlib import Path

import pytest
from repo_checks.checks_graph import RUST, platform_independent_tag, projects
from repo_checks.commands import LINT_DATA, Measured, coverage_combine, measured_for
from repo_checks.expect import contains, equal, truth
from repo_checks.gate_tier import PROJECTS_VARIABLE
from repo_checks.model import Repo
from repo_checks.parsing import recipes
from repo_checks.shell import run
from treecopy import REPO_ROOT, Closure, copy_tree

REPO = Repo(REPO_ROOT)
TAG = platform_independent_tag(REPO)
SIDES = (TAG, f"!{TAG}")


@pytest.fixture
def side(monkeypatch: pytest.MonkeyPatch) -> Iterator[None]:
    """Each test names its side itself; none leaks in from the job running this suite."""
    monkeypatch.delenv(PROJECTS_VARIABLE, raising=False)
    yield


def measured_on(monkeypatch: pytest.MonkeyPatch, named: str | None) -> Measured:
    """What `just coverage all` is over with `named` as the side, or over every project."""
    if named is None:
        monkeypatch.delenv(PROJECTS_VARIABLE, raising=False)
    else:
        monkeypatch.setenv(PROJECTS_VARIABLE, named)
    return measured_for(REPO, "all")


def crates_reported(measured: Measured) -> set[str]:
    """The crates a report covers: every crate but those its ignore pattern leaves out."""
    every = {f"crates/{crate.name}" for crate in REPO.crate_dirs}
    if measured.rust is None:
        return every
    return every & set(measured.rust)


def python_reported(measured: Measured) -> set[str]:
    """The Python source directories a report covers."""
    every = set(
        tomllib.loads((REPO_ROOT / "pyproject.toml").read_text(encoding="utf-8"))["tool"][
            "coverage"
        ]["run"]["source"]
    )
    if measured.python is None:
        return every
    return set(measured.python)


@pytest.mark.usefixtures("side")
def test_the_whole_graphs_report_is_over_every_crate_and_source(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    """The base's one report, with no side named, is the reference the sides are held to."""
    whole = measured_on(monkeypatch, None)

    equal((whole.rust, whole.python), (None, None), describing="the whole report's scope")
    truth(crates_reported(whole), describing="crates to report")
    truth(python_reported(whole), describing="Python sources to report")


@pytest.mark.usefixtures("side")
def test_the_two_sides_report_every_crate_and_source_once(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    """Every crate and source the whole report covers is on exactly one side's report."""
    whole = measured_on(monkeypatch, None)
    lint, test = (measured_on(monkeypatch, named) for named in SIDES)

    equal(crates_reported(lint) | crates_reported(test), crates_reported(whole))
    equal(crates_reported(lint) & crates_reported(test), set(), describing="crates on both")
    equal(python_reported(lint) | python_reported(test), python_reported(whole))
    equal(python_reported(lint) & python_reported(test), set(), describing="sources on both")


@pytest.mark.usefixtures("side")
def test_each_source_is_reported_on_the_side_of_the_project_holding_it(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    """A crate is the dependent side's and a source its own project's side, by the tag."""
    graph = projects(REPO)
    tagged_roots = [project.root for project in graph if TAG in project.tags]
    lint = measured_on(monkeypatch, TAG)
    for source in python_reported(lint):
        truth(
            any(source.startswith(f"{root}/") for root in tagged_roots),
            describing=f"`{source}` under a tagged project",
        )
    truth(
        not any(TAG in project.tags for project in graph if RUST in project.tags),
        describing="no crate tagged, so every crate's coverage is measured on every platform",
    )
    equal(crates_reported(lint), set(), describing="crates on the lint side")


@pytest.mark.usefixtures("side")
def test_a_sides_report_differs_from_the_whole_one_by_its_includes_alone(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    """The omit and exclusion settings are `pyproject.toml`'s for every report alike.

    A side narrows the Python report by `--include` and the Rust one by an
    ignore pattern naming the other side's crates, and by nothing else: no
    side's selection carries an omit of its own.
    """
    for named in SIDES:
        measured = measured_on(monkeypatch, named)
        included = measured.python_included()
        if included is not None:
            equal(
                set(included.split(",")),
                {f"{source}/*" for source in python_reported(measured)},
                describing=f"the {named} side's `--include`",
            )
        ignored = measured.rust_ignored(REPO)
        if ignored is not None:
            for crate in crates_reported(measured):
                truth(
                    crate.removeprefix("crates/") not in ignored,
                    describing=f"`{crate}` reported on the {named} side",
                )


def test_the_floors_are_the_repositorys_and_both_halves_run_the_coverage_recipe() -> None:
    """95% for every ecosystem, and each half's measured tiers end with `just coverage`."""
    equal(
        REPO.policy["gate"]["coverage"]["rust"],
        95,
        describing="the Rust floor `just coverage` enforces",
    )
    equal(REPO.policy["gate"]["coverage"]["python"], 95, describing="the Python floor")
    equal(REPO.policy["gate"]["coverage"]["typescript"], 95, describing="the TypeScript floor")
    declared = recipes(REPO.justfile)
    truth(
        any(line.strip().startswith("just coverage ") for line in declared["ci-measured"].body),
        describing="`ci-measured` running the coverage recipe after the test tier",
    )
    truth(
        any("python -m repo_checks coverage --tier" in line for line in declared["coverage"].body),
        describing="the coverage recipe reporting through `repo_checks coverage` at the tier",
    )


#: What a copy carries for a combined report: the coverage configuration, the
#: floors, and every source it reports over.
REPORTED = Closure(
    (
        "pyproject.toml",
        "repo-policy.toml",
        "tools/repo-checks/src/",
        "tools/contract-codegen/src/",
        "tools/release-artifacts/src/",
        "python/printobserver-sdk/src/",
    )
)


def kept(tree: Path, name: str, module: str) -> Path:
    """The data one CI half keeps, measured by really importing `module` under coverage.

    The run is configured by the copy's own `pyproject.toml`, as a job's tests
    are, and combined into the one file the job uploads.
    """
    scratch = tree.parent / f"half-{name}"
    scratch.mkdir()
    script = scratch / "half.py"
    script.write_text(f"import {module}\n", encoding="utf-8")
    environment = {**os.environ, "PYTHONPATH": str(tree / "tools/repo-checks/src")}
    for argv in (
        ["run", f"--data-file={scratch / 'data'}", str(script)],
        ["combine", f"--data-file={scratch / 'data'}", str(scratch)],
    ):
        done = run([sys.executable, "-m", "coverage", *argv], cwd=tree, env=environment)
        equal(done.returncode, 0, describing=f"coverage {argv[0]}: {done.stdout}{done.stderr}")
    into = tree / "coverage-data" / name
    into.parent.mkdir(exist_ok=True)
    shutil.copyfile(scratch / "data", into)
    return into


def floor_at(tree: Path, floor: int) -> None:
    """The copy's Python floor set to `floor`."""
    policy = tree / "repo-policy.toml"
    text = policy.read_text(encoding="utf-8")
    truth("python = 95\n" in text, describing="the committed Python floor")
    policy.write_text(text.replace("python = 95\n", f"python = {floor}\n"), encoding="utf-8")


def percent(report: str, source: str) -> int:
    """A file's figure in a printed report: its one column ending in `%`."""
    line = next(line for line in report.splitlines() if line.startswith(source))
    return int(next(word for word in line.split() if word.endswith("%")).rstrip("%"))


@pytest.fixture
def halves(tmp_path: Path) -> Path:
    """A copy whose `lint` half imported one module and whose Linux half another."""
    tree = copy_tree(tmp_path / "tree", closure=REPORTED)
    kept(tree, LINT_DATA, "repo_checks.expect")
    kept(tree, "linux-x86_64", "repo_checks.loopback")
    return tree


def test_each_platform_is_ruled_on_over_both_halves_combined(
    halves: Path, capsys: pytest.CaptureFixture[str]
) -> None:
    """The combined report carries what each half measured, and rules at the policy's floor."""
    floor_at(halves, 0)

    equal(coverage_combine(Repo(halves), halves / "coverage-data"), 0)

    said = capsys.readouterr().out
    contains(said, "coverage-combine: linux-x86_64 with lint")
    truth(
        percent(said, "tools/repo-checks/src/repo_checks/expect.py") > 0
        and percent(said, "tools/repo-checks/src/repo_checks/loopback.py") > 0,
        describing=f"both halves' modules measured in the one report:\n{said}",
    )
    contains(said, "coverage-combine: python lines linux-x86_64 ")


def test_a_platform_below_the_floor_fails_naming_it(
    halves: Path, capsys: pytest.CaptureFixture[str]
) -> None:
    """At the committed 95%, two imports cover far too little, and the platform is named."""
    equal(coverage_combine(Repo(halves), halves / "coverage-data"), 1)

    contains(capsys.readouterr().err, "Python line coverage on linux-x86_64 is below the 95% floor")


@pytest.mark.parametrize(
    ("arrange", "naming"),
    [
        pytest.param(
            lambda data: (data / LINT_DATA).unlink(), "no `lint` data under", id="no-lint-half"
        ),
        pytest.param(
            lambda data: (data / "linux-x86_64").unlink(), "no platform's data", id="no-platform"
        ),
        pytest.param(
            lambda data: (data / "linux-x86_64").rename(data / "solaris-sparc"),
            "`solaris-sparc` under",
            id="no-such-platform",
        ),
    ],
)
def test_a_directory_missing_a_half_is_refused(
    halves: Path,
    capsys: pytest.CaptureFixture[str],
    arrange: Callable[[Path], object],
    naming: str,
) -> None:
    """A combined report missing either half, or naming no platform, rules on nothing."""
    floor_at(halves, 0)
    arrange(halves / "coverage-data")

    equal(coverage_combine(Repo(halves), halves / "coverage-data"), 1)

    contains(capsys.readouterr().err, naming)
