"""The installed service, unattended: activated, answering, brought back, torn down.

What makes `printobserver` a product rather than a program is that it comes
back after a reboot and keeps running after a crash, and neither is something a
file can prove. This journey proves both through the service manager itself, on
whichever manager this host runs: it installs the service with the committed
installer for this platform's own service manager into a root of its own, fills
the configuration in as an operator would, activates it with the operator's own
documented command — read out of `AGENTS.md`'s install-path section for that
manager, never restated — and then reads the manager's own answers back.

The journey is one journey over the service managers the supported-platform list
names, and the manager is the one part of it that differs: a `systemd` back end
on Linux, driving a real system unit through `systemctl`, and a `windows-service`
back end on Windows, driving the service control manager through `sc.exe`. The
sequence and every assertion are the same, so the sequence is proven for real
wherever the tier runs and only the manager's vocabulary is left to each
platform's own cell.

Two things it never does. It never touches a service somebody else installed:
a service of this name that is not a leftover of this journey's own is a host
the journey skips, naming it. And it never leaves the host other than it found
it: the service is removed and the root deleted on every exit path, so a failing
assertion is not also a stranded service.
"""

from __future__ import annotations

import os
import shutil
import signal
import socket
import sys
import threading
import time
import tomllib
from collections.abc import Callable, Iterator
from dataclasses import dataclass
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from pathlib import Path
from typing import Protocol

import pytest
from journey import HERE, REPO_ROOT, clean_environment, run
from repo_checks import install_path as ip
from repo_checks.checks_service import ACTIVATION_NAMES
from repo_checks.expect import contains, equal, passing, truth
from repo_checks.platforms import ServiceManager
from repo_checks.shell import run as shell_run

#: The service's name on each manager, read off the activation command the
#: install path states, so a rename there is a rename here.
NAME_FIELD = {
    ServiceManager.SYSTEMD: "--now",
    ServiceManager.WINDOWS_SERVICE: "-Name",
}

#: Where a journey's root goes: a directory a sandboxed system service can see
#: (a systemd unit hides every home and every temporary directory from the
#: service) and a virtual account can traverse. Named after this journey so
#: that a leftover is recognisable as one.
ROOTS = {
    ServiceManager.SYSTEMD: Path("/var/lib/printobserver-journeys"),
    ServiceManager.WINDOWS_SERVICE: Path(r"C:\ProgramData\printobserver-journeys"),
}

#: How long the program build is given the first time this tier runs.
BUILD_TIMEOUT_SECONDS = 2400

#: How long a manager is given to report a state it was asked for.
WITHIN_SECONDS = 90

#: One state the API is asked for.
QUESTION = "/v1/prints"


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


def _said(result: object) -> str:
    """Everything one run said, on either stream."""
    out = getattr(result, "stdout", "") or ""
    err = getattr(result, "stderr", "") or ""
    return f"{out}{err}"


def _eventually(
    what: str, condition: Callable[[], bool], *, within: float = WITHIN_SECONDS
) -> None:
    """Wait for the manager to report `what`, and fail naming it when it does not.

    Raises:
        AssertionError: If `condition()` is still false after `within` seconds.
    """
    started = time.monotonic()
    while True:
        if condition():
            return
        if time.monotonic() - started > within:
            message = f"the service manager did not report {what} within {within} seconds"
            raise AssertionError(message)
        time.sleep(0.5)


@dataclass(frozen=True, slots=True)
class Installed:
    """What the installer put in place beneath the journey's root."""

    root: Path
    binary: Path
    configuration: Path
    state: Path


@dataclass(frozen=True, slots=True)
class Stopped:
    """How the manager recorded a stop."""

    #: Whether it recorded a clean exit.
    clean: bool
    #: What it recorded, in its own words, for a reader of a failure.
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
    name: str

    def preflight(self) -> None:
        """Refuse or clean up before anything is installed; skip a host this must not touch."""

    def install(self, root: Path, program: Path) -> Installed:
        """Run the committed installer for this manager against `root`."""

    def activate(self, installed: Installed) -> None:
        """Run the operator's own documented second command."""

    def starts_automatically(self) -> bool:
        """Whether the manager reports the service as one it starts by itself."""

    def is_running(self) -> bool:
        """Whether the manager reports the service running."""

    def main_pid(self) -> int:
        """The process the manager reports as the service's."""

    def end_abruptly(self, pid: int) -> None:
        """End the service's process the way a crash does."""

    def stop(self) -> None:
        """Ask the manager to stop the service."""

    def stopped_gracefully(self) -> Stopped:
        """Whether the manager recorded a clean exit, and what it recorded."""

    def remove(self, installed: Installed | None) -> None:
        """Remove the service and the root, leaving the host as it was found."""

    def is_present(self) -> bool:
        """Whether the manager knows the service at all."""


class Systemd:
    """The `systemd` back end: a real system unit, through `systemctl` under `sudo`."""

    manager = ServiceManager.SYSTEMD

    def __init__(self, name: str) -> None:
        """A back end over the unit `name`."""
        self.name = name

    # llmlint: ignore[async_typed_clients_at_boundaries] suppressions.toml has the reason.
    def _systemctl(self, *arguments: str, check: bool = False) -> str:
        result = shell_run(["sudo", "-n", "systemctl", *arguments], timeout=120)
        if check:
            passing(
                (result.returncode, _said(result)),
                describing=f"`systemctl {' '.join(arguments)}`",
            )
        return _said(result)

    def _show(self, prop: str) -> str:
        return self._systemctl("show", "-p", prop, "--value", self.name).strip()

    def preflight(self) -> None:
        """Need password-free sudo; leave a real installation alone; clear a leftover."""
        if shell_run(["sudo", "-n", "true"], timeout=30).returncode != 0:
            message = (
                "this journey activates a real systemd unit and needs password-free `sudo`, "
                "which this host does not grant"
            )
            raise AssertionError(message)
        fragment = self._show("FragmentPath")
        if fragment and not fragment.startswith(str(ROOTS[self.manager])):
            pytest.skip(
                f"this host already has a `{self.name}` at {fragment}, which is not this "
                f"journey's own, and the journey will not touch a service somebody installed"
            )
        if fragment:
            self.remove(None)

    def install(self, root: Path, program: Path) -> Installed:
        """The committed shell installer, as the invoking user, into `root`."""
        shell_run(["sudo", "-n", "mkdir", "-p", str(root)], check=True, timeout=60)
        # `_manager` hands this back end no Windows host; the check says so to a
        # type checker reading this file for Windows, where `getuid` is absent.
        if sys.platform == "win32":
            message = "the systemd back end was handed a Windows host"
            raise AssertionError(message)
        shell_run(["sudo", "-n", "chown", str(os.getuid()), str(root)], check=True, timeout=60)
        result = shell_run(
            [
                "sh",
                str(REPO_ROOT / "scripts" / "install-service.sh"),
                "--root",
                str(root),
                "--binary",
                str(program),
                "--user",
                os.environ.get("USER") or shell_run(["id", "-un"], timeout=30).stdout.strip(),
            ],
            cwd=root,
            env=clean_environment(),
            timeout=300,
        )
        passing((result.returncode, _said(result)), describing="the committed installer")
        return Installed(
            root,
            root / "usr/local/lib/printobserver/printobserver",
            root / "etc/printobserver/config.toml",
            root / "var/lib/printobserver",
        )

    def activate(self, installed: Installed) -> None:
        """Link the unit the installer wrote, then run the documented command verbatim."""
        unit = installed.root / "etc/systemd/system" / self.name
        self._systemctl("link", str(unit), check=True)
        activation = _activation(self.manager)
        words = activation.split()
        equal(words[:1], ["sudo"], describing="the documented command running as root")
        result = shell_run([words[0], "-n", *words[1:]], timeout=120)
        passing((result.returncode, _said(result)), describing=f"`{activation}`")

    def starts_automatically(self) -> bool:
        """`enabled` is the manager saying it starts the unit at boot."""
        return self._systemctl("is-enabled", self.name).strip() == "enabled"

    def is_running(self) -> bool:
        """`active`, and a main process the manager knows."""
        return self._systemctl("is-active", self.name).strip() == "active" and self.main_pid() > 0

    def main_pid(self) -> int:
        """The unit's main process, or zero while it has none."""
        shown = self._show("MainPID")
        return int(shown) if shown.isdigit() else 0

    def end_abruptly(self, pid: int) -> None:
        """A kill the process cannot answer, as a crash is: it runs as this user."""
        if sys.platform == "win32":
            message = "the systemd back end was handed a Windows host"
            raise AssertionError(message)
        os.kill(pid, signal.SIGKILL)

    def stop(self) -> None:
        """Ask the manager to stop the unit."""
        self._systemctl("stop", self.name, check=True)

    def stopped_gracefully(self) -> Stopped:
        """`Result=success` with an exit status of zero; a killed unit reads `signal`."""
        recorded = (
            f"Result={self._show('Result')} ExecMainCode={self._show('ExecMainCode')} "
            f"ExecMainStatus={self._show('ExecMainStatus')}"
        )
        return Stopped(recorded == "Result=success ExecMainCode=1 ExecMainStatus=0", recorded)

    def remove(self, installed: Installed | None) -> None:
        """Disable and unlink the unit, forget its state, and delete the root."""
        self._systemctl("disable", "--now", self.name)
        self._systemctl("reset-failed", self.name)
        self._systemctl("daemon-reload")
        target = installed.root if installed is not None else ROOTS[self.manager]
        shell_run(["sudo", "-n", "rm", "-rf", str(target)], timeout=120)
        # The journeys' own parent, gone too once nothing is left in it.
        shell_run(
            ["sudo", "-n", "rmdir", "--ignore-fail-on-non-empty", str(ROOTS[self.manager])],
            timeout=60,
        )

    def is_present(self) -> bool:
        """Whether the manager loads a unit of this name at all."""
        return self._show("LoadState") not in {"not-found", ""}


class WindowsService:
    """The `windows-service` back end: the service control manager, through `sc.exe`."""

    manager = ServiceManager.WINDOWS_SERVICE

    def __init__(self, name: str) -> None:
        """A back end over the service `name`."""
        self.name = name

    @staticmethod
    def _powershell() -> str:
        for candidate in ("pwsh", "powershell"):
            found = shutil.which(candidate)
            if found:
                return found
        message = "this host has neither `pwsh` nor `powershell` on PATH"
        raise AssertionError(message)

    # llmlint: ignore[async_typed_clients_at_boundaries] suppressions.toml has the reason.
    def _sc(self, *arguments: str, check: bool = False) -> tuple[int, str]:
        result = shell_run(["sc.exe", *arguments], timeout=120)
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

    def preflight(self) -> None:
        """Leave a real installation alone; clear a leftover of this journey's own."""
        code, listing = self._sc("qc", self.name)
        if code != 0:
            return
        binary = self._field(listing, "BINARY_PATH_NAME")
        if str(ROOTS[self.manager]).lower() not in binary.lower():
            pytest.skip(
                f"this host already has a `{self.name}` service running {binary}, which is "
                f"not this journey's own, and the journey will not touch a service somebody "
                f"installed"
            )
        self.remove(None)

    def install(self, root: Path, program: Path) -> Installed:
        """The committed PowerShell installer into `root`."""
        root.mkdir(parents=True, exist_ok=True)
        result = shell_run(
            [
                self._powershell(),
                "-NoProfile",
                "-File",
                str(REPO_ROOT / "scripts" / "install-service.ps1"),
                "-Root",
                str(root),
                "-Binary",
                str(program),
            ],
            cwd=root,
            env=clean_environment(),
            timeout=300,
        )
        passing((result.returncode, _said(result)), describing="the committed installer")
        return Installed(
            root,
            root / "Program Files" / "printobserver" / "printobserver.exe",
            root / "ProgramData" / "printobserver" / "config.toml",
            root / "ProgramData" / "printobserver" / "state",
        )

    def activate(self, installed: Installed) -> None:
        """Run the documented command verbatim, in PowerShell."""
        activation = _activation(self.manager)
        result = shell_run([self._powershell(), "-NoProfile", "-Command", activation], timeout=120)
        passing((result.returncode, _said(result)), describing=f"`{activation}`")

    def starts_automatically(self) -> bool:
        """`AUTO_START` is the manager saying it starts the service at boot."""
        _, listing = self._sc("qc", self.name)
        return "AUTO_START" in self._field(listing, "START_TYPE")

    def is_running(self) -> bool:
        """`RUNNING`, and a process the manager knows."""
        _, listing = self._sc("query", self.name)
        return "RUNNING" in self._field(listing, "STATE") and self.main_pid() > 0

    def main_pid(self) -> int:
        """The service's process, or zero while it has none."""
        _, listing = self._sc("queryex", self.name)
        pid = self._field(listing, "PID")
        return int(pid) if pid.isdigit() else 0

    def end_abruptly(self, pid: int) -> None:
        """Terminate the process the way a crash does; it is another account's, so elevated."""
        result = shell_run(["taskkill.exe", "/F", "/PID", str(pid)], timeout=60)
        passing((result.returncode, _said(result)), describing=f"ending process {pid} abruptly")

    def stop(self) -> None:
        """Ask the manager to stop the service."""
        self._sc("stop", self.name, check=True)

    def stopped_gracefully(self) -> Stopped:
        """Both exit codes zero; a killed service reads `1067` while it is down."""
        _, listing = self._sc("query", self.name)
        codes = {
            field: self._field(listing, field).split()[:1]
            for field in ("WIN32_EXIT_CODE", "SERVICE_EXIT_CODE")
        }
        recorded = " ".join(f"{field}={' '.join(code)}" for field, code in codes.items())
        return Stopped(all(code == ["0"] for code in codes.values()), recorded)

    def remove(self, installed: Installed | None) -> None:
        """Stop and delete the service, wait for the manager to forget it, delete the root."""
        self._sc("stop", self.name)
        _eventually(
            "the service stopped before its removal",
            lambda: not self.is_running(),
            within=60,
        )
        self._sc("delete", self.name)
        _eventually("the service removed", lambda: not self.is_present(), within=60)
        target = installed.root if installed is not None else ROOTS[self.manager]
        shutil.rmtree(target, ignore_errors=True)
        # The journeys' own parent, gone too once nothing is left in it.
        if ROOTS[self.manager].is_dir() and not any(ROOTS[self.manager].iterdir()):
            ROOTS[self.manager].rmdir()

    def is_present(self) -> bool:
        """Whether the manager knows a service of this name at all."""
        code, _ = self._sc("query", self.name)
        return code == 0


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
    """The service's name, read off the activation command's own naming option."""
    words = _activation(manager).split()
    field = NAME_FIELD[manager]
    truth(field in words, describing=f"`{field}` in the {manager.value} activation command")
    return words[words.index(field) + 1]


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


@pytest.fixture
def manager() -> Manager:
    """This host's own service manager, or a skip naming the back end still owed."""
    if sys.platform == "win32":
        truth(
            HERE.service_manager == ServiceManager.WINDOWS_SERVICE,
            describing="a Windows host to be a windows-service platform",
        )
        return WindowsService(_service_name(ServiceManager.WINDOWS_SERVICE))
    if HERE.service_manager == ServiceManager.SYSTEMD:
        return Systemd(_service_name(ServiceManager.SYSTEMD))
    pytest.skip(
        f"this host's service manager is `{HERE.service_manager}`, and the install path "
        f"states no pair for it yet; its back end comes with its platform node"
    )


def _fill_in(configuration: Path, octoprint: str) -> None:
    """Fill the template in exactly as an operator would, and take a free port."""
    filled = (
        configuration.read_text(encoding="utf-8")
        .replace('api_key = ""', 'api_key = "a-provisioned-key"')
        .replace('shared_secret = ""', 'shared_secret = "a-shared-secret"')
        .replace('url = "http://127.0.0.1:5000"', f'url = "{octoprint}"')
        .replace('listen = "127.0.0.1:8420"', 'listen = "127.0.0.1:0"')
    )
    configuration.write_text(filled, encoding="utf-8")


def _where_it_serves(state: Path) -> Served | None:
    """The address and credential the running service wrote for the clients beside it.

    `None` until the service has written them: the manager reports a process
    running from the moment it started it, and the process writes this file
    once it has bound its port and settled its credential.
    """
    client = state / "client.toml"
    if not client.is_file():
        return None
    try:
        written = tomllib.loads(client.read_text(encoding="utf-8"))
    except tomllib.TOMLDecodeError, OSError:
        return None
    table = written.get("client")
    server = table.get("server") if isinstance(table, dict) else None
    credential = table.get("credential") if isinstance(table, dict) else None
    if not isinstance(server, str) or not isinstance(credential, str):
        # The file is written in place, so a read can land between its lines.
        return None
    return Served(server.removeprefix("http://"), credential)


# llmlint: ignore[async_typed_clients_at_boundaries] suppressions.toml has the reason.
def _ask(served: Served) -> str:
    """One question to the API, written out over a socket, and the whole answer."""
    hostname, _, port = served.address.rpartition(":")
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


def _answered(state: Path, *, not_by: str | None = None) -> Answer:
    """The API's answer, and the address it came from, once the service answers.

    A service the manager has just started, or just brought back, takes a
    moment to bind a port of the operating system's choosing and write where it
    is; until then the address on record is the last process's and nothing
    answers there. `not_by` is that last address, so that an answer is one the
    new process gave rather than one the file still described.
    """
    latest: list[Answer] = []

    def answers() -> bool:
        served = _where_it_serves(state)
        if served is None or served.address == not_by:
            return False
        try:
            answer = _ask(served)
        except OSError:
            return False
        latest.append(Answer(answer, served.address))
        return "HTTP/1.1 200" in answer

    _eventually("a service answering its API", answers)
    return latest[-1]


@dataclass(frozen=True, slots=True)
class Activated:
    """The service, installed and activated, as the two journeys receive it."""

    manager: Manager
    installed: Installed


@pytest.fixture
def activated(manager: Manager, program: Path, octoprint: str) -> Iterator[Activated]:
    """The service installed into its own root, filled in, activated by the documented command.

    Removed on every exit path, so the host is left as it was found whether or
    not an assertion held.
    """
    manager.preflight()
    truth(not manager.is_present(), describing="the host to carry no service of this name")
    root = ROOTS[manager.manager] / f"journey-{os.getpid()}-{int(time.time())}"
    installed: Installed | None = None
    try:
        installed = manager.install(root, program)
        truth(not manager.is_running(), describing="the installer to have started nothing")
        _fill_in(installed.configuration, octoprint)
        manager.activate(installed)
        yield Activated(manager, installed)
    finally:
        manager.remove(installed)
        truth(
            not manager.is_present(),
            describing="the manager to have forgotten the service once removed",
        )
        truth(not root.exists(), describing="the root to be gone once removed")


def test_activated_by_the_documented_command_it_runs_answers_and_stops_cleanly(
    activated: Activated,
) -> None:
    """Started and stopped through the manager: running, answering the API, and a clean exit."""
    manager = activated.manager

    _eventually("the service running", manager.is_running)
    answer = _answered(activated.installed.state)
    contains(
        answer.text,
        "HTTP/1.1 200",
        describing=f"the running service's API answering:\n{answer.text}",
    )
    contains(answer.text, "application/json", describing="the answer's type")

    manager.stop()
    _eventually("the service stopped", lambda: not manager.is_running())
    stopped = manager.stopped_gracefully()
    truth(
        stopped.clean,
        describing=f"a graceful stop recorded by the manager; it recorded {stopped.recorded}",
    )


def test_activated_it_starts_automatically_and_comes_back_after_an_abrupt_end(
    activated: Activated,
) -> None:
    """The manager reports it starts the service by itself, and brings it back after a crash."""
    manager = activated.manager

    truth(
        manager.starts_automatically(),
        describing="the manager reporting the service as one it starts automatically",
    )
    _eventually("the service running", manager.is_running)
    first = _answered(activated.installed.state)
    before = manager.main_pid()
    truth(before > 0, describing="a process the manager reports as the service's")

    manager.end_abruptly(before)

    _eventually(
        "the service brought back with a new process",
        lambda: manager.is_running() and manager.main_pid() != before,
    )
    again = _answered(activated.installed.state, not_by=first.address)
    contains(
        again.text,
        "HTTP/1.1 200",
        describing="the API answering again from the process the manager brought back",
    )
    truth(
        again.address != first.address,
        describing="the brought-back service serving on a port of its own",
    )
