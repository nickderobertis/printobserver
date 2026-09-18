"""The Windows service installer, driven under PowerShell with the manager stood in for.

`scripts/install-service.ps1` is the first of the `windows-service` pair the
install path states. Only a Windows host has a service control manager for it
to register with, but everything else it does — the four placements, the
registration it hands the manager, what it refuses and what it prints — is
PowerShell, and PowerShell runs here. So this journey runs the committed script
under `pwsh` against a throwaway root, inside a session in which the manager's
own tools and every cmdlet that could start a service are functions that record
being called and do nothing: what the script asked of the manager is read out of
that recording, and what it placed is read off the root.

The programs recorded are the ones `repo-policy.toml`'s
`service.managers.windows-service` forbids, plus the two the installer is
allowed: `sc.exe`, which registers, and `icacls`, which grants. An installer
that started the service would leave a line in the recording, and an installer
that registered it to start at boot would leave `start= auto` there — and both
are refused here, on every platform, before a Windows runner ever sees the
script. The registration's other half — that the manager then starts it and
brings it back — is `test_service_manager_journey.py`'s, on a host that has one.
"""

from __future__ import annotations

import os
import re
import shutil
import threading
import tomllib
from collections.abc import Callable, Iterator
from dataclasses import dataclass
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from pathlib import Path

import pytest
from journey import REPO_ROOT, clean_environment, run
from repo_checks import install_path as ip
from repo_checks.expect import contains, equal, failing, passing, truth
from repo_checks.model import Repo
from repo_checks.platforms import ServiceManager, descriptor, host
from repo_checks.shell import run as shell_run
from repo_checks.shell import start

#: The committed installer the `windows-service` pair fetches.
INSTALLER = "scripts/install-service.ps1"

#: The manager whose pair that is.
MANAGER = ServiceManager.WINDOWS_SERVICE

#: The variable the fixture session records into.
RECORDING = "PRINTOBSERVER_FIXTURE_RECORDING"

#: The variable that makes the stand-in manager answer that the service exists.
REGISTERED = "PRINTOBSERVER_FIXTURE_REGISTERED"

#: The variable naming one thing the stand-ins refuse: a verb of the manager's
#: tool, or the granting tool by name. What a refused tool says is what the
#: real one would: a sentence of its own on standard error, and a non-zero
#: exit.
REFUSES = "PRINTOBSERVER_FIXTURE_REFUSES"

#: What a refused stand-in says.
REFUSAL = "Access is denied, says the stand-in."

#: What the manager answers a query about a service it does not have.
SERVICE_DOES_NOT_EXIST = 1060

#: The program the installer is allowed to run besides the manager's own tool,
#: recorded beside the forbidden ones so that every grant can be read back.
GRANTING = "icacls"

#: How long the program build is given the first time this tier runs.
BUILD_TIMEOUT_SECONDS = 2400

#: What a program's name may look like before it is written into the fixture
#: session as a function's name: a cmdlet or a program file, and nothing
#: PowerShell would read as anything else.
PROGRAM_NAME = re.compile(r"^[A-Za-z][A-Za-z0-9.-]*$")


def _powershell() -> str:
    """The PowerShell on this host, and a next action where there is none.

    Raises:
        AssertionError: If neither `pwsh` nor `powershell` is on PATH.
    """
    for candidate in ("pwsh", "powershell"):
        found = shutil.which(candidate)
        if found:
            return found
    message = (
        "this journey drives the Windows service installer under PowerShell, and this host "
        "has neither `pwsh` nor `powershell` on PATH; install PowerShell 7 "
        "(https://github.com/PowerShell/PowerShell/releases) and put `pwsh` on PATH"
    )
    raise AssertionError(message)


# llmlint: ignore[expensive_tests_stay_behind_their_own_edge] suppressions.toml has the reason.
@pytest.fixture(scope="module")
def program() -> Path:
    """The `printobserver` program built from this tree for this host, built once."""
    passing(
        run(
            ["cargo", "build", "--locked", "-p", "printobserver"],
            REPO_ROOT,
            timeout=BUILD_TIMEOUT_SECONDS,
        ),
        describing="building the printobserver program",
    )
    return REPO_ROOT / "target" / "debug" / host(Repo(REPO_ROOT)).program


@dataclass(frozen=True, slots=True)
class Rules:
    """The `windows-service` rules `repo-policy.toml` declares, narrowed."""

    #: Programs the installer may not run, each possibly with a verb.
    may_not_invoke: tuple[str, ...]
    #: Settings the installer may not write into what it runs.
    may_not_set: tuple[str, ...]
    #: What starts the service at boot, in the manager's vocabulary.
    starts_at_boot: str
    #: What brings it back after a crash, in the manager's vocabulary.
    restarts_after_crash: str

    @property
    def forbidden_programs(self) -> list[str]:
        """Every program the installer may not run, by the name PowerShell resolves."""
        return sorted({entry.split()[0] for entry in self.may_not_invoke})


def _strings(table: dict[str, object], key: str) -> tuple[str, ...]:
    """One list of a policy table, as strings; absent is empty."""
    value = table.get(key, [])
    truth(isinstance(value, list), describing=f"`{key}` to be a list")
    return tuple(str(entry) for entry in value) if isinstance(value, list) else ()


def _rules() -> Rules:
    """The `windows-service` rules the policy declares."""
    policy = tomllib.loads((REPO_ROOT / "repo-policy.toml").read_text(encoding="utf-8"))
    table = dict(policy["service"]["managers"][MANAGER.value])
    return Rules(
        _strings(table, "may_not_invoke"),
        _strings(table, "may_not_set"),
        str(table["starts_at_boot"]),
        str(table["restarts_after_crash"]),
    )


def _activation() -> str:
    """The operator's own second command, as the install path states it."""
    path = ip.parse((REPO_ROOT / "AGENTS.md").read_text(encoding="utf-8"))
    pair = path.commands_for(MANAGER.value)
    equal(len(pair), 2, describing="the windows-service pair the install path states")
    return pair[1]


@dataclass(frozen=True, slots=True)
class Ran:
    """One run of the installer under the fixture session."""

    code: int
    said: str
    root: Path
    recorded: list[str]

    @property
    def binary(self) -> Path:
        """Where the program goes beneath the root."""
        return self.root / "Program Files" / "printobserver" / "printobserver.exe"

    @property
    def configuration(self) -> Path:
        """Where the configuration goes beneath the root."""
        return self.root / "ProgramData" / "printobserver" / "config.toml"

    @property
    def state(self) -> Path:
        """Where the state directory goes beneath the root."""
        return self.root / "ProgramData" / "printobserver" / "state"

    def asked(self, verb: str) -> list[str]:
        """Every call of the manager's tool with one verb."""
        return [line for line in self.recorded if line.split()[:2] == ["sc.exe", verb]]


def _fixture_session(into: Path) -> Path:
    """A script that stands the manager in and then runs the committed installer.

    Every forbidden program and the granting one become functions of the
    session that append what they were called with to the recording, so that
    the installer resolves them before anything on PATH. The stand-in manager
    answers a query as Windows would: the service is not there, unless the
    journey says it is.

    The names written into the session are the policy's own, and each is held
    to the shape of a program's name before it is written into source; the
    installer's path is written as a PowerShell literal, its one escape applied.
    """
    names = [*_rules().forbidden_programs, GRANTING]
    for name in names:
        truth(
            PROGRAM_NAME.match(name) is not None,
            describing=f"`{name}` to be a program's name and nothing PowerShell reads otherwise",
        )
    # llmlint: ignore[e2e_not_mocked] suppressions.toml has the reason.
    # llmlint: ignore[tests_mirror_real_usage] suppressions.toml has the reason.
    shims = "\n".join(
        f"function {name} {{ Record ('{name} ' + ($args -join ' ')); Answer '{name}' }}"
        for name in names
        if name != "sc.exe"
    )
    installer = (REPO_ROOT / INSTALLER).as_posix().replace("'", "''")
    # llmlint: ignore[e2e_not_mocked] suppressions.toml has the reason.
    # llmlint: ignore[tests_mirror_real_usage] suppressions.toml has the reason.
    script = f"""
function Record([string]$Line) {{ Add-Content -LiteralPath $env:{RECORDING} -Value $Line }}
function Answer([string]$What) {{
    if ($env:{REFUSES} -eq $What) {{
        [Console]::Error.WriteLine('{REFUSAL}')
        $global:LASTEXITCODE = 5
    }} else {{
        $global:LASTEXITCODE = 0
    }}
}}
{shims}
function sc.exe {{
    Record ('sc.exe ' + ($args -join ' '))
    if ($args[0] -eq 'query' -and -not $env:{REGISTERED}) {{
        $global:LASTEXITCODE = {SERVICE_DOES_NOT_EXIST}
    }} else {{
        Answer $args[0]
    }}
}}
& '{installer}' @args
exit $LASTEXITCODE
"""
    path = into / "fixture-session.ps1"
    path.write_text(script, encoding="utf-8")
    return path


@pytest.fixture
def installer(tmp_path: Path) -> Callable[..., Ran]:
    """Run the committed installer under the fixture session, against a root here."""
    session = _fixture_session(tmp_path)
    powershell = _powershell()
    counter = {"n": 0}

    def install(
        *,
        binary: Path | None,
        root: Path | None = None,
        registered: bool = False,
        refuses: str | None = None,
        path: str | None = None,
    ) -> Ran:
        counter["n"] += 1
        root = root or tmp_path / f"root{counter['n']}"
        recording = tmp_path / f"recorded{counter['n']}"
        arguments = ["-Root", str(root)]
        if binary is not None:
            arguments += ["-Binary", str(binary)]
        environment = clean_environment(**{RECORDING: str(recording)})
        if registered:
            # llmlint: ignore[tests_mirror_real_usage] suppressions.toml has the reason.
            environment[REGISTERED] = "1"
        if refuses is not None:
            environment[REFUSES] = refuses
        if path is not None:
            environment["PATH"] = path
        result = shell_run(
            [powershell, "-NoProfile", "-File", str(session), *arguments],
            cwd=tmp_path,
            env=environment,
            timeout=300,
        )
        recorded = recording.read_text(encoding="utf-8").splitlines() if recording.exists() else []
        return Ran(result.returncode, (result.stdout or "") + (result.stderr or ""), root, recorded)

    return install


def test_it_places_four_things_and_starts_nothing(
    installer: Callable[..., Ran], program: Path
) -> None:
    """The program, the configuration, the state directory, the registration — and no start."""
    ran = installer(binary=program)

    passing((ran.code, ran.said), describing="the installer under the fixture session")
    truth(ran.binary.is_file(), describing=f"{ran.binary} to be the program it placed")
    equal(
        ran.binary.read_bytes(),
        program.read_bytes(),
        describing="the program placed, byte for byte",
    )
    truth(ran.configuration.is_file(), describing=f"{ran.configuration} to be the configuration")
    truth(ran.state.is_dir(), describing=f"{ran.state} to be the state directory")

    registration = ran.asked("create")
    equal(len(registration), 1, describing=f"one registration asked of the manager: {ran.recorded}")
    contains(
        registration[0],
        "start= demand",
        describing="a registration the manager starts on demand",
    )
    contains(
        registration[0],
        "obj= NT SERVICE\\printobserver",
        describing="the service's own virtual account",
    )
    contains(
        registration[0],
        f'\\"{ran.binary}\\" server --config \\"{ran.configuration}\\"',
        describing="the program and configuration it placed, as the service's command line",
    )

    rules = _rules()
    forbidden = [
        line
        for line in ran.recorded
        for entry in rules.may_not_invoke
        if " ".join(line.split()).startswith(entry)
    ]
    equal(forbidden, [], describing="programs that start or enable a service, run by the installer")
    written = [
        line
        for line in ran.recorded
        for setting in rules.may_not_set
        if setting in " ".join(line.split())
    ]
    equal(written, [], describing="settings that enable a service, written by the installer")
    contains(ran.said, "started nothing", describing="what the installer said it did")
    contains(ran.said, _activation(), describing="the operator's own start command, printed")


def test_the_registration_carries_the_managers_own_unattended_settings(
    installer: Callable[..., Ran], program: Path
) -> None:
    """Brought back after a crash by the registration; started at boot by the operator's command.

    Both are read off `repo-policy.toml`'s statement of them in the manager's
    own vocabulary rather than restated here, so a registration that would stay
    down after a crash is caught on this host rather than only on a runner.
    """
    ran = installer(binary=program)
    passing((ran.code, ran.said), describing="the installer under the fixture session")
    rules = _rules()

    restarts = [line for line in ran.asked("failure") if rules.restarts_after_crash in line]
    equal(
        len(restarts),
        1,
        describing=f"one registration of `{rules.restarts_after_crash}`: {ran.recorded}",
    )
    contains(
        _activation(),
        rules.starts_at_boot,
        describing="the activation command the install path states",
    )
    truth(
        not any(rules.starts_at_boot in line for line in ran.recorded),
        describing="the installer leaving the automatic start to the operator's own command",
    )


def test_what_it_writes_is_what_the_descriptor_and_the_install_path_say(
    installer: Callable[..., Ran], program: Path
) -> None:
    """The program's file name, the configuration's state directory, the service's name."""
    ran = installer(binary=program)
    passing((ran.code, ran.said), describing="the installer under the fixture session")

    windows = descriptor(Repo(REPO_ROOT), "windows-x86_64")
    equal(ran.binary.name, windows.program, describing="the program's file name on Windows")
    written = tomllib.loads(ran.configuration.read_text(encoding="utf-8"))
    equal(
        written["state_dir"],
        str(ran.state),
        describing="the state directory the configuration names",
    )
    activation = _activation()
    for line in ran.recorded:
        if line.startswith("sc.exe "):
            contains(
                activation,
                f"-Name {line.split()[2]}",
                describing=f"the service `{line}` names to be the one the activation command names",
            )


class _AnswersEverything(BaseHTTPRequestHandler):
    """An OctoPrint that answers every request with an empty document."""

    def do_GET(self) -> None:
        """Answer an empty document."""
        body = b"{}"
        self.send_response(200)
        self.send_header("Content-Type", "application/json")
        self.send_header("Content-Length", str(len(body)))
        self.end_headers()
        self.wfile.write(body)

    def log_message(self, format: str, *args: object) -> None:
        """Say nothing: the journey's own output is what a reader reads."""


@pytest.fixture
def octoprint() -> Iterator[str]:
    """Where a stand-in OctoPrint answers, for as long as the journey runs."""
    server = ThreadingHTTPServer(("127.0.0.1", 0), _AnswersEverything)
    thread = threading.Thread(target=server.serve_forever, daemon=True)
    thread.start()
    try:
        yield f"http://127.0.0.1:{server.server_address[1]}"
    finally:
        server.shutdown()
        server.server_close()


def test_the_configuration_it_writes_is_one_the_server_starts_under(
    installer: Callable[..., Ran], program: Path, octoprint: str, tmp_path: Path
) -> None:
    """Filled in as an operator would, the template is a document the real server accepts."""
    ran = installer(binary=program)
    passing((ran.code, ran.said), describing="the installer under the fixture session")
    filled = (
        ran.configuration.read_text(encoding="utf-8")
        .replace('api_key = ""', 'api_key = "a-provisioned-key"')
        .replace('shared_secret = ""', 'shared_secret = "a-shared-secret"')
        .replace('url = "http://127.0.0.1:5000"', f'url = "{octoprint}"')
        .replace('listen = "127.0.0.1:8420"', 'listen = "127.0.0.1:0"')
    )
    ran.configuration.write_text(filled, encoding="utf-8")

    server = start(
        [str(ran.binary), "server", "--config", str(ran.configuration)],
        cwd=tmp_path,
        env=clean_environment(),
    )
    try:
        said = ""
        stream = server.stderr
        truth(stream is not None, describing="the server's own output")
        if stream is not None:
            for line in stream:
                said += line
                if "printobserver is serving on" in line or "will not start" in line:
                    break
        contains(
            said,
            "printobserver is serving on",
            describing="the server starting under the template the installer wrote",
        )
    finally:
        server.kill()
        server.wait()


def test_a_reinstall_leaves_the_configuration_alone_and_updates_the_registration(
    installer: Callable[..., Ran], program: Path
) -> None:
    """The operator's values survive; the manager is told the program and nothing about starting."""
    first = installer(binary=program)
    passing((first.code, first.said), describing="the first install")
    operators_own = first.configuration.read_text(encoding="utf-8").replace(
        'api_key = ""', 'api_key = "the-operators-own-key"'
    )
    first.configuration.write_text(operators_own, encoding="utf-8")

    again = installer(binary=program, root=first.root, registered=True)

    passing((again.code, again.said), describing="the reinstall")
    equal(
        first.configuration.read_text(encoding="utf-8"),
        operators_own,
        describing="the operator's configuration after a reinstall",
    )
    contains(again.said, "was left alone", describing="what the reinstall said")
    updated = again.asked("config")
    equal(len(updated), 1, describing=f"one update of the registration: {again.recorded}")
    truth("start=" not in updated[0], describing="the start type left as the operator set it")
    equal(again.asked("create"), [], describing="no second registration of a registered service")


def test_an_installer_that_can_find_no_program_says_so_and_places_nothing(
    installer: Callable[..., Ran], tmp_path: Path
) -> None:
    """No program on PATH and none named is a stop naming the next action, placing nothing."""
    empty = tmp_path / "empty-path"
    empty.mkdir()
    powershell_home = str(Path(_powershell()).parent)

    ran = installer(binary=None, path=os.pathsep.join([powershell_home, str(empty)]))

    failing((ran.code, ran.said), naming="no printobserver program on PATH")
    contains(ran.said, "-Binary", describing="the next action it named")
    truth(not ran.root.exists(), describing="nothing to have been placed")
    equal(ran.recorded, [], describing="nothing asked of the manager")


def test_a_value_carrying_a_quote_is_refused_before_anything_is_placed(
    installer: Callable[..., Ran], program: Path, tmp_path: Path
) -> None:
    """A path a service's command line or a TOML document cannot carry is refused, not written."""
    ran = installer(binary=program, root=tmp_path / "it's")

    failing((ran.code, ran.said), naming="carries a quote")
    truth(not ran.root.exists(), describing="nothing to have been placed")
    equal(ran.recorded, [], describing="nothing asked of the manager")


def test_a_value_carrying_a_newline_is_refused_before_anything_is_placed(
    installer: Callable[..., Ran], program: Path, tmp_path: Path
) -> None:
    """A path a TOML document reads as two settings is refused, naming the next action."""
    ran = installer(binary=program, root=tmp_path / "two\nlines")

    failing((ran.code, ran.said), naming="carries a newline")
    contains(ran.said, "on one line", describing="the next action it named")
    truth(not (tmp_path / "two\nlines").exists(), describing="nothing to have been placed")
    equal(ran.recorded, [], describing="nothing asked of the manager")


def test_a_named_program_that_is_not_a_file_is_refused(
    installer: Callable[..., Ran], tmp_path: Path
) -> None:
    """`-Binary` naming nothing is a stop with the next action, before anything is placed."""
    ran = installer(binary=tmp_path / "nowhere" / "printobserver.exe")

    failing((ran.code, ran.said), naming="is not a file")
    contains(ran.said, "-Binary", describing="the next action it named")
    truth(not ran.root.exists(), describing="nothing to have been placed")
    equal(ran.recorded, [], describing="nothing asked of the manager")


def test_a_root_that_cannot_hold_a_directory_is_refused_naming_it(
    installer: Callable[..., Ran], program: Path, tmp_path: Path
) -> None:
    """A root that is a file is one no directory can be made under, and the refusal says which."""
    occupied = tmp_path / "occupied"
    occupied.write_text("a file where the root should be", encoding="utf-8")

    ran = installer(binary=program, root=occupied)

    failing((ran.code, ran.said), naming="could not be created")
    contains(ran.said, "-Root", describing="the next action it named")
    equal(ran.recorded, [], describing="nothing asked of the manager")


@pytest.mark.skipif(
    os.name == "nt", reason="an administrator writes anywhere, so nothing here can refuse the copy"
)
def test_a_program_directory_that_cannot_be_written_is_refused_naming_it(
    installer: Callable[..., Ran], program: Path, tmp_path: Path
) -> None:
    """A program that cannot be put in place is a stop naming the copy, before the manager hears."""
    root = tmp_path / "root-readonly"
    program_directory = root / "Program Files" / "printobserver"
    program_directory.mkdir(parents=True)
    program_directory.chmod(0o555)
    try:
        ran = installer(binary=program, root=root)
    finally:
        program_directory.chmod(0o755)

    failing((ran.code, ran.said), naming="could not be copied")
    contains(ran.said, "-Root", describing="the next action it named")
    equal(ran.recorded, [], describing="nothing asked of the manager")


def test_a_manager_that_refuses_the_registration_stops_the_install_naming_it(
    installer: Callable[..., Ran], program: Path
) -> None:
    """The manager's own refusal is quoted back, with what to do next, and nothing after it runs."""
    ran = installer(binary=program, refuses="create")

    failing((ran.code, ran.said), naming="registering the service failed")
    contains(ran.said, REFUSAL, describing="the manager's own words, quoted back")
    contains(ran.said, "elevated PowerShell", describing="the next action it named")
    equal(ran.asked("failure"), [], describing="nothing asked of the manager after the refusal")
    truth("started nothing" not in ran.said, describing="no success line after a refusal")


def test_a_grant_the_host_refuses_stops_the_install_naming_it(
    installer: Callable[..., Ran], program: Path
) -> None:
    """A state directory that cannot be made private is not left readable and reported installed."""
    ran = installer(binary=program, refuses=GRANTING)

    failing((ran.code, ran.said), naming="making the state directory private failed")
    contains(ran.said, REFUSAL, describing="the granting tool's own words, quoted back")
    truth(not ran.configuration.exists(), describing="no configuration written after the refusal")
    truth("started nothing" not in ran.said, describing="no success line after a refusal")
