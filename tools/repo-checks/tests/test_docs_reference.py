"""Each reference document is complete against the thing it inventories.

The check reads each inventory beside the thing it inventories — the command
surface the program declares, the operations the server serves, the crates the
workspace declares, the tiers the configuration declares, the fields the
contracts' rejection type declares — and runs over the committed tree on every
`just check-repo`. What this establishes beside it is that the documents this
repository ships pass it.
"""

from __future__ import annotations

from repo_checks.checks_docs import reference
from repo_checks.expect import accepted
from repo_checks.model import Repo


def test_the_committed_documents_are_accepted(committed: Repo) -> None:
    """The documents this repository ships pass every rule the check carries."""
    accepted(reference(committed))
