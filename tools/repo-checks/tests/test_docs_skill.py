"""The skill is short, carries what it owes, and restates nothing.

The positive half drives the committed check over the committed tree. The
negative half hands each of the check's own rules a bad value and reads the
finding back — no copy of this repository is assembled to do it, because the
rules are functions over the skill's text and a string is the cheapest thing
that is genuinely one of their inputs.
"""

from __future__ import annotations

from pathlib import Path

from repo_checks.checks_docs import (
    _bounds,
    _bundled_references,
    _missing_elements,
    _reference_material,
    _skill_links,
    skill,
)
from repo_checks.docs import DocsPolicy, docs_policy
from repo_checks.expect import accepted, equal, refused
from repo_checks.model import Repo


def _policy(committed: Repo) -> DocsPolicy:
    """The `[docs]` declarations the skill's rules are read out of."""
    return docs_policy(committed)


def _skill_text(committed: Repo) -> str:
    """The committed skill, which every negative below starts from."""
    return committed.read(_policy(committed).skill)


def test_the_committed_skill_is_accepted(committed: Repo) -> None:
    """The skill this repository ships passes every rule below."""
    accepted(skill(committed))


def test_the_check_enforces_the_bounds_this_repository_declares(committed: Repo) -> None:
    """The two numbers are 6,000 characters and 150 lines.

    A check free to choose its own budget could hold the skill to one no skill
    could exceed, and would pass every test in this file while buying none of
    the discipline the bounds are for.
    """
    policy = _policy(committed)

    equal(policy.skill_max_characters, 6000, describing="the character bound")
    equal(policy.skill_max_lines, 150, describing="the line bound")


def test_a_skill_over_the_character_bound_is_refused(committed: Repo) -> None:
    """One very long line satisfies a line count and is refused all the same."""
    policy = _policy(committed)

    refused(
        _bounds(policy, "A sentence about nothing. " * 400),
        "characters, and the bound is 6000",
    )


def test_a_skill_over_the_line_bound_is_refused(committed: Repo) -> None:
    """A wall of short lines satisfies a character count and is refused too."""
    policy = _policy(committed)

    refused(_bounds(policy, "x\n" * 200), "lines, and the bound is 150")


def test_a_skill_carrying_a_fenced_example_is_refused(committed: Repo) -> None:
    """A worked example belongs in a reference document."""
    policy = _policy(committed)
    text = "```console\nprintobserver context\n```\n"

    refused(_reference_material(committed, policy, text), "opens a fenced block")


def test_a_skill_carrying_an_indented_example_is_refused(committed: Repo) -> None:
    """The same example without a fence is the same example."""
    policy = _policy(committed)
    text = "Run it like this:\n\n    printobserver context\n"

    refused(_reference_material(committed, policy, text), "is an indented block")


def test_a_skill_carrying_an_argument_table_is_refused(committed: Repo) -> None:
    """An argument table belongs in a reference document."""
    policy = _policy(committed)
    text = "| Option | Meaning |\n| ------ | ------- |\n"

    refused(_reference_material(committed, policy, text), "is a table row")


def test_a_skill_carrying_an_argument_list_as_prose_is_refused(committed: Repo) -> None:
    """A list of arguments written as a sentence is still a list of arguments."""
    policy = _policy(committed)
    text = (
        "The adjustment takes print_id, factor and reason, and duration_s when the "
        "change is meant to be temporary.\n"
    )

    refused(_reference_material(committed, policy, text), "names `print_id`")


def test_a_skill_carrying_an_argument_list_as_bullets_is_refused(committed: Repo) -> None:
    """And so is one written as bullets rather than as a table."""
    policy = _policy(committed)
    text = "- --print-id — the print it is about\n- --factor — the multiplier asked for\n"

    refused(_reference_material(committed, policy, text), "names the option `--print-id`")


def test_a_skill_describing_an_output_shape_is_refused(committed: Repo) -> None:
    """What a command answers is a reference document's to describe."""
    policy = _policy(committed)
    text = "The read answers `recent_events` and `interventions` beside the rest.\n"

    refused(_reference_material(committed, policy, text), "which is part of the command surface")


def test_a_skill_carrying_a_schema_fragment_is_refused(committed: Repo) -> None:
    """The schemas are generated into a reference document of their own."""
    policy = _policy(committed)
    text = 'The answer looks like {"type": "object"} at the top.\n'

    refused(_reference_material(committed, policy, text), "carries a schema or document fragment")


def test_a_skill_linking_to_a_document_that_is_not_there_is_refused(committed: Repo) -> None:
    """A link the reader cannot follow is worse than no link."""
    policy = _policy(committed)
    text = f"{_skill_text(committed)}\n- [Something else](reference/nothing-here.md)\n"

    refused(_skill_links(committed, policy, text), "links to `reference/nothing-here.md`")


def test_a_skill_omitting_a_declared_document_is_refused(committed: Repo) -> None:
    """Everything the skill does not say has to be reachable from it."""
    policy = _policy(committed)
    dropped = policy.documents[-1].path
    # The skill links to it by the path it sits at beside the skill, which is
    # the path an installed program reproduces.
    linked = f"reference/{Path(dropped).name}"
    text = "\n".join(
        line for line in _skill_text(committed).splitlines() if f"({linked})" not in line
    )

    refused(
        _skill_links(committed, policy, text),
        f"links to no declared reference document `{dropped}`",
    )


def test_a_skill_link_that_climbs_out_of_its_own_directory_is_refused(committed: Repo) -> None:
    """An installed program can only carry what sits beside the skill it wrote."""
    policy = _policy(committed)
    text = f"{_skill_text(committed)}\n- [Somewhere else](../../../docs/reference/testing.md)\n"

    refused(_skill_links(committed, policy, text), "which climbs out of the skill's own directory")


def test_a_skill_link_to_an_address_rather_than_a_path_is_refused(committed: Repo) -> None:
    """A link off the host is one an agent with no network cannot follow."""
    policy = _policy(committed)
    text = f"{_skill_text(committed)}\n- [Somewhere else](https://example.invalid/testing.md)\n"

    refused(_skill_links(committed, policy, text), "which is not a path beside the skill")


def test_a_skill_omitting_any_of_the_nine_things_it_owes_is_refused(committed: Repo) -> None:
    """Being short, restating nothing and linking correctly is not enough.

    Every other rule about the skill is about what it may not carry. These are
    what it owes, and they cannot be left to a linked document: the skill is the
    whole of what the agent is given before it starts reading, so a workflow step
    it does not state is one the agent meets after it has already decided.
    """
    policy = _policy(committed)
    elements = policy.elements
    equal(len(elements), 9, describing="the things the skill owes")

    # The rule matches its markers over whitespace-flowed text, because a
    # passage that wraps at a different column is the same passage. So the
    # passage is taken out of the flowed form, which is the value the rule reads.
    flowed = " ".join(_skill_text(committed).split())
    for element in elements:
        marker = " ".join(element.marker.split())
        without = flowed.replace(marker, "")
        equal(without == flowed, False, describing=f"the skill carrying {element.name}")

        refused(_missing_elements(policy, without), f"carries nothing saying {element.name}")


def test_a_declared_document_the_artifact_does_not_bundle_is_refused(committed: Repo) -> None:
    """An install carrying the skill and not what it points at is dead links.

    The skill's links resolve in a checkout whether or not the built artifact
    carries the documents, so nothing about the skill itself catches this: it
    shows up in front of an agent on a host with no repository. This is what
    catches it here instead.
    """
    policy = _policy(committed)
    dropped = policy.documents[-1].path
    source = "\n".join(
        line
        for line in committed.read(policy.bundle_source).splitlines()
        if f"{policy.bundle_directory}/{Path(dropped).name}" not in line
    )

    refused(
        _bundled_references(committed, policy, source),
        f"bundles no reference document for `{dropped}`",
    )


def test_a_bundled_document_this_repository_does_not_declare_is_refused(committed: Repo) -> None:
    """And a document travelling in the artifact that nothing declares is refused too."""
    policy = _policy(committed)
    source = (
        f"{committed.read(policy.bundle_source)}\n"
        f'include_str!("../assets/{policy.bundle_directory}/loose.md")\n'
    )

    refused(
        _bundled_references(committed, policy, source),
        "which this repository declares no reference document for",
    )
