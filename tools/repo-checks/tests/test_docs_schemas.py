"""The generated schema document is what the types generate, over the whole set.

Three ways a generated document goes wrong, and only one of them is caught by
comparing the document with what a generator produces: the other two are about
the *set*. A generator handed less than the declared set writes a document that
matches its own output exactly and is missing a type; a document carrying an
entry for something the contracts do not declare is documentation of nothing.
"""

from __future__ import annotations

from repo_checks.checks_docs import _schema_findings, schema_document
from repo_checks.docs import docs_policy, schema_document_text, schema_members
from repo_checks.expect import accepted, refused
from repo_checks.model import Repo

#: A type the contracts do not declare, which nothing in this tree generates.
UNDECLARED = "ThermalRunawayReport"


def test_the_committed_schema_document_is_accepted(committed: Repo) -> None:
    """The document this repository ships is what the types generate."""
    accepted(schema_document(committed))


def test_a_declared_type_with_no_entry_is_refused(committed: Repo) -> None:
    """The state a generator whose input omitted a declared type leaves behind.

    A document that merely matched its own generated output could not detect
    this, which is why the rule reads the declared set beside it.
    """
    policy = docs_policy(committed)
    text = committed.read(policy.schema_document)
    declared = [name for _, name, _ in schema_members(committed, policy.schema_directory)]

    refused(
        _schema_findings(policy, text, text, [*declared, UNDECLARED]),
        f"carries no entry for the schema-emitting type `{UNDECLARED}`",
    )


def test_an_entry_that_is_not_what_the_type_generates_is_refused(committed: Repo) -> None:
    """A document edited by hand is a document that no longer says what the types do."""
    policy = docs_policy(committed)
    generated = schema_document_text(committed, policy)
    altered = generated.replace('"type": "object"', '"type": "banana"', 1)
    declared = [name for _, name, _ in schema_members(committed, policy.schema_directory)]

    refused(
        _schema_findings(policy, altered, generated, declared),
        "is not what the types generate",
    )


def test_an_entry_for_a_type_the_contracts_do_not_declare_is_refused(committed: Repo) -> None:
    """An entry for a type nothing emits is documentation of nothing."""
    policy = docs_policy(committed)
    text = f"{committed.read(policy.schema_document)}\n### {UNDECLARED}\n\nNothing emits this.\n"
    declared = [name for _, name, _ in schema_members(committed, policy.schema_directory)]

    refused(
        _schema_findings(policy, text, text, declared),
        f"carries an entry `{UNDECLARED}`, and there is no such schema-emitting type",
    )
