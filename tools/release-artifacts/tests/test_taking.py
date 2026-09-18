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
import sys
from collections.abc import Callable
from pathlib import Path

import pytest
from release_artifacts.__main__ import main
from release_artifacts.installing import NO_TOOLCHAIN, prove
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
from route_proof import ROUTE_PROOF

#: The three routes an end user gets the program by, each installed for real.
ROUTES = ["pypi:printobserver-cli", "npm:printobserver-cli", "release:printobserver"]


@ROUTE_PROOF
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


def _stand_in(root: Path, name: str, code: str) -> Path:
    """A program called `name` in `root` that runs the Python `code`, and its path.

    A POSIX host runs it by its interpreter line. A Windows host runs no
    interpreter line and finds a program by its suffix, so there the code sits
    beside a `.cmd` that hands it to this interpreter, and that is the program.
    """
    if sys.platform == "win32":
        (root / f"{name}.py").write_text(code, encoding="utf-8")
        launcher = root / f"{name}.cmd"
        launcher.write_text(f'@"{sys.executable}" "%~dp0{name}.py" %*\r\n', encoding="utf-8")
        return launcher
    program = root / name
    program.write_text(f"#!{sys.executable}\n{code}", encoding="utf-8")
    program.chmod(0o755)
    return program


def test_a_supervisor_that_stops_before_it_answers_is_said_to_have(
    repo: Repo, into: Callable[[str], Path]
) -> None:
    """A world that quietly waited would be a tier nobody could diagnose."""
    root = into("stops")
    stopping = _stand_in(
        root,
        "stops-at-once",
        "import sys\nprint('it will not start', file=sys.stderr)\nraise SystemExit(1)\n",
    )
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
    written = (
        '[client]\nserver = "http://127.0.0.1:1"\ncredential = "a-credential-nothing-checks"\n'
    )
    quiet = _stand_in(
        root,
        "answers-nowhere",
        f"import pathlib, time\npathlib.Path({str(state / CLIENT_CONFIG)!r}).write_text("
        f"{written!r}, encoding='utf-8')\ntime.sleep(60)\n",
    )
    world = World(quiet, root)

    try:
        # Each platform's own words for a connection nothing accepted.
        with pytest.raises(OSError, match=r"Connection refused|actively refused"):
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
    writing = _stand_in(
        root,
        "writes-a-client-configuration",
        f"import shutil, time\nshutil.copyfile({str(staged)!r}, {str(state / CLIENT_CONFIG)!r})\n"
        "time.sleep(60)\n",
    )
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


@ROUTE_PROOF
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
