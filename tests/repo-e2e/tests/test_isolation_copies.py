"""A journey's copy carries what a clone would, from a listing asked for once.

`copy_tracked` asks git which files a clone carries once per worker rather than
once a copy; these hold that what it copies is still exactly that set, in every
state a file of a change can be in.
"""

from __future__ import annotations

from pathlib import Path

from journey import COMMIT_IDENTITY, copy_tracked, listed
from repo_checks.expect import equal, truth
from repo_checks.fixture_repository import init_repository
from repo_checks.shell import run


def _source(root: Path) -> Path:
    """A repository holding every state a file of a change can be in."""
    root.mkdir()
    for name in ("kept.txt", "gone.txt", "nested/kept.txt"):
        (root / name).parent.mkdir(parents=True, exist_ok=True)
        (root / name).write_text(f"{name}\n", encoding="utf-8")
    (root / ".gitignore").write_text("ignored.txt\n", encoding="utf-8")
    (root / "link.txt").symlink_to("kept.txt")
    init_repository(root)
    run(["git", "add", "-A"], cwd=root, check=True)
    run(["git", *COMMIT_IDENTITY, "commit", "-q", "-m", "c"], cwd=root, check=True)
    (root / "gone.txt").unlink()
    (root / "added.txt").write_text("added\n", encoding="utf-8")
    run(["git", "add", "added.txt"], cwd=root, check=True)
    (root / "untracked.txt").write_text("untracked\n", encoding="utf-8")
    (root / "ignored.txt").write_text("ignored\n", encoding="utf-8")
    return root


def _files(root: Path) -> list[str]:
    """Every file and link under `root`, relative and in order."""
    return sorted(
        path.relative_to(root).as_posix()
        for path in root.rglob("*")
        if path.is_symlink() or path.is_file()
    )


def test_every_copy_carries_what_a_clone_of_the_change_would(tmp_path: Path) -> None:
    """Added and untracked files come, a deleted or ignored one does not, a link stays one."""
    source = _source(tmp_path / "source")
    before = listed.cache_info()

    copies = [copy_tracked(tmp_path / f"copy{n}", root=source) for n in range(2)]

    after = listed.cache_info()

    for copied in copies:
        equal(
            _files(copied),
            [".gitignore", "added.txt", "kept.txt", "link.txt", "nested/kept.txt", "untracked.txt"],
            describing=f"what {copied.name} carries",
        )
        truth((copied / "link.txt").is_symlink(), describing="the link to be copied as a link")
        equal((copied / "added.txt").read_text(encoding="utf-8"), "added\n", describing="added")
    equal(
        (after.misses - before.misses, after.hits - before.hits),
        (1, 1),
        describing="git asked for the listing once, and the second copy taking it as it stood",
    )
