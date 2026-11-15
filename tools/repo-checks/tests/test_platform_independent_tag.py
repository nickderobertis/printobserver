"""The `platform:independent` project tag: declared once, accepted, and selecting nothing yet.

`repo-policy.toml`'s `gate.platform_independent_tag` is the tag's one spelling.
Every check over project tags is driven through `python -m repo_checks <check>
--root`, as `just check-repo` runs it, over copies of the committed tree with
one project tagged; and the real tier selection, `repo_checks.gate_tier`, is
run for both gate tiers under each platform's gate settings over a real git
repository, once with one project tagged and once without.
"""

from __future__ import annotations

import json
from collections.abc import Callable
from dataclasses import dataclass
from pathlib import Path

import pytest
from repo_checks.__main__ import main
from repo_checks.expect import contains, equal, truth
from repo_checks.fixture_repository import init_repository
from repo_checks.gate_shards import plan
from repo_checks.gate_tier import AFFECTED, ALL, BASE_VARIABLE, command
from repo_checks.model import Repo
from repo_checks.parsing import jobs_of, load_workflow
from repo_checks.platforms import supported
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


def test_the_spelling_is_read_from_the_policy_alone(check: Run, tree: Callable[[], Tree]) -> None:
    """Respelt in `repo-policy.toml`, the new spelling passes and the old one is refused."""
    copy = tree()
    copy.edit(POLICY, DECLARED, 'platform_independent_tag = "platform:portable"\n')
    equal(check("project-tags", tagged(copy, "platform:portable")), (0, ""))

    code, err = check("project-tags", tagged(copy, "platform:independent", project=TAGGED))
    equal(code, 1)
    contains(err, "carries `platform:independent`")
    contains(err, "is `platform:portable`")


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


#: The projects of the selection fixture, by directory, and the tags each carries.
PROJECTS = {"alpha": ["lang:rust"], "beta": ["lang:python"], "gamma": ["lang:python"]}
IDENTITY = ("-c", "user.email=tag@printobserver.test", "-c", "user.name=tag")


def git(root: Path, *arguments: str) -> str:
    """Run git in `root` and answer what it printed."""
    return run(["git", *IDENTITY, *arguments], cwd=root, check=True).stdout.strip()


def selection_repository(root: Path, *, tag: bool) -> tuple[Repo, str]:
    """Three projects under the committed policy, `gamma` changed on a branch; and the base.

    With `tag`, `gamma` carries `platform:independent` from the first commit on,
    so the two repositories differ in that tag and in nothing else.
    """
    for name, tags in PROJECTS.items():
        (root / name).mkdir(parents=True)
        carried = [*tags, "platform:independent"] if tag and name == "gamma" else tags
        declared = {"name": name, "root": name, "tags": carried, "targets": {}}
        (root / name / "project.json").write_text(json.dumps(declared) + "\n", encoding="utf-8")
        (root / name / "source.txt").write_text(f"{name}\n", encoding="utf-8")
    (root / POLICY).write_text((REPO_ROOT / POLICY).read_text(encoding="utf-8"), encoding="utf-8")
    init_repository(root, branch="main")
    git(root, "add", "-A")
    git(root, "commit", "-q", "-m", "chore: three projects")
    base = git(root, "rev-parse", "HEAD")
    git(root, "switch", "-q", "-c", "change")
    (root / "gamma" / "source.txt").write_text("gamma, changed\n", encoding="utf-8")
    git(root, "commit", "-q", "-am", "feat: change gamma")
    return Repo(root), base


def shard_counts() -> dict[str, int]:
    """How many shards each supported platform's gate runs in, as the committed `ci.yml` says."""
    counts = {platform.id: 1 for platform in supported(Repo(REPO_ROOT))}
    workflow = load_workflow(REPO_ROOT / ".github/workflows/ci.yml")
    for name, job in jobs_of(workflow).items():
        if not name.startswith("gate-shard-"):
            continue
        for cell in job["strategy"]["matrix"]["platform"]:
            counts[cell["id"]] = int(cell["shards"])
    return counts


@dataclass(frozen=True, slots=True)
class Selection:
    """What the tier selection answered for one tier under one platform's shard step."""

    platform: str
    step: str
    tier: str
    #: Why the run reaches what it does, the base read as `<base>`.
    reason: str
    #: The Nx invocation it makes, the base read as `<base>`.
    invocation: tuple[str, ...]

    def __str__(self) -> str:
        """Where it was made, as a failure names it."""
        return f"{self.tier} on {self.platform}, {self.step}"


def selections(repo: Repo, base: str) -> list[Selection]:
    """What the tier selection answers for every tier, platform and shard step, in order."""
    answered: list[Selection] = []
    for platform, count in sorted(shard_counts().items()):
        for steps in plan(repo, count):
            for step in steps:
                for tier in (AFFECTED, ALL):
                    environ = {
                        **step.environment,
                        "PRINTOBSERVER_PLATFORM": platform,
                        BASE_VARIABLE: base,
                    }
                    invocation, reason = command(repo, tier, ["test"], environ)
                    # The base is the one thing the two repositories' commits
                    # make differ, so it is read as the base rather than as a SHA.
                    answered.append(
                        Selection(
                            platform=platform,
                            step=str(step),
                            tier=tier,
                            reason=reason.replace(base, "<base>").replace(base[:12], "<base>"),
                            invocation=tuple(part.replace(base, "<base>") for part in invocation),
                        )
                    )
    return answered


def test_the_tier_selection_selects_exactly_what_it_does_with_the_tag_absent(
    tmp_path: Path,
) -> None:
    """Every tier, on every platform's gate settings, reaches the same projects either way."""
    counts = shard_counts()
    truth(any(count > 1 for count in counts.values()), describing="a sharded platform")
    without, base_without = selection_repository(tmp_path / "untagged", tag=False)
    with_tag, base_with = selection_repository(tmp_path / "tagged", tag=True)

    untagged = selections(without, base_without)
    tagged_answers = selections(with_tag, base_with)

    equal(len(tagged_answers), len(untagged), describing="selections made")
    for before, after in zip(untagged, tagged_answers, strict=True):
        equal(after, before, describing=f"the selection for {before}")
    narrowed = [one for one in untagged if any("--exclude=" in part for part in one.invocation)]
    truth(bool(narrowed), describing="a selection narrowed by a project tag")
