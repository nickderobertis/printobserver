"""Taking each route the way its own consumer takes it, and bringing a world up.

The three routes are installed for real here — a virtual environment, a package
directory, the committed install script against a staged release — with a
program of this suite's own standing in for the one the workspace builds. What
these journeys are about is the taking rather than the program: whether an
artifact built from this tree installs, and whether what it installed runs.

The world's own bring-up is driven beside them, including the ways it can fail:
a supervisor that stops before it answers, one that answers nowhere, and one
that writes its clients an address or a credential none of them can use.
"""

from __future__ import annotations

import json
import os
import shutil
from collections.abc import Callable
from pathlib import Path

import pytest
from release_artifacts.__main__ import main
from release_artifacts.installing import NO_TOOLCHAIN, TOOLCHAIN, prove, without_rust
from release_artifacts.publishing import PublishError, publish
from release_artifacts.world import (
    CLIENT_CONFIG,
    INGRESS_WORD,
    Machine,
    World,
    WorldError,
    _as_toml,
    scripted_printer,
)
from repo_checks.expect import absent, contains, equal, truth
from repo_checks.model import Repo

#: The three routes an end user gets the program by, each installed for real.
ROUTES = ["pypi:printobserver-cli", "npm:printobserver-cli", "release:printobserver"]


def test_node_is_kept_when_a_runner_installs_it_beside_rust(
    monkeypatch: pytest.MonkeyPatch, tmp_path: Path
) -> None:
    """The npm route keeps its runtime when a hosted runner shares a tool directory."""
    shared = tmp_path / "hosted-tool-bin"
    shared.mkdir()
    node = shutil.which("node")
    cargo = shutil.which("cargo")
    if node is None or cargo is None:
        pytest.fail("the artifact journey needs the repository's Node and Rust toolchains")
    (shared / "node").symlink_to(node)
    (shared / "cargo").symlink_to(cargo)
    monkeypatch.setenv("PATH", os.pathsep.join((str(shared), os.environ["PATH"])))

    environment = without_rust(preserve=("node",), preserved_at=tmp_path / "route-path")

    truth(shutil.which("node", path=environment["PATH"]), describing="the npm runtime on PATH")
    equal(
        [name for name in TOOLCHAIN if shutil.which(name, path=environment["PATH"])],
        [],
        describing="the Rust programs reachable from the install path",
    )


def test_npm_route_installs_when_node_shares_a_runner_directory_with_rust(
    monkeypatch: pytest.MonkeyPatch,
    tmp_path: Path,
    repo: Repo,
    program: Path,
    into: Callable[[str], Path],
    version: str,
) -> None:
    """The real npm route preserves Node before removing a shared tool directory."""
    shared = tmp_path / "hosted-tool-bin"
    shared.mkdir()
    node = shutil.which("node")
    cargo = shutil.which("cargo")
    if node is None or cargo is None:
        pytest.fail("the artifact journey needs the repository's Node and Rust toolchains")
    (shared / "node").symlink_to(node)
    (shared / "cargo").symlink_to(cargo)
    monkeypatch.setenv("PATH", os.pathsep.join((str(shared), os.environ["PATH"])))

    said = prove(repo, "npm:printobserver-cli", into("runner-shaped-npm"), program)

    contains(said, f"printobserver {version}", describing="what the npm route installed")
    contains(said, NO_TOOLCHAIN, describing="what the npm route was taken with")


@pytest.mark.parametrize("identifier", ROUTES)
def test_each_route_installs_a_program_that_runs(
    identifier: str, repo: Repo, program: Path, into: Callable[[str], Path], version: str
) -> None:
    """A route is taken the way its own command takes it, and what it left runs.

    Its proof says what the path it was installed under carried, and a route
    that reached a Rust toolchain on the installing host would say so here —
    which is the whole reason each of them ships a program already built for
    the platform.
    """
    said = prove(repo, identifier, into(identifier.replace(":", "-")), program)

    contains(said, f"printobserver {version}", describing=f"what `{identifier}` installed")
    contains(said, NO_TOOLCHAIN, describing=f"what `{identifier}` was taken with")


def test_a_stand_in_machine_answers_what_the_adapter_reads(repo: Repo) -> None:
    """The supervisor reaches it over HTTP exactly as it reaches a real machine."""
    machine = Machine()
    try:
        status, printer = machine.answer("/api/printer?history=false")
        equal(status, 200)
        contains(json.loads(printer)["state"]["flags"], "printing", describing="the flags")
        _, job = machine.answer("/api/job")
        equal(json.loads(job)["job"]["file"]["name"], "benchy.gcode")
        _, snapshot = machine.answer("/snapshot.jpg")
        equal(snapshot[:2], b"\xff\xd8", describing="the snapshot's own first bytes")
        equal(machine.answer("/api/anything-else")[0], 204)
        contains(machine.url, "127.0.0.1", describing="where the stand-in answers")
    finally:
        machine.stop()


def test_the_configuration_a_supervisor_is_started_under_is_toml_it_reads() -> None:
    """One key is spelled `tool_target:0`, which a section header cannot carry."""
    written = _as_toml(
        {
            "state_dir": "/var/lib/printobserver",
            "safety": {"allowed": {"tool_target:0": {"min": 0.0, "max": 260.0}}},
            "actions": {"operator": ["pause"]},
        }
    )

    contains(written, '"tool_target:0" = { min = 0.0, max = 260.0 }', describing=written)
    contains(written, 'operator = ["pause"]', describing=written)
    truth(INGRESS_WORD, describing="the ingress to have a word of its own")


def test_a_supervisor_that_stops_before_it_answers_is_said_to_have(
    repo: Repo, into: Callable[[str], Path]
) -> None:
    """A world that quietly waited would be a tier nobody could diagnose."""
    root = into("stops")
    stopping = root / "stops-at-once"
    stopping.write_text("#!/bin/sh\necho 'it will not start' >&2\nexit 1\n", encoding="utf-8")
    stopping.chmod(0o755)
    world = World(stopping, root)

    try:
        with pytest.raises(WorldError, match="stopped before it answered"):
            world.start()
    finally:
        world.stop()


def test_a_supervisor_answering_nowhere_is_said_to_be(
    repo: Repo, into: Callable[[str], Path]
) -> None:
    """A world whose ingress reaches nothing says so rather than timing out silently."""
    root = into("nowhere")
    state = root / "state"
    state.mkdir(parents=True, exist_ok=True)
    # Port one is privileged and never listened on, so the ingress post below
    # reaches nothing however this host is configured.
    quiet = root / "answers-nowhere"
    quiet.write_text(
        f'#!/bin/sh\nprintf \'[client]\\nserver = "http://127.0.0.1:1"\\n'
        f'credential = "a-credential-nothing-checks"\\n\' '
        f'> "{state / CLIENT_CONFIG}"\nsleep 60\n',
        encoding="utf-8",
    )
    quiet.chmod(0o755)
    world = World(quiet, root)

    try:
        with pytest.raises(OSError, match="Connection refused"):
            world.start()
    finally:
        world.stop()


def _writing_client_configuration(root: Path, server: str, credential: str) -> World:
    """A world whose supervisor writes this `[client]` table and then waits.

    Every character outside printable ASCII, and each quote and backslash, is
    written as a TOML escape, so the file parses and carries exactly these.
    """

    def quoted(text: str) -> str:
        escaped = "".join(
            c if " " <= c <= "~" and c not in '"\\' else f"\\u{ord(c):04x}" for c in text
        )
        return f'"{escaped}"'

    state = root / "state"
    state.mkdir(parents=True, exist_ok=True)
    staged = root / "staged-client.toml"
    staged.write_text(
        f"[client]\nserver = {quoted(server)}\ncredential = {quoted(credential)}\n",
        encoding="utf-8",
    )
    writing = root / "writes-a-client-configuration"
    writing.write_text(
        f'#!/bin/sh\ncp "{staged}" "{state / CLIENT_CONFIG}"\nsleep 60\n', encoding="utf-8"
    )
    writing.chmod(0o755)
    return World(writing, root)


#: Credentials no `Authorization` header carries intact, one for every clause of
#: the rule the server holds its own credential to — the list the server's own
#: journey and each client's smoke check are held to.
UNPRESENTABLE: list[dict[str, str]] = json.loads(
    (
        Path(__file__).resolve().parents[3]
        / "crates/printobserver-server/tests/fixtures/unpresentable-credentials.json"
    ).read_text(encoding="utf-8")
)


@pytest.mark.parametrize("entry", UNPRESENTABLE, ids=[entry["what"] for entry in UNPRESENTABLE])
def test_a_supervisor_writing_an_unpresentable_credential_is_said_to_have(
    entry: dict[str, str], repo: Repo, into: Callable[[str], Path]
) -> None:
    """A world handing its clients a credential nothing serves under says so at once.

    It says so without quoting the credential, and before any request is made.
    """
    world = _writing_client_configuration(
        into(f"credential-{UNPRESENTABLE.index(entry)}"), "http://127.0.0.1:1", entry["credential"]
    )

    try:
        with pytest.raises(WorldError, match="a credential no request presents") as refused:
            world.start()
    finally:
        world.stop()
    absent(str(refused.value), "qx-distinctive", describing=f"a credential {entry['what']}")


@pytest.mark.parametrize(
    "server",
    [
        "",
        "127.0.0.1:8420",
        "https://127.0.0.1:8420",
        "http://127.0.0.1",
        "http://:8420",
        "http://127.0.0.1:a-port",
    ],
)
def test_a_supervisor_writing_no_http_address_is_said_to_have(
    server: str, repo: Repo, into: Callable[[str], Path]
) -> None:
    """A world handing its clients an address none of them connects to says so at once."""
    world = _writing_client_configuration(into("address"), server, "a-credential-nothing-checks")

    try:
        with pytest.raises(WorldError, match="no http://host:port address"):
            world.start()
    finally:
        world.stop()


def test_an_environment_with_no_scripted_printer_names_the_recipe(tmp_path: Path) -> None:
    """A tier that quietly passed against no printer would prove nothing."""
    with pytest.raises(WorldError, match="just octoprint-up"):
        scripted_printer(tmp_path)


def test_a_publish_with_no_credential_names_the_secret_it_needs(
    repo: Repo, program: Path, into: Callable[[str], Path]
) -> None:
    """A publish that failed after a merge for want of a token is a release lost."""
    dist = into("to-publish")
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

    with pytest.raises(PublishError, match="PYPI_TOKEN"):
        publish(repo, dist, {})


def test_the_command_surface_stages_a_release_and_proves_a_route(
    repo: Repo, program: Path, into: Callable[[str], Path]
) -> None:
    """Its own commands are what release automation and the journeys run."""
    staged = into("staged-by-the-surface")
    equal(
        main(
            [
                "stage-release",
                "--root",
                str(repo.root),
                "--into",
                str(staged),
                "--binary",
                str(program),
            ]
        ),
        0,
    )
    truth((staged / "latest/download").is_dir(), describing="a staged newest release")

    equal(
        main(
            [
                "prove",
                "--target",
                "release:printobserver",
                "--root",
                str(repo.root),
                "--into",
                str(into("proved-by-the-surface")),
                "--binary",
                str(program),
            ]
        ),
        0,
    )
    equal(main(["prove", "--root", str(repo.root)]), 2)
