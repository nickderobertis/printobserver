"""The two files this repository was branched with are still there, unchanged."""

from __future__ import annotations

import subprocess
from pathlib import Path

from conftest import REPO_ROOT
from repo_checks.model import Repo
from repo_checks.registry import base_files

MANIFEST = "gh-secrets.json"
IGNORE_RULE = ".gh-secrets-state.json"
DECLARED_SECRETS = {
    "OPENAI_API_KEY",
    "CLAUDE_CODE_OAUTH_TOKEN",
    "ANTHROPIC_API_KEY",
    "RELEASE_PLZ_TOKEN",
    "CARGO_REGISTRY_TOKEN",
    "NPM_TOKEN",
    "PYPI_TOKEN",
}


def _introducing_commit(path: str) -> str:
    """The commit that first brought a path into this repository."""
    log = subprocess.run(
        ["git", "log", "--reverse", "--format=%H", "--", path],
        cwd=REPO_ROOT,
        capture_output=True,
        text=True,
        check=True,
    ).stdout.split()
    return log[0]


def _blob_at(revision: str, path: str) -> str:
    return subprocess.run(
        ["git", "show", f"{revision}:{path}"],
        cwd=REPO_ROOT,
        capture_output=True,
        text=True,
        check=True,
    ).stdout


def test_the_secret_manifest_reads_exactly_as_it_did_on_the_base_commit() -> None:
    """The manifest is authoritative and this node overwrote none of it."""
    base = _introducing_commit(MANIFEST)

    assert (REPO_ROOT / MANIFEST).read_text(encoding="utf-8") == _blob_at(base, MANIFEST)


def test_the_manifest_declares_the_seven_secrets_this_repository_holds() -> None:
    """The names every workflow is held to come from here, not from convention."""
    import json

    manifest = json.loads((REPO_ROOT / MANIFEST).read_text(encoding="utf-8"))

    assert {entry["name"] for entry in manifest["secrets"]} == DECLARED_SECRETS


def test_the_gitignore_rule_that_was_already_there_is_still_there() -> None:
    """This node added rules to the committed .gitignore rather than writing a new one."""
    base = _introducing_commit(".gitignore")
    before = _blob_at(base, ".gitignore")
    after = (REPO_ROOT / ".gitignore").read_text(encoding="utf-8")

    for line in before.splitlines():
        assert line in after, line
    assert IGNORE_RULE in after


def test_the_check_refuses_a_tree_that_lost_the_ignore_rule(tmp_path: Path) -> None:
    """The rule the manifest's tooling needs cannot be dropped."""
    root = tmp_path / "tree"
    root.mkdir()
    (root / MANIFEST).write_text('{"secrets": [{"name": "A"}]}', encoding="utf-8")
    (root / ".gitignore").write_text("target/\n", encoding="utf-8")

    findings = base_files(Repo(root))

    assert any("no longer ignores" in finding for finding in findings), findings


def test_the_committed_tree_is_accepted() -> None:
    """Both paths are present and intact."""
    assert base_files(Repo(REPO_ROOT)) == []
