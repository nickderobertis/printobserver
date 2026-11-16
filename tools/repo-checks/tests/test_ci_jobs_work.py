"""`ci.yml`'s `lint` and `test` jobs, run step by step, do the whole gate's work once each.

Each job's own `run:` steps are taken from the committed `ci.yml`, with that
job's matrix cell and environment, and run in order under bash in a copy of the
tree, through the real `just` and the real recipes, the real tier selection
(`repo_checks.gate_tier`) and the real tool lookups (`repo_checks tool-version`)
beneath them. Only the programs those recipes launch to build, test, check or
install are doubled — Nx (`bunx`), `bun`, `cargo` and `uv` for everything but the
two modules above, and, because they would reach another host, `pwsh`, `npm`
and `llmlint` — each double recording what it was asked to run. An `nx run-many`
or `nx affected` it was asked becomes the concrete `target:project` tasks the
graph's targets give it.

The reference is what the base branch's whole-graph gate and integration tier
ran on every platform: `just check all` and the integration tier's three
recipes, in the same copy. The copy carries no history, so the affected tier
fails closed to the whole graph, which is the change the line is measured on.
What is held:

* on every platform, `lint` and that platform's `test` job together run every
  unit of the reference, and each exactly once;
* the projects tagged `platform:independent` have their `test` and `test-e2e`
  run in `lint` alone, and every other project's in each `test` job alone;
* no Nx task runs twice in one job, and `lint`'s own steps — the title, the
  supply chain, the skill install and the judged lint — run there once and in no
  `test` job.

A step dropped or changed, and work omitted or doubled, are each made in a copy
of the workflow and found.
"""

from __future__ import annotations

import json
import os
import re
import sys
from collections import Counter
from collections.abc import Callable, Iterator, Mapping
from dataclasses import dataclass
from pathlib import Path
from typing import Any

import pytest
from repo_checks.checks_graph import platform_independent_tag, projects
from repo_checks.expect import equal, truth
from repo_checks.model import Repo
from repo_checks.parsing import jobs_of, load_workflow, steps_of
from repo_checks.shell import run
from treecopy import Closure, copy_tree

CI = ".github/workflows/ci.yml"
#: The jobs whose steps are the gate's two halves: `lint` once, and each
#: platform's `test` cell — the macOS cell being the job `test-macos`.
LINT_JOB = "lint"
TEST_JOB = "test"
MACOS_JOB = "test-macos"
MACOS = "macos-aarch64"

#: The programs doubled, each recording what it was asked to run.
DOUBLED = ("bun", "bunx", "cargo", "uv", "pwsh", "npm", "llmlint")
#: The modules the `uv` double runs rather than records: the tier selection
#: and the tool lookup, which read the tree and launch only doubled programs.
PASSED_THROUGH = (("repo_checks.gate_tier",), ("repo_checks", "tool-version"))
#: The variables the doubles read: where to record, and which job and step ran them.
RECORD, STEP = "CI_WORK_RECORD", "CI_WORK_STEP"
#: The variable a side of the project tag is handed in, as `gate_tier` reads it.
SIDE = "PRINTOBSERVER_GATE_PROJECTS"

#: Leaf runs that prepare a runner rather than do the gate's work: every job
#: repeats them for its own runner, so they are recorded and not compared.
PREPARATION = (
    ("bun", "install"),
    ("cargo", "fetch"),
    ("cargo", "install"),
    ("cargo", "llvm-cov", "clean"),
    ("uv", "sync"),
    ("uv", "run", "-q", "coverage", "erase"),
    ("uv", "run", "-q", "python", "-m", "repo_checks", "check-justfile"),
    ("uv", "run", "-q", "python", "-m", "repo_checks", "install-tools"),
    ("uv", "run", "-q", "python", "-m", "repo_checks", "install-hooks"),
    ("uv", "run", "-q", "python", "-m", "release_artifacts", "release-program-key"),
    ("uv", "run", "-q", "python", "-m", "release_artifacts", "release-program"),
    ("pwsh",),
    # The judged lint's setup: its own install and the harness it judges through.
    ("uv", "tool", "install"),
    ("llmlint", "--version"),
    ("llmlint", "doctor"),
    ("npm", "install"),
)
#: `lint`'s own steps, beside its half of the gate: each runs there once and in
#: no `test` job.
LINT_OWN = (
    ("uv", "run", "-q", "python", "-m", "repo_checks", "pr-title"),
    ("cargo", "deny"),
    ("cargo", "machete"),
    ("uv", "run", "-q", "python", "-m", "repo_checks", "install-gh"),
    ("nx", "test-skill-install"),
    ("llmlint", "validate"),
    ("llmlint", "--diff"),
)
#: The integration tier's own recipes, as the reference runs them.
INTEGRATION = ("octoprint-up", "test-integration", "octoprint-down")

#: What a copy for these runs carries: the recipes, the policy, the graph, the
#: workflows, the tool packages the recipes put on `PYTHONPATH`, and the one
#: script a step runs.
CLOSURE = Closure(
    (
        "justfile",
        "repo-policy.toml",
        "AGENTS.md",
        "pyproject.toml",
        "release-plz.toml",
        "nx.json",
        "rust-toolchain.toml",
        "**/project.json",
        ".github/workflows/",
        "tools/repo-checks/src/",
        "tools/contract-codegen/src/",
        "tools/release-artifacts/src/",
        "scripts/setup-llmlint.sh",
    )
)

DOUBLE = f"""
import json, os, subprocess, sys
name = os.path.splitext(os.path.basename(sys.argv[0]))[0]
argv = sys.argv[1:]
if name == "uv" and argv[:4] == ["run", "-q", "python", "-m"]:
    for module in {PASSED_THROUGH!r}:
        if tuple(argv[4 : 4 + len(module)]) == module:
            sys.exit(subprocess.run([sys.executable, "-m", *argv[4:]]).returncode)
with open(os.environ[{RECORD!r}], "a", encoding="utf-8") as record:
    record.write(json.dumps({{
        "argv": [name, *argv],
        "step": os.environ.get({STEP!r}, ""),
        "side": os.environ.get({SIDE!r}, ""),
    }}) + "\\n")
"""

#: The `${{ }}` expressions a step of these jobs carries, and what each renders
#: to for a pull request: a matrix value, an earlier step's output, the title,
#: and the two credentials, which no run here has.
EXPRESSION = re.compile(r"\$\{\{\s*(?P<inner>.*?)\s*\}\}")


def program(directory: Path, name: str, code: str) -> None:
    """Put a program `name` running the Python `code` in `directory`."""
    written = directory / name
    written.write_text(f"#!{sys.executable}\n{code}", encoding="utf-8")
    written.chmod(0o755)


@dataclass(frozen=True)
class Cell:
    """One job instance to run: the job, and the matrix cell it ran as."""

    job: str
    platform: str
    matrix: Mapping[str, str]


def cells(workflow: dict[str, Any]) -> list[Cell]:
    """`lint`, each `test` cell and the macOS `test` job, as the workflow declares them."""
    jobs = jobs_of(workflow)
    tested = [
        Cell(TEST_JOB, str(entry["id"]), {str(k): str(v) for k, v in entry.items()})
        for entry in jobs[TEST_JOB]["strategy"]["matrix"]["platform"]
    ]
    return [
        Cell(LINT_JOB, "linux-x86_64", {}),
        *tested,
        Cell(MACOS_JOB, MACOS, {"id": MACOS}),
    ]


def render(text: str, cell: Cell, outputs: Mapping[str, Mapping[str, str]]) -> str:
    """`text` with each expression a pull request's run of `cell` gives it rendered.

    Raises:
        AssertionError: On an expression no pull request's run of these jobs carries.
    """

    def value(found: re.Match[str]) -> str:
        inner = found["inner"]
        if inner.startswith("matrix.platform."):
            return cell.matrix[inner.removeprefix("matrix.platform.")]
        step = re.fullmatch(r"steps\.([\w-]+)\.outputs\.([\w-]+)", inner)
        if step:
            return outputs.get(step[1], {}).get(step[2], "")
        if inner == "github.event.pull_request.title":
            return "ci: run the gate in eight jobs"
        if inner.startswith("secrets.") or inner == "github.token":
            return ""
        msg = f"`{found[0]}` is no expression a pull request's run of `{cell.job}` renders"
        raise AssertionError(msg)

    return EXPRESSION.sub(value, text)


def runs(step: dict[str, Any], failed: bool) -> bool:
    """Whether a step runs on a pull request from this repository, by its own `if:`.

    Raises:
        AssertionError: On a condition a run step of these jobs does not carry.
    """
    condition = str(step.get("if", "")).strip()
    if not condition:
        return not failed
    if condition == "always()":
        return True
    if condition == "failure()":
        return failed
    msg = f"`if: {condition}` is no condition a run step of these jobs carries"
    raise AssertionError(msg)


@dataclass(frozen=True)
class Copy:
    """A copy of the tree with its programs doubled, and the runs made in it."""

    root: Path
    programs: Path

    def environment(self, record: Path, extra: Mapping[str, str]) -> dict[str, str]:
        """What a step runs under: this host's, doubles first, and the pull request's."""
        return {
            **{k: v for k, v in os.environ.items() if not k.startswith(("NX_", "GITHUB_"))},
            "PATH": os.pathsep.join([str(self.programs), os.environ["PATH"]]),
            "HOME": str(self.root.parent / "home"),
            RECORD: str(record),
            "GITHUB_EVENT_NAME": "pull_request",
            "GITHUB_HEAD_REF": "nick/1/ci-work",
            "GITHUB_REPOSITORY": "nickderobertis/printobserver",
            **extra,
        }

    def run_job(self, workflow: dict[str, Any], cell: Cell, record: Path) -> None:
        """Run `cell`'s `run:` steps in order, as a pull request's run does, recording the leaves.

        Raises:
            AssertionError: If a step that runs fails.
        """
        job = jobs_of(workflow)[cell.job]
        outputs: dict[str, dict[str, str]] = {}
        base = {str(k): render(str(v), cell, outputs) for k, v in (job.get("env") or {}).items()}
        failed = False
        for index, step in enumerate(steps_of(job)):
            if "run" not in step or not runs(step, failed):
                continue
            files = {name: record.parent / f"{cell.job}-{index}.{name}" for name in ("out", "env")}
            for path in files.values():
                path.write_text("", encoding="utf-8")
            env = {
                **base,
                **{
                    str(k): render(str(v), cell, outputs)
                    for k, v in (step.get("env") or {}).items()
                },
                "GITHUB_OUTPUT": str(files["out"]),
                "GITHUB_ENV": str(files["env"]),
                "GITHUB_PATH": str(record.parent / f"{cell.job}-{index}.path"),
                STEP: f"{cell.job}:{index}",
            }
            script = render(str(step["run"]), cell, outputs)
            done = run(
                ["bash", "--noprofile", "--norc", "-eo", "pipefail", "-c", script],
                cwd=self.root,
                env=self.environment(record, env),
                timeout=600,
            )
            if done.returncode != 0:
                msg = (
                    f"{cell.job} ({cell.platform}) step {index} `{script}` exited "
                    f"{done.returncode}:\n{done.stdout}{done.stderr}"
                )
                raise AssertionError(msg)
            if "id" in step:
                outputs[str(step["id"])] = dict(
                    line.split("=", 1)
                    for line in files["out"].read_text(encoding="utf-8").splitlines()
                    if "=" in line
                )
            for line in files["env"].read_text(encoding="utf-8").splitlines():
                name, _, value = line.partition("=")
                base[name] = value

    def run_recipes(self, record: Path, *recipes: tuple[str, ...]) -> None:
        """Run each recipe through the real `just`, recording the leaves.

        Raises:
            AssertionError: If one fails.
        """
        for recipe in recipes:
            done = run(
                ["just", *recipe],
                cwd=self.root,
                env=self.environment(record, {STEP: " ".join(recipe)}),
                timeout=600,
            )
            if done.returncode != 0:
                said = f"{done.stdout}{done.stderr}"
                msg = f"`just {' '.join(recipe)}` exited {done.returncode}:\n{said}"
                raise AssertionError(msg)


@pytest.fixture(scope="module")
def doubled(tmp_path_factory: pytest.TempPathFactory) -> Callable[[], Copy]:
    """A factory for fresh copies of the tree whose leaf programs are doubled."""
    count = {"n": 0}

    def make() -> Copy:
        count["n"] += 1
        here = tmp_path_factory.mktemp(f"ci-work-{count['n']}")
        programs = here / "programs"
        programs.mkdir()
        (here / "home").mkdir()
        for name in DOUBLED:
            program(programs, name, DOUBLE)
        return Copy(copy_tree(here / "tree", closure=CLOSURE), programs)

    return make


def planned(argv: list[str], graph: Mapping[str, set[str]]) -> list[str]:
    """Each `target:project` task one `nx run-many` or `nx affected` invocation runs."""
    verb = next(word for word in argv if word in {"run-many", "affected"})
    words = iter(argv[argv.index(verb) + 1 :])
    targets: list[str] = []
    excluded: set[str] = set()
    for word in words:
        if word in {"-t", "--targets"}:
            for following in words:
                if following.startswith("-"):
                    word = following
                    break
                targets.extend(one for one in following.split(",") if one)
        if word.startswith("--exclude="):
            excluded |= set(word.removeprefix("--exclude=").split(","))
    return [
        f"{target}:{project}"
        for target in targets
        for project, declared in sorted(graph.items())
        if target in declared and project not in excluded
    ]


def _tier_free(argv: list[str]) -> str:
    """A leaf run, the tier it was handed written as `<tier>`, so both tiers' runs compare."""
    return " ".join("<tier>" if word in {"affected", "all"} else word for word in argv)


@dataclass(frozen=True)
class Work:
    """The units one job did: Nx tasks and other leaf runs, each with the side it ran on."""

    units: tuple[tuple[str, str], ...]
    own: tuple[str, ...]
    integration: tuple[str, ...]

    @property
    def tasks(self) -> list[str]:
        """Its Nx tasks, each as often as it ran."""
        return [unit for unit, _ in self.units if _task(unit)]


def work(record: Path, graph: Mapping[str, set[str]]) -> Work:
    """What the leaves recorded in `record` stand for, preparation left out."""
    units: list[tuple[str, str]] = []
    own: list[str] = []
    integration: list[str] = []
    lines = record.read_text(encoding="utf-8").splitlines() if record.exists() else []
    for line in lines:
        entry = json.loads(line)
        argv: list[str] = entry["argv"]
        if any(tuple(argv[: len(one)]) == one for one in PREPARATION):
            continue
        if argv[:2] == ["bunx", "nx"]:
            tasks = planned(argv, graph)
            if any(task.startswith("test-skill-install:") for task in tasks):
                own.append("nx test-skill-install")
                continue
            if any(task.startswith("test-integration:") for task in tasks):
                integration.extend(tasks)
            units.extend((task, entry["side"]) for task in tasks)
            continue
        named = next((" ".join(one) for one in LINT_OWN if tuple(argv[: len(one)]) == one), None)
        if named is not None:
            own.append(named)
            continue
        if "octoprint_env.py" in " ".join(argv):
            integration.append(_tier_free(argv))
        units.append((_tier_free(argv), entry["side"]))
    return Work(tuple(units), tuple(own), tuple(integration))


def _task(unit: str) -> bool:
    """Whether a unit is one project's Nx task, `target:project`, rather than a leaf run."""
    return ":" in unit and " " not in unit


def _partitions(sides: set[str], tag: str) -> bool:
    """Whether `sides` are exactly the two sides of `tag`."""
    return sides == {tag, f"!{tag}"}


def findings(lint: Work, tested: Mapping[str, Work], whole: Work, tag: str) -> list[str]:
    """How the jobs' work differs from the reference's, on any platform."""
    found: list[str] = []
    expected = Counter(unit for unit, _ in whole.units)
    for platform, test in sorted(tested.items()):
        ran: dict[str, list[str]] = {}
        for unit, side in (*lint.units, *test.units):
            ran.setdefault(unit, []).append(side)
        found.extend(
            f"`{unit}` is omitted on {platform}" for unit in sorted(expected.keys() - ran.keys())
        )
        found.extend(
            f"`{unit}` is run on {platform} and is no unit of the reference"
            for unit in sorted(ran.keys() - expected.keys())
        )
        for unit, sides in sorted(ran.items()):
            if unit not in expected:
                continue
            # A project's task is one side's alone, and runs as often as the
            # reference ran it; a report over the projects runs whole, or once
            # over each side of the tag.
            if len(sides) == expected[unit] and (_task(unit) or set(sides) <= {""}):
                continue
            if (
                not _task(unit)
                and len(sides) == 2 * expected[unit]
                and _partitions(set(sides), tag)
            ):
                continue
            found.append(f"`{unit}` is run {len(sides)} times on {platform}: {sorted(sides)}")
    for name, job in (("lint", lint), *((f"test ({p})", t) for p, t in sorted(tested.items()))):
        found.extend(
            f"`{task}` runs {seen} times in {name}"
            for task, seen in sorted(Counter(job.tasks).items())
            if seen > 1
        )
    return found


@dataclass(frozen=True)
class Ran:
    """Every job's work in one copy, the reference's, and the graph they were planned over."""

    lint: Work
    tested: dict[str, Work]
    whole: Work
    graph: dict[str, set[str]]
    tagged: frozenset[str]
    tag: str


def ran_in(copy: Copy) -> Ran:
    """Run every job of the copy's `ci.yml`, and the reference, recording each separately."""
    repo = Repo(copy.root)
    graph = {project.name: set(project.targets) for project in projects(repo)}
    tag = platform_independent_tag(repo)
    tagged = frozenset(project.name for project in projects(repo) if tag in project.tags)
    workflow = load_workflow(copy.root / CI)
    records = copy.root.parent / "records"
    records.mkdir(exist_ok=True)
    lint: Work | None = None
    tested: dict[str, Work] = {}
    for cell in cells(workflow):
        record = records / f"{cell.job}-{cell.platform}.jsonl"
        copy.run_job(workflow, cell, record)
        if cell.job == LINT_JOB:
            lint = work(record, graph)
        else:
            tested[cell.platform] = work(record, graph)
    reference = records / "reference.jsonl"
    copy.run_recipes(reference, ("check", "all"), *((recipe,) for recipe in INTEGRATION))
    truth(lint is not None, describing="the `lint` job run")
    return Ran(lint or Work((), (), ()), tested, work(reference, graph), graph, tagged, tag)


@pytest.fixture(scope="module")
def committed(doubled: Callable[[], Copy]) -> Ran:
    """The committed workflow's jobs, run once for every assertion over them."""
    return ran_in(doubled())


def test_the_reference_reaches_every_projects_tests_through_the_doubled_nx(committed: Ran) -> None:
    """The comparison is over real work: the whole graph's tests, builds and end-to-end tier."""
    tasks = {unit for unit, _ in committed.whole.units}
    for target in ("test", "build", "test-e2e", "lint", "format-check", "typecheck"):
        truth(
            any(task.startswith(f"{target}:") for task in tasks),
            describing=f"the reference's `{target}` tasks",
        )
    truth(committed.whole.integration, describing="the reference's integration tier")


def test_every_platform_runs_the_whole_gate_and_integration_tier_once(committed: Ran) -> None:
    """With `lint`, each platform's `test` job runs every unit of the reference, each once."""
    equal(
        sorted(committed.tested),
        ["linux-aarch64", "linux-x86_64", MACOS, "windows-aarch64", "windows-x86_64"],
        describing="the platforms a `test` job ran on",
    )
    equal(findings(committed.lint, committed.tested, committed.whole, committed.tag), [])


def test_tagged_projects_are_tested_in_lint_alone_and_every_other_in_each_test_job(
    committed: Ran,
) -> None:
    """`test` and `test-e2e` of a `platform:independent` project run once, in `lint`."""
    truth(committed.tagged, describing="projects carrying the tag")
    for target in ("test", "test-e2e"):
        declared = {name for name, targets in committed.graph.items() if target in targets}
        in_lint = {
            task.split(":", 1)[1] for task in committed.lint.tasks if task.startswith(f"{target}:")
        }
        equal(in_lint, declared & committed.tagged, describing=f"the `{target}` tasks `lint` runs")
        for platform, test in sorted(committed.tested.items()):
            in_test = {
                task.split(":", 1)[1] for task in test.tasks if task.startswith(f"{target}:")
            }
            equal(
                in_test,
                declared - committed.tagged,
                describing=f"the `{target}` tasks `test ({platform})` runs",
            )


def test_lints_own_steps_run_there_once_and_the_integration_tier_in_each_test_job(
    committed: Ran,
) -> None:
    """The title, supply chain, skill install and judged lint in `lint`; the tier in `test`."""
    equal(sorted(committed.lint.own), sorted(" ".join(one) for one in LINT_OWN))
    equal(committed.lint.integration, (), describing="the integration tier in `lint`")
    for platform, test in sorted(committed.tested.items()):
        equal(test.own, (), describing=f"`lint`'s own steps in `test ({platform})`")
        equal(
            sorted(test.integration),
            sorted(committed.whole.integration),
            describing=f"the integration tier in `test ({platform})`",
        )


def broken(copy: Copy, old: str, new: str) -> Ran:
    """The jobs' work with one passage of the copy's `ci.yml` replaced, every copy of it."""
    path = copy.root / CI
    text = path.read_text(encoding="utf-8")
    truth(old in text, describing=f"the committed ci.yml carrying {old!r}")
    path.write_text(text.replace(old, new), encoding="utf-8")
    return ran_in(copy)


@pytest.mark.parametrize(
    ("old", "new", "naming"),
    [
        pytest.param(
            "      - run: just ci-lint\n",
            "",
            "`format-check:repo-checks` is omitted on linux-x86_64",
            id="the-lint-half-dropped",
        ),
        pytest.param(
            "      - run: just ci-test\n",
            "      - run: just ci-lint\n",
            "is run 2 times",
            id="the-test-half-changed-to-the-lint-half",
        ),
        pytest.param(
            "      - run: just test-integration\n",
            "",
            "`test-integration:",
            id="the-integration-tier-dropped",
        ),
        pytest.param(
            "      - run: just ci-test\n",
            "      - run: just ci-test\n      - run: just ci-test\n",
            "runs 2 times in test",
            id="the-test-half-doubled",
        ),
    ],
)
def test_a_step_dropped_changed_or_doubled_is_found(
    doubled: Callable[[], Copy], old: str, new: str, naming: str
) -> None:
    """Each defect, made in a copy of the workflow, is a finding naming the work it moved."""
    ran = broken(doubled(), old, new)
    said = "\n".join(findings(ran.lint, ran.tested, ran.whole, ran.tag))

    truth(naming in said, describing=f"a finding naming {naming!r} in:\n{said}")


#: The two halves' own lines of the committed justfile.
LINT_HALF = "    just ci-measured {{quote(PLATFORM_INDEPENDENT)}} {{quote(tier)}}\n"
TEST_HALF = "    just ci-measured {{quote(PLATFORM_DEPENDENT)}} {{quote(tier)}}\n"


@pytest.mark.parametrize(
    ("old", "new", "naming"),
    [
        pytest.param(
            TEST_HALF,
            TEST_HALF + LINT_HALF,
            "`test-e2e:repo-e2e-independent` is run 2 times on linux-x86_64",
            id="run-per-platform",
        ),
        pytest.param(
            LINT_HALF,
            "",
            "`test-e2e:repo-e2e-independent` is omitted on linux-x86_64",
            id="run-nowhere",
        ),
    ],
)
def test_a_tagged_projects_end_to_end_run_per_platform_or_nowhere_is_found(
    doubled: Callable[[], Copy], old: str, new: str, naming: str
) -> None:
    """The tagged end-to-end tier moved into the `test` jobs, or out of `lint`, is found."""
    copy = doubled()
    path = copy.root / "justfile"
    text = path.read_text(encoding="utf-8")
    truth(old in text, describing="the committed half of the gate the defect edits")
    path.write_text(text.replace(old, new, 1), encoding="utf-8")
    ran = ran_in(copy)
    said = "\n".join(findings(ran.lint, ran.tested, ran.whole, ran.tag))

    truth(naming in said, describing=f"a finding naming {naming!r} in:\n{said}")


@pytest.fixture(autouse=True)
def _no_ambient_side(monkeypatch: pytest.MonkeyPatch) -> Iterator[None]:
    """No side of the tag leaks into the runs from the job running this suite."""
    monkeypatch.delenv(SIDE, raising=False)
    yield
