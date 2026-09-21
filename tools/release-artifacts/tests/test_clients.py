"""Each client is installed the way its own consumers install it, and proved there.

The three routes are proved beside these in `test_taking.py`; these are the
three clients a dependent takes as a dependency. Each is built from the
committed tree, installed into a throwaway environment holding no copy of these
sources — a virtual environment, a package directory, an unpacked crate — and
then proved there by its own committed smoke check against a **real
supervisor**: the program this repository builds, over a stand-in machine, with
a print and an image opened through the supervisor's own ingress.

The supervisor these run against is the debug build rather than the one a
release carries, and deliberately: what a client's smoke check proves is the
client, and the program on the other side of the socket is the same program
either way. What proves the *published* program is the route journeys, which
carry it.
"""

from __future__ import annotations

import json
import os
import re
import sys
import tempfile
import tomllib
from collections.abc import Callable
from pathlib import Path, PurePosixPath, PureWindowsPath

import pytest
from release_artifacts.installing import (
    InstallError,
    consumer_manifest,
    consumer_program,
    executable,
    install,
    interpreter_in,
    npm_global_program,
    programs_in,
    prove,
    prove_client,
    release_program,
    smoke_check,
    without_rust,
)
from release_artifacts.world import World
from repo_checks import platforms
from repo_checks.expect import absent, contains, equal, passing
from repo_checks.model import Repo
from repo_checks.shell import run

#: The three clients a dependent takes as a dependency.
CLIENTS = ["crate:printobserver-sdk", "pypi:printobserver-sdk", "npm:@printobserver/sdk"]

#: How long the one program build these share is given.
BUILD_TIMEOUT_SECONDS = 2400

#: A credential shaped like the one a supervisor generates for itself — 32
#: random bytes as unpadded URL-safe base64 — and beginning with `-`, as one in
#: sixty-four of those does. An option parser reads a value beginning with `-`
#: as the start of another option, so a smoke check that parsed its arguments
#: that way stopped as a usage error on a credential the server itself issued.
HYPHEN_LEADING = "-qx_generated-shaped_credential_of_43_chars"


@pytest.fixture(scope="module")
def supervisor(request: pytest.FixtureRequest) -> Path:
    """The program a client's smoke check is proved against, built once."""
    repo = Repo(Path(__file__).resolve().parents[3])
    built = repo.root / "target" / "debug" / platforms.host(repo).program
    if not built.is_file():
        passing(
            run(
                ["cargo", "build", "--locked", "-p", "printobserver"],
                cwd=repo.root,
                timeout=BUILD_TIMEOUT_SECONDS,
            ),
            describing="building the supervisor these clients are proved against",
        )
    return built


#: Credentials no `Authorization` header carries intact, one for every clause of
#: the rule the server holds its own credential to. The server's own journey
#: refuses every one of them as a configured credential, and each smoke check
#: here refuses every one as a usage error before it makes any request — so the
#: three smoke checks' copies of that rule and the server's are held to one list.
#: Every one with any text is spelled so a search for it finds only a quotation.
UNPRESENTABLE: list[dict[str, str]] = json.loads(
    (
        Path(__file__).resolve().parents[3]
        / "crates/printobserver-server/tests/fixtures/unpresentable-credentials.json"
    ).read_text(encoding="utf-8")
)


@pytest.mark.parametrize("identifier", CLIENTS)
def test_each_client_is_installed_and_proved_against_a_real_supervisor(
    identifier: str,
    repo: Repo,
    supervisor: Path,
    into: Callable[[str], Path],
) -> None:
    """Its own smoke check runs where it was installed, and reaches a real server.

    The check reads a status, materializes an image, opens the file at the path
    the server answered and checks its bytes against the digest the record
    declares. A check that reached no server would say nothing about the
    artifact.

    Given a credential no header could carry, the same installed check stops
    as a usage error naming the option, and quotes nothing it was given.
    """
    taken = install(
        repo, identifier, into(identifier.replace(":", "-").replace("@", "")), supervisor
    )
    for entry in UNPRESENTABLE:
        what, credential = entry["what"], entry["credential"]
        stopped = run(
            [
                *smoke_check(repo, taken),
                "--server",
                "http://127.0.0.1:9",
                "--credential",
                credential,
                "--print-id",
                "a-print",
                "--image-id",
                "an-image",
            ],
            cwd=taken.environment,
        )
        equal(stopped.returncode, 2, describing=f"the exit `{identifier}` gave a credential {what}")
        contains(
            stopped.stderr,
            "--credential takes the credential the supervisor serves under",
            describing=f"what `{identifier}` said of a credential {what}",
        )
        absent(
            stopped.stdout + stopped.stderr,
            "qx-distinctive",
            describing=f"what `{identifier}` said of a credential {what}",
        )

    said = prove_client(repo, taken, supervisor)

    contains(said, "smoke: contract", describing=f"what `{identifier}` said where it was put")
    contains(said, "image ", describing=f"what `{identifier}` said where it was put")


@pytest.mark.parametrize("identifier", CLIENTS)
def test_each_installed_client_takes_a_credential_beginning_with_a_hyphen(
    identifier: str,
    repo: Repo,
    supervisor: Path,
    into: Callable[[str], Path],
) -> None:
    """A credential the supervisor itself could generate is taken exactly as given.

    The supervisor is configured to serve under one beginning with `-`, its
    installed smoke check is handed that credential the way `prove_client`
    hands every credential, and it reaches the server rather than stopping as
    a usage error over the character the credential begins with.
    """
    taken = install(
        repo, identifier, into(identifier.replace(":", "-").replace("@", "")), supervisor
    )
    world = World(supervisor, taken.environment / "world", credential=HYPHEN_LEADING)
    try:
        running = world.start()
        equal(
            running.credential,
            HYPHEN_LEADING,
            describing="the credential the supervisor wrote for its clients",
        )
        proved = run(
            [
                *smoke_check(repo, taken),
                "--server",
                running.server,
                "--credential",
                running.credential,
                "--print-id",
                running.print_id,
                "--image-id",
                running.image_id,
            ],
            cwd=taken.environment,
        )
    finally:
        world.stop()

    passing(
        proved, describing=f"`{identifier}`'s smoke check under a credential beginning with `-`"
    )
    contains(proved.stdout, "smoke: contract", describing=f"what `{identifier}` said")


def test_a_client_with_no_smoke_check_is_refused(
    repo: Repo, supervisor: Path, into: Callable[[str], Path], monkeypatch: pytest.MonkeyPatch
) -> None:
    """An artifact nothing proves where it was installed is a stop, not a pass."""
    from release_artifacts import installing

    monkeypatch.delitem(installing.SMOKE, "pypi:printobserver-sdk")

    with pytest.raises(installing.InstallError, match="has no committed smoke check"):
        prove(repo, "pypi:printobserver-sdk", into("no-smoke-check"), supervisor)


#: Where each host family's own installers put what taking a client reaches for,
#: relative to the environment they installed into: the interpreter a Python
#: client is run under, the `pip` a registry proof installs with, the consumer a
#: Rust client's smoke check is built as, and the program a global npm install
#: leaves on a path.
LAYOUTS: dict[str, dict[str, str]] = {
    "linux": {
        "python": "bin/python",
        "pip": "bin/pip",
        "smoke": "target/release/printobserver-sdk-smoke",
        "npm": "bin/printobserver",
    },
    "win32": {
        "python": "Scripts/python.exe",
        "pip": "Scripts/pip.exe",
        "smoke": "target/release/printobserver-sdk-smoke.exe",
        "npm": "printobserver.cmd",
    },
}


@pytest.mark.parametrize("host", sorted(LAYOUTS))
def test_an_installed_client_is_reached_where_its_own_hosts_installer_put_it(
    host: str, tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    """A virtual environment on Windows keeps its programs under `Scripts`, as `.exe`.

    And a global npm install there writes a `.cmd` straight into its prefix
    rather than linking into `bin`. A proof asking for the POSIX layout on
    Windows reports a program missing that the install put somewhere else — so
    each host's answer is asked for on every host, whichever one this runs on.
    """
    monkeypatch.setattr(sys, "platform", host)
    environment = tmp_path / "env"
    layout = LAYOUTS[host]

    equal(
        interpreter_in(environment),
        environment / layout["python"],
        describing="the interpreter a Python client is run under",
    )
    equal(
        programs_in(environment) / executable("pip"),
        environment / layout["pip"],
        describing="the `pip` a registry proof installs with",
    )
    equal(
        release_program(environment / "target", "printobserver-sdk-smoke"),
        environment / layout["smoke"],
        describing="the program the Rust client's smoke check is built as",
    )
    equal(
        npm_global_program(environment, "printobserver"),
        environment / layout["npm"],
        describing="what a global npm install puts on a path",
    )


#: What a Rust toolchain's own program is called in the directory rustup puts it in.
TOOLCHAIN_FILES = {"linux": "cargo", "win32": "cargo.exe"}


@pytest.mark.parametrize("host", sorted(TOOLCHAIN_FILES))
def test_a_rust_toolchain_is_taken_off_the_path_under_the_name_its_host_gives_it(
    host: str, tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    """A directory holding `cargo.exe` is a toolchain on Windows, and is taken away.

    Looked for as a bare `cargo`, a Windows `PATH` keeps its toolchain, and a
    route proven "with no Rust toolchain on the path" was proven with one on it.
    """
    toolchain = tmp_path / "toolchain"
    toolchain.mkdir()
    (toolchain / TOOLCHAIN_FILES[host]).write_bytes(b"")
    elsewhere = tmp_path / "elsewhere"
    elsewhere.mkdir()
    monkeypatch.setenv("PATH", os.pathsep.join([str(toolchain), str(elsewhere)]))
    monkeypatch.setattr(sys, "platform", host)

    kept = without_rust()["PATH"].split(os.pathsep)

    equal(kept, [str(elsewhere)], describing="the path an install is run under")


#: A consumer that depends on nothing, so where it builds is settled with no
#: crate resolved.
BARE_CONSUMER = """[package]
name = "printobserver-sdk-smoke"
version = "0.0.0"
edition = "2024"

[workspace]
"""


def _consumer_at(directory: Path) -> Path:
    (directory / "src").mkdir(parents=True, exist_ok=True)
    (directory / "src" / "main.rs").write_text("fn main() {}\n", encoding="utf-8")
    (directory / "Cargo.toml").write_text(BARE_CONSUMER, encoding="utf-8")
    return directory


def test_a_consumers_program_is_looked_for_where_cargo_says_it_builds(
    repo: Repo, tmp_path: Path
) -> None:
    """Inside this clone that is the clone's own `target`; outside it, the consumer's own.

    `.cargo/config.toml` at the root sends every build under the clone into
    `<clone>/target`, and the proofs write their consumers under `dist/` — so a
    proof that looked beside the manifest would report the program it had just
    built as missing. A consumer in a temporary directory is under no such
    file and builds beside itself, which is what a proof run from one sees.

    The consumer inside the clone has to be inside it, so it sits where the
    proofs put theirs — and in a directory of its own that is gone when the
    test is, so a run leaves nothing under `dist/`.
    """
    smoke = executable("printobserver-sdk-smoke")
    dist = repo.root / "dist"
    dist.mkdir(exist_ok=True)
    with tempfile.TemporaryDirectory(dir=dist, prefix="test-clients-") as scratch:
        inside = _consumer_at(Path(scratch) / "consumer")
        equal(
            consumer_program(inside, "printobserver-sdk-smoke"),
            repo.root / "target" / "release" / smoke,
            describing="where a consumer inside the clone is built",
        )

    outside = _consumer_at(tmp_path / "consumer")
    equal(
        consumer_program(outside, "printobserver-sdk-smoke").resolve(),
        (outside / "target" / "release" / smoke).resolve(),
        describing="where a consumer outside the clone is built",
    )


def test_a_consumer_cargo_cannot_read_is_a_stop_naming_it(tmp_path: Path) -> None:
    """A consumer with no manifest gets no guessed path: the proof stops saying so."""
    consumer = tmp_path / "consumer"
    consumer.mkdir()

    with pytest.raises(InstallError, match="asking where the consumer at"):
        consumer_program(consumer, "printobserver-sdk-smoke")


#: What a `cargo` standing in on the path answers `metadata` with, and what a
#: proof that trusted it would have done with each: no JSON at all, and JSON
#: that names no target directory or names one that is not a path.
UNANSWERED = [
    ("plain text", "answered something other than JSON"),
    ('{"packages": []}', "answered no `target_directory`"),
    ('{"target_directory": 7}', "answered no `target_directory`"),
    ('{"target_directory": ""}', "answered no `target_directory`"),
    ("[]", "answered no `target_directory`"),
]


def _cargo_answering(directory: Path, answer: str) -> None:
    """A `cargo` in `directory` that answers `answer` to everything.

    A POSIX host runs it by its interpreter line. A Windows host finds a
    program by its suffix and runs no interpreter line, so there the code sits
    beside a `.cmd` that hands it to this interpreter.
    """
    code = f"import sys\nsys.stdout.write({answer!r})\n"
    if sys.platform == "win32":
        (directory / "cargo.py").write_text(code, encoding="utf-8")
        (directory / "cargo.cmd").write_text(
            f'@"{sys.executable}" "%~dp0cargo.py" %*\r\n', encoding="utf-8"
        )
        return
    written = directory / "cargo"
    written.write_text(f"#!{sys.executable}\n{code}", encoding="utf-8")
    written.chmod(0o755)


@pytest.mark.parametrize(("answer", "refused"), UNANSWERED, ids=[a for a, _ in UNANSWERED])
def test_a_cargo_that_names_no_target_directory_is_a_stop_quoting_its_answer(
    answer: str, refused: str, tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    """An answer naming no target directory gets no guessed path and no traceback."""
    standing_in = tmp_path / "bin"
    standing_in.mkdir()
    _cargo_answering(standing_in, answer)
    monkeypatch.setenv("PATH", os.pathsep.join([str(standing_in), os.environ["PATH"]]))

    with pytest.raises(InstallError, match=re.escape(refused)) as stopped:
        consumer_program(_consumer_at(tmp_path / "consumer"), "printobserver-sdk-smoke")

    contains(str(stopped.value), answer, describing="what the stop quotes")


def test_a_consumer_manifest_names_a_windows_path_cargo_can_parse() -> None:
    """The unpacked crate's path reaches `cargo` intact, whichever separator it has.

    Written into a TOML string as it stands, a Windows path is a run of escapes
    TOML refuses, and the Rust client could not be taken on Windows at all.
    """
    for inside, expected in (
        (
            PureWindowsPath(r"D:\a\printobserver\env\vendor\printobserver-sdk-0.2.0"),
            "D:/a/printobserver/env/vendor/printobserver-sdk-0.2.0",
        ),
        (
            PurePosixPath("/home/proof/env/vendor/printobserver-sdk-0.2.0"),
            "/home/proof/env/vendor/printobserver-sdk-0.2.0",
        ),
    ):
        parsed = tomllib.loads(consumer_manifest(inside))
        equal(
            parsed["dependencies"]["printobserver-sdk"]["path"],
            expected,
            describing=f"the path a consumer of {inside} depends on",
        )
