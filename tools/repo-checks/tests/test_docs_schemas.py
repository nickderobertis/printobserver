"""The generated schema document is what the types generate, over the whole set.

The check holds the document to the schema set the contracts' own generation
target maintains, in both directions, and to what those types generate for each
member of it. What this establishes beside it is that the document this
repository ships passes it.
"""

from __future__ import annotations

import os
import sys
from pathlib import Path

from repo_checks.checks_docs import schema_document
from repo_checks.expect import accepted, contains, equal, passing
from repo_checks.model import Repo
from repo_checks.shell import run
from test_docs_policy import DECLARED


def tool_packages(committed: Repo) -> str:
    """The tool packages the checks' own CLI imports, absolute, read from the justfile.

    Read rather than restated, so a package this repository grows is one this
    test finds without being told: the checks over the generated clients import
    the generator's own model, so `repo_checks` no longer imports from its own
    source root alone. Absolute because the CLI below runs with its working
    directory outside any checkout, where a relative root resolves to nothing.

    Args:
        committed: The committed tree, whose justfile declares the set.

    Returns:
        The roots, joined as a search path.

    Raises:
        AssertionError: If the justfile exports none.
    """
    for line in committed.justfile.splitlines():
        if line.startswith("export PYTHONPATH :="):
            roots = line.partition(":=")[2].strip().strip('"').split(":")
            return os.pathsep.join(str(committed.path(root)) for root in roots)
    message = "the justfile exports no PYTHONPATH, and the checks' own tools live on it"
    raise AssertionError(message)


def test_the_committed_schema_document_is_accepted(committed: Repo) -> None:
    """The document this repository ships is what the types generate."""
    accepted(schema_document(committed))


def test_schema_generation_cli_writes_the_declared_schema(tmp_path: Path, committed: Repo) -> None:
    """The real CLI generates a document from one declared schema, without a checkout."""
    (tmp_path / "repo-policy.toml").write_text(DECLARED, encoding="utf-8")
    schemas = tmp_path / "schemas" / "example-types"
    schemas.mkdir(parents=True)
    (schemas / "Reading.json").write_text('{"type":"integer"}', encoding="utf-8")

    result = run(
        [sys.executable, "-m", "repo_checks", "docs-schemas-write"],
        cwd=tmp_path,
        env={**os.environ, "PYTHONPATH": tool_packages(committed)},
    )

    passing(result)
    generated = (tmp_path / "schemas.md").read_text(encoding="utf-8")
    contains(generated, "1 types.")
    equal(generated.count("### "), 1, describing="the generated schema entries")
    contains(
        generated,
        '### Reading\n\nDeclared by `example-types`.\n\n```json\n{\n  "type": "integer"\n}\n```',
    )
    accepted(schema_document(Repo(tmp_path)), describing="the CLI-generated document")
