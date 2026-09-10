"""Putting each built artifact where its own consumers install it from.

The registries are stood in for by programs on this journey's own PATH, and
deliberately: what a publish does is run its registry's own client with a
credential and a file, and what these assert is that it runs the right one with
the right file. Whether a registry accepts what it was sent is that registry's
to say, and the merge path's to find out.
"""

from __future__ import annotations

import os
from collections.abc import Callable
from pathlib import Path

import pytest
from release_artifacts.__main__ import main
from release_artifacts.publishing import CREDENTIALS, PublishError, publish, registries_of
from repo_checks.expect import contains, equal, truth
from repo_checks.model import Repo

#: The registry clients a publish runs, each stood in for by a program that
#: records what it was asked to do.
CLIENTS = ("uv", "npm", "gh")


@pytest.fixture
def registries(tmp_path: Path, monkeypatch: pytest.MonkeyPatch) -> Path:
    """Programs standing in for the registry clients, ahead of the real ones.

    They go on this process's own PATH, because that is what the one place a
    subprocess starts resolves an executable against — and a journey that left
    the real ones there would publish this repository to a real registry.
    """
    shims = tmp_path / "registries"
    shims.mkdir()
    recording = shims / "what-was-run"
    for name in CLIENTS:
        program = shims / name
        program.write_text(
            f'#!/bin/sh\necho "{name} $*" >> "{recording}"\nexit 0\n', encoding="utf-8"
        )
        program.chmod(0o755)
    monkeypatch.setenv("PATH", f"{shims}{os.pathsep}{os.environ['PATH']}")
    return shims


def _published(repo: Repo, dist: Path, shims: Path) -> str:
    """Publish, and answer everything the registry clients were asked to do."""
    environment = {
        **os.environ,
        "PATH": f"{shims}{os.pathsep}{os.environ['PATH']}",
        **{name: "a-token-this-journey-made-up" for name in CREDENTIALS.values()},
    }
    publish(repo, dist, environment)
    return (shims / "what-was-run").read_text(encoding="utf-8")


def test_every_artifact_reaches_the_registry_it_is_declared_for(
    repo: Repo, program: Path, into: Callable[[str], Path], registries: Path
) -> None:
    """A wheel goes to one, a package to another, and a release artifact to the third."""
    dist = into("everything")
    equal(
        main(
            [
                "build-all",
                "--root",
                str(repo.root),
                "--into",
                str(dist),
                "--binary",
                str(program),
            ]
        ),
        0,
    )

    said = _published(repo, dist, registries)

    contains(said, "uv publish", describing="what the Python registry's client was asked")
    contains(said, "npm publish --access public", describing="what npm's client was asked")
    contains(said, "gh release upload", describing="what the forge's client was asked")
    for name in ("printobserver_sdk", "printobserver_cli"):
        truth(name in said, describing=f"{name} to have been published")


@pytest.mark.parametrize("registry", sorted(CREDENTIALS))
def test_a_publish_with_no_credential_names_the_secret_it_needs(
    registry: str, repo: Repo, program: Path, into: Callable[[str], Path], registries: Path
) -> None:
    """A publish that failed after a merge for want of a token is a release lost."""
    dist = into(f"without-{registry}")
    main(
        [
            "build-all",
            "--root",
            str(repo.root),
            "--into",
            str(dist),
            "--binary",
            str(program),
        ]
    )
    environment = {
        **os.environ,
        "PATH": f"{registries}{os.pathsep}{os.environ['PATH']}",
        **{name: "a-token" for name in CREDENTIALS.values()},
    }
    environment[CREDENTIALS[registry]] = "   "

    with pytest.raises(PublishError, match=CREDENTIALS[registry]):
        publish(repo, dist, environment)


def test_a_registry_that_refuses_what_it_was_sent_is_reported(
    repo: Repo,
    program: Path,
    into: Callable[[str], Path],
    tmp_path: Path,
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    """A publish that failed silently would be a release nobody knew was lost."""
    refusing = tmp_path / "refusing"
    refusing.mkdir()
    for name in CLIENTS:
        client = refusing / name
        client.write_text(f'#!/bin/sh\necho "{name}: no" >&2\nexit 1\n', encoding="utf-8")
        client.chmod(0o755)
    monkeypatch.setenv("PATH", f"{refusing}{os.pathsep}{os.environ['PATH']}")
    dist = into("refused")
    main(
        [
            "build",
            "--target",
            "pypi:printobserver-sdk",
            "--root",
            str(repo.root),
            "--into",
            str(dist),
            "--binary",
            str(program),
        ]
    )

    with pytest.raises(PublishError, match="publishing"):
        publish(
            repo,
            dist,
            {
                **os.environ,
                "PATH": f"{refusing}{os.pathsep}{os.environ['PATH']}",
                **{name: "a-token" for name in CREDENTIALS.values()},
            },
        )


def test_the_tool_publishes_exactly_what_the_declaration_names(repo: Repo) -> None:
    """Nothing here restates a name, a registry or a route."""
    shipped = {target.id for target in registries_of(repo)}

    equal(len(shipped), 6, describing="the artifacts this tool publishes")
    for registry in CREDENTIALS:
        truth(
            any(target.startswith(f"{registry}:") for target in shipped),
            describing=f"an artifact reaching {registry}",
        )
