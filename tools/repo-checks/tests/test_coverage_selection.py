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

import tomllib
from collections.abc import Iterator

import pytest
from repo_checks.checks_graph import RUST, platform_independent_tag, projects
from repo_checks.commands import Measured, measured_for
from repo_checks.expect import equal, truth
from repo_checks.gate_tier import PROJECTS_VARIABLE
from repo_checks.model import Repo
from repo_checks.parsing import recipes
from treecopy import REPO_ROOT

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
