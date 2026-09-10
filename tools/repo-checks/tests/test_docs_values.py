"""Documentation rules exercised with inline values, without copying shipped documents."""

from __future__ import annotations

from pathlib import Path

import pytest
from repo_checks.checks_docs import _document_frame, _reference_material, _skill_links, skill
from repo_checks.docs import Document, docs_policy
from repo_checks.expect import refused
from repo_checks.model import Repo
from test_docs_policy import DECLARED


@pytest.mark.parametrize(
    ("text", "finding"),
    [
        ("x" * 6001, "characters, and the bound is 6000"),
        ("x\n" * 151, "lines, and the bound is 150"),
        ("Supervise the print.\n", "carries nothing saying the role"),
    ],
)
def test_skill_bounds_and_required_content(tmp_path: Path, text: str, finding: str) -> None:
    """A small declaration and one inline skill reach the public check directly."""
    declaration = DECLARED.replace("characters = 100", "characters = 6000").replace(
        "lines = 10", "lines = 150"
    )
    (tmp_path / "repo-policy.toml").write_text(declaration, encoding="utf-8")
    (tmp_path / "skill.md").write_text(text, encoding="utf-8")

    refused(skill(Repo(tmp_path)), finding)


@pytest.mark.parametrize(
    ("text", "finding"),
    [
        ("```console\nprintobserver context\n```", "opens a fenced block"),
        ("Run this:\n\n    printobserver context", "is an indented block"),
        ("| Option | Meaning |", "is a table row"),
        ("The adjustment takes print_id and duration_s.", "names `print_id`"),
        ("- --print-id identifies the print", "names the option `--print-id`"),
        ("The answer carries `recent_events`.", "part of the command surface"),
        ('The answer is {"type": "object"}.', "carries a schema or document fragment"),
    ],
)
def test_reference_material_belongs_outside_the_skill(
    committed: Repo, text: str, finding: str
) -> None:
    """Inline prose is checked against the real surface and schema vocabulary."""
    refused(_reference_material(committed, docs_policy(committed), text), finding)


@pytest.mark.parametrize(
    ("text", "finding"),
    [
        ("[Guide](reference/absent.md)", "there is no such document"),
        ("[Guide](../guide.md)", "climbs out of the skill's own directory"),
        ("[Guide](https://example.invalid/guide.md)", "not a path beside the skill"),
        ("Read the guide.", "links to no declared reference document"),
    ],
)
def test_skill_links_must_reach_installed_references(
    committed: Repo, text: str, finding: str
) -> None:
    """One inline link exercises each resolution failure without editing any document."""
    refused(_skill_links(committed, docs_policy(committed), text), finding)


@pytest.mark.parametrize(
    ("text", "finding"),
    [
        ("# Guide\n\n## Usage\n", "carries no opening coverage statement"),
        ("# Guide\n\nCovers Usage.\n", "does not carry the declared heading"),
        ("# Guide\n\nCovers Usage and Errors.\n\n## Usage\n", "says it covers `Errors`"),
        ("# Guide\n\nCovers Errors.\n\n## Usage\n", "opening statement does not say"),
        ("# Guide\n\nCovers Usage.\n\n## Usage\n### status\n", "entry says nothing"),
    ],
)
def test_reference_openings_and_sections_agree(text: str, finding: str) -> None:
    """A document declaration and a few lines of Markdown expose framing mistakes."""
    refused(_document_frame(Document("guide.md", ("Usage",)), text, {"Usage", "Errors"}), finding)
