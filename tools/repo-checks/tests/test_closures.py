"""A copy cut to a check's closure is, to that check, the whole tree.

Each check `closures` declares a closure for is run over a copy of only that
closure and over a whole copy, and the two must say the same thing. The copies
are cut from the tree the way a clone would carry it — files added and not yet
committed included, a deletion nobody staged left out, a link kept a link —
and a read the closure did not foresee is named rather than passed over.
"""

from __future__ import annotations

from pathlib import Path

import pytest
from closures import CHECKS, closure
from repo_checks.expect import equal, truth
from repo_checks.model import Repo
from repo_checks.registry import ALL
from repo_checks.shell import run
from treecopy import REPO_ROOT, Closure, copy_tree, listed, strayed


@pytest.fixture(scope="module")
def whole(tmp_path_factory: pytest.TempPathFactory) -> Path:
    """One whole copy of the committed tree, which no test here changes."""
    return _with_dependencies(copy_tree(tmp_path_factory.mktemp("whole")))


def _with_dependencies(root: Path) -> Path:
    """Link the committed tree's JavaScript dependencies in, as `test_clients` does.

    The client generator lays the Node client out with the `biome` installed
    there, so without them the drift check says it could not run, over either
    copy alike, and the comparison would prove nothing of it.
    """
    (root / "node_modules").symlink_to(REPO_ROOT / "node_modules")
    return root


def _said(root: Path, name: str) -> list[str]:
    """What one check finds in the tree at `root`, with that root's own path left out."""
    findings = ALL[name](Repo(root))
    return [finding.replace(str(root.resolve()), "<root>") for finding in findings]


@pytest.mark.parametrize("name", sorted(CHECKS))
def test_a_check_finds_in_its_closure_what_it_finds_in_the_whole_tree(
    name: str, whole: Path, tmp_path: Path
) -> None:
    """The closure carries every file the check's verdict on the committed tree depends on."""
    cut = _with_dependencies(copy_tree(tmp_path / "cut", closure=CHECKS[name]))

    equal(_said(cut, name), _said(whole, name), describing=f"what `{name}` finds in its closure")


def test_every_closure_is_of_a_check_and_of_files_the_tree_has() -> None:
    """A closure naming no check, or a pattern no committed file falls under, is stale."""
    names = listed(REPO_ROOT)

    equal(sorted(set(CHECKS) - set(ALL)), [], describing="closures naming no registered check")
    for check, cut in CHECKS.items():
        for pattern in cut.patterns:
            truth(
                Closure((pattern,)).carries_any(names),
                describing=f"`{check}`'s pattern {pattern!r} to carry a committed file",
            )


def test_a_closure_carries_exactly_the_files_its_patterns_name() -> None:
    """A directory, a glob spanning directories and one file, and nothing beside them."""
    cut = Closure(("schemas/", "**/project.json", "crates/*/Cargo.toml", "justfile"))

    truth(cut.carries("schemas/printobserver-core/PrintAction.json"), describing="a directory")
    truth(cut.carries("project.json"), describing="`**` spanning no directory at all")
    truth(cut.carries("tools/repo-checks/project.json"), describing="`**` spanning two")
    truth(cut.carries("crates/printobserver/Cargo.toml"), describing="`*` spanning one")
    truth(cut.carries("justfile"), describing="one file")
    truth(not cut.carries("crates/printobserver/fuzz/Cargo.toml"), describing="`*` spanning two")
    truth(not cut.carries("schemas.md"), describing="a file beside a directory's name")
    truth(not cut.carries("justfile.bak"), describing="a file beside a file's name")
    equal(
        (closure("ci") | closure("ci", also=("README.md",))).patterns,
        (*closure("ci").patterns, "README.md"),
        describing="two closures joined, each pattern once",
    )


def test_a_read_outside_the_closure_is_named_and_a_write_is_not(tmp_path: Path) -> None:
    """Opening a committed file the copy left out is the one thing reported."""
    copied = copy_tree(tmp_path / "cut", closure=Closure(("justfile",)))
    strayed()

    (copied / "justfile").read_text(encoding="utf-8")
    (copied / "the-work-this-change-is.md").write_text("new\n", encoding="utf-8")
    (copied / "README.md").write_text("replaced\n", encoding="utf-8")
    with pytest.raises(FileNotFoundError):
        (copied / "AGENTS.md").read_text(encoding="utf-8")
    (copied / "the-work-this-change-is.md").read_text(encoding="utf-8")

    equal(strayed(), ["AGENTS.md"], describing="the reads outside the closure")
    equal(strayed(), [], describing="what is reported once it has been asked for")


def _source(root: Path) -> Path:
    """A repository holding every state a file of a change can be in."""
    root.mkdir()
    for name in ("kept.txt", "gone.txt", "nested/kept.txt"):
        (root / name).parent.mkdir(parents=True, exist_ok=True)
        (root / name).write_text(f"{name}\n", encoding="utf-8")
    (root / ".gitignore").write_text("ignored.txt\ntarget/\n", encoding="utf-8")
    (root / "link.txt").symlink_to("kept.txt")
    run(["git", "init", "-q"], cwd=root, check=True)
    run(["git", "add", "-A"], cwd=root, check=True)
    run(
        ["git", "-c", "user.name=t", "-c", "user.email=t@t", "commit", "-q", "-m", "c"],
        cwd=root,
        check=True,
    )
    (root / "gone.txt").unlink()
    (root / "added.txt").write_text("added\n", encoding="utf-8")
    run(["git", "add", "added.txt"], cwd=root, check=True)
    (root / "untracked.txt").write_text("untracked\n", encoding="utf-8")
    (root / "ignored.txt").write_text("ignored\n", encoding="utf-8")
    return root


def _files(root: Path) -> list[str]:
    """Every path under `root`, relative and in order, directories left out."""
    return sorted(
        path.relative_to(root).as_posix()
        for path in root.rglob("*")
        if path.is_symlink() or path.is_file()
    )


def test_a_closure_copy_carries_what_a_clone_would_of_the_files_it_names(
    tmp_path: Path,
) -> None:
    """Added and untracked files come, a deleted or ignored one does not, a link stays one."""
    source = _source(tmp_path / "source")
    names = ("kept.txt", "gone.txt", "added.txt", "untracked.txt", "ignored.txt", "link.txt")

    cut = copy_tree(tmp_path / "cut", closure=Closure(names), source=source)
    whole = copy_tree(tmp_path / "whole", source=source)

    equal(
        _files(cut),
        ["added.txt", "kept.txt", "link.txt", "untracked.txt"],
        describing="what the closure copy carries",
    )
    equal(
        _files(whole),
        [".gitignore", "added.txt", "kept.txt", "link.txt", "nested/kept.txt", "untracked.txt"],
        describing="what the whole copy carries",
    )
    for copied in (cut, whole):
        truth((copied / "link.txt").is_symlink(), describing="the link to be copied as a link")
        equal((copied / "link.txt").readlink(), Path("kept.txt"), describing="where it points")
        equal((copied / "added.txt").read_text(encoding="utf-8"), "added\n", describing="added")
