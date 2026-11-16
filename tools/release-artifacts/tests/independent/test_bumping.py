"""Moving a copy's version as a release pull request does, over a real copy of the tree."""

from __future__ import annotations

from pathlib import Path

import pytest
from conftest import REPO_ROOT
from release_artifacts import bumping
from repo_checks import scratch
from repo_checks.expect import contains, equal


def test_a_bump_moves_the_workspace_and_every_internal_requirement(tmp_path: Path) -> None:
    """The version the copy declares afterwards is the one it was moved to."""
    copy = scratch.copy_tree(REPO_ROOT, tmp_path / "tree")
    was = bumping.workspace_version(copy)
    version = bumping.next_minor(was)

    equal(bumping.bump_workspace_version(copy, version), was, describing="what the bump moved from")
    equal(bumping.workspace_version(copy), version, describing="the version after the bump")
    for manifest in (copy / "crates").glob("*/Cargo.toml"):
        text = manifest.read_text(encoding="utf-8")
        for line in text.splitlines():
            if "path = " in line and "version = " in line:
                contains(line, f'version = "{version}"', describing=f"{manifest.name}'s {line}")


@pytest.mark.parametrize(
    "manifest",
    [
        '[workspace]\nmembers = ["crates/*"]\n',
        '[workspace.package]\nversion = "latest"\n',
        "[workspace.package]\nversion = 0.3\n",
    ],
    ids=["versionless", "not-a-version", "not-a-string"],
)
def test_a_workspace_declaring_no_release_version_is_refused_naming_its_manifest(
    tmp_path: Path, manifest: str
) -> None:
    """Nothing is bumped from a version that is not one."""
    path = tmp_path / bumping.WORKSPACE_MANIFEST
    path.write_text(manifest, encoding="utf-8")

    with pytest.raises(RuntimeError, match="declares no") as refused:
        bumping.workspace_version(tmp_path)

    contains(str(refused.value), str(path), describing="the refusal")
