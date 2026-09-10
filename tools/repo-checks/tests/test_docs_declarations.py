"""The documentation checks read the tree, and say so when the tree reads back nothing.

Every rule these four checks enforce is read off something else: `[docs]` in
`repo-policy.toml`, the generated surface manifest, the generated schema set,
the committed workflows. What is proven here is what each of them does when the
thing it reads is absent, or is not what it reads it as — because a check that
quietly read nothing would accept every document in the tree while proving none
of them.

Every test drives the committed check against a real copy of the committed tree
with one such defect in it. Nothing is mocked.
"""

from __future__ import annotations

from collections.abc import Callable

from repo_checks.checks_docs import reference, schema_document, skill
from repo_checks.expect import accepted, refused
from treecopy import Tree

POLICY = "repo-policy.toml"
SKILL = "crates/printobserver-oneharness/assets/printobserver-skill.md"
SURFACE = "docs/reference/surface.json"
SCHEMAS = "docs/reference/schemas.md"
ARCHITECTURE = "docs/reference/architecture.md"
API = "docs/reference/api-and-clients.md"
TESTING = "docs/reference/testing.md"
INTERVENTION = "docs/reference/intervention-policy.md"

#: Where `[docs]` opens, and where each of its arrays of tables does.
DOCS_SECTION = "\n[docs]\n"
FIRST_ELEMENT = "[[docs.skill_element]]\n"
FIRST_DOCUMENT = "[[docs.document]]\n"


def _cut(copy: Tree, opening: str, closing: str | None = None) -> None:
    """Cut `repo-policy.toml` from `opening` to `closing`, or to its end."""
    text = copy.read(POLICY)
    start = text.index(opening)
    end = len(text) if closing is None else text.index(closing)
    copy.write(POLICY, text[:start] + text[end:])


def test_a_tree_declaring_no_docs_section_is_refused_by_every_check(
    tree: Callable[[], Tree],
) -> None:
    """`[docs]` is the one declaration of what is documented; without it nothing is."""
    copy = tree()
    _cut(copy, DOCS_SECTION)

    for findings in (
        skill(copy.repo),
        reference(copy.repo),
        schema_document(copy.repo),
    ):
        refused(findings, "declares no `[docs]` section")


def test_a_skill_bound_that_is_not_a_whole_number_is_refused(tree: Callable[[], Tree]) -> None:
    """A bound the check cannot compare against is a bound nothing enforces."""
    copy = tree()
    copy.edit(POLICY, "skill_max_characters = 6000", 'skill_max_characters = "6000"')

    refused(skill(copy.repo), "no positive `docs.skill_max_characters` whole number")


def test_a_policy_declaring_no_skill_element_is_refused(tree: Callable[[], Tree]) -> None:
    """Every other rule is about what the skill may not carry; these are what it owes."""
    copy = tree()
    _cut(copy, FIRST_ELEMENT, FIRST_DOCUMENT)

    refused(skill(copy.repo), "declares no `docs.skill_element`")


def test_a_skill_element_declared_as_anything_but_a_table_is_refused(
    tree: Callable[[], Tree],
) -> None:
    """A key of the right name and the wrong shape owes the skill nothing either."""
    copy = tree()
    _cut(copy, FIRST_ELEMENT, FIRST_DOCUMENT)
    copy.edit(
        POLICY,
        "skill_max_characters = 6000",
        'skill_element = "the role"\nskill_max_characters = 6000',
    )

    refused(skill(copy.repo), "declares no `docs.skill_element`")


def test_a_policy_declaring_no_document_is_refused(tree: Callable[[], Tree]) -> None:
    """The skill links out for everything; a policy naming no document declares no links."""
    copy = tree()
    _cut(copy, FIRST_DOCUMENT)

    refused(skill(copy.repo), "declares no `docs.document`")


def test_a_tree_with_no_generated_surface_manifest_is_refused(tree: Callable[[], Tree]) -> None:
    """It is what both checks read this program's command surface off."""
    copy = tree()
    copy.remove(SURFACE)

    refused(skill(copy.repo), f"`{SURFACE}` is absent")
    refused(reference(copy.repo), "Run `just docs-generate`")


def test_a_surface_manifest_that_is_not_json_is_refused(tree: Callable[[], Tree]) -> None:
    """A half-written generated artifact is a check reading a surface that is not there."""
    copy = tree()
    copy.write(SURFACE, '{"commands": [')

    refused(skill(copy.repo), f"`{SURFACE}` is not JSON")


def test_a_surface_manifest_naming_no_command_is_refused(tree: Callable[[], Tree]) -> None:
    """A manifest of no commands would make every inventory complete by inventorying nothing."""
    copy = tree()
    copy.write(SURFACE, '{"commands": [], "global_options": []}')

    refused(reference(copy.repo), f"`{SURFACE}` names no command")


def test_a_tree_with_no_committed_skill_is_refused(tree: Callable[[], Tree]) -> None:
    """The skill is the system prompt of every supervision turn."""
    copy = tree()
    copy.remove(SKILL)

    refused(skill(copy.repo), f"the committed skill `{SKILL}` is absent")


def test_a_tree_whose_bundle_source_is_absent_is_refused(tree: Callable[[], Tree]) -> None:
    """Nothing carries the documents onto an installed host without it."""
    copy = tree()
    copy.remove("crates/printobserver-oneharness/src/lib.rs")

    refused(skill(copy.repo), "the source that bundles the reference documents")


def test_a_tree_with_no_generated_schema_document_is_refused(tree: Callable[[], Tree]) -> None:
    """It is generated rather than written, so an absent one is a generation nobody ran."""
    copy = tree()
    copy.remove(SCHEMAS)

    refused(schema_document(copy.repo), f"`{SCHEMAS}` is absent")


def test_a_tree_whose_contracts_generate_no_schema_at_all_is_refused(
    tree: Callable[[], Tree],
) -> None:
    """A document complete against an empty set is complete against nothing."""
    copy = tree()
    copy.remove("schemas")

    refused(schema_document(copy.repo), "declares no schema at all")


def test_a_schema_that_is_not_json_is_read_past_rather_than_raised_on(
    tree: Callable[[], Tree],
) -> None:
    """The skill check reads the set for vocabulary, so one it cannot parse is one it skips."""
    copy = tree()
    copy.write("schemas/printobserver-types/PrintAction.json", "{")

    accepted(skill(copy.repo), describing="a skill beside a schema that is not JSON")


def test_a_tree_whose_contracts_generate_no_rejection_type_is_refused(
    tree: Callable[[], Tree],
) -> None:
    """The fields the policy document owes are the ones that type declares."""
    copy = tree()
    copy.remove("schemas/printobserver-types/RejectionReason.json")

    refused(reference(copy.repo), "no `RejectionReason` schema")


def test_a_policy_declaring_no_core_crate_is_refused(tree: Callable[[], Tree]) -> None:
    """The dependency direction the architecture document states is stated about that crate."""
    copy = tree()
    copy.edit(POLICY, '\ncore = "printobserver-core"\n', "\n")

    refused(reference(copy.repo), "declares no `crates.core`")


def test_an_architecture_document_stating_no_dependency_direction_is_refused(
    tree: Callable[[], Tree],
) -> None:
    """A section saying why the rule holds and never saying what it is holds nothing to the tree."""
    copy = tree()
    copy.edit(
        ARCHITECTURE,
        "prose: `printobserver-core` depends\n",
        "prose, the edge is one a check reads:\n",
    )

    refused(reference(copy.repo), "states no dependency direction")


def test_an_api_document_with_no_entry_for_a_declared_client_is_refused(
    tree: Callable[[], Tree],
) -> None:
    """A client with no entry is one whose methods nothing is read against."""
    copy = tree()
    text = copy.read(API)
    copy.write(API, text[: text.index("### printobserver-sdk")])

    refused(reference(copy.repo), "carries no entry for the client `printobserver-sdk`")


def test_a_declared_client_the_document_has_no_entry_for_is_refused(
    tree: Callable[[], Tree],
) -> None:
    """The clients the document owes are read off the declared list, so adding one owes an entry."""
    copy = tree()
    copy.edit(
        POLICY,
        'clients = ["printobserver-sdk"]',
        'clients = ["printobserver-sdk", "printobserver-sdk-node"]',
    )

    refused(reference(copy.repo), "carries no entry for the client `printobserver-sdk-node`")


def test_an_integration_table_declaring_no_tier_names_no_tier_to_document(
    tree: Callable[[], Tree],
) -> None:
    """The tiers the testing document owes are the ones the configuration declares."""
    copy = tree()
    copy.edit(POLICY, '\ntier = "test-integration"\n', "\n")

    refused(reference(copy.repo), "carries an entry `test-integration`, and there is no such tier")


def test_a_workflow_that_gives_a_tier_no_cron_requires_none_of_the_document(
    tree: Callable[[], Tree],
) -> None:
    """The schedule the document owes is read off the committed workflow, not written here."""
    without_schedule = tree()
    text = without_schedule.read(".github/workflows/obico.yml")
    without_schedule.write(
        ".github/workflows/obico.yml",
        text.replace('  schedule:\n    - cron: "17 4 * * 1"\n', ""),
    )

    accepted(reference(without_schedule.repo), describing="a tier whose workflow schedules nothing")

    without_cron = tree()
    without_cron.edit(".github/workflows/obico.yml", '- cron: "17 4 * * 1"', "- {}")

    accepted(reference(without_cron.repo), describing="a schedule entry naming no cron")


def test_a_scheduled_tier_the_testing_document_omits_is_refused(tree: Callable[[], Tree]) -> None:
    """A tier with no entry is one no schedule is read against."""
    copy = tree()
    text = copy.read(TESTING)
    copy.write(TESTING, text[: text.index("### test-obico")])

    refused(reference(copy.repo), "carries no entry for the tier `test-obico`")


def test_an_intervention_policy_naming_no_source_for_the_bounds_is_refused(
    tree: Callable[[], Tree],
) -> None:
    """The source it names is the one the server reads the envelope from, held to the tree."""
    copy = tree()
    copy.edit(
        INTERVENTION,
        "in `crates/printobserver-server/src/config.rs`,\n",
        "from where it is configured,\n",
    )

    refused(reference(copy.repo), "names no source the server reads the safety envelope from")


def test_an_intervention_policy_missing_the_bounds_heading_names_no_source_either(
    tree: Callable[[], Tree],
) -> None:
    """A section that is not there reads as a section that says nothing."""
    copy = tree()
    copy.edit(INTERVENTION, "## Where the bounds come from\n", "")

    refused(reference(copy.repo), "names no source the server reads the safety envelope from")


def test_a_document_that_is_a_title_and_a_sentence_is_refused_for_every_heading_it_owes(
    tree: Callable[[], Tree],
) -> None:
    """A placeholder file is a declared document a reader reaches and learns nothing from."""
    copy = tree()
    copy.write(TESTING, "# Testing\n\nThis document covers the tiers.")

    refused(reference(copy.repo), f"`{TESTING}` does not carry the declared heading")


def test_a_document_opening_with_a_line_above_its_title_still_says_what_it_covers(
    tree: Callable[[], Tree],
) -> None:
    """The coverage statement is the paragraph after the title, whatever precedes it."""
    copy = tree()
    copy.write(INTERVENTION, "[//]: # (generated by nothing)\n" + copy.read(INTERVENTION))

    accepted(reference(copy.repo), describing="a document carrying a line above its title")
