"""The documentation checks read the tree, and say so when the tree reads back nothing.

Every rule these three checks enforce is read off something else: `[docs]` in
`repo-policy.toml`, the generated surface manifest, the generated schema set.
What is proven here is what each of them does when the thing it reads is absent,
or is not what it reads it as — because a check that quietly read nothing would
accept every document in the tree while proving none of them.

Nothing here copies this repository or its documentation. Each negative is the
smallest input that is genuinely one of these functions' own: a `[docs]`
declaration written out in a handful of lines, and, where the rule then reads a
file, that one file.
"""

from __future__ import annotations

import json
from pathlib import Path

import pytest
from repo_checks.checks_docs import (
    Surface,
    _architecture,
    _intervention_policy,
    _reference_material,
    _rejection_vocabulary,
    reference,
    schema_document,
    skill,
)
from repo_checks.docs import docs_policy
from repo_checks.expect import accepted, equal, refused
from repo_checks.model import Repo

#: A `[docs]` declaration these checks can act on, naming nothing that is there.
DECLARED = """\
[crates]
core = "printobserver-core"

[docs]
skill = "skill.md"
skill_max_characters = 100
skill_max_lines = 10
surface_manifest = "surface.json"
schema_document = "schemas.md"
schema_directory = "schemas"
example_fence = "console"
bundle_source = "bundle.rs"
bundle_assets = "assets"
bundle_directory = "reference"

[[docs.skill_element]]
name = "the role"
marker = "## The role"

[[docs.document]]
path = "guide.md"
headings = ["What it covers"]
"""

#: The one entry of a manifest that names a command, so a surface reads back.
MANIFEST = {"commands": [{"command": "status", "options": []}], "global_options": ["--json"]}

#: A surface with nothing in it, for the rules that take one and never read it.
EMPTY = Surface(commands={}, operations=(), options=frozenset(), fields=frozenset())


def _declaring(root: Path, policy: str = DECLARED) -> Repo:
    """A repository whose whole declaration is the text given."""
    (root / "repo-policy.toml").write_text(policy, encoding="utf-8")
    return Repo(root)


def _write(root: Path, relative: str, text: str) -> None:
    """Put one file in that repository, making the directories above it."""
    path = root / relative
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(text, encoding="utf-8")


def test_a_declaration_with_no_docs_section_is_refused_by_every_check(tmp_path: Path) -> None:
    """`[docs]` is the one declaration of what is documented; without it nothing is."""
    repo = _declaring(tmp_path, "schema_version = 1\n")

    for findings in (skill(repo), reference(repo), schema_document(repo)):
        refused(findings, "declares no `[docs]` section")


def test_a_skill_bound_that_is_not_a_whole_number_is_refused(tmp_path: Path) -> None:
    """A bound is a number to compare against, so a string is no bound at all."""
    repo = _declaring(tmp_path, DECLARED.replace("skill_max_lines = 10", 'skill_max_lines = "10"'))

    refused(skill(repo), "no positive `docs.skill_max_lines` whole number")


def test_a_declaration_naming_no_skill_element_is_refused(tmp_path: Path) -> None:
    """A skill that owes nothing is one every empty file satisfies."""
    repo = _declaring(tmp_path, DECLARED[: DECLARED.index("[[docs.skill_element]]")])

    refused(skill(repo), "declares no `docs.skill_element`")


def test_a_skill_element_that_names_no_marker_is_refused(tmp_path: Path) -> None:
    """An element with no marker is one no skill can be held to."""
    repo = _declaring(tmp_path, DECLARED.replace('marker = "## The role"', ""))

    refused(skill(repo), "docs.skill_element")


def test_a_declaration_naming_no_document_is_refused(tmp_path: Path) -> None:
    """A skill that links to nothing is a skill carrying everything."""
    repo = _declaring(tmp_path, DECLARED[: DECLARED.index("[[docs.document]]")])

    refused(reference(repo), "declares no `docs.document`")


def test_a_skill_that_is_not_there_is_refused(tmp_path: Path) -> None:
    """The declaration names it, so nothing having written it is a finding."""
    repo = _declaring(tmp_path)

    refused(skill(repo), "the committed skill `skill.md` is absent")


def test_a_bundle_source_that_is_not_there_is_refused(tmp_path: Path) -> None:
    """Without it nothing says what the built artifact carries."""
    repo = _declaring(tmp_path)
    _write(tmp_path, "skill.md", "# A skill\n")

    refused(skill(repo), "`bundle.rs`, is absent")


def test_a_reference_check_with_no_generated_surface_manifest_is_refused(tmp_path: Path) -> None:
    """The surface is what every inventory is read beside; absent, none of them is."""
    repo = _declaring(tmp_path)

    refused(reference(repo), "surface manifest `surface.json` is absent")


def test_a_surface_manifest_that_is_not_json_is_refused(tmp_path: Path) -> None:
    """A generated artifact that will not parse is one no check can read."""
    repo = _declaring(tmp_path)
    _write(tmp_path, "surface.json", "not json at all\n")

    refused(reference(repo), "is not JSON")


def test_a_surface_manifest_naming_no_command_is_refused(tmp_path: Path) -> None:
    """A manifest with no command in it would make every document complete."""
    repo = _declaring(tmp_path)
    _write(tmp_path, "surface.json", json.dumps({"commands": []}))

    refused(reference(repo), "names no command")


def test_a_declared_document_that_is_absent_is_refused(tmp_path: Path) -> None:
    """The skill links to it, so a reader reaches nothing."""
    repo = _declaring(tmp_path)
    _write(tmp_path, "surface.json", json.dumps(MANIFEST))

    refused(reference(repo), "`guide.md` is absent, and the skill links to it")


def test_a_schema_document_that_is_absent_is_refused(tmp_path: Path) -> None:
    """It is generated, so a tree without it is a tree nobody generated it in."""
    repo = _declaring(tmp_path)

    refused(schema_document(repo), "`schemas.md` is absent")


def test_contracts_that_generate_no_schema_at_all_are_refused(tmp_path: Path) -> None:
    """A document that inventories an empty set is complete and proves nothing."""
    repo = _declaring(tmp_path)
    _write(tmp_path, "schemas.md", "# The schemas\n")

    refused(schema_document(repo), "declares no schema at all")


def test_a_schema_that_is_not_json_is_read_past_rather_than_raised_on(tmp_path: Path) -> None:
    """The vocabulary is a best reading of the set, not a second gate over it."""
    repo = _declaring(tmp_path)
    _write(tmp_path, "surface.json", json.dumps(MANIFEST))
    _write(tmp_path, "schemas/printobserver-types/Broken.json", "{not json\n")

    accepted(
        _reference_material(repo, docs_policy(repo), "A sentence about a print.\n"),
        describing="a skill read beside a schema that will not parse",
    )


def test_an_architecture_document_with_no_core_crate_declared_is_refused(tmp_path: Path) -> None:
    """The rule is about one crate, so a declaration naming none states no rule."""
    repo = _declaring(tmp_path, DECLARED.replace('core = "printobserver-core"', ""))
    _write(tmp_path, "surface.json", json.dumps(MANIFEST))

    refused(_architecture(repo, "guide.md", "# A guide\n", EMPTY), "declares no `crates.core`")


def test_an_intervention_policy_with_no_rejection_schema_says_so(tmp_path: Path) -> None:
    """The fields a rejection carries are read off the contracts; absent, none are."""
    repo = _declaring(tmp_path)

    findings = _intervention_policy(repo, "guide.md", "# A guide\n", EMPTY)
    refused(findings, "names no source the server reads the safety envelope from")
    refused(findings, "generate no `RejectionReason` schema")


@pytest.mark.parametrize(
    ("schema", "diagnostic"),
    [
        ("{", "Expecting property name"),
        ("[]", "expected an object"),
        ('{"oneOf": {}}', "non-empty `oneOf` array"),
        ('{"oneOf": []}', "non-empty `oneOf` array"),
        ('{"oneOf": [null]}', "each `oneOf` arm must be an object"),
        ('{"oneOf": [{"const": 1}]}', "`const` must be a string"),
        ('{"oneOf": [{}]}', "non-empty `properties`"),
        ('{"oneOf": [{"properties": {"limit": null}}]}', "`limit` must be an object"),
        (
            '{"oneOf": [{"properties": {"limit": {"properties": []}}}]}',
            "`limit.properties` must be an object",
        ),
    ],
)
def test_an_unreadable_rejection_schema_is_a_finding(
    tmp_path: Path, schema: str, diagnostic: str
) -> None:
    """The policy check diagnoses its input instead of raising or skipping variants."""
    repo = _declaring(tmp_path)
    _write(tmp_path, "schemas/printobserver-types/RejectionReason.json", schema)

    findings = _intervention_policy(repo, "guide.md", "# A guide\n", EMPTY)

    refused(findings, "RejectionReason.json` is not a readable rejection schema")
    refused(findings, diagnostic)


def test_rejection_vocabulary_reads_unit_and_payload_variants(tmp_path: Path) -> None:
    """Inline schemas contribute their tags and payload field names to the inventory."""
    repo = _declaring(tmp_path)
    _write(
        tmp_path,
        "schemas/printobserver-types/RejectionReason.json",
        json.dumps(
            {
                "oneOf": [
                    {"const": "idle"},
                    {"properties": {"limit": {"properties": {"requested": {"type": "number"}}}}},
                    {"properties": {"message": {"type": "string"}}},
                ]
            }
        ),
    )

    equal(
        _rejection_vocabulary(repo, docs_policy(repo)),
        {"idle", "limit", "requested", "message"},
        describing="the rejection tags and fields",
    )
