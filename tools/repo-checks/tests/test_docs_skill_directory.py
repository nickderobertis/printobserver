"""The skill's directory carries itself, and no document links through a directory symlink.

Two failures of one layout, each rebuilt here with real symlinks on disk. The
skill once sat in a crate's assets beside a `reference` symlink to the
documents: GitHub does not follow a directory symlink when it resolves a link,
so every link through it was a 404 on the forge, and `gh skill install` drops
every symlink, so an installed skill carried none of them. The finished layout
is one real directory, and the committed tree is held to it.
"""

from __future__ import annotations

from pathlib import Path

import pytest
from repo_checks.checks_docs import link_symlinks, skill, skill_directory
from repo_checks.expect import accepted, refused_naming, truth
from repo_checks.model import Repo

#: The skill, where `repo-policy.toml` declares it.
SKILL = "skills/printobserver/SKILL.md"

#: The smallest `[docs]` declaration naming that skill.
DECLARED = f"""\
[docs]
skill = "{SKILL}"
skill_max_characters = 6000
skill_max_lines = 150
surface_manifest = "skills/printobserver/reference/surface.json"
schema_document = "skills/printobserver/reference/schemas.md"
schema_directory = "schemas"
example_fence = "console"

[[docs.skill_element]]
name = "the role"
marker = "## The role"

[[docs.document]]
path = "skills/printobserver/reference/a.md"
headings = ["A"]
"""

#: A skill whose one link stays inside its directory.
SKILL_TEXT = "---\nname: printobserver\ndescription: d\n---\n# A skill\n\n[a](reference/a.md)\n"


def _write(root: Path, relative: str, text: str) -> None:
    """Put one file in the tree, making the directories above it."""
    path = root / relative
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(text, encoding="utf-8")


def _symlink(link: Path, target: str, *, directory: bool) -> None:
    """Make `link` a relative symlink to `target`, which must resolve through it.

    The target is handed over as a `Path` so Windows stores it with its own
    separator: a relative target written with `/` is one Windows keeps verbatim
    and never resolves, and a link that dangles is no directory for a rule to
    find. A host that may not create a symlink at all — a Windows account
    without the privilege — cannot build these trees, so that is reported as
    the reason this test did not run rather than as a finding the rule missed.
    """
    try:
        link.symlink_to(Path(target), target_is_directory=directory)
    except OSError as refused:
        pytest.skip(f"this host may not create a symlink, so the tree cannot be built: {refused}")
    reached = link.is_dir() if directory else link.is_file()
    truth(reached, describing=f"the symlink `{link.name}` resolves to `{target}` on this host")


def _finished(root: Path) -> Repo:
    """The finished layout: one real skill directory holding its references."""
    _write(root, "repo-policy.toml", DECLARED)
    _write(root, SKILL, SKILL_TEXT)
    _write(
        root, "skills/printobserver/reference/a.md", "# A\n\nBack to [the skill](../SKILL.md).\n"
    )
    return Repo(root)


def test_the_committed_layout_is_accepted_by_every_rule(committed: Repo) -> None:
    """The tree this repository ships links through no symlink and carries its skill whole."""
    accepted(link_symlinks(committed), describing="the committed documents' links")
    accepted(skill_directory(committed), describing="the committed skill's directory")
    accepted(skill(committed), describing="the committed skill")


def test_the_finished_layout_is_accepted(tmp_path: Path) -> None:
    """A real directory, a link down into it and one back up inside it: nothing to refuse."""
    repo = _finished(tmp_path)

    accepted(link_symlinks(repo), describing="links inside one real directory")
    accepted(skill_directory(repo), describing="a skill directory with no symlink")


def test_a_link_through_a_directory_symlink_is_refused(tmp_path: Path) -> None:
    """The base layout: a document in a crate's assets beside a symlink to the documents."""
    _write(tmp_path, "docs/reference/a.md", "# A\n")
    _write(tmp_path, "crates/x/assets/skill.md", "# A skill\n\n[a](reference/a.md#top)\n")
    _symlink(tmp_path / "crates/x/assets/reference", "../../../docs/reference", directory=True)

    refused_naming(
        link_symlinks(Repo(tmp_path)),
        "`crates/x/assets/skill.md`",
        "`reference/a.md#top`",
        "`crates/x/assets/reference`",
        "GitHub does not follow a directory symlink",
    )


def test_a_link_to_a_file_symlink_is_not_refused(tmp_path: Path) -> None:
    """The forge serves a symlinked file as the link it is; that is not this rule."""
    _write(tmp_path, "AGENTS.md", "# Agents\n")
    _write(tmp_path, "README.md", "# Read me\n\nSee [the notes](CLAUDE.md).\n")
    _symlink(tmp_path / "CLAUDE.md", "AGENTS.md", directory=False)

    accepted(link_symlinks(Repo(tmp_path)), describing="a link to a symlinked file")


def test_links_a_document_only_quotes_are_not_followed(tmp_path: Path) -> None:
    """A link inside a fence or a code span is shown rather than rendered."""
    _write(tmp_path, "docs/reference/a.md", "# A\n")
    _write(
        tmp_path,
        "notes.md",
        "# Notes\n\n`[a](linked/a.md)`\n\n```json\n[a](linked/a.md)\n```\n",
    )
    _symlink(tmp_path / "linked", "docs/reference", directory=True)

    accepted(link_symlinks(Repo(tmp_path)), describing="links a document only quotes")


@pytest.mark.parametrize("kind", ["file", "directory"])
def test_a_symlink_inside_the_skill_directory_is_refused(tmp_path: Path, kind: str) -> None:
    """`gh skill install` drops it, so an installed skill would not carry it."""
    repo = _finished(tmp_path)
    _write(tmp_path, "elsewhere/b.md", "# B\n")
    if kind == "file":
        _symlink(
            tmp_path / "skills/printobserver/reference/b.md",
            "../../../elsewhere/b.md",
            directory=False,
        )
    else:
        _symlink(tmp_path / "skills/printobserver/more", "../../elsewhere", directory=True)

    refused_naming(
        skill_directory(repo),
        "a symlink under the skill's directory",
        "`skills/printobserver/reference/b.md`"
        if kind == "file"
        else "`skills/printobserver/more`",
        "`gh skill install` installs the skill's directory alone",
    )


def test_a_skill_link_climbing_out_of_its_directory_is_refused(tmp_path: Path) -> None:
    """The document is there in a checkout and nowhere an install puts the skill."""
    repo = _finished(tmp_path)
    _write(tmp_path, "README.md", "# Read me\n")
    _write(tmp_path, SKILL, SKILL_TEXT + "\nSee [the readme](../../README.md).\n")

    refused_naming(
        skill_directory(repo),
        f"`{SKILL}`",
        "`../../README.md`",
        "resolves outside the skill's directory",
        "`gh skill install` installs the skill's directory alone",
    )


def test_a_reference_link_climbing_out_of_the_skill_directory_is_refused(tmp_path: Path) -> None:
    """A document under the skill's directory is installed with it, and held the same way."""
    repo = _finished(tmp_path)
    _write(tmp_path, "AGENTS.md", "# Agents\n")
    _write(tmp_path, "skills/printobserver/reference/a.md", "# A\n\n[x](../../../AGENTS.md)\n")

    refused_naming(
        skill_directory(repo),
        "`skills/printobserver/reference/a.md`",
        "`../../../AGENTS.md`",
        "resolves outside",
    )


def test_a_skill_link_to_a_directory_is_refused(tmp_path: Path) -> None:
    """A link has to name a document an agent can read, not a directory of them."""
    repo = _finished(tmp_path)
    _write(tmp_path, SKILL, SKILL_TEXT + "\nAll [of them](reference/).\n")

    refused_naming(skill_directory(repo), f"`{SKILL}`", "`reference/`", "not a regular file inside")
