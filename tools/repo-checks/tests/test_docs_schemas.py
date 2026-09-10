"""The generated schema document is what the types generate, over the whole set.

The check holds the document to the schema set the contracts' own generation
target maintains, in both directions, and to what those types generate for each
member of it. What this establishes beside it is that the document this
repository ships passes it.
"""

from __future__ import annotations

from repo_checks.checks_docs import schema_document
from repo_checks.expect import accepted
from repo_checks.model import Repo


def test_the_committed_schema_document_is_accepted(committed: Repo) -> None:
    """The document this repository ships is what the types generate."""
    accepted(schema_document(committed))
