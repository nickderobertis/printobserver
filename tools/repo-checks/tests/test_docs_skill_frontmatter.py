"""The skill is an Agent Skill: frontmatter first, and the prose after it measured.

`gh skill install` reads a `SKILL.md` by agentskills.io's rules, and a turn is
sent the prose after the frontmatter rather than the file. So the frontmatter is
held to those rules — a YAML mapping, a `name` that is its directory's and is
spelled the way those rules allow, a `description` a reader can be shown — and
every bound and marker the skill check carries is measured over the prose alone.

Each tree here is the smallest one that is genuinely the check's input: a
`[docs]` declaration, a generated surface manifest, and the skill under a
directory named the way the committed one is.
"""

from __future__ import annotations

import json
from pathlib import Path

import pytest
from repo_checks.checks_docs import skill
from repo_checks.docs import UnclosedFrontmatterError, skill_prose
from repo_checks.expect import accepted, equal, refused, refused_naming
from repo_checks.model import Repo

#: Where the skill sits, named the way the committed one is.
SKILL = "skills/printobserver/SKILL.md"

#: A `[docs]` declaration whose one document and one element the prose below owes.
DECLARED = f"""\
[docs]
skill = "{SKILL}"
skill_max_characters = 300
skill_max_lines = 12
surface_manifest = "surface.json"
schema_document = "skills/printobserver/reference/schemas.md"
schema_directory = "schemas"
example_fence = "console"

[[docs.skill_element]]
name = "the role"
marker = "## The role"

[[docs.document]]
path = "skills/printobserver/reference/guide.md"
headings = ["What it covers"]
"""

#: A frontmatter block every rule accepts.
FRONTMATTER = "---\nname: printobserver\ndescription: Supervise one 3D print.\nlicense: MIT\n---\n"

#: Prose every rule accepts: a heading, the element's marker, and the one link.
PROSE = "# Supervising a print\n\n## The role\n\nRead [the guide](reference/guide.md).\n"


def _skill_tree(root: Path, text: str) -> Repo:
    """A repository whose skill is the text given, beside everything else it reads."""
    files = {
        "repo-policy.toml": DECLARED,
        "surface.json": json.dumps({"commands": [{"command": "status", "options": []}]}),
        "skills/printobserver/reference/guide.md": "# The guide\n",
        SKILL: text,
    }
    for relative, body in files.items():
        path = root / relative
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_text(body, encoding="utf-8")
    return Repo(root)


def test_a_skill_every_rule_accepts_is_accepted(tmp_path: Path) -> None:
    """The fixture is right, so every refusal below is the defect it introduces."""
    accepted(skill(_skill_tree(tmp_path, FRONTMATTER + PROSE)), describing="the fixture skill")


#: The inputs and answers the adapter's own split is held to, beside its suite.
SHARED_CASES = (
    Path(__file__).resolve().parents[3]
    / "crates/printobserver-oneharness/tests/supervision/skill-prose-cases.json"
)


def test_the_split_answers_every_case_the_adapters_split_is_held_to() -> None:
    """One file of cases, and both splits run over it: the Rust adapter's suite reads it too.

    The adapter splits a `SKILL.md` to send its prose and this check splits it to
    measure that prose; a `null` answer is a block that never closes.
    """
    cases = json.loads(SHARED_CASES.read_text(encoding="utf-8"))
    equal(bool(cases), True, describing="whether the shared cases carry any case")
    for case in cases:
        if case["prose"] is None:
            with pytest.raises(UnclosedFrontmatterError):
                skill_prose(case["text"])
            continue
        equal(
            skill_prose(case["text"]).prose, case["prose"], describing=f"the case {case['case']!r}"
        )


def test_the_split_is_the_adapters(tmp_path: Path) -> None:
    """Frontmatter between the fences, prose after them, and the whole text when none opens."""
    split = skill_prose("---\r\nname: x\r\n---\r\n# Prose\n")
    equal(split.frontmatter, "name: x\r\n", describing="the frontmatter")
    equal(split.prose, "# Prose\n", describing="the prose")
    equal(split.offset, 3, describing="the lines before the prose")

    unfenced = skill_prose("# Prose\n---\n")
    equal(unfenced.frontmatter, None, describing="a skill opening with no fence")
    equal(unfenced.prose, "# Prose\n---\n", describing="the prose of a skill with no fence")

    with pytest.raises(UnclosedFrontmatterError):
        skill_prose("---\nname: x\n")


@pytest.mark.parametrize(
    ("text", "naming"),
    [
        (PROSE, "does not open with a `---` frontmatter block"),
        ("---\nname: printobserver\n" + PROSE, "no later line closes it"),
        ("---\n- a list\n---\n" + PROSE, "frontmatter is not a YAML mapping"),
        ("---\nname: [unclosed\n---\n" + PROSE, "frontmatter is not YAML"),
    ],
)
def test_a_frontmatter_block_that_is_not_a_mapping_is_refused(
    tmp_path: Path, text: str, naming: str
) -> None:
    """Missing, unclosed or not a mapping, the block carries no `name` to install by."""
    refused_naming(skill(_skill_tree(tmp_path, text)), f"`{SKILL}`", naming)


@pytest.mark.parametrize(
    ("name", "naming"),
    [
        ("print-observer", "the directory the skill is in is `printobserver`"),
        ("Print--Observer", "not 1 to 64 lowercase ASCII letters"),
        ("-printobserver", "not 1 to 64 lowercase ASCII letters"),
        ("printobserver-", "not 1 to 64 lowercase ASCII letters"),
        ("print--observer", "not 1 to 64 lowercase ASCII letters"),
        ("p" * 65, "not 1 to 64 lowercase ASCII letters"),
        ("42", "absent or not a string"),
    ],
)
def test_a_name_that_is_not_its_directorys_or_breaks_the_rules_is_refused(
    tmp_path: Path, name: str, naming: str
) -> None:
    """An Agent Skill is named by its directory, spelled the way agentskills.io allows."""
    text = FRONTMATTER.replace("name: printobserver", f"name: {name}") + PROSE

    refused_naming(skill(_skill_tree(tmp_path, text)), f"`{SKILL}`'s frontmatter `name`", naming)


def test_a_frontmatter_with_no_name_is_refused(tmp_path: Path) -> None:
    """Without a name there is nothing to install the skill as."""
    text = FRONTMATTER.replace("name: printobserver\n", "") + PROSE

    refused_naming(skill(_skill_tree(tmp_path, text)), f"`{SKILL}`'s frontmatter `name`", "absent")


@pytest.mark.parametrize(
    ("description", "naming"),
    [
        (None, "absent or empty"),
        ('""', "absent or empty"),
        ("'   '", "absent or empty"),
        ("[a, list]", "absent or empty"),
        ("d" * 1025, "is 1025 characters"),
    ],
)
def test_a_description_that_is_missing_empty_or_too_long_is_refused(
    tmp_path: Path, description: str | None, naming: str
) -> None:
    """The description is what says when the skill applies, in at most 1,024 characters."""
    replacement = "" if description is None else f"description: {description}\n"
    text = FRONTMATTER.replace("description: Supervise one 3D print.\n", replacement) + PROSE

    refused_naming(
        skill(_skill_tree(tmp_path, text)), f"`{SKILL}`'s frontmatter `description`", naming
    )


def test_a_description_of_exactly_the_bound_is_accepted(tmp_path: Path) -> None:
    """1,024 characters is the bound, not one over it."""
    text = FRONTMATTER.replace("Supervise one 3D print.", "d" * 1024) + PROSE

    accepted(skill(_skill_tree(tmp_path, text)), describing="a 1,024-character description")


def test_prose_that_does_not_open_with_a_heading_is_refused(tmp_path: Path) -> None:
    """What a turn is sent opens by saying what it is."""
    text = FRONTMATTER + "\n" + PROSE

    refused_naming(skill(_skill_tree(tmp_path, text)), f"`{SKILL}`'s prose", "`# ` heading")


def test_the_bounds_are_measured_over_the_prose_alone(tmp_path: Path) -> None:
    """A long frontmatter is not sent to a turn, so it spends none of the bound.

    The frontmatter here carries more characters and more lines than both
    bounds allow on their own, and the prose after it fits inside them.
    """
    padding = "".join(f"x-note-{index}: {'n' * 40}\n" for index in range(12))
    text = FRONTMATTER.replace("license: MIT\n", padding) + PROSE
    equal(len(text) > 300 and len(text.splitlines()) > 12, True, describing="the fixture")

    accepted(skill(_skill_tree(tmp_path, text)), describing="a long frontmatter over short prose")


def test_prose_over_the_bounds_is_refused(tmp_path: Path) -> None:
    """The prose itself is still held to both bounds."""
    text = FRONTMATTER + PROSE + "".join(f"A line of prose {index}.\n" for index in range(20))

    findings = skill(_skill_tree(tmp_path, text))

    refused_naming(findings, f"`{SKILL}`'s prose", "characters, and the bound is 300")
    refused_naming(findings, f"`{SKILL}`'s prose", "lines, and the bound is 12")


def test_a_marker_only_the_frontmatter_carries_is_refused(tmp_path: Path) -> None:
    """The frontmatter is not what a turn is sent, so a marker there is carried nowhere."""
    text = FRONTMATTER.replace("license: MIT", "role: '## The role'") + PROSE.replace(
        "## The role\n\n", ""
    )

    refused(skill(_skill_tree(tmp_path, text)), "carries nothing saying the role")


def test_reference_material_is_reported_at_its_line_of_the_file(tmp_path: Path) -> None:
    """A finding in the prose names the line a reader opens the file at."""
    text = FRONTMATTER + PROSE + "\n```console\n"

    refused_naming(skill(_skill_tree(tmp_path, text)), f"`{SKILL}`:12", "opens a fenced block")
