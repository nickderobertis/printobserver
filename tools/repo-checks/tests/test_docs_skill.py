"""The skill is short, carries what it owes, and restates nothing.

Every test here drives the committed check against a real copy of the committed
tree with one defect in it. The defects are the ones the skill's own rules are
about: it is over either bound, it carries reference material in one of the
forms reference material takes, it links to a document that is not there, it
omits a link to one that is, or one of the nine things it owes is gone.
"""

from __future__ import annotations

from collections.abc import Callable

from repo_checks.checks_docs import skill
from repo_checks.docs import docs_policy
from repo_checks.expect import accepted, equal, refused
from repo_checks.model import Repo
from treecopy import Tree

SKILL = "crates/printobserver-oneharness/assets/printobserver-skill.md"


def test_the_committed_skill_is_accepted(committed: Repo) -> None:
    """The skill this repository ships passes every rule below."""
    accepted(skill(committed))


def test_the_check_enforces_the_bounds_this_repository_declares(committed: Repo) -> None:
    """The two numbers are 6,000 characters and 150 lines.

    A check free to choose its own budget could hold the skill to one no skill
    could exceed, and would pass every test in this file while buying none of
    the discipline the bounds are for.
    """
    policy = docs_policy(committed)

    equal(policy.skill_max_characters, 6000, describing="the character bound")
    equal(policy.skill_max_lines, 150, describing="the line bound")


def test_a_skill_over_the_character_bound_is_refused(tree: Callable[[], Tree]) -> None:
    """One very long line satisfies a line count and is refused all the same."""
    copy = tree()
    copy.append(SKILL, "\nA sentence about nothing. " * 400 + "\n")

    refused(skill(copy.repo), "characters, and the bound is 6000")


def test_a_skill_over_the_line_bound_is_refused(tree: Callable[[], Tree]) -> None:
    """A wall of short lines satisfies a character count and is refused too."""
    copy = tree()
    copy.append(SKILL, "\n" + "x\n" * 200)

    refused(skill(copy.repo), "lines, and the bound is 150")


def test_a_skill_carrying_a_fenced_example_is_refused(tree: Callable[[], Tree]) -> None:
    """A worked example belongs in a reference document."""
    copy = tree()
    copy.append(SKILL, "\n```console\nprintobserver context\n```\n")

    refused(skill(copy.repo), "opens a fenced block")


def test_a_skill_carrying_an_indented_example_is_refused(tree: Callable[[], Tree]) -> None:
    """The same example without a fence is the same example."""
    copy = tree()
    copy.append(SKILL, "\nRun it like this:\n\n    printobserver context\n")

    refused(skill(copy.repo), "is an indented block")


def test_a_skill_carrying_an_argument_table_is_refused(tree: Callable[[], Tree]) -> None:
    """An argument table belongs in a reference document."""
    copy = tree()
    copy.append(SKILL, "\n| Option | Meaning |\n| ------ | ------- |\n")

    refused(skill(copy.repo), "is a table row")


def test_a_skill_carrying_an_argument_list_as_prose_is_refused(
    tree: Callable[[], Tree],
) -> None:
    """The same list written as prose or as bullets is the same list."""
    copy = tree()
    copy.append(
        SKILL,
        "\nThe adjustment takes the print it is about, the multiplier asked for and, "
        "optionally, the seconds it stands for.\n",
    )
    accepted(skill(copy.repo), describing="prose that names no option")

    copy.append(SKILL, "\n- the print it is about, given as --print-id\n")
    refused(skill(copy.repo), "names the option `--print-id`")


def test_a_skill_describing_an_output_shape_is_refused(tree: Callable[[], Tree]) -> None:
    """What a command answers is a reference document's to describe."""
    copy = tree()
    copy.append(
        SKILL,
        "\nThe read answers `recent_events` and `interventions` beside the rest.\n",
    )

    refused(skill(copy.repo), "which is part of the command surface")


def test_a_skill_carrying_a_schema_fragment_is_refused(tree: Callable[[], Tree]) -> None:
    """The schemas are generated into a reference document of their own."""
    copy = tree()
    copy.append(SKILL, '\nThe answer looks like {"type": "object"} at the top.\n')

    refused(skill(copy.repo), "carries a schema or document fragment")


def test_a_skill_linking_to_a_document_that_is_not_there_is_refused(
    tree: Callable[[], Tree],
) -> None:
    """A link the reader cannot follow is worse than no link."""
    copy = tree()
    copy.append(SKILL, "\n- [Something else](docs/reference/nothing-here.md)\n")

    refused(skill(copy.repo), "links to `docs/reference/nothing-here.md`")


def test_a_skill_omitting_a_declared_document_is_refused(tree: Callable[[], Tree]) -> None:
    """Everything the skill does not say has to be reachable from it."""
    copy = tree()
    dropped = docs_policy(copy.repo).documents[-1].path
    copy.write(
        SKILL,
        "\n".join(line for line in copy.read(SKILL).splitlines() if f"({dropped})" not in line)
        + "\n",
    )

    refused(skill(copy.repo), f"links to no declared reference document `{dropped}`")


def _passage_start(lines: list[str], marker: str) -> int:
    """The line one passage of the skill opens on, whatever column it wraps at."""
    wanted = " ".join(marker.split())
    for index in range(len(lines)):
        if wanted in " ".join(" ".join(lines[index:]).split()):
            continue
        return index - 1
    msg = f"the skill carries no passage marked {marker!r}"
    raise AssertionError(msg)


def _opens_a_passage(line: str) -> bool:
    """Whether one line of the skill opens a passage of its own."""
    stripped = line.lstrip()
    if line.startswith("## ") or stripped.startswith("- **"):
        return True
    head, _, rest = stripped.partition(". **")
    return bool(rest) and head.isdigit()


def _without(text: str, marker: str) -> str:
    """The skill with the passage carrying one marker taken out, and nothing else."""
    lines = text.splitlines()
    start = _passage_start(lines, marker)
    end = start + 1
    while end < len(lines) and not _opens_a_passage(lines[end]):
        end += 1
    return "\n".join(lines[:start] + lines[end:]) + "\n"


def test_a_skill_omitting_any_of_the_nine_things_it_owes_is_refused(
    tree: Callable[[], Tree],
    committed: Repo,
) -> None:
    """Being short, restating nothing and linking correctly is not enough.

    Every other rule about the skill is about what it may not carry. These are
    what it owes, and they cannot be left to a linked document: the skill is the
    whole of what the agent is given before it starts reading, so a workflow step
    it does not state is one the agent meets after it has already decided.
    """
    elements = docs_policy(committed).elements
    equal(len(elements), 9, describing="the things the skill owes")

    for element in elements:
        copy = tree()
        copy.write(SKILL, _without(copy.read(SKILL), element.marker))

        refused(skill(copy.repo), f"carries nothing saying {element.name}")
