"""The `platform:independent` project tag: declared once, and splitting CI's measured tiers.

`repo-policy.toml`'s `gate.platform_independent_tag` is the tag's one spelling.
Every check over project tags is driven through `python -m repo_checks <check>
--root`, as `just check-repo` runs it, over copies of the committed tree with
one project tagged.

The tag selects which side of CI's measured tiers a project's suites run on:
`just ci-lint` runs `just ci-measured` over the tagged side, once, and `just
ci-test` over the untagged side, on every platform, each by setting
`PRINTOBSERVER_GATE_PROJECTS` for `repo_checks.gate_tier`. The two sides are
read off the real recipes through `just --dry-run`, and the real tier selection
is run for both of them at both tiers, over a real git repository and over the
committed graph.
"""

from __future__ import annotations

import json
from collections.abc import Callable
from pathlib import Path

import pytest
from repo_checks.__main__ import main
from repo_checks.checks_graph import platform_independent_tag, projects
from repo_checks.expect import contains, equal, truth
from repo_checks.fixture_repository import init_repository
from repo_checks.gate_tier import AFFECTED, ALL, BASE_VARIABLE, PROJECTS_VARIABLE, command
from repo_checks.model import Repo
from repo_checks.shell import run
from treecopy import REPO_ROOT, Tree

#: The project the copies tag: a Python project, so the Python checks read it too.
TAGGED = "tests/real-prints-lfs/project.json"
#: Every check that reads a project's tags.
TAG_CHECKS = ("project-tags", "graph-edges", "python-typecheck-platforms")
POLICY = "repo-policy.toml"
DECLARED = 'platform_independent_tag = "platform:independent"\n'

Run = Callable[[str, Tree | None], tuple[int, str]]


@pytest.fixture
def check(capsys: pytest.CaptureFixture[str]) -> Run:
    """Run one check over a tree, answering its exit and what it printed."""

    def run_check(name: str, tree: Tree | None = None) -> tuple[int, str]:
        root = REPO_ROOT if tree is None else tree.root
        code = main([name, "--root", str(root)])
        return code, capsys.readouterr().err

    return run_check


def tagged(tree: Tree, *tags: str, project: str = TAGGED) -> Tree:
    """The tree with `tags` added to one project's `tags`."""
    declared = json.loads(tree.read(project))
    declared["tags"] = [*declared.get("tags", []), *tags]
    tree.write(project, json.dumps(declared, indent=2) + "\n")
    return tree


def committed_carriers() -> list[str]:
    """The committed projects that carry the declared tag, by their `project.json`'s path."""
    return sorted(
        path.relative_to(REPO_ROOT).as_posix()
        for path in REPO_ROOT.glob("**/project.json")
        if "node_modules" not in path.parts
        and "platform:independent" in json.loads(path.read_text(encoding="utf-8")).get("tags", [])
    )


def test_the_committed_tree_declares_the_tag_and_passes(check: Run) -> None:
    """The committed policy spells the tag, and no committed project misspells it."""
    contains((REPO_ROOT / POLICY).read_text(encoding="utf-8"), DECLARED)
    equal(check("project-tags", None), (0, ""))


@pytest.mark.parametrize("name", TAG_CHECKS)
def test_a_project_tagged_platform_independent_passes_every_check_over_tags(
    check: Run, tree: Callable[[], Tree], name: str
) -> None:
    """The declared tag is accepted wherever a project's tags are read."""
    equal(check(name, tagged(tree(), "platform:independent")), (0, ""))


@pytest.mark.parametrize(
    "misspelt", ["platform:independant", "platform:linux-x86_64", "platform:Independent"]
)
def test_any_other_platform_tag_is_refused_naming_the_declared_spelling(
    check: Run, tree: Callable[[], Tree], misspelt: str
) -> None:
    """A misspelling cannot silently keep a suite on every platform."""
    code, err = check("project-tags", tagged(tree(), misspelt))
    equal(code, 1)
    contains(err, f"tests/real-prints-lfs/project.json carries `{misspelt}`")
    contains(err, "the one `repo-policy.toml`'s `gate.platform_independent_tag` declares")
    contains(err, "is `platform:independent`")


def test_a_tag_of_another_dimension_is_not_read_as_a_platform_tag(
    check: Run, tree: Callable[[], Tree]
) -> None:
    """Only the `platform:` dimension is held to the declared spelling."""
    equal(check("project-tags", tagged(tree(), "platforms:independent", "type:x")), (0, ""))


#: The justfile's one spelling of the tag, which the CI jobs' recipes select by.
JUSTFILE_SPELLING = 'PLATFORM_INDEPENDENT := "platform:independent"\n'


def test_the_spelling_is_read_from_the_policy_alone(check: Run, tree: Callable[[], Tree]) -> None:
    """Respelt in `repo-policy.toml`, the new spelling passes and the old one is refused."""
    copy = tree()
    copy.edit(POLICY, DECLARED, 'platform_independent_tag = "platform:portable"\n')
    for carrier in committed_carriers():
        copy.edit(carrier, '"platform:independent"', '"platform:portable"')
    copy.edit("justfile", JUSTFILE_SPELLING, 'PLATFORM_INDEPENDENT := "platform:portable"\n')
    equal(check("project-tags", tagged(copy, "platform:portable")), (0, ""))

    code, err = check("project-tags", tagged(copy, "platform:independent", project=TAGGED))
    equal(code, 1)
    contains(err, "carries `platform:independent`")
    contains(err, "is `platform:portable`")


@pytest.mark.parametrize(
    ("spelling", "naming"),
    [
        (
            'PLATFORM_INDEPENDENT := "platform:portable"\n',
            "the justfile's `PLATFORM_INDEPENDENT` is `platform:portable`, and the one",
        ),
        ("", "the justfile declares no `PLATFORM_INDEPENDENT"),
    ],
    ids=["respelt", "absent"],
)
def test_a_justfile_selecting_by_another_spelling_is_refused(
    check: Run, tree: Callable[[], Tree], spelling: str, naming: str
) -> None:
    """The CI jobs' recipes select by the policy's spelling, or every tagged suite moves."""
    copy = tree()
    copy.edit("justfile", JUSTFILE_SPELLING, spelling)

    code, err = check("project-tags", copy)

    equal(code, 1)
    contains(err, naming)


@pytest.mark.parametrize(
    ("declared", "naming"),
    [
        ("", "declares no `gate.platform_independent_tag` string"),
        ('platform_independent_tag = "independent"\n', "is no tag of the `platform:` dimension"),
        ('platform_independent_tag = "lang:independent"\n', "is no tag of the `platform:`"),
    ],
    ids=["absent", "no-dimension", "another-dimension"],
)
def test_a_policy_that_does_not_spell_the_tag_is_refused(
    check: Run, tree: Callable[[], Tree], declared: str, naming: str
) -> None:
    """With no usable spelling there is nothing to hold a project's tag to."""
    copy = tree()
    copy.edit(POLICY, DECLARED, declared)
    code, err = check("project-tags", copy)
    equal(code, 1)
    contains(err, naming)


#: The projects of the selection fixture, by directory, and the tags each carries:
#: `gamma` alone is platform-independent.
PROJECTS = {
    "alpha": ["lang:rust"],
    "beta": ["lang:python"],
    "gamma": ["lang:python", "platform:independent"],
}
IDENTITY = ("-c", "user.email=tag@printobserver.test", "-c", "user.name=tag")
#: The targets whose suites the tag says run once or on every platform.
SUITES = ("test", "test-e2e")


def git(root: Path, *arguments: str) -> str:
    """Run git in `root` and answer what it printed."""
    return run(["git", *IDENTITY, *arguments], cwd=root, check=True).stdout.strip()


def selection_repository(root: Path) -> tuple[Repo, str]:
    """`PROJECTS` under the committed policy, each changed on a branch; and the base."""
    for name, tags in PROJECTS.items():
        (root / name).mkdir(parents=True)
        declared = {"name": name, "root": name, "tags": tags, "targets": {}}
        (root / name / "project.json").write_text(json.dumps(declared) + "\n", encoding="utf-8")
        (root / name / "source.txt").write_text(f"{name}\n", encoding="utf-8")
    (root / POLICY).write_text((REPO_ROOT / POLICY).read_text(encoding="utf-8"), encoding="utf-8")
    init_repository(root, branch="main")
    git(root, "add", "-A")
    git(root, "commit", "-q", "-m", "chore: three projects")
    base = git(root, "rev-parse", "HEAD")
    git(root, "switch", "-q", "-c", "change")
    for name in PROJECTS:
        (root / name / "source.txt").write_text(f"{name}, changed\n", encoding="utf-8")
    git(root, "commit", "-q", "-am", "feat: change every project")
    return Repo(root), base


def side_of(recipe: str) -> str:
    """The project side the committed `just <recipe>` hands `just ci-measured`."""
    dry = run(["just", "--dry-run", recipe], cwd=REPO_ROOT, timeout=300)
    equal(dry.returncode, 0, describing=f"`just --dry-run {recipe}`: {dry.stderr}")
    measured = [
        line.split() for line in dry.stderr.splitlines() if line.startswith("just ci-measured ")
    ]
    equal(len(measured), 1, describing=f"the runs of `just ci-measured` in `just {recipe}`")
    return measured[0][2].strip("'")


def sides() -> dict[str, str]:
    """Each half of CI's gate, and the side of the tag it measures."""
    return {recipe: side_of(recipe) for recipe in ("ci-lint", "ci-test")}


def selected(repo: Repo, tier: str, target: str, environ: dict[str, str]) -> set[str]:
    """The projects one real selection runs `target` over: every one it does not exclude."""
    invocation, _ = command(repo, tier, [target], environ)
    contains(invocation, target, describing=f"the targets of {invocation}")
    excluded = {
        name
        for part in invocation
        if part.startswith("--exclude=")
        for name in part.removeprefix("--exclude=").split(",")
    }
    return {project.name for project in projects(repo)} - excluded


def test_the_two_halves_measure_the_two_sides_of_the_declared_tag() -> None:
    """`just ci-lint` measures the tagged projects and `just ci-test` every other one."""
    declared = platform_independent_tag(Repo(REPO_ROOT))

    equal(sides(), {"ci-lint": declared, "ci-test": f"!{declared}"})


@pytest.mark.parametrize("tier", [AFFECTED, ALL])
@pytest.mark.parametrize("target", SUITES)
def test_the_two_sides_partition_every_project_by_the_tag(
    tmp_path: Path, tier: str, target: str
) -> None:
    """Every suite runs on one side: the tagged project's once, every other's per platform."""
    repo, base = selection_repository(tmp_path / "repository")
    halves = sides()

    independent = selected(
        repo, tier, target, {PROJECTS_VARIABLE: halves["ci-lint"], BASE_VARIABLE: base}
    )
    dependent = selected(
        repo, tier, target, {PROJECTS_VARIABLE: halves["ci-test"], BASE_VARIABLE: base}
    )

    invocation, _ = command(repo, tier, [target], {BASE_VARIABLE: base})
    equal(
        invocation[2],
        "affected" if tier == AFFECTED else "run-many",
        describing=f"the Nx verb a `{tier}` run over a change to project sources uses",
    )
    equal(independent, {"gamma"}, describing=f"the `{target}` projects `just ci-lint` measures")
    equal(
        dependent, {"alpha", "beta"}, describing=f"the `{target}` projects `just ci-test` measures"
    )
    equal(independent & dependent, set(), describing="the projects both sides run")
    equal(independent | dependent, set(PROJECTS), describing="the projects either side runs")
    equal(
        selected(repo, tier, target, {BASE_VARIABLE: base}),
        set(PROJECTS),
        describing=f"the `{target}` projects an unnarrowed `{tier}` run reaches",
    )


def test_the_committed_graph_is_partitioned_by_the_two_halves() -> None:
    """Every committed tagged project is measured once, and every other one per platform."""
    repo = Repo(REPO_ROOT)
    every = {project.name for project in projects(repo)}
    tagged_names = {
        project.name for project in projects(repo) if "platform:independent" in project.tags
    }
    truth(bool(tagged_names), describing="a committed project tagged `platform:independent`")
    truth(bool(every - tagged_names), describing="a committed project carrying no such tag")
    halves = sides()

    for target in SUITES:
        independent = selected(repo, ALL, target, {PROJECTS_VARIABLE: halves["ci-lint"]})
        dependent = selected(repo, ALL, target, {PROJECTS_VARIABLE: halves["ci-test"]})

        equal(independent, tagged_names, describing=f"the `{target}` projects `just ci-lint` runs")
        equal(dependent, every - tagged_names, describing=f"the `{target}` projects `ci-test` runs")
