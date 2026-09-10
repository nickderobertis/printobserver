"""Malformed generated manifests produce findings through the public reference check."""

from __future__ import annotations

import json
from pathlib import Path

import pytest
from repo_checks.checks_docs import reference
from repo_checks.expect import refused
from repo_checks.model import Repo
from test_docs_policy import DECLARED


@pytest.mark.parametrize(
    ("command", "finding"),
    [
        (None, "commands[0] must be an object"),
        ({}, "commands[0].command must be a nonempty string"),
        ({"command": " "}, "commands[0].command must be a nonempty string"),
        ({"command": "status", "operation": 1}, "commands[0].operation"),
        ({"command": "status"}, "commands[0].options must be a list"),
        ({"command": "status", "options": {}}, "commands[0].options must be a list"),
        ({"command": "status", "options": [None]}, "commands[0].options[0] must be an object"),
        ({"command": "status", "options": [{}]}, "commands[0].options[0].option"),
        (
            {"command": "status", "options": [{"option": "--print-id", "field": 1}]},
            "commands[0].options[0].field",
        ),
    ],
)
def test_malformed_commands_are_diagnosed(tmp_path: Path, command: object, finding: str) -> None:
    """A declaration and one small JSON file exercise the actual boundary."""
    (tmp_path / "repo-policy.toml").write_text(DECLARED, encoding="utf-8")
    (tmp_path / "surface.json").write_text(json.dumps({"commands": [command]}), encoding="utf-8")

    refused(reference(Repo(tmp_path)), finding)


@pytest.mark.parametrize(
    ("globals_", "finding"),
    [("--json", "global_options must be a list"), ([False], "global_options[0]")],
)
def test_malformed_global_options_are_diagnosed(
    tmp_path: Path, globals_: object, finding: str
) -> None:
    """Global options must be a list of names before vocabulary rules read them."""
    (tmp_path / "repo-policy.toml").write_text(DECLARED, encoding="utf-8")
    document = {
        "commands": [{"command": "status", "options": []}],
        "global_options": globals_,
    }
    (tmp_path / "surface.json").write_text(json.dumps(document), encoding="utf-8")

    refused(reference(Repo(tmp_path)), finding)


def test_duplicate_commands_are_diagnosed(tmp_path: Path) -> None:
    """Two entries cannot silently overwrite each other's documented options."""
    (tmp_path / "repo-policy.toml").write_text(DECLARED, encoding="utf-8")
    document = {"commands": [{"command": "status", "options": []}] * 2}
    (tmp_path / "surface.json").write_text(json.dumps(document), encoding="utf-8")

    refused(reference(Repo(tmp_path)), "commands repeats `status`")
