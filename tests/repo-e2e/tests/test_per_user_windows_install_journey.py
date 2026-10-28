# llmlint: ignore[expensive_tests_stay_behind_their_own_edge] suppressions.toml has the reason.
"""The per-user Windows install: installed, upgraded, migrated to, and run by Task Scheduler.

`scripts/install.ps1` with no switch is the `windows-scheduled-task` pair's
installer: it puts the program in place, writes the user's own server
configuration and state under `%LOCALAPPDATA%`, and registers and starts the
Scheduled Task that runs the server as that user. `scripts/migrate-service-to-
per-user.ps1` is the one elevated step that moves a service install's
configuration and record there.

Both are PowerShell, which runs on every host, and only a Windows host has
Task Scheduler and a service control manager. So the journey keeps the split
`test_windows_service_installer_journey.py` keeps:

- **On every host**, the committed scripts run under `pwsh` inside a session in
  which `sc.exe`, `whoami.exe`, `icacls` and — for the installer — the Task
  Scheduler cmdlets are functions that record being called. What the scripts
  wrote is read off a throwaway root, and what they asked of the host is read
  out of that recording: the task definition they registered, the order of
  their requests, what they refused. The migration's record is a real one: a
  real server, started under the configuration the service installer writes,
  records a print and an image, and a real server started under the migrated
  configuration serves them back.
- **On Windows**, the installer runs against the real Task Scheduler, with only
  the service query stood in for — the gate's own service journeys register a
  `printobserver` service of their own while this runs. The task it registers
  runs the real server, which is asked for `prints`, killed and brought back,
  reconfigured, and upgraded. Every task is named for this run, through the
  installer's own `PRINTOBSERVER_TASK_NAME`, and unregistered on every exit
  path.
"""

from __future__ import annotations

import functools
import gzip
import hashlib
import html
import http.client
import io
import json
import os
import re
import secrets
import shutil
import socket
import sqlite3
import sys
import tarfile
import time
import tomllib
from collections.abc import Callable, Iterator
from dataclasses import dataclass
from pathlib import Path
from typing import NewType
from urllib.parse import urlsplit

import pytest
from journey import DEBUG_BUILD, HERE, REPO_ROOT, clean_environment, install_the_skill, run
from release_artifacts.installing import powershell
from release_artifacts.stand_in import stand_in_program
from release_artifacts.world import (
    IMAGE,
    INGRESS_HEADER,
    INGRESS_WORD,
    Credential,
    Machine,
    verifier_of,
)
from repo_checks import platforms
from repo_checks.checks_service import installer_for
from repo_checks.expect import absent, contains, equal, failing, passing, truth
from repo_checks.model import Repo
from repo_checks.platforms import ServiceManager
from repo_checks.shell import run as shell_run
from repo_checks.shell import start

pytestmark = DEBUG_BUILD

REPO = Repo(REPO_ROOT)

#: The committed installer and the migration, as the policy declares them.
INSTALLER = REPO_ROOT / installer_for(REPO, ServiceManager.WINDOWS_SCHEDULED_TASK)
SERVICE_INSTALLER = REPO_ROOT / installer_for(REPO, ServiceManager.WINDOWS_SERVICE)
MIGRATION = REPO_ROOT / "scripts" / "migrate-service-to-per-user.ps1"

#: The file a release publishes its digests in.
CHECKSUMS = "SHA256SUMS"

#: The variables the fixture session reads: where it records, whether the
#: stand-in manager holds the service, whether the stand-in token is a filtered
#: one, a copied file the stand-in owner-setter makes differ, and whether Task
#: Scheduler is stood in at all.
RECORDING = "PRINTOBSERVER_FIXTURE_RECORDING"
REGISTERED = "PRINTOBSERVER_FIXTURE_REGISTERED"
NOT_ELEVATED = "PRINTOBSERVER_FIXTURE_NOT_ELEVATED"
ALTERS = "PRINTOBSERVER_FIXTURE_ALTERS"
#: The requests the stand-ins refuse, comma-separated: a verb of the manager's
#: tool, `icacls`, or a Task Scheduler cmdlet by name, a verb optionally with
#: the exit status it answers (`stop=1062`); a refused cmdlet throws, as the
#: real one does.
REFUSES = "PRINTOBSERVER_FIXTURE_REFUSES"
TASKS = "PRINTOBSERVER_FIXTURE_TASKS"

#: What Task Scheduler answers a start of an interactive-token task whose user
#: has no session: ERROR_REQUEST_REFUSED, as an HRESULT and in its own words.
SESSIONLESS_START_REFUSED_CODE = "0x800710E0"
SESSIONLESS_START_REFUSED_TEXT = "The operator or administrator has refused the request"

#: The installer's own seam for the task's name.
TASK_NAME = "PRINTOBSERVER_TASK_NAME"

#: The account the stand-in session runs as.
DOMAIN, USER = "PRINTPC", "operator"
ACCOUNT = f"{DOMAIN}\\{USER}"


@functools.cache
def _account() -> str:
    """The account the installer registers the task for: the stand-in's, or this Windows host's."""
    if not ON_WINDOWS:
        return ACCOUNT
    return _powershell_answer("[System.Security.Principal.WindowsIdentity]::GetCurrent().Name")


#: One element of a task definition, read by its tag: a definition is a fixed
#: document of Task Scheduler's own shape, in which each element named here
#: occurs once under its parent.
ELEMENT = r"<{tag}(?:\s[^>]*)?>(.*?)</{tag}>"

#: The service's own places beneath a root, as the service installer writes them.
SERVICE_CONFIG = ("ProgramData", "printobserver", "config.toml")
SERVICE_STATE = ("ProgramData", "printobserver", "state")

#: How long a journey waits for a server, a task or a restart before calling
#: it hung: a hang guard, not a pace.
WITHIN_SECONDS = 180

#: The store the server keeps its record in.
STORE = "printobserver.sqlite3"

ON_WINDOWS = sys.platform == "win32"
WINDOWS_ONLY = pytest.mark.skipif(
    not ON_WINDOWS, reason="Task Scheduler is Windows's; every other host runs the stand-in cases"
)


def _version() -> str:
    """The version release automation wrote into the workspace."""
    with (REPO_ROOT / "Cargo.toml").open("rb") as handle:
        return str(tomllib.load(handle)["workspace"]["package"]["version"])


def _older(version: str) -> str:
    """A version of the same length as `version` that is not it."""
    last = "0" if version[-1] != "0" else "9"
    return version[:-1] + last


# llmlint: ignore[expensive_tests_stay_behind_their_own_edge] suppressions.toml has the reason.
@pytest.fixture(scope="module")
def program() -> Path:
    """The `printobserver` program built from this tree for this host, built once."""
    passing(
        run(["cargo", "build", "--locked", "-p", "printobserver"], REPO_ROOT, timeout=2400),
        describing="building the printobserver program",
    )
    return REPO_ROOT / "target" / "debug" / HERE.program


def _windows_platform() -> platforms.Platform:
    """The Windows platform a release is staged for: this host's own, or the first."""
    if ON_WINDOWS:
        return platforms.host(REPO)
    return next(
        one
        for one in platforms.supported(REPO)
        if one.service_manager == ServiceManager.WINDOWS_SERVICE
    )


def _machine(identifier: str) -> str:
    """What `PROCESSOR_ARCHITECTURE` reports on one Windows platform."""
    return next(
        machine
        for (system, machine), named in platforms.HOSTS.items()
        if system == "Windows" and named == identifier
    )


def _stage(into: Path, program: bytes) -> Path:
    """A release directory serving `program` as the newest release, as the forge serves it."""
    platform = _windows_platform()
    download = into / "latest" / "download"
    download.mkdir(parents=True, exist_ok=True)
    raw = io.BytesIO()
    with tarfile.open(fileobj=raw, mode="w") as archive:
        info = tarfile.TarInfo(platform.program)
        info.size = len(program)
        info.mode = 0o755
        archive.addfile(info, io.BytesIO(program))
    asset = download / platform.asset
    asset.write_bytes(gzip.compress(raw.getvalue(), mtime=0))
    (download / CHECKSUMS).write_text(
        f"{hashlib.sha256(asset.read_bytes()).hexdigest()}  {asset.name}\n", encoding="utf-8"
    )
    return into


def _session(into: Path, *, stand_in_tasks: bool, piped: bool = False) -> Path:
    """A script that stands the host's tools in, then runs the script it is handed.

    Each stand-in appends what it was called with to the recording. The
    manager answers a query as Windows does — the service is not there unless
    the journey says it is — and a stopped service's process as gone; the
    token reads as an elevated one unless the journey says otherwise; and the
    owner-setter, which the migration runs between its copy and its
    verification, makes one copied file differ when the journey names one. Task
    Scheduler's cmdlets are stood in only where the journey asks: the registered
    definition is written beside the recording, and a task is there once it has
    been registered.
    """
    # llmlint: ignore[e2e_not_mocked, tests_mirror_real_usage] suppressions.toml has the reasons.
    tasks = (
        f"""
function Get-ScheduledTask {{
    [CmdletBinding()] param([string]$TaskName, [string]$TaskPath)
    Record "Get-ScheduledTask $TaskName"
    if (Refused 'Get-ScheduledTask') {{
        Write-Error -Category PermissionDenied 'Access is denied, says the stand-in.'
        return
    }}
    if (Test-Path -LiteralPath "$env:{RECORDING}.task.xml") {{
        [pscustomobject]@{{ TaskName = $TaskName }}
    }}
}}
function Register-ScheduledTask {{
    [CmdletBinding()]
    param([string]$TaskName, [string]$TaskPath, [string]$Xml, [switch]$Force)
    Record "Register-ScheduledTask $TaskName $TaskPath"
    if (Refused 'Register-ScheduledTask') {{
        throw 'Access is denied, says the stand-in.'
    }}
    [IO.File]::WriteAllText("$env:{RECORDING}.task.xml", $Xml)
    [pscustomobject]@{{ TaskName = $TaskName }}
}}
function Start-ScheduledTask {{
    [CmdletBinding()] param([string]$TaskName, [string]$TaskPath)
    Record "Start-ScheduledTask $TaskName"
    if (Refused 'Start-ScheduledTask') {{ throw 'The user account is not logged on.' }}
}}
function Stop-ScheduledTask {{
    [CmdletBinding()] param([string]$TaskName, [string]$TaskPath)
    Record "Stop-ScheduledTask $TaskName"
    if (Refused 'Stop-ScheduledTask') {{ throw 'The task is busy, says the stand-in.' }}
}}
function Disable-ScheduledTask {{
    [CmdletBinding()] param([string]$TaskName, [string]$TaskPath)
    Record "Disable-ScheduledTask $TaskName"
    if (Refused 'Disable-ScheduledTask') {{ throw 'Access is denied, says the stand-in.' }}
}}
function Enable-ScheduledTask {{
    [CmdletBinding()] param([string]$TaskName, [string]$TaskPath)
    Record "Enable-ScheduledTask $TaskName"
    if (Refused 'Enable-ScheduledTask') {{ throw 'Access is denied, says the stand-in.' }}
}}
"""
        if stand_in_tasks
        else ""
    )
    # llmlint: ignore[e2e_not_mocked, tests_mirror_real_usage] suppressions.toml has the reasons.
    script = f"""
function Record([string]$Line) {{ Add-Content -LiteralPath $env:{RECORDING} -Value $Line }}
function Refused([string]$Name) {{
    $named = @("$env:{REFUSES}" -split ',' | Where-Object {{ ($_ -split '=', 2)[0] -eq $Name }})
    if ($named.Count -eq 0) {{ return $false }}
    $parts = @($named[0] -split '=', 2)
    $status = if ($parts.Count -gt 1) {{ $parts[1] }} else {{ '' }}
    [Console]::Error.WriteLine('Access is denied, says the stand-in.')
    $global:LASTEXITCODE = if ($status) {{ [int]$status }} else {{ 5 }}
    return $true
}}
function sc.exe {{
    Record ('sc.exe ' + ($args -join ' '))
    $global:LASTEXITCODE = 0
    if ($args[0] -eq 'queryex' -and "$env:{REFUSES}" -split ',' -contains 'queryex') {{
        'SERVICE_NAME: printobserver'
        '        STATE              : 4  RUNNING'
        '        PID                : 4242'
        return
    }}
    if (Refused $args[0]) {{ return }}
    switch ($args[0]) {{
        'query' {{ if (-not $env:{REGISTERED}) {{ $global:LASTEXITCODE = 1060 }} }}
        'queryex' {{
            'SERVICE_NAME: printobserver'
            '        STATE              : 1  STOPPED'
            '        PID                : 0'
        }}
    }}
}}
function whoami.exe {{
    Record ('whoami.exe ' + ($args -join ' '))
    if ($env:{NOT_ELEVATED}) {{ 'Mandatory Label\\Medium Mandatory Level Label S-1-16-8192' }}
    else {{ 'Mandatory Label\\High Mandatory Level Label S-1-16-12288' }}
    $global:LASTEXITCODE = 0
}}
function icacls {{
    Record ('icacls ' + ($args -join ' '))
    $global:LASTEXITCODE = 0
    if (Refused 'icacls') {{ return }}
    if ("$env:{ALTERS}".StartsWith('remove:')) {{
        Remove-Item -LiteralPath "$env:{ALTERS}".Substring(7)
    }} elseif ("$env:{ALTERS}".StartsWith('unreadable:')) {{
        & chmod 000 "$env:{ALTERS}".Substring(11)
    }} elseif ($env:{ALTERS}) {{
        Add-Content -LiteralPath $env:{ALTERS} -Value 'made to differ'
    }}
}}
{tasks}
{"" if piped else _AS_A_FILE}
"""
    name = "tasks-session" if stand_in_tasks else "host-session"
    path = into / (f"{name}-piped.ps1" if piped else f"{name}.ps1")
    path.write_text(script, encoding="utf-8")
    return path


#: How a session runs the script it is handed as a file, whose exit is the
#: session's. A piped session carries the stand-ins alone: it is dot-sourced at
#: a prompt of `pwsh -Command`, where the install path's own command pipes the
#: script's text into `iex` — a prompt, unlike a file, being what that command
#: is typed at — and the session goes on and says what status it read.
_AS_A_FILE = """
$target = $args[0]
$rest = @($args | Select-Object -Skip 1)
& $target @rest
exit $LASTEXITCODE
"""
_PIPED = (
    ". '{session}'; Get-Content -LiteralPath '{script}' -Raw | iex; "
    'Write-Output "the session goes on, LASTEXITCODE=$LASTEXITCODE"'
)


@dataclass(frozen=True, slots=True)
class Ran:
    """One run of a script under the fixture session."""

    code: int
    said: str
    recorded: list[str]
    #: The task definition the stand-in Task Scheduler was handed, if any.
    definition: str | None


@dataclass(frozen=True, slots=True)
class Home:
    """One user's profile root, and where the per-user install keeps things in it."""

    root: Path

    @property
    def local(self) -> Path:
        """`%LOCALAPPDATA%`."""
        return self.root / "AppData" / "Local"

    @property
    def program(self) -> Path:
        """Where the program goes."""
        return self.local / "Programs" / "printobserver" / _windows_platform().program

    @property
    def config(self) -> Path:
        """The per-user configuration."""
        return self.local / "printobserver" / "config.toml"

    @property
    def state(self) -> Path:
        """The per-user state directory."""
        return self.local / "printobserver" / "state"


def _home(tmp_path: Path, name: str) -> Home:
    home = Home(tmp_path / name)
    home.local.mkdir(parents=True)
    (home.root / "temp").mkdir()
    return home


def _environment(home: Home, recording: Path, **extra: str) -> dict[str, str]:
    """A Windows session of one user: their local application data, their temp, their name."""
    temporary = str(home.root / "temp")
    environment = clean_environment(
        LOCALAPPDATA=str(home.local),
        TMPDIR=temporary,
        TMP=temporary,
        TEMP=temporary,
        **{RECORDING: str(recording)},
        **extra,
    )
    for name in (REGISTERED, NOT_ELEVATED, ALTERS, REFUSES, TASK_NAME):
        if name not in extra:
            environment.pop(name, None)
    if not ON_WINDOWS:
        # The answers only Windows gives, given as Windows gives them.
        environment["OS"] = "Windows_NT"
        environment["PROCESSOR_ARCHITECTURE"] = _machine(_windows_platform().id)
        environment.pop("PROCESSOR_ARCHITEW6432", None)
        environment["USERDOMAIN"] = extra.get("USERDOMAIN", DOMAIN)
        environment["USERNAME"] = extra.get("USERNAME", USER)
    return environment


def _run_script(
    session: Path, script: Path, cwd: Path, environment: dict[str, str], *arguments: str
) -> Ran:
    recording = Path(environment[RECORDING])
    piped = session.stem.endswith("-piped")
    invoked = (
        [
            "-Command",
            _PIPED.format(
                session=str(session).replace("'", "''"), script=str(script).replace("'", "''")
            ),
        ]
        if piped
        else ["-File", str(session), str(script), *arguments]
    )
    result = shell_run(
        [powershell(), "-NoProfile", *invoked], cwd=cwd, env=environment, timeout=600
    )
    recorded = recording.read_text(encoding="utf-8").splitlines() if recording.exists() else []
    definition = Path(f"{recording}.task.xml")
    return Ran(
        result.returncode,
        (result.stdout or "") + (result.stderr or ""),
        recorded,
        definition.read_text(encoding="utf-8") if definition.exists() else None,
    )


@dataclass
class Installing:
    """The installer under the stand-in session, against one home and one release."""

    tmp_path: Path
    session: Path
    counter: int = 0

    def __call__(
        self,
        home: Home,
        release: Path,
        *arguments: str,
        recording: Path | None = None,
        extra: dict[str, str] | None = None,
    ) -> Ran:
        """Run the installer for `home` against `release`, recording into a file of its own."""
        self.counter += 1
        recording = recording or self.tmp_path / f"recorded-{self.counter}"
        environment = _environment(
            home, recording, PRINTOBSERVER_RELEASE_BASE=str(release), **(extra or {})
        )
        return _run_script(self.session, INSTALLER, home.root, environment, *arguments)


@pytest.fixture
def installing(tmp_path: Path) -> Installing:
    """The committed installer under a session standing Task Scheduler in."""
    return Installing(tmp_path, _session(tmp_path, stand_in_tasks=True))


@pytest.fixture
def releases(tmp_path: Path) -> Callable[[str], Path]:
    """A release directory per version, each serving a program that reports it."""

    def stage(version: str) -> Path:
        built = stand_in_program(
            REPO, tmp_path / f"program-{version}" / "printobserver.exe", f"printobserver {version}"
        )
        return _stage(tmp_path / f"release-{version}", built.read_bytes())

    return stage


def _task(definition: str | None) -> str:
    """A task definition handed to, or exported by, Task Scheduler."""
    truth(definition is not None, describing="a task definition handed to Task Scheduler")
    return definition or ""


def _text(task: str, path: str) -> str:
    """The text of the element at `path`, each step a tag under the one before it."""
    found = task
    for tag in path.split("/"):
        match = re.search(ELEMENT.format(tag=re.escape(tag)), found, re.DOTALL)
        truth(match is not None, describing=f"the task definition to carry `{path}`")
        if match is None:
            return ""
        found = match.group(1)
    return html.unescape(found.strip())


def _holds_the_contract(task: str, account: str, program: Path, config: Path) -> None:
    """The task's principal, triggers, settings and action, as the install path states them."""
    equal(_text(task, "Principals/Principal/UserId"), account, describing="the principal")
    equal(
        _text(task, "Principals/Principal/LogonType"),
        "InteractiveToken",
        describing="a principal run while its user is logged on",
    )
    equal(
        _text(task, "Principals/Principal/RunLevel"),
        "LeastPrivilege",
        describing="a principal run with its limited token",
    )
    equal(
        _text(task, "Triggers/LogonTrigger/UserId"),
        account,
        describing="the trigger: at logon of the installing user",
    )
    equal(
        _text(task, "Triggers/TimeTrigger/Repetition/Interval"),
        "PT1M",
        describing="the start that brings a server back once a minute",
    )
    for setting, value in (
        ("ExecutionTimeLimit", "PT0S"),
        ("MultipleInstancesPolicy", "IgnoreNew"),
        ("DisallowStartIfOnBatteries", "false"),
        ("StopIfGoingOnBatteries", "false"),
        ("RestartOnFailure/Interval", "PT1M"),
    ):
        equal(_text(task, f"Settings/{setting}"), value, describing=f"the `{setting}` setting")
    equal(
        _text(task, "Actions/Exec/Command").strip('"'),
        str(program),
        describing="the program the task runs",
    )
    equal(
        _text(task, "Actions/Exec/Arguments"),
        f'server --config "{config}"',
        describing="the server and the configuration the task runs",
    )


def _tree(root: Path) -> dict[str, str]:
    """Every file under `root`, by its path there, with its SHA-256."""
    return {
        path.relative_to(root).as_posix(): hashlib.sha256(path.read_bytes()).hexdigest()
        for path in sorted(root.rglob("*"))
        if path.is_file()
    }


def test_with_no_switch_it_installs_the_per_user_server_and_says_what_is_next(
    installing: Installing, releases: Callable[[str], Path], tmp_path: Path
) -> None:
    """The program, the configuration, the state, the task registered as the user and started."""
    home = _home(tmp_path, "home")
    release = releases(_version())
    outside = {path for path in tmp_path.rglob("*") if home.root not in path.parents}

    ran = installing(home, release)

    passing((ran.code, ran.said), describing="install.ps1 with no switch")
    truth(home.program.is_file(), describing=f"{home.program} to be the program")
    written = tomllib.loads(home.config.read_text(encoding="utf-8"))
    equal(written["state_dir"], str(home.state), describing="the per-user state directory")
    equal(
        written["supervisor"]["skill_path"],
        str(home.state / "skills" / "printobserver" / "SKILL.md"),
        describing="the skill the per-user configuration names",
    )
    truth(home.state.is_dir(), describing=f"{home.state} to be the state directory")

    registered = [line for line in ran.recorded if line.startswith("Register-ScheduledTask")]
    equal(registered, ["Register-ScheduledTask printobserver \\"], describing="one registration")
    truth(
        ran.recorded.index(registered[0]) < ran.recorded.index("Start-ScheduledTask printobserver"),
        describing=f"the task started after it was registered: {ran.recorded}",
    )
    _holds_the_contract(_task(ran.definition), _account(), home.program, home.config)
    for elevated in ("icacls", "sc.exe create", "sc.exe config", "whoami.exe"):
        truth(
            not any(line.startswith(elevated) for line in ran.recorded),
            describing=f"no `{elevated}` from a per-user install: {ran.recorded}",
        )

    for step in (
        "printobserver credential issue",
        f"into {home.config}",
        "gh skill install nickderobertis/printobserver printobserver --dir "
        f"{home.state / 'skills'}",
        "printobserver sign-in",
        "Stop-ScheduledTask -TaskName printobserver; Start-ScheduledTask -TaskName printobserver",
    ):
        contains(ran.said, step, describing="the next steps it printed")
    absent(ran.said, "elevated PowerShell,", describing="a next step needing elevation")
    created = {path for path in tmp_path.rglob("*") if home.root not in path.parents}
    equal(
        sorted(str(path) for path in created - outside if not path.name.startswith("recorded-")),
        [],
        describing="anything written outside the user's profile root",
    )
    equal(sorted((home.root / "temp").iterdir()), [], describing="its own directory, removed")


def test_the_configuration_is_the_service_installers_template_but_for_its_two_paths(
    installing: Installing, releases: Callable[[str], Path], tmp_path: Path, program: Path
) -> None:
    """One template, two installers: only `state_dir` and `supervisor.skill_path` differ."""
    home = _home(tmp_path, "home-template")
    installed = installing(home, releases(_version()))
    passing((installed.code, installed.said), describing="the per-user install")
    root = tmp_path / "service-root"
    service = _run_script(
        _session(tmp_path, stand_in_tasks=False),
        SERVICE_INSTALLER,
        tmp_path,
        _environment(home, tmp_path / "recorded-service"),
        "-Root",
        str(root),
        "-Binary",
        str(program),
    )
    passing((service.code, service.said), describing="the service installer")

    per_user = tomllib.loads(home.config.read_text(encoding="utf-8"))
    serviced = tomllib.loads(root.joinpath(*SERVICE_CONFIG).read_text(encoding="utf-8"))
    for document in (per_user, serviced):
        document.pop("state_dir")
        document["supervisor"].pop("skill_path")
    equal(per_user, serviced, describing="the per-user configuration beside the service's")


def test_an_observation_interval_is_written_and_one_the_server_refuses_is_refused(
    installing: Installing, releases: Callable[[str], Path], tmp_path: Path
) -> None:
    """`-ObservationIntervalSeconds` written and refused as the service installer does."""
    release = releases(_version())
    home = _home(tmp_path, "home-interval")
    ran = installing(home, release, "-ObservationIntervalSeconds", "60")
    passing((ran.code, ran.said), describing="install.ps1 given an interval")
    written = tomllib.loads(home.config.read_text(encoding="utf-8"))
    equal(written["supervisor"]["observation_interval_s"], 60, describing="the interval written")

    for refused in ("0", "060", "sixty", "9223372036854775808"):
        elsewhere = _home(tmp_path, f"home-interval-{refused}")
        ran = installing(elsewhere, release, "-ObservationIntervalSeconds", refused)
        failing((ran.code, ran.said), naming=f"'{refused}' is not one")
        equal(ran.recorded, [], describing="nothing asked of the host")
        equal(sorted(elsewhere.local.iterdir()), [], describing="nothing written for a refused run")


def test_with_the_service_registered_it_refuses_naming_the_migration_and_changes_nothing(
    installing: Installing, releases: Callable[[str], Path], tmp_path: Path
) -> None:
    """A host whose server is the service gets no second server beside it."""
    home = _home(tmp_path, "home-serviced")

    ran = installing(home, releases(_version()), extra={REGISTERED: "1"})

    failing((ran.code, ran.said), naming="service is installed on this machine")
    contains(
        ran.said,
        "scripts/migrate-service-to-per-user.ps1))) -LocalAppData "
        f"'{home.local}' -User '{_account()}'",
        describing="the migration command it named, for this user",
    )
    contains(ran.said, "-ClientOnly", describing="the other way on it named")
    equal(ran.recorded, ["sc.exe query printobserver"], describing="what it asked of the host")
    equal(sorted(home.local.iterdir()), [], describing="anything written under the profile")
    equal(sorted((home.root / "temp").iterdir()), [], describing="anything left in its temp")


def test_client_only_beside_the_service_installs_the_program_alone_as_before(
    installing: Installing, releases: Callable[[str], Path], tmp_path: Path
) -> None:
    """`-ClientOnly` asks nothing of the manager or Task Scheduler; it prints the old next steps."""
    home = _home(tmp_path, "home-client")

    ran = installing(home, releases(_version()), "-ClientOnly", extra={REGISTERED: "1"})

    passing((ran.code, ran.said), describing="install.ps1 -ClientOnly beside the service")
    truth(home.program.is_file(), describing="the program installed")
    truth(not home.config.exists(), describing="no per-user configuration")
    equal(ran.recorded, [], describing="nothing asked of the manager or Task Scheduler")
    contains(ran.said, "next, in an elevated PowerShell", describing="the service's next steps")
    contains(ran.said, "scripts/install-service.ps1 | iex", describing="the installer it named")

    refused = installing(
        _home(tmp_path, "home-client-interval"),
        releases(_version()),
        "-ClientOnly",
        "-ObservationIntervalSeconds",
        "60",
    )
    failing((refused.code, refused.said), naming="-ClientOnly, which installs no server")


def test_running_it_again_upgrades_in_place_and_the_installer_writes_no_state(
    installing: Installing, releases: Callable[[str], Path], tmp_path: Path
) -> None:
    """The task held off and stopped, the program replaced, the configuration and state kept.

    No server runs under the stand-in Task Scheduler, so every byte under the
    profile after the second run that is not the program is one the installer
    either wrote or left — which is what separates its writes from a running
    server's own.
    """
    home = _home(tmp_path, "home-upgrade")
    older = _older(_version())
    first = installing(home, releases(older))
    passing((first.code, first.said), describing="the first install")
    operators_own = home.config.read_text(encoding="utf-8").replace(
        'api_key = ""', 'api_key = "the-operators-own-key"'
    )
    home.config.write_text(operators_own, encoding="utf-8")
    # llmlint: ignore[tests_mirror_real_usage] suppressions.toml has the reason.
    (home.state / STORE).write_bytes(b"a record the server wrote")
    (home.state / "images").mkdir()
    # llmlint: ignore[tests_mirror_real_usage] suppressions.toml has the reason.
    (home.state / "images" / "frame.jpg").write_bytes(IMAGE)
    before = _tree(home.state)
    config_before = home.config.read_bytes()

    again = installing(home, releases(_version()), recording=tmp_path / "recorded-1")

    passing((again.code, again.said), describing="the second run of the same command")
    equal(home.config.read_bytes(), config_before, describing="the configuration, byte for byte")
    equal(_tree(home.state), before, describing="the state directory after the upgrade")
    contains(again.said, "kept", describing="what the upgrade said it kept")
    result = run([str(home.program), "--version"], home.root, timeout=120)
    contains(result.stdout, _version(), describing="the program after the upgrade")
    upgrade = again.recorded[len(first.recorded) :]
    order = [
        "Disable-ScheduledTask printobserver",
        "Stop-ScheduledTask printobserver",
        "Register-ScheduledTask printobserver \\",
        "Start-ScheduledTask printobserver",
    ]
    equal(
        [line for line in upgrade if line in order],
        order,
        describing="the task held off and stopped first, re-registered and started after",
    )


def test_a_task_name_that_is_not_one_is_refused_before_anything_is_written(
    installing: Installing, releases: Callable[[str], Path], tmp_path: Path
) -> None:
    """The seam that names a test's own task is held to a task's name."""
    home = _home(tmp_path, "home-badname")
    ran = installing(home, releases(_version()), extra={TASK_NAME: "two words"})
    failing((ran.code, ran.said), naming="not a name this script registers")
    equal(sorted(home.local.iterdir()), [], describing="nothing written")


@dataclass(frozen=True, slots=True)
class Serviced:
    """A service install's root, holding a real record the real server wrote."""

    root: Path
    print_id: PrintId
    image_id: ImageId

    @property
    def config(self) -> Path:
        """The service's configuration beneath the root."""
        return self.root.joinpath(*SERVICE_CONFIG)

    @property
    def state(self) -> Path:
        """The service's state directory beneath the root."""
        return self.root.joinpath(*SERVICE_STATE)


def _serving(server: object) -> str:
    """The address a started server says it serves on."""
    stream = getattr(server, "stderr", None)
    said = ""
    if stream is not None:
        for line in stream:
            said += line
            if "printobserver is serving on " in line:
                return "http://" + line.split("printobserver is serving on ", 1)[1].strip()
            if "will not start" in line:
                break
    message = f"the server did not say where it serves:\n{said}"
    raise AssertionError(message)


def _fill_in(config: Path, machine: Machine, credential: Credential, listen: str) -> None:
    """Fill a configuration in as an operator does, with the verifier above its first table."""
    filled = (
        config.read_text(encoding="utf-8")
        .replace('api_key = ""', 'api_key = "a-provisioned-key"')
        .replace('shared_secret = ""', f'shared_secret = "{INGRESS_WORD}"')
        .replace('url = "http://127.0.0.1:5000"', f'url = "{machine.url}"')
        .replace('listen = "127.0.0.1:8420"', f'listen = "{listen}"')
    )
    filled = f'api.credential_verifier = "{verifier_of(credential)}"\n' + filled
    config.write_text(filled, encoding="utf-8")


def _alert(server: str, machine: Machine) -> None:
    """Post one failure alert to the ingress, which opens a print and stores its image."""
    body = json.dumps(
        {
            "event": {"type": "PrintFailure", "is_warning": False, "print_paused": True},
            "printer": {"id": 17, "name": "A stand-in machine"},
            "print": {"id": 4211, "filename": "benchy.gcode", "started_at": 1772366400.5},
            "img_url": f"{machine.url}/snapshot.jpg",
        }
    )
    # llmlint: ignore[async_typed_clients_at_boundaries] one post in a sequential proof
    connection = http.client.HTTPConnection(urlsplit(server).netloc, timeout=30)
    connection.request(
        "POST",
        "/obico/webhook",
        body=body,
        headers={"Content-Type": "application/json", INGRESS_HEADER: INGRESS_WORD},
    )
    answered = connection.getresponse()
    answered.read()
    connection.close()
    truth(
        answered.status in (200, 202),
        describing=f"the ingress taking the alert ({answered.status})",
    )


#: A print's identifier, as the store mints it.
PrintId = NewType("PrintId", str)

#: An image's identifier, as the store mints it.
ImageId = NewType("ImageId", str)


@dataclass(frozen=True, slots=True)
class Recorded:
    """The print and the image one alert opened, as the store identifies them."""

    print_id: PrintId
    image_id: ImageId


@dataclass(frozen=True, slots=True)
class Answer:
    """What one client command answered."""

    code: int
    #: Its standard output: the machine-readable answer.
    out: str
    #: Its standard error: what it said about a refusal.
    said: str


def _recorded(state: Path) -> Recorded | None:
    """The print and image the alert opened, once both are in the store."""
    store = state / STORE
    if not store.is_file():
        return None
    # llmlint: ignore[async_typed_clients_at_boundaries, tests_mirror_real_usage] see the allowlist.
    connection = sqlite3.connect(f"file:{store}?mode=ro", uri=True)
    try:
        prints = connection.execute("SELECT id FROM prints").fetchall()
        images = connection.execute("SELECT id FROM images").fetchall()
    except sqlite3.DatabaseError:
        return None
    finally:
        connection.close()
    if not prints or not images:
        return None
    return Recorded(PrintId(str(prints[0][0])), ImageId(str(images[0][0])))


def _client(program: Path, server: str, credential: str, *arguments: str) -> Answer:
    """One client command against `server`, as the operator it authenticates as."""
    result = shell_run(
        [str(program), *arguments, "--json"],
        cwd=program.parent,
        env=clean_environment(PRINTOBSERVER_SERVER=server, PRINTOBSERVER_CREDENTIAL=credential),
        timeout=120,
    )
    return Answer(result.returncode, result.stdout or "", result.stderr or "")


def _await_recorded(state: Path) -> Recorded:
    """The alert's print and image, once a server has put both in the store at `state`."""
    found: list[Recorded] = []

    def recorded() -> bool:
        seen = _recorded(state)
        if seen is not None:
            found.append(seen)
        return seen is not None

    _eventually(f"the alert's print and image in the store at {state}", recorded)
    return found[-1]


def _eventually(what: str, ready: Callable[[], bool]) -> None:
    deadline = time.monotonic() + WITHIN_SECONDS
    while time.monotonic() < deadline:
        if ready():
            return
        time.sleep(0.5)
    message = f"{what} within {WITHIN_SECONDS}s"
    raise AssertionError(message)


@pytest.fixture
def machine() -> Iterator[Machine]:
    """An OctoPrint answering on a real socket, holding one printing job."""
    held = Machine()
    try:
        yield held
    finally:
        held.stop()


CREDENTIAL = Credential("a-journeys-own-operator-credential-" + secrets.token_hex(8))


@pytest.fixture
def serviced(tmp_path: Path, program: Path, machine: Machine) -> Serviced:
    """A service install's root, with a real record a real server wrote into its state.

    The root is laid out by the committed service installer under its stand-ins,
    its configuration filled in and its skill installed; the real server then
    runs under it, takes one alert, opens a print and stores its image, and
    is stopped.
    """
    root = tmp_path / "serviced"
    home = _home(tmp_path, "home-installer")
    placed = _run_script(
        _session(tmp_path, stand_in_tasks=False),
        SERVICE_INSTALLER,
        tmp_path,
        _environment(home, tmp_path / "recorded-placing"),
        "-Root",
        str(root),
        "-Binary",
        str(program),
    )
    passing((placed.code, placed.said), describing="the service installer laying the root out")
    config = root.joinpath(*SERVICE_CONFIG)
    _fill_in(config, machine, CREDENTIAL, "127.0.0.1:0")
    # llmlint: ignore[e2e_not_mocked, tests_mirror_real_usage] suppressions.toml has the reasons.
    install_the_skill(config)
    no_harness = tmp_path / "no-harness"
    no_harness.mkdir()
    server = start(
        [str(program), "server", "--config", str(config)],
        cwd=tmp_path,
        env=clean_environment(PATH=str(no_harness)),
    )
    try:
        address = _serving(server)
        _alert(address, machine)
        recorded = _await_recorded(root.joinpath(*SERVICE_STATE))
    finally:
        server.terminate()
        server.wait(timeout=60)
    return Serviced(root, recorded.print_id, recorded.image_id)


@dataclass
class Migrating:
    """The migration under the stand-in session, against one service root."""

    tmp_path: Path
    session: Path
    counter: int = 0

    def __call__(self, serviced: Serviced, home: Home, *arguments: str, **extra: str) -> Ran:
        """Run the migration over `serviced`'s root, recording into a file of its own."""
        self.counter += 1
        environment = _environment(home, self.tmp_path / f"migrated-{self.counter}", **extra)
        return _run_script(
            self.session,
            MIGRATION,
            self.tmp_path,
            environment,
            "-Root",
            str(serviced.root),
            *arguments,
        )


@pytest.fixture
def migrating(tmp_path: Path) -> Migrating:
    """The committed migration under the stand-in session."""
    return Migrating(tmp_path, _session(tmp_path, stand_in_tasks=False))


def test_the_migration_moves_the_record_verifies_it_and_only_then_deletes_the_service(
    migrating: Migrating, serviced: Serviced, program: Path, tmp_path: Path, machine: Machine
) -> None:
    """Every file identical, two lines rewritten, the service deleted last, the record served."""
    home = _home(tmp_path, "home-migrated")
    # An account this host resolves: on Windows the migration asks it of the
    # host's own account database, before anything is stopped.
    owner = _account()
    state_before = _tree(serviced.state)
    config_before = serviced.config.read_text(encoding="utf-8")

    ran = migrating(
        serviced, home, "-LocalAppData", str(home.local), "-User", owner, **{REGISTERED: "1"}
    )

    passing((ran.code, ran.said), describing="the migration")
    equal(_tree(home.state), state_before, describing="every copied file, by SHA-256")
    truth(
        any(name.endswith(".sqlite3") for name in state_before)
        and any(name.startswith("images/") for name in state_before),
        describing=f"a database and an image among what was copied: {sorted(state_before)}",
    )
    migrated = home.config.read_text(encoding="utf-8").splitlines()
    original = config_before.splitlines()
    changed = [(was, now) for was, now in zip(original, migrated, strict=True) if was != now]
    equal(
        changed,
        [
            (
                original[[line.startswith("state_dir") for line in original].index(True)],
                f"state_dir = '{home.state}'",
            ),
            (
                original[[line.startswith("skill_path") for line in original].index(True)],
                f"skill_path = '{home.state / 'skills' / 'printobserver' / 'SKILL.md'}'",
            ),
        ],
        describing="the only lines the migration changed",
    )
    owned = [line for line in ran.recorded if line.startswith("icacls")]
    equal(
        owned,
        [f"icacls {home.local / 'printobserver'} /setowner {owner} /T /C"],
        describing="the copies made the named user's",
    )
    stop = ran.recorded.index("sc.exe stop printobserver")
    delete = ran.recorded.index("sc.exe delete printobserver")
    truth(
        stop < ran.recorded.index(owned[0]) < delete,
        describing=f"stopped before the copy, deleted after it verified: {ran.recorded}",
    )
    equal(
        serviced.config.read_text(encoding="utf-8"),
        config_before,
        describing="the service's config",
    )
    equal(_tree(serviced.state), state_before, describing="the service's state, left as it was")
    contains(ran.said, "scripts/install.ps1 | iex", describing="the next step it printed")
    contains(ran.said, "printobserver sign-in", describing="the next step it printed")

    # The per-user server started on the migrated configuration serves the
    # record the service wrote.
    _fill_listen(home.config)
    no_harness = tmp_path / "no-harness-after"
    no_harness.mkdir()
    server = start(
        [str(program), "server", "--config", str(home.config)],
        cwd=tmp_path,
        env=clean_environment(PATH=str(no_harness)),
    )
    try:
        address = _serving(server)
        listed = _client(program, address, CREDENTIAL, "prints")
        passing((listed.code, listed.said), describing="`prints` from the per-user server")
        contains(listed.out, serviced.print_id, describing="the print the service recorded")
        image = _client(program, address, CREDENTIAL, "image", "--image-id", serviced.image_id)
        passing((image.code, image.said), describing="`image` from the per-user server")
        answered = json.loads(image.out)
        path = Path(str(answered.get("path", "")))
        truth(
            home.state in path.parents, describing=f"the image under the per-user state: {answered}"
        )
        equal(path.read_bytes(), IMAGE, describing="the image's bytes, read as the user")
    finally:
        server.terminate()
        server.wait(timeout=60)


def _fill_listen(config: Path) -> None:
    """Let the migrated configuration's server take a port of its own."""
    text = config.read_text(encoding="utf-8")
    if 'listen = "127.0.0.1:0"' not in text:
        config.write_text(
            text.replace('listen = "127.0.0.1:8420"', 'listen = "127.0.0.1:0"'), encoding="utf-8"
        )


def _untouched(serviced: Serviced, home: Home, before: dict[str, str], ran: Ran) -> None:
    equal(_tree(serviced.state), before, describing="the service's state")
    truth(not home.config.exists(), describing="no per-user configuration left behind")
    truth(
        not any(line.startswith(("sc.exe stop", "sc.exe delete")) for line in ran.recorded),
        describing=f"nothing stopped or deleted: {ran.recorded}",
    )


def test_the_migration_refuses_a_user_who_already_has_a_per_user_install(
    migrating: Migrating, serviced: Serviced, tmp_path: Path
) -> None:
    """A per-user configuration already there is never overwritten, and nothing is stopped."""
    home = _home(tmp_path, "home-taken")
    home.config.parent.mkdir(parents=True)
    home.config.write_text("# the user's own\n", encoding="utf-8")
    before = _tree(serviced.state)

    ran = migrating(serviced, home, "-LocalAppData", str(home.local), **{REGISTERED: "1"})

    failing((ran.code, ran.said), naming="is already there")
    equal(home.config.read_text(encoding="utf-8"), "# the user's own\n", describing="their config")
    equal(_tree(serviced.state), before, describing="the service's state")
    truth(
        not any(line.startswith(("sc.exe stop", "sc.exe delete")) for line in ran.recorded),
        describing="nothing stopped",
    )


@pytest.mark.parametrize(
    ("extra", "naming"),
    [
        ({}, "no printobserver service is registered"),
        ({REGISTERED: "1", NOT_ELEVATED: "1"}, "not an elevated PowerShell"),
    ],
)
def test_the_migration_refuses_with_no_service_or_no_elevation(
    migrating: Migrating, serviced: Serviced, tmp_path: Path, extra: dict[str, str], naming: str
) -> None:
    """No service to move, or a filtered token, is a refusal that changes nothing."""
    home = _home(tmp_path, "home-refused")
    before = _tree(serviced.state)
    ran = migrating(serviced, home, "-LocalAppData", str(home.local), **extra)
    failing((ran.code, ran.said), naming=naming)
    _untouched(serviced, home, before, ran)


def test_the_migration_refuses_local_application_data_that_is_not_there(
    migrating: Migrating, serviced: Serviced, tmp_path: Path
) -> None:
    """`-LocalAppData` naming no directory is refused before anything is stopped."""
    home = _home(tmp_path, "home-nowhere")
    before = _tree(serviced.state)
    ran = migrating(serviced, home, "-LocalAppData", str(tmp_path / "nowhere"), **{REGISTERED: "1"})
    failing((ran.code, ran.said), naming="is not a directory")
    _untouched(serviced, home, before, ran)


def test_a_copy_that_does_not_verify_deletes_nothing_and_leaves_the_service_as_it_was(
    migrating: Migrating, serviced: Serviced, tmp_path: Path
) -> None:
    """A copied database made to differ after the copy is caught before the service is deleted."""
    home = _home(tmp_path, "home-differs")
    before = _tree(serviced.state)
    config_before = serviced.config.read_bytes()
    altered = home.state / STORE

    ran = migrating(
        serviced,
        home,
        "-LocalAppData",
        str(home.local),
        **{REGISTERED: "1", ALTERS: str(altered)},
    )

    failing((ran.code, ran.said), naming="the copy did not verify")
    contains(ran.said, f"{STORE} differs", describing="the file it named")
    truth("sc.exe delete printobserver" not in ran.recorded, describing="no deletion requested")
    equal(serviced.config.read_bytes(), config_before, describing="the service's configuration")
    equal(_tree(serviced.state), before, describing="the service's state")
    truth(not home.config.exists(), describing="the unverified copy removed")


def test_the_piped_form_installs_the_per_user_server_and_leaves_the_session_a_zero_status(
    releases: Callable[[str], Path], tmp_path: Path
) -> None:
    """`irm ... | iex` — the command the README leads with — installs the per-user server."""
    home = _home(tmp_path, "home-piped")
    recording = tmp_path / "recorded-piped"
    environment = _environment(
        home, recording, PRINTOBSERVER_RELEASE_BASE=str(releases(_version()))
    )

    ran = _run_script(
        _session(tmp_path, stand_in_tasks=True, piped=True), INSTALLER, home.root, environment
    )

    passing((ran.code, ran.said), describing="the session the script was piped into")
    contains(ran.said, "the session goes on, LASTEXITCODE=0", describing="the session after")
    truth(home.config.is_file(), describing="the per-user configuration")
    contains(ran.recorded, "Start-ScheduledTask printobserver", describing="the task started")
    _holds_the_contract(_task(ran.definition), _account(), home.program, home.config)


def _powershell_answer(command: str, *, check: bool = True) -> str:
    """What one PowerShell command printed, from this host's own session."""
    result = shell_run([powershell(), "-NoProfile", "-Command", command], timeout=300)
    if check:
        passing(
            (result.returncode, (result.stdout or "") + (result.stderr or "")),
            describing=f"`{command}`",
        )
    return (result.stdout or "").strip()


def _powershell_status(command: str) -> int:
    """The status one PowerShell command exits with, printing what it said when it fails."""
    result = shell_run([powershell(), "-NoProfile", "-Command", command], timeout=300)
    if result.returncode != 0:
        sys.stderr.write((result.stdout or "") + (result.stderr or ""))
    return result.returncode


def _quoted(text: str) -> str:
    """`text` as a single-quoted PowerShell literal."""
    return "'" + text.replace("'", "''") + "'"


def _sid(account: str) -> str:
    """The security identifier an account name or a SID names, as Windows resolves it."""
    if account.upper().startswith("S-1-"):
        return account.upper()
    return _powershell_answer(
        f"(New-Object System.Security.Principal.NTAccount({_quoted(account)}))"
        ".Translate([System.Security.Principal.SecurityIdentifier]).Value"
    ).upper()


def _exported(task: str) -> str:
    """The definition Task Scheduler holds for one task, as it exports it."""
    return _task(
        _powershell_answer(f"Export-ScheduledTask -TaskName {_quoted(task)} -TaskPath '\\'")
    )


def _servers(program: Path) -> list[int]:
    """Every process running the program at `program`, by its id."""
    listed = _powershell_answer(
        "Get-CimInstance Win32_Process -Filter \"Name='printobserver.exe'\" | "
        f"Where-Object {{ $_.ExecutablePath -eq {_quoted(str(program))} }} | "
        "ForEach-Object { $_.ProcessId }"
    )
    return [int(line) for line in listed.split() if line.isdigit()]


#: Asks one process's console whether its window is shown: attached to that
#: console, the console's window is the one `GetConsoleWindow` answers. Written
#: to a file and run by path, so nothing in it is a command line's to quote.
CONSOLE_PROBE = """$ErrorActionPreference = 'Stop'
$signature = @'
[DllImport("kernel32.dll", SetLastError = true)] public static extern bool FreeConsole();
[DllImport("kernel32.dll", SetLastError = true)] public static extern bool AttachConsole(uint id);
[DllImport("kernel32.dll")] public static extern System.IntPtr GetConsoleWindow();
[DllImport("user32.dll")] public static extern bool IsWindowVisible(System.IntPtr window);
'@
Add-Type -Namespace PrintObserverJourney -Name Probe -MemberDefinition $signature
[void][PrintObserverJourney.Probe]::FreeConsole()
if (-not [PrintObserverJourney.Probe]::AttachConsole({pid})) {{ 'no console'; exit 0 }}
$window = [PrintObserverJourney.Probe]::GetConsoleWindow()
if ($window -eq [System.IntPtr]::Zero) {{ 'no window' }}
elseif ([PrintObserverJourney.Probe]::IsWindowVisible($window)) {{ 'shown' }}
else {{ 'hidden' }}
"""


def _console_window_shown(pid: int, into: Path) -> str:
    """Whether the console of process `pid` shows a window: shown, hidden, or none."""
    probe = into / f"console-probe-{pid}.ps1"
    probe.write_text(CONSOLE_PROBE.format(pid=pid), encoding="utf-8")
    return _powershell_answer(f"& {_quoted(str(probe))}").splitlines()[-1].strip()


def _free_port() -> int:
    """A port nothing listens on, for a configuration that names one."""
    with socket.socket(socket.AF_INET, socket.SOCK_STREAM) as probe:
        probe.bind(("127.0.0.1", 0))
        return int(probe.getsockname()[1])


def _serves(program: Path, port: int) -> bool:
    return _client(program, f"http://127.0.0.1:{port}", CREDENTIAL, "prints").code == 0


def _kept_files(state: Path) -> list[Path]:
    """Every file the server keeps under `state`: its store, images and turn records."""
    return sorted(path for path in state.rglob("*") if path.is_file())


def _restart_command(task: str) -> str:
    """The documented restart, with the journey's own task name in place of `printobserver`."""
    from repo_checks import install_path as ip

    pair = ip.parse((REPO_ROOT / "AGENTS.md").read_text(encoding="utf-8")).commands_for(
        ServiceManager.WINDOWS_SCHEDULED_TASK.value
    )
    equal(len(pair), 2, describing="the windows-scheduled-task pair the install path states")
    return pair[1].replace("-TaskName printobserver", f"-TaskName {task}")


def _remove_task(task: str, program: Path) -> None:
    _powershell_answer(
        f"Unregister-ScheduledTask -TaskName {_quoted(task)} -TaskPath '\\' -Confirm:$false "
        "-ErrorAction SilentlyContinue",
        check=False,
    )
    for pid in _servers(program):
        shell_run(["taskkill.exe", "/F", "/PID", str(pid)], timeout=60)


@WINDOWS_ONLY
def test_on_windows_the_task_serves_comes_back_takes_a_change_and_upgrades_in_place(
    program: Path, tmp_path: Path, machine: Machine
) -> None:
    """The real Task Scheduler runs the server the installer registered for this account.

    A hosted runner cannot perform an interactive logon, so the server is
    started through the same task definition its logon trigger runs, with
    `Start-ScheduledTask`, and the trigger itself is asserted to be at logon of
    the installing user.
    """
    home = _home(tmp_path, "home-real")
    task = f"printobserver-journey-{os.getpid()}-{int(time.time())}"
    older = _older(_version())
    # llmlint: ignore[tests_mirror_real_usage] suppressions.toml has the reason.
    release = _stage(
        tmp_path / "release-older",
        program.read_bytes().replace(_version().encode(), older.encode()),
    )
    newer = _stage(tmp_path / "release-newer", program.read_bytes())
    session = _session(tmp_path, stand_in_tasks=False)

    def install(base: Path, recording: str) -> Ran:
        environment = _environment(
            home,
            tmp_path / recording,
            PRINTOBSERVER_RELEASE_BASE=str(base),
            **{TASK_NAME: task},
        )
        # On the session's path already, so the run leaves the user's own
        # PATH alone: that is the install-script journey's to prove.
        environment["Path"] = os.pathsep.join(
            [str(home.program.parent), environment.get("PATH", "")]
        )
        return _run_script(session, INSTALLER, home.root, environment)

    try:
        first = install(release, "recorded-first")
        passing((first.code, first.said), describing="install.ps1 against the real Task Scheduler")
        account = _powershell_answer(
            "[System.Security.Principal.WindowsIdentity]::GetCurrent().Name"
        )
        task_xml = _exported(task)
        # A hosted runner cannot perform an interactive logon: the logon trigger is
        # asserted from the registered definition, and the server is started through
        # that same definition below; a real logon is checked on a real Windows host.
        equal(
            _sid(_text(task_xml, "Triggers/LogonTrigger/UserId")),
            _sid(account),
            describing="the logon trigger, for the installing user",
        )
        equal(
            _sid(_text(task_xml, "Principals/Principal/UserId")),
            _sid(account),
            describing="the principal, the installing user",
        )
        equal(
            _powershell_answer(f"(Get-ScheduledTask -TaskName {_quoted(task)}).Principal.RunLevel"),
            "Limited",
            describing="the task's run level",
        )

        # The steps the installer printed, taken: the configuration filled in,
        # the skill laid down, and the documented restart.
        port = _free_port()
        _fill_in(home.config, machine, CREDENTIAL, f"127.0.0.1:{port}")
        # llmlint: ignore[e2e_not_mocked, tests_mirror_real_usage] see suppressions.toml.
        install_the_skill(home.config)
        _powershell_answer(_restart_command(task))
        _eventually("the task's server answering `prints`", lambda: _serves(home.program, port))

        served = _servers(home.program)
        equal(len(served), 1, describing=f"one server process: {served}")
        equal(
            _console_window_shown(served[0], tmp_path),
            "hidden",
            describing="the server's console window, asked of its own console",
        )

        # Through the task: ended, then started by Task Scheduler.
        _powershell_answer(f"Stop-ScheduledTask -TaskName {_quoted(task)}")
        _eventually("the server ended by its task", lambda: not _servers(home.program))
        _powershell_answer(f"Start-ScheduledTask -TaskName {_quoted(task)}")
        _eventually("the server started by Task Scheduler", lambda: _serves(home.program, port))

        # A crash: the process killed, and the task bringing it back.
        killed = _servers(home.program)
        passing(
            (shell_run(["taskkill.exe", "/F", "/PID", str(killed[0])], timeout=60).returncode, ""),
            describing="killing the server",
        )
        _eventually(
            "the server back after it was killed",
            lambda: (
                bool((now := _servers(home.program)) and now[0] != killed[0])
                and _serves(home.program, port)
            ),
        )

        # An image and the turn records, read as the user.
        _alert(f"http://127.0.0.1:{port}", machine)
        recorded = _await_recorded(home.state)
        image = _client(
            home.program,
            f"http://127.0.0.1:{port}",
            CREDENTIAL,
            "image",
            "--image-id",
            recorded.image_id,
        )
        passing((image.code, image.said), describing="`image` from the task's server")
        path = Path(str(json.loads(image.out).get("path", "")))
        equal(path.read_bytes(), IMAGE, describing="the image, opened as the user")
        # Every file the server keeps under the state directory — the store,
        # the images and whatever turn records the alert's turn left — opened
        # and read as the user, with no elevation.
        read = {path: len(path.read_bytes()) for path in _kept_files(home.state)}
        truth(
            any(kept.name == path.name for kept in read)
            and any(kept.name == STORE for kept in read),
            describing=f"the image and the store among what was read: {sorted(read)}",
        )

        # A configuration change, as the user, taken by the documented restart.
        moved = _free_port()
        home.config.write_text(
            home.config.read_text(encoding="utf-8")
            .replace(f'listen = "127.0.0.1:{port}"', f'listen = "127.0.0.1:{moved}"')
            .replace("# observation_interval_s = 120", "observation_interval_s = 60"),
            encoding="utf-8",
        )
        _powershell_answer(_restart_command(task))
        _eventually(
            "the server serving where the change moved it", lambda: _serves(home.program, moved)
        )

        # The upgrade: the same command against a newer release.
        config_before = home.config.read_bytes()
        upgraded = install(newer, "recorded-upgrade")
        passing((upgraded.code, upgraded.said), describing="the same command, as an upgrade")
        contains(
            run([str(home.program), "--version"], home.root, timeout=120).stdout,
            _version(),
            describing="the program the task runs after the upgrade",
        )
        equal(home.config.read_bytes(), config_before, describing="the configuration, unchanged")
        _eventually("the upgraded server serving", lambda: _serves(home.program, moved))
        listed = _client(home.program, f"http://127.0.0.1:{moved}", CREDENTIAL, "prints")
        passing((listed.code, listed.said), describing="`prints` after the upgrade")
        contains(listed.out, recorded.print_id, describing="the print recorded before the upgrade")
        again = _client(
            home.program,
            f"http://127.0.0.1:{moved}",
            CREDENTIAL,
            "image",
            "--image-id",
            recorded.image_id,
        )
        passing((again.code, again.said), describing="`image` for an image from before the upgrade")
        equal(
            _powershell_answer(f"@(Get-ScheduledTask -TaskName {_quoted(task)}).Count"),
            "1",
            describing="tasks of that name",
        )
    finally:
        _remove_task(task, home.program)


@WINDOWS_ONLY
def test_on_windows_an_ordinary_account_installs_with_no_administrator_rights(
    tmp_path: Path, program: Path
) -> None:
    """An account outside Administrators installs, registers its own task, and asks it to start.

    The account is created for this run, so it has no interactive session, and
    Task Scheduler runs an interactive-token task only while its user has one:
    a start it refuses for that reason is recorded, and the start itself is
    the runner's own account's case above.
    """
    suffix = f"{os.getpid() % 100000}"
    name = f"pojourney{suffix}"
    # Fourteen characters: `net user` asks before taking a longer one, and
    # nothing here can answer it.
    password = "Aa1!" + secrets.token_hex(5)
    root = (
        Path(os.environ.get("SYSTEMDRIVE", "C:") + "\\")
        / "printobserver-journeys"
        / f"ordinary-{suffix}"
    )
    home = Home(root / "profile")
    task = f"printobserver-journey-ordinary-{suffix}"
    stray = [
        Path(os.environ.get("PROGRAMDATA", "C:\\ProgramData")) / "printobserver",
        Path(os.environ.get("PROGRAMFILES", "C:\\Program Files")) / "printobserver",
    ]
    before = {str(path): path.exists() for path in stray}
    try:
        passing(
            (
                _powershell_status(
                    f"& net.exe user {_quoted(name)} {_quoted(password)} /add; exit $LASTEXITCODE"
                ),
                f"net.exe user {name} /add",
            ),
            describing=f"creating the ordinary account {name}",
        )
        home.local.mkdir(parents=True)
        (home.root / "temp").mkdir()
        release = _stage(root / "release", program.read_bytes())
        session = _session(root, stand_in_tasks=False)
        _powershell_answer(
            f"& icacls.exe {_quoted(str(root))} /grant {_quoted(name + ':(OI)(CI)F')}; "
            "exit $LASTEXITCODE"
        )
        outcome = root / "outcome"
        wrapper = root / "as-the-account.ps1"
        wrapper.write_text(
            "\n".join(
                [
                    f"$env:LOCALAPPDATA = {_quoted(str(home.local))}",
                    f"$env:TEMP = {_quoted(str(home.root / 'temp'))}",
                    f"$env:TMP = {_quoted(str(home.root / 'temp'))}",
                    f"$env:PRINTOBSERVER_RELEASE_BASE = {_quoted(str(release))}",
                    f"$env:{TASK_NAME} = {_quoted(task)}",
                    f"$env:{RECORDING} = {_quoted(str(root / 'recorded'))}",
                    f"$env:Path = {_quoted(str(home.program.parent))} + ';' + $env:Path",
                    # Windows's own `whoami`, by its path: a runner puts Git's
                    # Unix one ahead of it on PATH.
                    "$groups = (& (Join-Path $env:SystemRoot 'System32\\whoami.exe') /all 2>&1 "
                    "| Out-String) + "
                    "[Security.Principal.WindowsIdentity]::GetCurrent().Name",
                    f"Set-Content -LiteralPath {_quoted(str(outcome) + '-groups.txt')} "
                    "-Value $groups -Encoding utf8",
                    f"$said = & {_quoted(powershell())} -NoProfile -File {_quoted(str(session))} "
                    f"{_quoted(str(INSTALLER))} 2>&1 | Out-String",
                    "$code = $LASTEXITCODE",
                    f"Set-Content -LiteralPath {_quoted(str(outcome) + '-said.txt')} "
                    "-Value $said -Encoding utf8",
                    f"Set-Content -LiteralPath {_quoted(str(outcome) + '-code.txt')} "
                    "-Value $code -Encoding utf8",
                ]
            ),
            encoding="utf-8",
        )
        _powershell_answer(
            f"$credential = New-Object System.Management.Automation.PSCredential("
            f"{_quoted('.' + chr(92) + name)}, "
            f"(ConvertTo-SecureString {_quoted(password)} -AsPlainText -Force)); "
            f"Start-Process -FilePath {_quoted(powershell())} -ArgumentList "
            f"@('-NoProfile', '-File', {_quoted(str(wrapper))}) -Credential $credential "
            f"-WorkingDirectory {_quoted(str(root))} -LoadUserProfile -Wait"
        )
        groups = Path(f"{outcome}-groups.txt").read_text(encoding="utf-8-sig", errors="replace")
        contains(groups, name, describing="the identity the install ran as")
        absent(groups, "S-1-16-12288", describing="a high-integrity token")
        contains(groups, "S-1-16-8192", describing="the account's medium-integrity token")
        said = Path(f"{outcome}-said.txt").read_text(encoding="utf-8-sig", errors="replace")
        code = int(
            Path(f"{outcome}-code.txt").read_text(encoding="utf-8-sig", errors="replace").strip()
            or "1"
        )

        truth(home.program.is_file(), describing=f"the program, installed as {name}: {said}")
        truth(home.config.is_file(), describing="the per-user configuration")
        truth(home.state.is_dir(), describing="the per-user state")
        exported = _exported(task)
        equal(
            _sid(_text(exported, "Principals/Principal/UserId")),
            _sid(f"{os.environ.get('COMPUTERNAME', '')}\\{name}"),
            describing="the principal, the ordinary account",
        )
        equal(
            _powershell_answer(f"(Get-ScheduledTask -TaskName {_quoted(task)}).Principal.RunLevel"),
            "Limited",
            describing="the task's run level",
        )
        if code != 0:
            # The one refusal the ruling admits: Task Scheduler declining to
            # start an interactive-token task for a user with no session, which
            # it answers with ERROR_REQUEST_REFUSED (0x800710E0, "The operator
            # or administrator has refused the request"). Any other failure,
            # before or after the registration, fails here.
            contains(
                said,
                f"registered the task {task} for",
                describing=f"the start refused after registering: {said}",
            )
            truth(
                any(
                    refusal in said
                    for refusal in (
                        SESSIONLESS_START_REFUSED_CODE,
                        SESSIONLESS_START_REFUSED_TEXT,
                    )
                ),
                describing=f"Task Scheduler's refusal for a user with no session: {said}",
            )
        for path in stray:
            equal(path.exists(), before[str(path)], describing=f"{path}, untouched")
        equal(
            (
                Path(os.environ.get("SYSTEMDRIVE", "C:") + "\\")
                / "Users"
                / name
                / "AppData"
                / "Local"
                / "printobserver"
            ).exists(),
            False,
            describing="nothing in the account's own profile outside the root it was given",
        )
    finally:
        _remove_task(task, home.program)
        _powershell_status(f"& net.exe user {_quoted(name)} /delete; exit $LASTEXITCODE")
        _powershell_answer(
            "Get-CimInstance Win32_UserProfile | Where-Object { $_.LocalPath -like "
            f"{_quoted('*' + chr(92) + name)} }} | Remove-CimInstance",
            check=False,
        )
        shutil.rmtree(root, ignore_errors=True)


@pytest.mark.parametrize(
    ("extra", "naming", "asked"),
    [
        (
            {REFUSES: "query"},
            "asking the service control manager whether the printobserver service",
            ["sc.exe query printobserver"],
        ),
        (
            {REFUSES: "Register-ScheduledTask"},
            "Task Scheduler would not register the task printobserver",
            None,
        ),
        (
            {REFUSES: "Start-ScheduledTask"},
            "Task Scheduler registered the task printobserver for",
            None,
        ),
    ],
    ids=["an unanswered service query", "a refused registration", "a refused start"],
)
def test_a_refusal_of_the_host_stops_the_install_naming_it(
    installing: Installing,
    releases: Callable[[str], Path],
    tmp_path: Path,
    extra: dict[str, str],
    naming: str,
    asked: list[str] | None,
) -> None:
    """What the manager or Task Scheduler refused is said, with what to do next."""
    home = _home(tmp_path, "home-refused-host")

    ran = installing(home, releases(_version()), extra=extra)

    failing((ran.code, ran.said), naming=naming)
    if asked is not None:
        equal(ran.recorded, asked, describing="what it asked before it stopped")
        equal(sorted(home.local.iterdir()), [], describing="nothing written for a refused run")


def test_a_profile_path_carrying_a_quote_is_refused_before_anything_is_written(
    installing: Installing, releases: Callable[[str], Path], tmp_path: Path
) -> None:
    """A path the configuration and the task's command line cannot carry is refused, not written."""
    release = releases(_version())
    home = Home(tmp_path / "o'brien")
    home.local.mkdir(parents=True)
    (home.root / "temp").mkdir()

    ran = installing(home, release)

    failing((ran.code, ran.said), naming="carries a quote or a newline")
    equal(sorted(home.local.iterdir()), [], describing="nothing written")


@pytest.mark.skipif(ON_WINDOWS, reason="a Windows session always names the user it runs as")
def test_a_session_naming_no_user_is_refused_before_anything_is_written(
    installing: Installing, releases: Callable[[str], Path], tmp_path: Path
) -> None:
    """With nobody to register the task for, nothing is installed."""
    home = _home(tmp_path, "home-nobody")

    ran = installing(home, releases(_version()), extra={"USERNAME": "", "USERDOMAIN": ""})

    failing((ran.code, ran.said), naming="names no user")
    equal(sorted(home.local.iterdir()), [], describing="nothing written")


def test_a_state_directory_that_cannot_be_made_is_refused_naming_it(
    installing: Installing, releases: Callable[[str], Path], tmp_path: Path
) -> None:
    """Something in the state directory's way stops the install before a task is registered."""
    home = _home(tmp_path, "home-blocked")
    home.state.parent.mkdir(parents=True)
    home.state.write_text("a file where the state directory goes", encoding="utf-8")

    ran = installing(home, releases(_version()))

    failing((ran.code, ran.said), naming="could not be created")
    truth(
        not any(line.startswith("Register-ScheduledTask") for line in ran.recorded),
        describing=f"no task registered over a refused install: {ran.recorded}",
    )


def test_a_reinstall_given_an_interval_keeps_the_configuration_and_says_so(
    installing: Installing, releases: Callable[[str], Path], tmp_path: Path
) -> None:
    """A configuration already there is the user's; the interval not written is named."""
    home = _home(tmp_path, "home-reinterval")
    first = installing(home, releases(_version()))
    passing((first.code, first.said), describing="the first install")
    before = home.config.read_bytes()

    again = installing(
        home, releases(_version()), "-ObservationIntervalSeconds", "60", recording=tmp_path / "r"
    )

    passing((again.code, again.said), describing="the reinstall given an interval")
    equal(home.config.read_bytes(), before, describing="the configuration")
    contains(again.said, "-ObservationIntervalSeconds was not written", describing="what it said")


def test_a_relative_install_directory_is_registered_as_a_whole_path(
    installing: Installing, releases: Callable[[str], Path], tmp_path: Path
) -> None:
    """Task Scheduler runs the program from its own directory, so the task names a whole path."""
    home = _home(tmp_path, "home-relative")

    ran = installing(home, releases(_version()), "-To", "relative-bin")

    passing((ran.code, ran.said), describing="install.ps1 given a relative directory")
    command = _text(_task(ran.definition), "Actions/Exec/Command").strip('"')
    equal(
        command,
        str(home.root / "relative-bin" / _windows_platform().program),
        describing="the program the task runs",
    )


def _service_file(serviced: Serviced, old: str, new: str) -> None:
    serviced.config.write_text(
        serviced.config.read_text(encoding="utf-8").replace(old, new), encoding="utf-8"
    )


@pytest.mark.parametrize(
    ("arrange", "extra", "naming"),
    [
        ("none", {REFUSES: "query"}, "asking the service control manager about printobserver"),
        ("state-without-config", {}, "already has per-user state"),
        ("no-config", {}, "has no configuration to move"),
        ("no-state", {}, "is not there, so the service has no record to move"),
        ("no-skill-path", {}, "no skill_path in [supervisor]"),
        ("computed-state", {}, "state_dir is not one plain string"),
        ("computed-skill", {}, "supervisor.skill_path is not one plain string"),
        ("quoted-local", {}, "carries a quote or a newline"),
        ("none", {REFUSES: "stop"}, "stopping the printobserver service failed"),
        ("bad-user", {}, "is not an account name"),
    ],
)
def test_the_migration_refuses_before_it_copies_anything(
    migrating: Migrating,
    serviced: Serviced,
    tmp_path: Path,
    arrange: str,
    extra: dict[str, str],
    naming: str,
) -> None:
    """A refusal before the copy leaves the service's files and the user's profile as they were."""
    home = _home(tmp_path, "home-before-copy")
    user = _account()
    match arrange:
        case "state-without-config":
            home.state.mkdir(parents=True)
        case "no-config":
            serviced.config.unlink()
        case "no-state":
            shutil.rmtree(serviced.state)
        case "no-skill-path":
            lines = serviced.config.read_text(encoding="utf-8").splitlines(keepends=True)
            serviced.config.write_text(
                "".join(line for line in lines if not line.startswith("skill_path")),
                encoding="utf-8",
            )
        case "computed-skill":
            skill_line = next(
                line
                for line in serviced.config.read_text(encoding="utf-8").splitlines()
                if line.startswith("skill_path")
            )
            _service_file(serviced, skill_line, 'skill_path = "a\\u0020path"')
        case "computed-state":
            state_line = next(
                line
                for line in serviced.config.read_text(encoding="utf-8").splitlines()
                if line.startswith("state_dir")
            )
            _service_file(serviced, state_line, "state_dir = { path = 'nowhere' }")
        case "bad-user":
            user = "a/name with a slash"
        case "quoted-local":
            home = _home(tmp_path, "o'brien")
        case _:
            pass
    before = _tree(serviced.state) if serviced.state.exists() else {}

    ran = migrating(
        serviced,
        home,
        "-LocalAppData",
        str(home.local),
        "-User",
        user,
        **{REGISTERED: "1", **extra},
    )

    failing((ran.code, ran.said), naming=naming)
    if serviced.state.exists():
        equal(_tree(serviced.state), before, describing="the service's state")
    truth(not home.config.exists(), describing="no per-user configuration")
    truth(
        not any(line.startswith(("sc.exe delete", "icacls")) for line in ran.recorded),
        describing=f"nothing copied, owned or deleted: {ran.recorded}",
    )


def test_a_stop_of_a_service_that_was_not_running_is_taken_as_stopped(
    migrating: Migrating, serviced: Serviced, tmp_path: Path
) -> None:
    """The manager answering 1062 to a stop is a service already down, and the move goes on."""
    home = _home(tmp_path, "home-not-running")

    ran = migrating(
        serviced, home, "-LocalAppData", str(home.local), **{REGISTERED: "1", REFUSES: "stop=1062"}
    )

    passing((ran.code, ran.said), describing="the migration of a stopped service")
    contains(ran.recorded, "sc.exe delete printobserver", describing="the service deleted")


@pytest.mark.parametrize(
    ("extra", "naming", "deleted", "kept"),
    [
        ({REFUSES: "icacls"}, "the owner of", False, False),
        ({REFUSES: "delete"}, "deleting the printobserver service failed", True, True),
    ],
    ids=["a refused owner", "a refused deletion"],
)
def test_a_refusal_after_the_copy_says_where_things_were_left(
    migrating: Migrating,
    serviced: Serviced,
    tmp_path: Path,
    extra: dict[str, str],
    naming: str,
    deleted: bool,
    kept: bool,
) -> None:
    """A refused owner removes the copies and deletes nothing; a refused deletion keeps them."""
    home = _home(tmp_path, "home-after-copy")
    before = _tree(serviced.state)

    ran = migrating(serviced, home, "-LocalAppData", str(home.local), **{REGISTERED: "1", **extra})

    failing((ran.code, ran.said), naming=naming)
    equal(
        "sc.exe delete printobserver" in ran.recorded,
        deleted,
        describing=f"whether the deletion was asked for: {ran.recorded}",
    )
    equal(home.config.exists(), kept, describing="whether the copies were kept")
    equal(_tree(serviced.state), before, describing="the service's state")


def test_a_copied_file_gone_before_the_check_deletes_nothing(
    migrating: Migrating, serviced: Serviced, tmp_path: Path
) -> None:
    """A copy missing a file is caught by name, and the service is left registered."""
    home = _home(tmp_path, "home-missing-copy")
    gone = home.state / STORE

    ran = migrating(
        serviced,
        home,
        "-LocalAppData",
        str(home.local),
        **{REGISTERED: "1", ALTERS: f"remove:{gone}"},
    )

    failing((ran.code, ran.said), naming=f"{STORE} is missing")
    truth("sc.exe delete printobserver" not in ran.recorded, describing="no deletion requested")
    truth(not home.config.exists(), describing="the unverified copy removed")


def test_an_upgrade_whose_stop_is_refused_puts_the_task_back_and_replaces_nothing(
    installing: Installing, releases: Callable[[str], Path], tmp_path: Path
) -> None:
    """A task Task Scheduler will not stop is enabled again, and the program left as it was."""
    home = _home(tmp_path, "home-unstoppable")
    first = installing(home, releases(_older(_version())))
    passing((first.code, first.said), describing="the first install")
    before = home.program.read_bytes()

    again = installing(
        home,
        releases(_version()),
        recording=tmp_path / "recorded-1",
        extra={REFUSES: "Stop-ScheduledTask"},
    )

    failing((again.code, again.said), naming="would not stop the task printobserver")
    equal(home.program.read_bytes(), before, describing="the program, left as it was")
    upgrade = again.recorded[len(first.recorded) :]
    contains(upgrade, "Enable-ScheduledTask printobserver", describing="the task put back")
    contains(again.said, "was put back and started", describing="what it said of the task")


UNWRITABLE = pytest.mark.skipif(
    ON_WINDOWS, reason="an administrator writes anywhere, so nothing here can refuse the write"
)


@UNWRITABLE
def test_an_upgrade_that_cannot_replace_the_program_puts_the_task_back(
    installing: Installing, releases: Callable[[str], Path], tmp_path: Path
) -> None:
    """A program that cannot be replaced leaves the old one running under its task again."""
    home = _home(tmp_path, "home-unreplaceable")
    first = installing(home, releases(_older(_version())))
    passing((first.code, first.said), describing="the first install")
    home.program.parent.chmod(0o555)
    try:
        again = installing(home, releases(_version()), recording=tmp_path / "recorded-1")
    finally:
        home.program.parent.chmod(0o755)

    failing((again.code, again.said), naming="could not be put in")
    upgrade = again.recorded[len(first.recorded) :]
    equal(
        [line for line in upgrade if "Enable" in line or "Start" in line],
        ["Enable-ScheduledTask printobserver", "Start-ScheduledTask printobserver"],
        describing="the task put back and started",
    )


@UNWRITABLE
def test_a_configuration_that_cannot_be_written_is_refused_before_a_task_is_registered(
    installing: Installing, releases: Callable[[str], Path], tmp_path: Path
) -> None:
    """A configuration directory refusing the write stops the install, saying what is in place."""
    home = _home(tmp_path, "home-unwritable")
    home.state.mkdir(parents=True)
    home.config.parent.chmod(0o555)
    try:
        ran = installing(home, releases(_version()))
    finally:
        home.config.parent.chmod(0o755)

    failing((ran.code, ran.said), naming="could not be written")
    contains(ran.said, "The program is installed at", describing="what it said is in place")
    truth(
        not any(line.startswith("Register-ScheduledTask") for line in ran.recorded),
        describing=f"no task registered: {ran.recorded}",
    )


@UNWRITABLE
def test_a_copy_that_cannot_be_made_leaves_the_service_registered(
    migrating: Migrating, serviced: Serviced, tmp_path: Path
) -> None:
    """A profile refusing the copy stops it: the service stays registered, its files unchanged."""
    home = _home(tmp_path, "home-refusing-copy")
    before = _tree(serviced.state)
    home.local.chmod(0o555)
    try:
        ran = migrating(serviced, home, "-LocalAppData", str(home.local), **{REGISTERED: "1"})
    finally:
        home.local.chmod(0o755)

    failing((ran.code, ran.said), naming="copying the service's configuration and record failed")
    truth("sc.exe delete printobserver" not in ran.recorded, describing="no deletion requested")
    equal(_tree(serviced.state), before, describing="the service's state")


def test_a_copied_configuration_made_to_differ_deletes_nothing(
    migrating: Migrating, serviced: Serviced, tmp_path: Path
) -> None:
    """The configuration is checked too: one that is not what was written is caught."""
    home = _home(tmp_path, "home-config-differs")

    ran = migrating(
        serviced,
        home,
        "-LocalAppData",
        str(home.local),
        **{REGISTERED: "1", ALTERS: str(home.config)},
    )

    failing((ran.code, ran.said), naming="differs from what was written")
    truth("sc.exe delete printobserver" not in ran.recorded, describing="no deletion requested")
    truth(not home.config.exists(), describing="the unverified copy removed")


def test_an_explicit_directory_with_no_local_application_data_is_refused_for_the_server(
    installing: Installing, releases: Callable[[str], Path], tmp_path: Path
) -> None:
    """`-To` names where the program goes; the server's own files still need the profile's."""
    home = _home(tmp_path, "home-no-local")
    release = releases(_version())
    recording = tmp_path / "recorded-no-local"
    environment = _environment(home, recording, PRINTOBSERVER_RELEASE_BASE=str(release))
    del environment["LOCALAPPDATA"]

    ran = _run_script(
        installing.session, INSTALLER, home.root, environment, "-To", str(home.root / "bin")
    )

    failing((ran.code, ran.said), naming="nowhere of your own to keep the server")
    truth(not (home.root / "bin").exists(), describing="nothing installed")


@pytest.mark.parametrize(
    ("refuses", "naming", "put_back"),
    [
        ("Disable-ScheduledTask", "would not stop the task printobserver", False),
        ("Stop-ScheduledTask,Enable-ScheduledTask", "could not be put back", True),
        ("Stop-ScheduledTask,Start-ScheduledTask", "could not be put back", True),
    ],
    ids=[
        "a refused hold",
        "a refused hold and a refused enable",
        "a refused hold and a refused start",
    ],
)
def test_an_upgrade_task_scheduler_will_not_pause_replaces_nothing_and_says_where_it_left_it(
    installing: Installing,
    releases: Callable[[str], Path],
    tmp_path: Path,
    refuses: str,
    naming: str,
    put_back: bool,
) -> None:
    """A task never held off is not touched again; one that cannot be put back is named."""
    home = _home(tmp_path, "home-unpausable")
    first = installing(home, releases(_older(_version())))
    passing((first.code, first.said), describing="the first install")
    before = home.program.read_bytes()

    again = installing(
        home, releases(_version()), recording=tmp_path / "recorded-1", extra={REFUSES: refuses}
    )

    failing((again.code, again.said), naming=naming)
    equal(home.program.read_bytes(), before, describing="the program, left as it was")
    upgrade = again.recorded[len(first.recorded) :]
    equal(
        "Enable-ScheduledTask printobserver" in upgrade,
        put_back,
        describing=f"whether it tried to put the task back: {upgrade}",
    )


def test_the_piped_form_that_stopped_keeps_the_session_and_leaves_a_failing_status(
    releases: Callable[[str], Path], tmp_path: Path
) -> None:
    """A refusal under `irm ... | iex` keeps the caller's window, and reads as a failure."""
    home = _home(tmp_path, "home-piped-refused")
    environment = _environment(
        home,
        tmp_path / "recorded-piped-refused",
        PRINTOBSERVER_RELEASE_BASE=str(releases(_version())),
        **{REGISTERED: "1"},
    )

    ran = _run_script(
        _session(tmp_path, stand_in_tasks=True, piped=True), INSTALLER, home.root, environment
    )

    contains(
        ran.said,
        "the session goes on, LASTEXITCODE=1",
        describing="the session after the stop, which a stop must not end",
    )
    contains(ran.said, "migrate-service-to-per-user.ps1", describing="the migration it named")
    equal(sorted(home.local.iterdir()), [], describing="nothing written")


@WINDOWS_ONLY
def test_on_windows_the_migration_refuses_an_account_this_machine_does_not_have(
    migrating: Migrating, serviced: Serviced, tmp_path: Path
) -> None:
    """A name Windows cannot resolve is refused before the service is stopped."""
    home = _home(tmp_path, "home-nobody-here")

    ran = migrating(
        serviced,
        home,
        "-LocalAppData",
        str(home.local),
        "-User",
        f"{os.environ.get('COMPUTERNAME', 'HOST')}\\nobody-{secrets.token_hex(4)}",
        **{REGISTERED: "1"},
    )

    failing((ran.code, ran.said), naming="which is no account on this machine")
    truth(
        not any(line.startswith("sc.exe stop") for line in ran.recorded),
        describing=f"nothing stopped: {ran.recorded}",
    )


def test_an_upgrade_task_scheduler_cannot_answer_about_replaces_nothing(
    installing: Installing, releases: Callable[[str], Path], tmp_path: Path
) -> None:
    """Only a task that is not there reads as absent; any other answer stops the upgrade."""
    home = _home(tmp_path, "home-unanswered-task")
    first = installing(home, releases(_older(_version())))
    passing((first.code, first.said), describing="the first install")
    before = home.program.read_bytes()

    again = installing(
        home,
        releases(_version()),
        recording=tmp_path / "recorded-1",
        extra={REFUSES: "Get-ScheduledTask"},
    )

    failing((again.code, again.said), naming="would not say whether the task printobserver")
    equal(home.program.read_bytes(), before, describing="the program, left as it was")
    upgrade = again.recorded[len(first.recorded) :]
    truth(
        not any(line.startswith(("Disable-", "Register-")) for line in upgrade),
        describing=f"nothing asked of Task Scheduler after its refusal: {upgrade}",
    )


@pytest.mark.skipif(
    ON_WINDOWS, reason="a stand-in program cannot be kept running at the installed path there"
)
def test_an_upgrade_whose_server_will_not_exit_replaces_nothing_and_puts_the_task_back(
    installing: Installing, releases: Callable[[str], Path], tmp_path: Path
) -> None:
    """A server still running a minute after its task stopped is not replaced from under it."""
    home = _home(tmp_path, "home-lingering")
    first = installing(home, releases(_older(_version())))
    passing((first.code, first.said), describing="the first install")
    sleeper = shutil.which("sleep")
    truth(sleeper is not None, describing="this host to carry a `sleep` to keep running")
    # llmlint: ignore[tests_mirror_real_usage] suppressions.toml has the reason.
    shutil.copy(str(sleeper), home.program)
    home.program.chmod(0o755)
    lingering = start([str(home.program), "300"], cwd=home.root)
    try:
        again = installing(home, releases(_version()), recording=tmp_path / "recorded-1")
    finally:
        lingering.kill()
        lingering.wait(timeout=30)

    failing((again.code, again.said), naming="is still running a minute after its task was stopped")
    equal(
        home.program.read_bytes(),
        Path(str(sleeper)).read_bytes(),
        describing="the program, left as it was",
    )
    upgrade = again.recorded[len(first.recorded) :]
    contains(upgrade, "Enable-ScheduledTask printobserver", describing="the task put back")


def test_a_service_that_will_not_stop_is_left_registered_and_nothing_is_copied(
    migrating: Migrating, serviced: Serviced, tmp_path: Path
) -> None:
    """A service still running a minute after its stop is not copied from under it."""
    home = _home(tmp_path, "home-unstopped")
    before = _tree(serviced.state)

    ran = migrating(
        serviced, home, "-LocalAppData", str(home.local), **{REGISTERED: "1", REFUSES: "queryex"}
    )

    failing((ran.code, ran.said), naming="has not stopped a minute after it was asked to")
    truth(not home.state.exists(), describing="nothing copied")
    truth("sc.exe delete printobserver" not in ran.recorded, describing="no deletion requested")
    equal(_tree(serviced.state), before, describing="the service's state")


def test_local_application_data_overlapping_the_services_record_is_refused(
    migrating: Migrating, serviced: Serviced, tmp_path: Path
) -> None:
    """A destination inside the record would be copied into itself; nothing is stopped."""
    home = _home(tmp_path, "home-overlapping")
    before = _tree(serviced.state)

    ran = migrating(serviced, home, "-LocalAppData", str(serviced.state), **{REGISTERED: "1"})

    failing((ran.code, ran.said), naming="overlap, so the copy would be made into or around")
    equal(_tree(serviced.state), before, describing="the service's state")
    truth(
        not any(line.startswith("sc.exe stop") for line in ran.recorded),
        describing=f"nothing stopped: {ran.recorded}",
    )


@UNWRITABLE
def test_a_service_configuration_that_cannot_be_read_is_refused_before_anything_is_stopped(
    migrating: Migrating, serviced: Serviced, tmp_path: Path
) -> None:
    """A configuration this session may not read is said so, and the service left running."""
    home = _home(tmp_path, "home-unreadable-config")
    serviced.config.chmod(0o000)
    try:
        ran = migrating(serviced, home, "-LocalAppData", str(home.local), **{REGISTERED: "1"})
    finally:
        serviced.config.chmod(0o644)

    failing((ran.code, ran.said), naming="could not be read")
    truth(
        not any(line.startswith("sc.exe stop") for line in ran.recorded),
        describing=f"nothing stopped: {ran.recorded}",
    )


@UNWRITABLE
def test_a_copy_that_cannot_be_read_back_deletes_nothing(
    migrating: Migrating, serviced: Serviced, tmp_path: Path
) -> None:
    """A copy the check cannot read is one it cannot verify: the service is left registered."""
    home = _home(tmp_path, "home-unreadable-copy")
    unreadable = home.state / STORE
    try:
        ran = migrating(
            serviced,
            home,
            "-LocalAppData",
            str(home.local),
            **{REGISTERED: "1", ALTERS: f"unreadable:{unreadable}"},
        )
    finally:
        if unreadable.exists():
            unreadable.chmod(0o644)

    failing((ran.code, ran.said), naming="could not be read back")
    truth("sc.exe delete printobserver" not in ran.recorded, describing="no deletion requested")


@WINDOWS_ONLY
def test_on_windows_a_server_started_from_a_shell_keeps_the_shells_window(
    installing: Installing,
    releases: Callable[[str], Path],
    program: Path,
    tmp_path: Path,
    machine: Machine,
) -> None:
    """A console the server shares with the shell that started it stays shown."""
    home = _home(tmp_path, "home-shell")
    ran = installing(home, releases(_version()))
    passing((ran.code, ran.said), describing="the per-user install, its task stood in")
    port = _free_port()
    _fill_in(home.config, machine, CREDENTIAL, f"127.0.0.1:{port}")
    # llmlint: ignore[e2e_not_mocked, tests_mirror_real_usage] see suppressions.toml.
    install_the_skill(home.config)
    # A shell in a console window of its own, as `Start-Process` opens one,
    # which then starts the server in it: the console a person's own window is.
    shell = int(
        _powershell_answer(
            # One argument, the whole command in a pair of quotes of its own:
            # `cmd /s /k` strips exactly that outer pair and runs the rest.
            "(Start-Process -FilePath cmd.exe -PassThru -ArgumentList "
            + _quoted(f'/s /k ""{program}" server --config "{home.config}""')
            + ").Id"
        )
    )
    try:
        _eventually("the shell's server answering `prints`", lambda: _serves(program, port))
        served = _servers(program)
        equal(len(served), 1, describing=f"one server process: {served}")
        equal(
            _console_window_shown(served[0], tmp_path),
            "shown",
            describing="the window of the console the server shares with its shell",
        )
    finally:
        for pid in _servers(program):
            shell_run(["taskkill.exe", "/F", "/PID", str(pid)], timeout=60)
        shell_run(["taskkill.exe", "/F", "/T", "/PID", str(shell)], timeout=60)
