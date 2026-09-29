"""The installed service, unattended: activated, answering, brought back, torn down.

What makes `printobserver` a product rather than a program is that it comes
back after a reboot and keeps running after a crash, and neither is something a
file can prove. This journey proves both through the service manager itself: it
installs the service with the committed installer for that manager, fills the
configuration in as an operator would and lays the agent's skill down where it
names one, activates it with the operator's own documented command — read out of
`AGENTS.md`'s install-path section for that manager, never restated — and then
reads the manager's own answers back:

1. the installer started nothing;
2. the documented command brings the program up, as the service's own user, and
   it answers the API with nobody having launched it;
3. the manager reports it as one it starts at boot;
4. a stop through the manager is recorded as a clean exit;
5. a kill the process cannot answer is followed by the manager bringing up a new
   process, on a port of its own, as the service's user again;
6. teardown through the manager leaves the host as the journey found it.

This is the one walk, over every service manager the supported-platform list
names, and the manager is the one part of it that differs — each an adapter of
`Manager` below:

- **systemd**, on Linux, runs in a throwaway container whose first process is
  systemd, over the host's read-only `/usr`: the installer runs there as root,
  creates the service's own user, and the unit it enables exists in the
  container alone. The host needs Docker, and nothing of it is changed.
- **launchd**, on macOS, drives the host's own launchd through a password-free
  `sudo`: nothing isolates a system daemon there, so it refuses a Mac already
  carrying anything it would install and removes everything it installed.
- **launchd against a stand-in**, on Linux, puts `launchctl_standin.py` on
  `PATH` as `launchctl`. It starts and kills the real `printobserver` exactly as
  the property list the installer wrote tells launchd to, so the adapter's whole
  sequence and the service's behaviour under it run on every Linux cell; the
  real launchd takes the same sequence on the macOS cell.
- **windows-service**, on Windows, drives the service control manager through
  `sc.exe`, from a shell that may register a service.

A missing prerequisite — Docker, or a password-free `sudo` — skips a case,
naming it, unless `PRINTOBSERVER_SERVICE_JOURNEY` is `required`, which the
gate's cells set so that on the merge path a case that could not run fails
rather than vanishing. The journey never touches a service somebody else
installed, and it removes what it installed on every exit path, so a failing
assertion is not also a stranded service.
"""

from __future__ import annotations

import getpass
import io
import json
import os
import plistlib
import re
import shlex
import shutil
import signal
import socket
import subprocess
import sys
import tarfile
import tempfile
import threading
import time
import tomllib
from collections.abc import Callable, Iterator
from dataclasses import dataclass
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from pathlib import Path, PurePath, PurePosixPath
from typing import Protocol

import launchctl_standin
import pytest
import yaml
from journey import (
    HERE,
    REPO_ROOT,
    SKILL_DIRECTORY,
    clean_environment,
    install_the_skill,
    plain,
    pythonpath,
    run,
)
from repo_checks import install_path as ip
from repo_checks.checks_service import ACTIVATION_NAMES, installer_for
from repo_checks.expect import absent, contains, equal, passing, truth
from repo_checks.model import Repo
from repo_checks.platforms import ServiceManager
from repo_checks.shell import run as shell_run

#: The gate job in `ci.yml` sets this, so that on the merge path a case whose
#: prerequisite is missing fails instead of skipping out of sight.
REQUIRED = "PRINTOBSERVER_SERVICE_JOURNEY"
REQUIRED_VALUE = "required"
GATE_WORKFLOW = REPO_ROOT / ".github" / "workflows" / "ci.yml"

#: Where a Windows journey's root goes: a directory a virtual account can
#: traverse, named after this journey so that a leftover is recognisable.
WINDOWS_ROOT = Path(r"C:\ProgramData\printobserver-journeys")

#: The places the installer puts things when it runs as root, as the service
#: sees them.
BINARY_DIRECTORY = PurePosixPath("/usr/local/lib/printobserver")
CONFIGURATION = PurePosixPath("/etc/printobserver/config.toml")
STATE = PurePosixPath("/var/lib/printobserver")

SERVICE_USER = "printobserver"

#: Where the journey's container carries what it mounts from this host.
CONTAINED_INSTALLER = "/journey/install-service.sh"
CONTAINED_PROGRAM = "/journey/printobserver"
CONTAINED_SKILL = "/journey/skill/printobserver"

BUILD_TIMEOUT_SECONDS = 2400

WITHIN_SECONDS = 120

QUESTION = "/v1/prints"


class _AnswersEveryRead(BaseHTTPRequestHandler):
    """An OctoPrint that answers every read with an empty document, and nothing else."""

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
    # llmlint: ignore[e2e_not_mocked] suppressions.toml has the reason.
    server = ThreadingHTTPServer(("127.0.0.1", 0), _AnswersEveryRead)
    thread = threading.Thread(target=server.serve_forever, daemon=True)
    thread.start()
    try:
        yield f"http://127.0.0.1:{server.server_address[1]}"
    finally:
        server.shutdown()
        server.server_close()


def _said(result: subprocess.CompletedProcess[str]) -> str:
    """Everything one run said, on either stream."""
    return f"{result.stdout or ''}{result.stderr or ''}"


def _required() -> bool:
    """Whether this run requires every case to run, read from `PRINTOBSERVER_SERVICE_JOURNEY`.

    Raises:
        AssertionError: If it is set to anything but `required`, since a misspelt
            setting would otherwise turn a lane's failures back into skips.
    """
    setting = os.environ.get(REQUIRED, "")
    if setting not in {"", REQUIRED_VALUE}:
        message = f"{REQUIRED}={setting!r} is neither unset nor {REQUIRED_VALUE!r}"
        raise AssertionError(message)
    return setting == REQUIRED_VALUE


def _unmet(prerequisite: str) -> None:
    """Skip a case whose prerequisite this host lacks, or fail where a lane requires it.

    Raises:
        AssertionError: If `PRINTOBSERVER_SERVICE_JOURNEY` is `required`.
    """
    if _required():
        message = (
            f"{prerequisite}; {REQUIRED}={REQUIRED_VALUE}, so this case fails rather than skips"
        )
        raise AssertionError(message)
    pytest.skip(prerequisite)


@dataclass(frozen=True, slots=True)
class Installed:
    """What the installer put in place, as the manager's side of the journey names it."""

    configuration: PurePath
    state: PurePath


@dataclass(frozen=True, slots=True)
class Reported:
    """One thing the manager was asked, whether it held, and what it said."""

    held: bool
    recorded: str


@dataclass(frozen=True, slots=True)
class Served:
    """Where the running service said it serves, and what authenticates to it."""

    address: str
    credential: str


@dataclass(frozen=True, slots=True)
class Answer:
    """One answer the API gave, and the address it came from."""

    text: str
    address: str


class Manager(Protocol):
    """One service manager, as the journey drives it.

    Every method here is the manager's own vocabulary for one step of the
    journey; the journey itself is written once over this protocol.
    """

    manager: ServiceManager

    def prepare(self) -> None:
        """Skip or fail a host this must not touch, and bring the manager's side up."""

    def install(self) -> Installed:
        """Run the committed installer for this manager."""

    def read(self, path: PurePath) -> str | None:
        """One of the service's files, read as the operator reads it, or `None`."""

    def write(self, path: PurePath, text: str) -> None:
        """Replace one of the service's files, as the operator edits it."""

    def lay_down_skill(self, installed: Installed) -> None:
        """Put the agent's skill where the configuration names it, as `gh skill install` does."""

    def activate(self) -> None:
        """Run the operator's own documented second command."""

    def starts_automatically(self) -> Reported:
        """Whether the manager reports the service as one it starts by itself."""

    def switch_off(self) -> None:
        """Tell the manager not to start the service at boot, as an operator would."""

    def is_running(self) -> bool:
        """Whether the manager reports the service running."""

    def main_pid(self) -> int:
        """The process the manager reports as the service's, or zero."""

    def runs_as_the_service_user(self, pid: int) -> Reported:
        """Whether `pid` runs as the account the service is installed to run as."""

    def end_abruptly(self, pid: int) -> None:
        """End the service's process the way a crash does."""

    def stop(self) -> None:
        """Ask the manager to stop the service."""

    def stopped_gracefully(self) -> Reported:
        """Whether the manager recorded a clean exit, and what it recorded."""

    def remove(self) -> None:
        """Tear the service down through the manager, and remove what was installed."""

    def is_present(self) -> bool:
        """Whether the manager knows the service at all."""

    def leftovers(self) -> list[str]:
        """Everything of the journey the host still carries."""

    def diagnosis(self) -> str:
        """Everything the manager says of the service, for a failure message."""


def _eventually(
    manager: Manager, what: str, condition: Callable[[], bool], *, within: float = WITHIN_SECONDS
) -> None:
    """Wait for the manager to report `what`, and fail naming it and what the manager says.

    Raises:
        AssertionError: If `condition()` is still false after `within` seconds.
    """
    started = time.monotonic()
    while True:
        if condition():
            return
        if time.monotonic() - started > within:
            message = (
                f"the service manager did not report {what} within {within} seconds; "
                f"it says:\n{manager.diagnosis()}"
            )
            raise AssertionError(message)
        time.sleep(0.5)


def _skill_directory(configuration: str) -> PurePosixPath:
    """The directory a configuration's `supervisor.skill_path` names the skill in."""
    supervisor = tomllib.loads(configuration).get("supervisor")
    named = supervisor.get("skill_path") if isinstance(supervisor, dict) else None
    truth(
        isinstance(named, str) and named,
        describing="the installed configuration to name a `supervisor.skill_path`",
    )
    return PurePosixPath(str(named)).parent


def _container_root() -> bytes:
    """The root file system of the journey's container, as a tar archive.

    The directories and links a Linux root has, and an `/etc` holding root
    alone, so that the user the installer creates is the container's own.
    """
    archive = io.BytesIO()
    with tarfile.open(fileobj=archive, mode="w") as tar:
        for directory in (
            "etc/systemd/system",
            "var",
            "tmp",
            "run",
            "root",
            "home",
            "proc",
            "sys",
            "dev",
            "usr",
            "journey",
        ):
            entry = tarfile.TarInfo(directory)
            entry.type, entry.mode = tarfile.DIRTYPE, 0o755
            tar.addfile(entry)
        for link, target in (("bin", "usr/bin"), ("sbin", "usr/sbin"), ("lib", "usr/lib")):
            entry = tarfile.TarInfo(link)
            entry.type, entry.linkname = tarfile.SYMTYPE, target
            tar.addfile(entry)
        entry = tarfile.TarInfo("lib64")
        entry.type, entry.linkname = tarfile.SYMTYPE, "usr/lib64"
        tar.addfile(entry)
        for name, text in (
            ("etc/passwd", "root:x:0:0:root:/root:/bin/sh\n"),
            ("etc/group", "root:x:0:\n"),
            ("etc/shadow", ""),
            ("etc/gshadow", ""),
            ("etc/machine-id", ""),
            ("etc/nsswitch.conf", "passwd: files\ngroup: files\nshadow: files\ngshadow: files\n"),
        ):
            data = text.encode()
            entry = tarfile.TarInfo(name)
            entry.size, entry.mode = len(data), 0o644
            tar.addfile(entry, io.BytesIO(data))
    return archive.getvalue()


class Systemd:
    """The `systemd` adapter: systemd as PID 1 of a throwaway container, over this host's `/usr`."""

    manager = ServiceManager.SYSTEMD

    def __init__(self, name: str, program: Path) -> None:
        """An adapter over the unit `name`, installing `program`."""
        self.name = name
        self.program = program
        stamp = f"{os.getpid()}-{time.time_ns()}"
        self.container = f"printobserver-service-journey-{stamp}"
        self.image = f"{self.container}:root"

    def _docker(
        self, *arguments: str, stdin: str | None = None, timeout: float = 120
    ) -> subprocess.CompletedProcess[str]:
        # llmlint: ignore[async_typed_clients_at_boundaries] suppressions.toml has the reason.
        return shell_run(["docker", *arguments], stdin=stdin, timeout=timeout)

    def _as_root(self, *argv: str, stdin: str | None = None) -> subprocess.CompletedProcess[str]:
        """One command as root inside the container."""
        return self._docker("exec", "--interactive", self.container, *argv, stdin=stdin)

    # llmlint: ignore[async_typed_clients_at_boundaries] suppressions.toml has the reason.
    def _systemctl(self, *arguments: str, check: bool = False) -> str:
        result = self._as_root("systemctl", *arguments)
        if check:
            passing(
                (result.returncode, _said(result)),
                describing=f"`systemctl {' '.join(arguments)}`",
            )
        return result.stdout

    def _show(self, prop: str) -> str:
        return self._systemctl("show", "-p", prop, "--value", self.name).strip()

    def prepare(self) -> None:
        """Need Docker; start a container whose first process is systemd."""
        if shutil.which("docker") is None or self._docker("version").returncode != 0:
            _unmet(
                "this journey's systemd runs as the first process of a container, and this "
                "host has no `docker` that answers"
            )
        installer = REPO_ROOT / installer_for(Repo(REPO_ROOT), ServiceManager.SYSTEMD)
        with tempfile.TemporaryDirectory() as staging:
            archive = Path(staging) / "root.tar"
            archive.write_bytes(_container_root())
            imported = self._docker("import", str(archive), self.image, timeout=300)
        passing(
            (imported.returncode, _said(imported)),
            describing="importing the container's root file system",
        )
        started = self._docker(
            "run",
            "--detach",
            "--name",
            self.container,
            # systemd as the container's PID 1 mounts its own cgroup hierarchy
            # and the tmpfs under `/run`, and starts units as other users; the
            # capabilities that takes are the ones `--privileged` grants, on a
            # throwaway container of this journey's own removed on every exit path.
            # llmlint: ignore[least_privilege_grants] suppressions.toml has the reason.
            "--privileged",
            "--cgroupns=private",
            # The host's network, so the service it runs answers on this loopback.
            "--network=host",
            "--tmpfs=/run",
            "--tmpfs=/run/lock",
            "--tmpfs=/tmp",
            "--tmpfs=/usr/local:exec",
            "--volume=/usr:/usr:ro",
            # The certificate authorities this host trusts, which the service's
            # HTTP clients load when they are built.
            "--volume=/etc/ssl:/etc/ssl:ro",
            "--env=container=docker",
            f"--volume={installer.resolve()}:{CONTAINED_INSTALLER}:ro",
            f"--volume={self.program.resolve()}:{CONTAINED_PROGRAM}:ro",
            f"--volume={SKILL_DIRECTORY.resolve()}:{CONTAINED_SKILL}:ro",
            self.image,
            "/usr/lib/systemd/systemd",
        )
        try:
            passing(
                (started.returncode, _said(started)),
                describing="starting a container whose first process is systemd",
            )
            _eventually(
                self,
                "systemd finishing its start",
                lambda: self._systemctl("is-system-running").strip() in {"running", "degraded"},
            )
        except BaseException:
            self._discard()
            raise

    def install(self) -> Installed:
        """The committed shell installer, as root, into the container's own `/`."""
        result = self._as_root("sh", CONTAINED_INSTALLER, "--binary", CONTAINED_PROGRAM)
        passing((result.returncode, _said(result)), describing="the committed installer")
        return Installed(CONFIGURATION, STATE)

    def read(self, path: PurePath) -> str | None:
        """`cat` as root in the container."""
        result = self._as_root("cat", str(path))
        return result.stdout if result.returncode == 0 else None

    def write(self, path: PurePath, text: str) -> None:
        """`cat >` as root in the container."""
        result = self._as_root("sh", "-c", 'cat > "$1"', "sh", str(path), stdin=text)
        passing((result.returncode, _said(result)), describing=f"writing {path}")

    def lay_down_skill(self, installed: Installed) -> None:
        """The committed skill's directory, copied as root to where the configuration names it."""
        target = _skill_directory(self.read(installed.configuration) or "")
        for argv in (
            ("mkdir", "-p", str(target.parent)),
            ("cp", "-R", CONTAINED_SKILL, str(target)),
        ):
            result = self._as_root(*argv)
            passing((result.returncode, _said(result)), describing="laying the skill down")

    def activate(self) -> None:
        """The documented command, as root; the container's shell already is, so `sudo` goes."""
        words = _activation(self.manager).split()
        equal(words[:1], ["sudo"], describing="the documented command running as root")
        result = self._as_root(*words[1:])
        passing((result.returncode, _said(result)), describing=f"`{' '.join(words)}`")

    def starts_automatically(self) -> Reported:
        """`enabled` is systemd saying the unit is linked into a boot target through `/etc`."""
        said = self._systemctl("is-enabled", self.name).strip()
        return Reported(said == "enabled", f"systemctl is-enabled says `{said}`")

    def switch_off(self) -> None:
        """`systemctl disable`, which unlinks the unit from its boot target."""
        self._systemctl("disable", self.name, check=True)

    def is_running(self) -> bool:
        """`active`, and a main process the manager knows."""
        return self._systemctl("is-active", self.name).strip() == "active" and self.main_pid() > 0

    def main_pid(self) -> int:
        """The unit's main process, or zero while it has none."""
        shown = self._show("MainPID")
        return int(shown) if shown.isdigit() else 0

    def runs_as_the_service_user(self, pid: int) -> Reported:
        """The process's user id against the id of the user the installer created."""
        return _compare_users(self._as_root, pid, SERVICE_USER)

    def end_abruptly(self, pid: int) -> None:
        """`SIGKILL`, which the process cannot answer, as a crash is."""
        result = self._as_root("kill", "-KILL", str(pid))
        passing((result.returncode, _said(result)), describing=f"killing process {pid}")

    def stop(self) -> None:
        """Ask the manager to stop the unit."""
        self._systemctl("stop", self.name, check=True)

    def stopped_gracefully(self) -> Reported:
        """`Result=success` with an exit status of zero; a killed unit reads `signal`."""
        recorded = (
            f"Result={self._show('Result')} ExecMainCode={self._show('ExecMainCode')} "
            f"ExecMainStatus={self._show('ExecMainStatus')}"
        )
        return Reported(recorded == "Result=success ExecMainCode=1 ExecMainStatus=0", recorded)

    def remove(self) -> None:
        """Disable and stop the unit through systemd, then discard the container and its image."""
        try:
            self._systemctl("disable", "--now", self.name, check=True)
            _eventually(self, "the service gone once disabled", lambda: self.main_pid() == 0)
        finally:
            self._discard()

    def _discard(self) -> None:
        self._docker("rm", "--force", self.container)
        self._docker("rmi", "--force", self.image)

    def is_present(self) -> bool:
        """Whether systemd loads a unit of this name at all."""
        return self._show("LoadState") not in {"not-found", ""}

    def leftovers(self) -> list[str]:
        """The journey's container and image, if either is still on this host."""
        return [
            f"the {what} {name}"
            for what, name in (("container", self.container), ("image", self.image))
            if self._docker(what, "inspect", name).returncode == 0
        ]

    def diagnosis(self) -> str:
        """`systemctl status` and the unit's journal."""
        return "\n".join(
            _said(self._as_root(*argv))
            for argv in (
                ("systemctl", "status", "--no-pager", self.name),
                ("journalctl", "--unit", self.name, "--no-pager", "--lines=40"),
            )
        )


#: One entry of `launchctl print-disabled`: a quoted label and whether it is switched off,
#: spelled `disabled`/`enabled` by current launchd and `true`/`false` by older ones.
DISABLED_ENTRY = re.compile(r'"(?P<label>[^"]+)" => (?P<state>disabled|enabled|true|false)')


def _disabled_services(listed: str) -> set[str] | None:
    """The labels a `launchctl print-disabled` listing switches off, or `None` for no listing.

    The listing opens `disabled services = {`, holds one entry per line and closes
    with `}`; launchd may print further blocks after it, which say nothing of this.
    """
    lines = [line.strip() for line in listed.splitlines() if line.strip()]
    if lines[:1] != ["disabled services = {"]:
        return None
    switched_off: set[str] = set()
    for line in lines[1:]:
        if line == "}":
            return switched_off
        entry = DISABLED_ENTRY.fullmatch(line)
        if entry is None:
            return None
        if entry["state"] in {"disabled", "true"}:
            switched_off.add(entry["label"])
    return None


def _compare_users(
    on_machine: Callable[..., subprocess.CompletedProcess[str]], pid: int, user: str
) -> Reported:
    """Whether `pid` runs as `user`, compared by user id rather than a name `ps` may shorten.

    `on_machine` runs a command on the machine the service is on, with whatever
    privilege the adapter runs its commands there with.
    """
    running = on_machine("ps", "-o", "uid=", "-p", str(pid)).stdout.strip()
    expected = on_machine("id", "-u", user).stdout.strip()
    return Reported(
        bool(expected) and running == expected,
        f"process {pid} runs as uid {running or '(none)'}; {user} is uid {expected or '(none)'}",
    )


@dataclass(frozen=True, slots=True)
class StandIn:
    """The stand-in machine a launchd adapter drives when this host has no launchd."""

    #: The directory the stand-in's machine has at `/`.
    root: Path
    #: Where the stand-in keeps its records, the recording of its invocations included.
    state: Path

    def recorded(self) -> list[list[str]]:
        """Every `launchctl` invocation the stand-in answered, in order."""
        recording = self.state / launchctl_standin.RECORDING
        if not recording.is_file():
            return []
        return [json.loads(line) for line in recording.read_text(encoding="utf-8").splitlines()]


def _stand_in_launchd(scratch: Path, monkeypatch: pytest.MonkeyPatch) -> StandIn:
    """Put `launchctl` on `PATH` as the stand-in, and `uname` answering `Darwin` beside it.

    The installer selects the manager it writes a definition for from `uname
    -s`, which is how it knows it is on a launchd host; on this host that is
    the one other answer the stand-in machine has to give.
    """
    shims = scratch / "bin"
    shims.mkdir()
    # The stand-in is this repository's own Python, which finds its tools by
    # absolute path whatever directory `launchctl` is run from.
    packages = os.pathsep.join(str(REPO_ROOT / entry) for entry in pythonpath().split(os.pathsep))
    standin = Path(launchctl_standin.__file__).resolve()
    # llmlint: ignore[e2e_not_mocked] suppressions.toml has the reason.
    (shims / "launchctl").write_text(
        f"#!/bin/sh\nPYTHONPATH={shlex.quote(packages)} "
        f'exec {shlex.quote(sys.executable)} {shlex.quote(str(standin))} "$@"\n',
        encoding="utf-8",
    )
    real_uname = shutil.which("uname")
    truth(real_uname is not None, describing="`uname` on this host's PATH")
    # llmlint: ignore[e2e_not_mocked] suppressions.toml has the reason.
    (shims / "uname").write_text(
        f'#!/bin/sh\n[ "$*" = "-s" ] && {{ echo Darwin; exit 0; }}\n'
        f'exec {shlex.quote(str(real_uname))} "$@"\n',
        encoding="utf-8",
    )
    for shim in shims.iterdir():
        shim.chmod(0o755)
    stand_in = StandIn(scratch / "machine", scratch / "launchd")
    stand_in.root.mkdir()
    # llmlint: ignore[e2e_not_mocked] suppressions.toml has the reason.
    monkeypatch.setenv("PATH", f"{shims}{os.pathsep}{os.environ['PATH']}")
    monkeypatch.setenv(launchctl_standin.ROOT, str(stand_in.root))
    monkeypatch.setenv(launchctl_standin.STATE, str(stand_in.state))
    return stand_in


class Launchd:
    """The `launchd` adapter: `launchctl` in the system domain, on the host or its stand-in.

    Against the host's own launchd every command runs through `sudo`, and the
    installer runs as root, creating the service's own user. Against the
    stand-in, nothing needs root: the installer is pointed at the stand-in
    machine's root and at the invoking user, which is the one user the stand-in
    can run a job as, and the documented command runs unchanged but for the
    `sudo` it opens with.
    """

    manager = ServiceManager.LAUNCHD

    def __init__(self, program: Path, stand_in: StandIn | None) -> None:
        """An adapter installing `program`, against the host's launchd or `stand_in`."""
        self.program = program
        self.stand_in = stand_in
        self.words = _activation(self.manager).split()
        self.definition = PurePosixPath(self.words[-1])
        self.label = self.definition.name.removesuffix(".plist")
        self.root = stand_in.root if stand_in is not None else Path("/")
        self.user = getpass.getuser() if stand_in is not None else SERVICE_USER
        self.had_var_lib = Path("/var/lib").exists()

    def _beneath(self, path: PurePosixPath) -> PurePath:
        """Where one of the paths the service sees is, on the machine the adapter drives."""
        return self.root / path.relative_to("/")

    def _on_machine(
        self, *argv: str, stdin: str | None = None, timeout: float = 120
    ) -> subprocess.CompletedProcess[str]:
        """One command on the machine this adapter drives.

        As root through `sudo` on the host, and as the invoking user on the
        stand-in's machine, which that user owns.
        """
        prefix = ["sudo", "-n"] if self.stand_in is None else []
        # llmlint: ignore[async_typed_clients_at_boundaries] suppressions.toml has the reason.
        return shell_run([*prefix, *argv], stdin=stdin, timeout=timeout)

    # llmlint: ignore[async_typed_clients_at_boundaries] suppressions.toml has the reason.
    def _launchctl(self, *arguments: str, check: bool = False) -> subprocess.CompletedProcess[str]:
        result = self._on_machine("launchctl", *arguments)
        if check:
            passing(
                (result.returncode, _said(result)),
                describing=f"`launchctl {' '.join(arguments)}`",
            )
        return result

    def _installed_definition(self) -> dict[str, object]:
        """The property list the installer wrote, which launchd reads as a dictionary."""
        with Path(self._beneath(self.definition)).open("rb") as handle:
            written = plistlib.load(handle)
        truth(
            isinstance(written, dict),
            describing=f"the installed {self.definition} to be a dictionary; it is {written!r}",
        )
        return written if isinstance(written, dict) else {}

    def _print(self) -> subprocess.CompletedProcess[str]:
        return self._launchctl("print", f"system/{self.label}")

    def prepare(self) -> None:
        """On the host: need a password-free `sudo`, and a Mac carrying nothing it would install."""
        if self.stand_in is not None:
            return
        if self._on_machine("true", timeout=30).returncode != 0:
            _unmet(
                "this journey loads a real launchd daemon and needs password-free `sudo`, "
                "which this host does not grant"
            )
        present = self.leftovers()
        if present:
            _unmet(
                f"this host already carries {present}, and this journey removes everything it "
                f"installs; it will not run over an installation it did not make"
            )

    def install(self) -> Installed:
        """The committed shell installer: as root on the host, or into the stand-in's root."""
        installer = str(REPO_ROOT / installer_for(Repo(REPO_ROOT), ServiceManager.LAUNCHD))
        # The stand-in's machine is one the invoking user owns, so the installer
        # is told its root and that user rather than creating a system user.
        into = [] if self.stand_in is None else ["--root", str(self.root), "--user", self.user]
        result = self._on_machine(
            "sh", installer, *into, "--binary", str(self.program), timeout=300
        )
        passing((result.returncode, _said(result)), describing="the committed installer")
        written = self._installed_definition()
        equal(
            set(written),
            set(launchctl_standin.KEYS),
            describing="the property list's keys, which the launchctl stand-in acts on",
        )
        binary = self._beneath(BINARY_DIRECTORY / "printobserver")
        equal(
            (written.get("ProgramArguments"), written.get("WorkingDirectory")),
            (
                [str(binary), "server", "--config", str(self._beneath(CONFIGURATION))],
                str(self._beneath(STATE)),
            ),
            describing="the places the installed property list names, which teardown removes",
        )
        return Installed(self._beneath(CONFIGURATION), self._beneath(STATE))

    def read(self, path: PurePath) -> str | None:
        """`cat` as root on the host; the file itself on the stand-in's machine."""
        if self.stand_in is None:
            result = self._on_machine("cat", str(path))
            return result.stdout if result.returncode == 0 else None
        try:
            return Path(path).read_text(encoding="utf-8")
        except OSError:
            return None

    def write(self, path: PurePath, text: str) -> None:
        """`cat >` as root on the host; the file itself on the stand-in's machine."""
        if self.stand_in is not None:
            Path(path).write_text(text, encoding="utf-8")
            return
        result = self._on_machine("sh", "-c", 'cat > "$1"', "sh", str(path), stdin=text)
        passing((result.returncode, _said(result)), describing=f"writing {path}")

    def lay_down_skill(self, installed: Installed) -> None:
        """The committed skill's directory, where the configuration names it."""
        if self.stand_in is None:
            target = _skill_directory(self.read(installed.configuration) or "")
            for argv in (
                ("mkdir", "-p", str(target.parent)),
                ("cp", "-R", str(SKILL_DIRECTORY), str(target)),
            ):
                result = self._on_machine(*argv)
                passing((result.returncode, _said(result)), describing="laying the skill down")
            return
        # llmlint: ignore[e2e_not_mocked, tests_mirror_real_usage] suppressions.toml has the reason.
        install_the_skill(Path(installed.configuration))

    def activate(self) -> None:
        """The documented command: through `sudo` on the host, and without it on the stand-in."""
        equal(self.words[:1], ["sudo"], describing="the documented command running as root")
        result = self._on_machine(*self.words[1:])
        passing((result.returncode, _said(result)), describing=f"`{' '.join(self.words)}`")

    def starts_automatically(self) -> Reported:
        """Loaded from the boot directory, asking to start at load, and not switched off.

        launchd reports a service loaded from its boot directory by that path,
        and lists a service the operator switched off in the domain's disabled
        set; one loaded from anywhere else is gone at the next boot.
        """
        printing = self._print()
        if printing.returncode != 0:
            return Reported(False, f"launchd does not report it loaded:\n{_said(printing)}")
        printed = printing.stdout
        if not any(line.strip() == f"path = {self.definition}" for line in printed.splitlines()):
            return Reported(
                False, f"launchd does not report it loaded from {self.definition}:\n{printed}"
            )
        if self._installed_definition().get("RunAtLoad") is not True:
            return Reported(False, f"{self.definition} does not ask launchd to start it at load")
        disabled = self._launchctl("print-disabled", "system")
        if disabled.returncode != 0:
            return Reported(False, f"launchd could not list what is disabled:\n{_said(disabled)}")
        switched_off = _disabled_services(disabled.stdout)
        if switched_off is None:
            return Reported(False, f"launchd's disabled listing is not one:\n{disabled.stdout}")
        return Reported(
            self.label not in switched_off, f"launchd lists it as disabled:\n{disabled.stdout}"
        )

    def _switched_off(self) -> bool:
        """Whether the system domain's disabled set lists the service.

        Raises:
            AssertionError: If launchd's answer is not a disabled listing.
        """
        listed = self._launchctl("print-disabled", "system", check=True).stdout
        switched_off = _disabled_services(listed)
        if switched_off is None:
            message = f"`launchctl print-disabled system` answered no listing:\n{listed}"
            raise AssertionError(message)
        return self.label in switched_off

    def switch_off(self) -> None:
        """`launchctl disable`, which adds the service to the domain's disabled set."""
        self._launchctl("disable", f"system/{self.label}", check=True)

    def is_running(self) -> bool:
        """Loaded, and a process the manager knows."""
        return self.main_pid() > 0

    def main_pid(self) -> int:
        """The `pid = ` launchd prints for the service, or zero while it has none."""
        for line in self._print().stdout.splitlines():
            pid = line.strip().removeprefix("pid = ")
            if pid != line.strip() and pid.isdigit():
                return int(pid)
        return 0

    def runs_as_the_service_user(self, pid: int) -> Reported:
        """The process's user id against the id of the user the property list names."""
        named = self._installed_definition().get("UserName")
        equal(named, self.user, describing="the user the installed property list runs it as")
        return _compare_users(self._on_machine, pid, self.user)

    def end_abruptly(self, pid: int) -> None:
        """`SIGKILL`, which the process cannot answer, as a crash is."""
        result = self._on_machine("kill", "-KILL", str(pid))
        passing((result.returncode, _said(result)), describing=f"killing process {pid}")

    def stop(self) -> None:
        """`SIGTERM` through launchd, which the service answers by exiting cleanly.

        `KeepAlive` with `SuccessfulExit` false leaves a job that exited
        successfully down, so this is launchd's stop that is not a removal.
        """
        self._launchctl("kill", str(int(signal.SIGTERM)), f"system/{self.label}", check=True)

    def stopped_gracefully(self) -> Reported:
        """`last exit code = 0` and no terminating signal; a killed job reads `Killed: 9`."""
        lines = [line.strip() for line in self._print().stdout.splitlines()]
        recorded = [line for line in lines if line.startswith("last ")]
        clean = "last exit code = 0" in lines and not any(
            line.startswith("last terminating signal") for line in lines
        )
        return Reported(clean, "; ".join(recorded) or "nothing about its last exit")

    def remove(self) -> None:
        """Boot the service out through launchd, then remove everything the installer put down."""
        try:
            self._launchctl("bootout", f"system/{self.label}")
            _eventually(self, "the service gone once booted out", lambda: not self.is_present())
            # launchd keeps the disabled set across a bootout, so a switch-off is undone too.
            if self._switched_off():
                self._launchctl("enable", f"system/{self.label}", check=True)
        finally:
            if self.stand_in is not None:
                shutil.rmtree(self.root, ignore_errors=True)
            else:
                self._on_machine(
                    "rm",
                    "-rf",
                    str(BINARY_DIRECTORY),
                    str(CONFIGURATION.parent),
                    str(STATE),
                    str(self.definition),
                )
                if not self.had_var_lib:
                    self._on_machine("rmdir", "/var/lib")
                for record in ("Users", "Groups"):
                    self._on_machine("dscl", ".", "-delete", f"/{record}/{SERVICE_USER}")

    def is_present(self) -> bool:
        """Whether launchd has the service loaded at all.

        Raises:
            AssertionError: If launchd answers neither with the service nor with
                its own not-found exit, since that says nothing about presence.
        """
        printing = self._print()
        if printing.returncode not in {0, launchctl_standin.NOT_FOUND}:
            message = f"`launchctl print system/{self.label}` failed:\n{_said(printing)}"
            raise AssertionError(message)
        return printing.returncode == 0

    def leftovers(self) -> list[str]:
        """What of an installation this host carries, and a loaded service."""
        present: list[str] = []
        if self.is_present():
            present.append(f"a loaded launchd service {self.label}")
        if self._switched_off():
            present.append(f"{self.label} in launchd's disabled set")
        if self.stand_in is not None:
            return [*present, *([str(self.root)] if self.root.exists() else [])]
        present.extend(
            str(path)
            for path in (BINARY_DIRECTORY, CONFIGURATION, STATE, self.definition)
            if self._on_machine("test", "-e", str(path)).returncode == 0
        )
        if self._on_machine("id", "-u", SERVICE_USER).returncode == 0:
            present.append(f"the user {SERVICE_USER}")
        if self._on_machine("dscl", ".", "-read", f"/Groups/{SERVICE_USER}").returncode == 0:
            present.append(f"the group {SERVICE_USER}")
        if Path("/var/lib").exists() != self.had_var_lib:
            present.append("/var/lib, which the journey found absent")
        return present

    def diagnosis(self) -> str:
        """`launchctl print` and the service's own error log."""
        log = self.read(self._beneath(STATE / "printobserver.log")) or "(no log)"
        asked = ""
        if self.stand_in is not None:
            asked = "\nasked of the stand-in:\n" + "\n".join(
                " ".join(invocation) for invocation in self.stand_in.recorded()
            )
        return f"{_said(self._print())}\n{log}{asked}"


class WindowsService:
    """The `windows-service` adapter: the service control manager, through `sc.exe`."""

    manager = ServiceManager.WINDOWS_SERVICE

    def __init__(self, name: str, program: Path) -> None:
        """An adapter over the service `name`, installing `program`."""
        self.name = name
        self.program = program
        self.root = WINDOWS_ROOT / f"journey-{os.getpid()}-{int(time.time())}"

    @staticmethod
    def _powershell() -> str:
        for candidate in ("pwsh", "powershell"):
            found = shutil.which(candidate)
            if found:
                return found
        message = "this host has neither `pwsh` nor `powershell` on PATH"
        raise AssertionError(message)

    def _run(
        self, *argv: str, cwd: Path | None = None, env: dict[str, str] | None = None
    ) -> subprocess.CompletedProcess[str]:
        """One command from the elevated shell the operator runs the pair in."""
        # llmlint: ignore[async_typed_clients_at_boundaries] suppressions.toml has the reason.
        return shell_run(list(argv), cwd=cwd, env=env, timeout=300)

    def _sc(self, *arguments: str, check: bool = False) -> tuple[int, str]:
        result = self._run("sc.exe", *arguments)
        if check:
            passing(
                (result.returncode, _said(result)),
                describing=f"`sc.exe {' '.join(arguments)}`",
            )
        return result.returncode, _said(result)

    def _field(self, listing: str, field: str) -> str:
        """`FIELD : value` out of `sc.exe`'s own listing, or an empty string."""
        for line in listing.splitlines():
            head, _, value = line.partition(":")
            if head.strip() == field:
                return value.strip()
        return ""

    def prepare(self) -> None:
        """Leave a real installation alone; clear a leftover of this journey's own."""
        code, listing = self._sc("qc", self.name)
        if code != 0:
            return
        binary = self._field(listing, "BINARY_PATH_NAME")
        # A leftover of this journey's own runs the program its installer placed
        # in a root of the journey's shape, and nothing else is one to remove.
        owned = re.match(
            rf'"?{re.escape(str(WINDOWS_ROOT).lower())}\\(journey-\d+-\d+)'
            r'\\program files\\printobserver\\printobserver\.exe"?(?:\s|$)',
            binary.lower(),
        )
        if owned is None:
            _unmet(
                f"this host already has a `{self.name}` service running {binary}, which is "
                f"not this journey's own, and the journey will not touch a service somebody "
                f"installed"
            )
            return
        self._unregister()
        shutil.rmtree(WINDOWS_ROOT / owned[1], ignore_errors=True)

    def install(self) -> Installed:
        """The committed PowerShell installer into the journey's own root."""
        self.root.mkdir(parents=True, exist_ok=True)
        result = self._run(
            self._powershell(),
            "-NoProfile",
            "-File",
            str(REPO_ROOT / installer_for(Repo(REPO_ROOT), ServiceManager.WINDOWS_SERVICE)),
            "-Root",
            str(self.root),
            "-Binary",
            str(self.program),
            cwd=self.root,
            env=clean_environment(),
        )
        passing((result.returncode, _said(result)), describing="the committed installer")
        return Installed(
            self.root / "ProgramData" / "printobserver" / "config.toml",
            self.root / "ProgramData" / "printobserver" / "state",
        )

    def read(self, path: PurePath) -> str | None:
        """The file itself: this shell is the elevated one the operator uses."""
        try:
            return Path(path).read_text(encoding="utf-8")
        except OSError:
            return None

    def write(self, path: PurePath, text: str) -> None:
        """The file itself."""
        Path(path).write_text(text, encoding="utf-8")

    def lay_down_skill(self, installed: Installed) -> None:
        """The committed skill's directory, where the configuration names it."""
        # llmlint: ignore[e2e_not_mocked, tests_mirror_real_usage] suppressions.toml has the reason.
        install_the_skill(Path(installed.configuration))

    def activate(self) -> None:
        """Run the documented command verbatim, in PowerShell."""
        activation = _activation(self.manager)
        result = self._run(self._powershell(), "-NoProfile", "-Command", activation)
        passing((result.returncode, _said(result)), describing=f"`{activation}`")

    def starts_automatically(self) -> Reported:
        """`AUTO_START` is the manager saying it starts the service at boot."""
        _, listing = self._sc("qc", self.name)
        start_type = self._field(listing, "START_TYPE")
        return Reported("AUTO_START" in start_type, f"START_TYPE : {start_type}")

    def switch_off(self) -> None:
        """`sc.exe config start= demand`, which the manager starts only when asked."""
        self._sc("config", self.name, "start=", "demand", check=True)

    def is_running(self) -> bool:
        """`RUNNING`, and a process the manager knows."""
        _, listing = self._sc("query", self.name)
        return "RUNNING" in self._field(listing, "STATE") and self.main_pid() > 0

    def main_pid(self) -> int:
        """The service's process, or zero while it has none."""
        _, listing = self._sc("queryex", self.name)
        pid = self._field(listing, "PID")
        return int(pid) if pid.isdigit() else 0

    def runs_as_the_service_user(self, pid: int) -> Reported:
        """The process's owner against the account the manager was told to run it as."""
        _, listing = self._sc("qc", self.name)
        account = self._field(listing, "SERVICE_START_NAME")
        owner = self._run(
            self._powershell(),
            "-NoProfile",
            "-Command",
            f'$owner = Get-CimInstance Win32_Process -Filter "ProcessId = {pid}" | '
            f"Invoke-CimMethod -MethodName GetOwner; "
            f'"$($owner.Domain)\\$($owner.User)"',
        ).stdout.strip()
        return Reported(
            bool(account) and owner.lower() == account.lower(),
            f"process {pid} runs as {owner or '(nobody)'}; the service is registered as {account}",
        )

    def end_abruptly(self, pid: int) -> None:
        """Terminate the process the way a crash does; it is another account's, so elevated."""
        result = self._run("taskkill.exe", "/F", "/PID", str(pid))
        passing((result.returncode, _said(result)), describing=f"ending process {pid} abruptly")

    def stop(self) -> None:
        """Ask the manager to stop the service."""
        self._sc("stop", self.name, check=True)

    def stopped_gracefully(self) -> Reported:
        """Both exit codes zero; a killed service reads `1067` while it is down."""
        _, listing = self._sc("query", self.name)
        codes = {
            field: self._field(listing, field).split()[:1]
            for field in ("WIN32_EXIT_CODE", "SERVICE_EXIT_CODE")
        }
        recorded = " ".join(f"{field}={' '.join(code)}" for field, code in codes.items())
        return Reported(all(code == ["0"] for code in codes.values()), recorded)

    def _unregister(self) -> None:
        """Stop and delete the service, and wait for the manager to forget it."""
        self._sc("stop", self.name)
        _eventually(
            self, "the service stopped before its removal", lambda: not self.is_running(), within=60
        )
        self._sc("delete", self.name)
        _eventually(self, "the service removed", lambda: not self.is_present(), within=60)

    def remove(self) -> None:
        """Unregister the service, then delete the root and, once empty, the journeys' parent."""
        try:
            self._unregister()
        finally:
            shutil.rmtree(self.root, ignore_errors=True)
            if WINDOWS_ROOT.is_dir() and not any(WINDOWS_ROOT.iterdir()):
                WINDOWS_ROOT.rmdir()

    def is_present(self) -> bool:
        """Whether the manager knows a service of this name at all."""
        code, _ = self._sc("query", self.name)
        return code == 0

    def leftovers(self) -> list[str]:
        """A registered service, and the journey's root."""
        present = [f"the service {self.name}"] if self.is_present() else []
        return [*present, *([str(self.root)] if self.root.exists() else [])]

    def diagnosis(self) -> str:
        """`sc.exe queryex` and `sc.exe qc`."""
        return "\n".join(self._sc(verb, self.name)[1] for verb in ("queryex", "qc"))


def _activation(manager: ServiceManager) -> str:
    """The operator's own second command for one manager, as the install path states it.

    Run verbatim, because running what the operator is told to run is the
    point; held first to the shape `just check-repo` holds that command to, so
    that what reaches a shell is a command of that manager naming a service.
    """
    path = ip.parse((REPO_ROOT / "AGENTS.md").read_text(encoding="utf-8"))
    pair = path.commands_for(manager.value)
    equal(len(pair), 2, describing=f"the {manager.value} pair the install path states")
    truth(
        ACTIVATION_NAMES[manager].search(pair[1]) is not None,
        describing=f"`{pair[1]}` to be a {manager.value} activation command naming a service",
    )
    return pair[1]


def _service_name(manager: ServiceManager) -> str:
    """The service's name, read off the activation command's own naming match."""
    match = ACTIVATION_NAMES[manager].search(_activation(manager))
    truth(match is not None, describing=f"the {manager.value} activation command naming a service")
    return match["service"] if match is not None else ""


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
    return REPO_ROOT / "target" / "debug" / HERE.program


def _adapters() -> list[str]:
    """The adapters this host walks: its own manager's, and on Linux launchd's stand-in too."""
    if sys.platform == "win32":
        return [ServiceManager.WINDOWS_SERVICE.value]
    if HERE.service_manager == ServiceManager.LAUNCHD:
        return [ServiceManager.LAUNCHD.value]
    # llmlint: ignore[e2e_not_mocked] suppressions.toml has the reason.
    return [ServiceManager.SYSTEMD.value, "launchd-stand-in"]


@pytest.fixture(params=_adapters())
def manager(
    request: pytest.FixtureRequest,
    program: Path,
    tmp_path: Path,
    monkeypatch: pytest.MonkeyPatch,
) -> Manager:
    """One adapter this host walks the journey through."""
    match request.param:
        case ServiceManager.WINDOWS_SERVICE:
            return WindowsService(_service_name(ServiceManager.WINDOWS_SERVICE), program)
        case ServiceManager.SYSTEMD:
            return Systemd(_service_name(ServiceManager.SYSTEMD), program)
        case ServiceManager.LAUNCHD:
            return Launchd(program, None)
        case _:
            # llmlint: ignore[e2e_not_mocked] suppressions.toml has the reason.
            return Launchd(program, _stand_in_launchd(tmp_path, monkeypatch))


def _fill_in(manager: Manager, installed: Installed, octoprint: str) -> None:
    """Fill the template in as an operator would, take a free port, and install the skill."""
    template = manager.read(installed.configuration)
    truth(template is not None, describing="the installer to have written a configuration")
    filled = (
        (template or "")
        .replace('api_key = ""', 'api_key = "a-provisioned-key"')
        .replace('shared_secret = ""', 'shared_secret = "a-shared-secret"')
        # llmlint: ignore[e2e_not_mocked] suppressions.toml has the reason.
        .replace('url = "http://127.0.0.1:5000"', f'url = "{octoprint}"')
        .replace('listen = "127.0.0.1:8420"', 'listen = "127.0.0.1:0"')
    )
    manager.write(installed.configuration, filled)
    manager.lay_down_skill(installed)


def _where_it_serves(manager: Manager, state: PurePath) -> Served | None:
    """The address and credential the running service wrote for the clients beside it.

    `None` until the service has written them: the manager reports a process
    running from the moment it started it, and the process writes this file
    once it has bound its port and settled its credential.
    """
    text = manager.read(state / "client.toml")
    if text is None:
        return None
    try:
        written = tomllib.loads(text)
    except tomllib.TOMLDecodeError:
        return None
    table = written.get("client")
    server = table.get("server") if isinstance(table, dict) else None
    credential = table.get("credential") if isinstance(table, dict) else None
    if not isinstance(server, str) or not isinstance(credential, str) or not credential.strip():
        # The file is written in place, so a read can land between its lines.
        return None
    # The service writes a plain-HTTP address; anything else is not one `_ask` can dial.
    if not server.startswith("http://"):
        return None
    address = server.removeprefix("http://")
    hostname, _, port = address.rpartition(":")
    # `_fill_in` has the service listen on loopback, so that is the one host it may name.
    if hostname != "127.0.0.1" or not port.isdigit() or not 0 < int(port) < 65536:
        return None
    return Served(address, credential)


def _ask(served: Served) -> str:
    """One question to the API, written out over a socket, and the whole answer."""
    hostname, _, port = served.address.rpartition(":")
    # llmlint: ignore[async_typed_clients_at_boundaries] suppressions.toml has the reason.
    with socket.create_connection((hostname, int(port)), timeout=10) as stream:
        stream.sendall(
            (
                f"GET {QUESTION} HTTP/1.1\r\nHost: {served.address}\r\n"
                f"Accept: application/json\r\nAuthorization: Bearer {served.credential}\r\n"
                f"Connection: close\r\n\r\n"
            ).encode()
        )
        answer = b""
        while chunk := stream.recv(65536):
            answer += chunk
    return answer.decode(errors="replace")


def _status_line(answer: str) -> str:
    """An HTTP answer's first line, which alone says whether the request succeeded."""
    return answer.partition("\r\n")[0]


def _answered(manager: Manager, state: PurePath, *, not_by: str | None = None) -> Answer:
    """The API's answer, and the address it came from, once the service answers.

    A service the manager has just started, or just brought back, takes a
    moment to bind a port of the operating system's choosing and write where it
    is; until then the address on record is the last process's and nothing
    answers there. `not_by` is that last address, so that an answer is one the
    new process gave rather than one the file still described.
    """
    latest: list[Answer] = []

    def answers() -> bool:
        served = _where_it_serves(manager, state)
        if served is None or served.address == not_by:
            return False
        try:
            answer = _ask(served)
        except OSError:
            return False
        latest.append(Answer(answer, served.address))
        return _status_line(answer).startswith("HTTP/1.1 200 ")

    _eventually(manager, "a service answering its API", answers)
    return latest[-1]


def _holds(reported: Reported, describing: str) -> None:
    """Fail naming what the manager reported, unless it holds."""
    truth(reported.held, describing=f"{describing}; the manager reported {reported.recorded}")


@dataclass(frozen=True, slots=True)
class Activated:
    """The service, installed and activated, as the two journeys receive it."""

    manager: Manager
    installed: Installed


@pytest.fixture
def activated(manager: Manager, octoprint: str) -> Iterator[Activated]:
    """The service installed, filled in, and activated by the documented command.

    Torn down on every exit path, and the host then held to carrying nothing of
    the journey, whether or not an assertion held.
    """
    _required()
    manager.prepare()
    try:
        truth(not manager.is_present(), describing="the host to carry no service of this name")
        installed = manager.install()
        truth(not manager.is_running(), describing="the installer to have started nothing")
        _fill_in(manager, installed, octoprint)
        manager.activate()
        yield Activated(manager, installed)
    finally:
        manager.remove()
    equal(manager.leftovers(), [], describing="what the journey left on the host once removed")


# llmlint: ignore[expensive_tests_stay_behind_their_own_edge] suppressions.toml has the reason.
def test_activated_by_the_documented_command_it_runs_answers_and_stops_cleanly(
    activated: Activated,
) -> None:
    """Started through the manager as the service's user, answering, and stopped cleanly."""
    manager = activated.manager

    _eventually(manager, "the service running", manager.is_running)
    answer = _answered(manager, activated.installed.state)
    truth(
        _status_line(answer.text).startswith("HTTP/1.1 200 "),
        describing=f"the running service's API answering:\n{answer.text}",
    )
    contains(answer.text, "application/json", describing="the answer's type")
    _holds(
        manager.runs_as_the_service_user(manager.main_pid()),
        "the service running as the user it is installed to run as",
    )

    manager.stop()
    _eventually(manager, "the service stopped", lambda: not manager.is_running())
    _holds(manager.stopped_gracefully(), "a graceful stop recorded by the manager")


# llmlint: ignore[expensive_tests_stay_behind_their_own_edge] suppressions.toml has the reason.
def test_activated_it_starts_automatically_and_comes_back_after_an_abrupt_end(
    activated: Activated,
) -> None:
    """The manager reports it starts the service by itself, and brings it back after a crash."""
    manager = activated.manager

    _holds(
        manager.starts_automatically(),
        "the manager reporting the service as one it starts automatically",
    )
    _eventually(manager, "the service running", manager.is_running)
    first = _answered(manager, activated.installed.state)
    before = manager.main_pid()
    truth(before > 0, describing="a process the manager reports as the service's")

    manager.end_abruptly(before)

    _eventually(
        manager,
        "the service brought back with a new process",
        lambda: manager.is_running() and manager.main_pid() not in {0, before},
    )
    again = _answered(manager, activated.installed.state, not_by=first.address)
    truth(
        _status_line(again.text).startswith("HTTP/1.1 200 "),
        describing=f"the API answering again from the brought-back process:\n{again.text}",
    )
    truth(
        again.address != first.address,
        describing="the brought-back service serving on a port of its own",
    )
    _holds(
        manager.runs_as_the_service_user(manager.main_pid()),
        "the brought-back service running as the user it is installed to run as",
    )

    manager.switch_off()
    truth(
        not manager.starts_automatically().held,
        describing="the manager no longer reporting a service switched off as one it starts",
    )


@dataclass(frozen=True, slots=True)
class Prerequisite:
    """The program a host-backed case needs, and the words its absence is reported in."""

    program: str
    named: str


PREREQUISITES = {
    ServiceManager.SYSTEMD.value: Prerequisite("docker", "no `docker` that answers"),
    ServiceManager.LAUNCHD.value: Prerequisite("sudo", "needs password-free `sudo`"),
}


# llmlint: ignore[expensive_tests_stay_behind_their_own_edge] suppressions.toml has the reason.
@pytest.mark.parametrize("adapter", [name for name in _adapters() if name in PREREQUISITES])
def test_under_the_gates_setting_a_case_it_cannot_run_fails_rather_than_skipping(
    adapter: str, tmp_path: Path
) -> None:
    """A case whose prerequisite does not answer: failed under the gate's setting, else skipped.

    A setting that is neither unset nor the gate's own fails before that question arises.
    """
    workflow = yaml.safe_load(GATE_WORKFLOW.read_text(encoding="utf-8"))
    setting = workflow["jobs"]["gate"].get("env", {}).get(REQUIRED)
    equal(setting, REQUIRED_VALUE, describing=f"the gate job's `{REQUIRED}` in {GATE_WORKFLOW}")
    prerequisite = PREREQUISITES[adapter]
    program = prerequisite.program
    unanswering = tmp_path / "bin"
    unanswering.mkdir()
    # llmlint: ignore[e2e_not_mocked] suppressions.toml has the reason.
    (unanswering / program).write_text("#!/bin/sh\nexit 1\n", encoding="utf-8")
    (unanswering / program).chmod(0o755)
    case = [sys.executable, "-m", "pytest", "-p", "no:cacheprovider", "-rs", "-q", __file__]
    case += ["-k", f"{adapter}] and documented_command"]
    path = f"{unanswering}{os.pathsep}{os.environ['PATH']}"
    ungated = clean_environment(PATH=path)
    ungated.pop(REQUIRED, None)
    misspelt = "requried"
    for environment, fails, fragments in (
        (
            clean_environment(PATH=path, **{REQUIRED: str(setting)}),
            True,
            (prerequisite.named, "so this case fails rather than skips"),
        ),
        (ungated, False, (prerequisite.named, "SKIPPED")),
        # A misspelt setting is refused before any prerequisite is looked at.
        (
            clean_environment(PATH=path, **{REQUIRED: misspelt}),
            True,
            (f"{REQUIRED}={misspelt!r} is neither unset nor {REQUIRED_VALUE!r}",),
        ),
    ):
        # llmlint: ignore[async_typed_clients_at_boundaries] suppressions.toml has the reason.
        ran = shell_run(case, cwd=REPO_ROOT, env=environment, timeout=BUILD_TIMEOUT_SECONDS)
        said = plain(_said(ran))
        equal(ran.returncode != 0, fails, describing=f"whether the case failed:\n{said}")
        for fragment in fragments:
            contains(said, fragment, describing=f"how the case ended:\n{said}")


def _stand_in_launchctl(*arguments: str) -> subprocess.CompletedProcess[str]:
    """One `launchctl` command, answered by the stand-in `_stand_in_launchd` put on `PATH`."""
    # llmlint: ignore[async_typed_clients_at_boundaries] suppressions.toml has the reason.
    return shell_run(["launchctl", *arguments], timeout=60)


@pytest.mark.skipif(
    "launchd-stand-in" not in _adapters(), reason="the launchctl stand-in runs on Linux alone"
)
def test_the_launchctl_stand_in_refuses_a_relative_property_list_path(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    """A relative `bootstrap` path is refused with launchd's usage exit, and nothing is loaded.

    The property list it names is a loadable one beneath the stand-in's root, so
    what is refused is the path's shape rather than what it points at.
    """
    stand_in = _stand_in_launchd(tmp_path, monkeypatch)
    label = "io.github.nickderobertis.printobserver.relative"
    relative = f"Library/LaunchDaemons/{label}.plist"
    (stand_in.root / relative).parent.mkdir(parents=True)
    with (stand_in.root / relative).open("wb") as handle:
        plistlib.dump(
            {
                "Label": label,
                "ProgramArguments": ["/bin/sleep", "60"],
                "UserName": getpass.getuser(),
                "RunAtLoad": False,
            },
            handle,
        )

    refused = _stand_in_launchctl("bootstrap", "system", relative)

    try:
        equal(refused.returncode, launchctl_standin.USAGE, describing=_said(refused))
        contains(_said(refused), f"{relative} is not an absolute path", describing="the refusal")
        printed = _stand_in_launchctl("print", f"system/{label}")
        equal(
            printed.returncode,
            launchctl_standin.NOT_FOUND,
            describing=f"the stand-in to have loaded nothing:\n{_said(printed)}",
        )
    finally:
        _stand_in_launchctl("bootout", f"system/{label}")


@pytest.mark.skipif(
    "launchd-stand-in" not in _adapters(), reason="the launchctl stand-in runs on Linux alone"
)
def test_the_launchctl_stand_in_refuses_a_corrupt_disabled_record_and_a_foreign_job(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    """A disabled record it cannot read, and a job naming another user, are refused, not guessed.

    A corrupt record read as empty would report a switched-off service as one
    launchd starts; a job for another user started through `--supervise` would
    skip the refusal `bootstrap` makes.
    """
    stand_in = _stand_in_launchd(tmp_path, monkeypatch)
    stand_in.state.mkdir()
    # No `launchctl` verb writes a corrupt record, so the state is put there directly.
    # llmlint: ignore[tests_mirror_real_usage] suppressions.toml has the reason.
    (stand_in.state / launchctl_standin.DISABLED).write_text('{"not": "a list"}', encoding="utf-8")
    listed = _stand_in_launchctl("print-disabled", "system")
    equal(listed.returncode, launchctl_standin.INPUT_OUTPUT_ERROR, describing=_said(listed))
    absent(listed.stdout, "disabled services", describing="the answer to a corrupt record")

    # A state directory that is a file is refused before anything is recorded in it.
    unusable = tmp_path / "not-a-directory"
    unusable.write_text("", encoding="utf-8")
    monkeypatch.setenv(launchctl_standin.STATE, str(unusable))
    refused = _stand_in_launchctl("print-disabled", "system")
    equal(refused.returncode, launchctl_standin.INPUT_OUTPUT_ERROR, describing=_said(refused))
    contains(_said(refused), "is not a directory it can use", describing="the refusal")
    monkeypatch.setenv(launchctl_standin.STATE, str(stand_in.state))

    given = "/Library/LaunchDaemons/io.github.nickderobertis.printobserver.foreign.plist"
    written = stand_in.root / given.lstrip("/")
    written.parent.mkdir(parents=True)
    with written.open("wb") as handle:
        plistlib.dump(
            {
                "Label": "io.github.nickderobertis.printobserver.foreign",
                "ProgramArguments": ["/bin/sleep", "60"],
                "UserName": f"not-{getpass.getuser()}",
                "RunAtLoad": True,
            },
            handle,
        )
    # `bootstrap` refuses this job before it reaches a supervisor (the test below
    # proves it), so the supervisor's own refusal is reached by starting one.
    # llmlint: ignore[tests_mirror_real_usage] suppressions.toml has the reason.
    supervised = _stand_in_launchctl("--supervise", given, str(written.resolve()))
    equal(supervised.returncode, launchctl_standin.INPUT_OUTPUT_ERROR, describing=_said(supervised))
    contains(_said(supervised), "not as this user", describing="the refusal")


@pytest.mark.skipif(
    "launchd-stand-in" not in _adapters(), reason="the launchctl stand-in runs on Linux alone"
)
def test_the_launchctl_stand_in_refuses_what_it_does_not_implement(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    """A verb it lacks, and a property list it would not honour whole, are refused and load nothing.

    Each list is otherwise loadable, so what is refused is the one thing wrong with it.
    """
    stand_in = _stand_in_launchd(tmp_path, monkeypatch)
    unknown = _stand_in_launchctl("list")
    equal(unknown.returncode, launchctl_standin.USAGE, describing=_said(unknown))
    contains(_said(unknown), "Unrecognized subcommand: list", describing="the refusal")

    loadable: dict[str, object] = {
        "ProgramArguments": ["/bin/sleep", "60"],
        "UserName": getpass.getuser(),
        "RunAtLoad": False,
    }
    for name, departure in (
        ("unimplemented-key", {"Sockets": {}}),
        ("unimplemented-keep-alive", {"KeepAlive": {"Crashed": True}}),
        ("boolean-throttle", {"ThrottleInterval": True}),
        ("foreign-user", {"UserName": f"not-{getpass.getuser()}"}),
    ):
        label = f"io.github.nickderobertis.printobserver.{name}"
        given = f"/Library/LaunchDaemons/{label}.plist"
        written = stand_in.root / given.lstrip("/")
        written.parent.mkdir(parents=True, exist_ok=True)
        with written.open("wb") as handle:
            plistlib.dump({"Label": label, **loadable, **departure}, handle)
        try:
            refused = _stand_in_launchctl("bootstrap", "system", given)
            equal(
                refused.returncode,
                launchctl_standin.INPUT_OUTPUT_ERROR,
                describing=f"bootstrapping a {name} list:\n{_said(refused)}",
            )
            printed = _stand_in_launchctl("print", f"system/{label}")
            equal(
                printed.returncode,
                launchctl_standin.NOT_FOUND,
                describing=f"nothing loaded from a {name} list:\n{_said(printed)}",
            )
        finally:
            _stand_in_launchctl("bootout", f"system/{label}")

    malformed = stand_in.root / "Library/LaunchDaemons/malformed.plist"
    malformed.write_text("<plist><dict><key>Label</key>", encoding="utf-8")
    refused = _stand_in_launchctl("bootstrap", "system", "/Library/LaunchDaemons/malformed.plist")
    equal(refused.returncode, launchctl_standin.INPUT_OUTPUT_ERROR, describing=_said(refused))


def test_a_disabled_listing_is_read_only_when_it_is_one() -> None:
    """The answer to `print-disabled` is read as a listing, and anything else as none."""
    listing = (
        "disabled services = {\n"
        '\t"com.example.off" => disabled\n'
        '\t"com.example.on" => enabled\n'
        '\t"com.example.older" => true\n'
        "}\n"
        "login item associations = {\n}\n"
    )
    equal(
        _disabled_services(listing),
        {"com.example.off", "com.example.older"},
        describing="the labels a well-formed listing switches off",
    )
    for malformed in (
        "",
        "Unrecognized subcommand: print-disabled\n",
        'disabled services = {\n\t"com.example.off" => disabled\n',
        "disabled services = {\n\tcom.example.off => disabled\n}\n",
        'disabled services = {\n\t"com.example.off" => maybe\n}\n',
    ):
        equal(
            _disabled_services(malformed),
            None,
            describing=f"no listing read out of {malformed!r}",
        )
