"""The generated schema document is what the types generate, over the whole set.

Three ways a generated document goes wrong, and only one of them is caught by
comparing the document with what a generator produces: the other two are about
the *set*. A generator handed less than the declared set writes a document that
matches its own output exactly and is missing a type; a document carrying an
entry for something the contracts do not declare is documentation of nothing.
"""

from __future__ import annotations

import json
from collections.abc import Callable

from repo_checks.checks_docs import schema_document
from repo_checks.expect import accepted, refused
from repo_checks.model import Repo
from treecopy import Tree

DOCUMENT = "docs/reference/schemas.md"


def test_the_committed_schema_document_is_accepted(committed: Repo) -> None:
    """The document this repository ships is what the types generate."""
    accepted(schema_document(committed))


def test_a_declared_type_with_no_entry_is_refused(tree: Callable[[], Tree]) -> None:
    """The state a generator whose input omitted a declared type leaves behind.

    A document that merely matched its own generated output could not detect
    this, which is why the check reads the declared set beside it.
    """
    copy = tree()
    copy.write(
        "schemas/printobserver-types/ThermalRunawayReport.json",
        json.dumps({"title": "ThermalRunawayReport", "type": "object"}, indent=2) + "\n",
    )

    refused(
        schema_document(copy.repo),
        "carries no entry for the schema-emitting type `ThermalRunawayReport`",
    )


def test_an_entry_that_is_not_what_the_type_generates_is_refused(
    tree: Callable[[], Tree],
) -> None:
    """One altered entry is one entry a reader would build the wrong thing from."""
    copy = tree()
    copy.edit(
        DOCUMENT,
        '"description": "Carry on printing."',
        '"description": "Carry on printing, but slowly."',
    )

    refused(schema_document(copy.repo), "is not what the types generate")


def test_an_entry_for_a_type_the_contracts_do_not_declare_is_refused(
    tree: Callable[[], Tree],
) -> None:
    """A schema nothing generates is one nothing keeps right."""
    copy = tree()
    copy.append(DOCUMENT, "\n### ThermalRunawayReport\n\nDeclared by nothing.\n")

    refused(
        schema_document(copy.repo),
        "carries an entry `ThermalRunawayReport`, and there is no such schema-emitting type",
    )
