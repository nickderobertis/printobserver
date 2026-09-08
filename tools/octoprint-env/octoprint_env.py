#!/usr/bin/env python3
"""The scripted OctoPrint environment — one script, two modes, no container.

The printer integration tier drives a real OctoPrint, because a fake one would
prove a fake. The real printer is reached over a USB serial port, so this is a
script rather than a container: a containerized OctoPrint would be a second,
different installation beside the one that actually drives the machine.

    octoprint_env.py install   provision an instance under a state directory
    octoprint_env.py up        provision, start, and wait until it is usable
    octoprint_env.py down      stop it, leaving no process behind

Two modes, one flag apart. `--mode virtual` enables OctoPrint's own virtual
printer and connects to it, which is what the integration tier drives.
`--mode serial` connects to the device named by `--device` at the baud rate
named by `--baudrate`, which is what the real machine uses. Both modes compose
the same configuration and differ in the connection alone — the keys
`CONNECTION_KEYS` names — so the tier proves the shape the printer runs.

`up` answers on stdout with one JSON document naming the state directory, the
URL, the file it wrote the provisioned API key to, and the minimum it holds a
print for; progress goes to stderr. API authentication is left ENABLED and that
key is what the printer plugin authenticates with, so an instance that answered
without one would prove a configuration nobody runs.

Every way starting can fail is one of `FAILURE_CLASSES`, reported by name with
the next action to take. Anything outside that closed set is reported with the
underlying error's own text, because one diagnosed failure path and a bare
timeout everywhere else reads as diagnostics without being any.
"""

from __future__ import annotations

import argparse
import http.client
import json
import os
import secrets
import shutil
import signal
import socket
import subprocess
import sys
import time
import urllib.parse
from dataclasses import dataclass
from pathlib import Path
from typing import IO, Any

import yaml

# The OctoPrint release this environment provisions, and the interpreter it runs
# on. Both are pinned: an environment that resolved to whatever was newest would
# make the tier's failures depend on the day it ran. This repository's own
# Python is newer than anything OctoPrint supports, which is why the instance
# gets an interpreter and a virtual environment of its own.
OCTOPRINT_REQUIREMENT = "OctoPrint==1.11.8"
OCTOPRINT_PYTHON = "3.11"

# The account the provisioned API key belongs to. OctoPrint's global API key is
# deprecated and stops working in 1.13, so the key is a user key.
ACCOUNT = "printobserver"

HOST = "127.0.0.1"
VIRTUAL_DEVICE = "VIRTUAL"
DEFAULT_BAUDRATE = 115200
DEFAULT_STATE_DIR = ".octoprint-env"

# The minimum this script states the hold print keeps running for, measured from
# the moment `up` answers. The tier needs a print that is still there to be
# acted on after it has taken its own time to act; `gcode/hold.gcode` dwells for
# well over twice this, so the margin lives in the file rather than in the
# promise.
HOLD_SECONDS = 150

# The configuration keys the two modes differ in, and the only ones they may.
CONNECTION_KEYS: tuple[tuple[str, ...], ...] = (
    ("serial", "port"),
    ("serial", "baudrate"),
    ("serial", "additionalPorts"),
    ("plugins", "virtual_printer", "enabled"),
)

# Every way starting this environment can fail, and the next action for each.
# The set is closed: `up` reaches no other diagnosed exit, and anything outside
# it is reported with the underlying error's own text.
FAILURE_CLASSES: dict[str, str] = {
    "config-unreadable": (
        "read the file the message names, fix or delete it, and run `install` again — "
        "a state directory with no config.yaml is provisioned from scratch"
    ),
    "port-in-use": (
        "pass a different `--port`, or `--port auto` to be given a free one; "
        "`ss -ltnp` names what is holding the one that was asked for"
    ),
    "serial-device-unopenable": (
        "check the device path (`ls -l /dev/serial/by-id/`), that the printer is "
        "plugged in and powered, and that this user is in the `dialout` group"
    ),
    "never-answered": (
        "read the server log the message names: OctoPrint started but never answered "
        "its own API, so its own startup output is what says why"
    ),
    "printer-not-connected": (
        "read the log the message names; in `--mode serial` check that the device is "
        "a printer speaking at `--baudrate`, and in `--mode virtual` that the bundled "
        "virtual printer plugin is enabled"
    ),
    "print-not-running": (
        "read the server log the message names: the instance accepted the upload but "
        "did not report a running print, which the tier needs to act on"
    ),
}


class StartupError(Exception):
    """One of the declared failure classes, with what happened and what to do."""

    def __init__(self, failure_class: str, detail: str) -> None:
        """Name a declared class and say what happened.

        Args:
            failure_class: A key of `FAILURE_CLASSES`.
            detail: What happened, concretely enough to act on.

        Raises:
            KeyError: If the class is not one this script declares, so that an
                undeclared failure cannot be dressed up as a declared one.
        """
        self.failure_class = failure_class
        self.detail = detail
        self.next_action = FAILURE_CLASSES[failure_class]
        super().__init__(f"{failure_class}: {detail}")


@dataclass(frozen=True, slots=True)
class Connection:
    """The connection one mode configures."""

    mode: str
    device: str
    baudrate: int

    @property
    def is_virtual(self) -> bool:
        """Whether this is the virtual printer rather than a serial device."""
        return self.mode == "virtual"


class Instance:
    """One OctoPrint instance under one state directory."""

    def __init__(self, state_dir: Path, connection: Connection) -> None:
        """Bind to the state directory this instance lives under."""
        self.state_dir = state_dir.resolve()
        self.connection = connection

    @property
    def venv(self) -> Path:
        """The virtual environment OctoPrint itself is installed into."""
        return self.state_dir / "venv"

    @property
    def executable(self) -> Path:
        """The `octoprint` program of that environment."""
        return self.venv / "bin" / "octoprint"

    @property
    def basedir(self) -> Path:
        """OctoPrint's own base directory: config, uploads, logs."""
        return self.state_dir / "instance"

    @property
    def config_file(self) -> Path:
        """The configuration file OctoPrint reads and rewrites."""
        return self.basedir / "config.yaml"

    @property
    def users_file(self) -> Path:
        """The account file the provisioned API key is stored in."""
        return self.basedir / "users.yaml"

    @property
    def api_key_file(self) -> Path:
        """The file the provisioned API key is written to for a caller to read."""
        return self.state_dir / "api-key"

    @property
    def account_password_file(self) -> Path:
        """The file the account's generated password is written to."""
        return self.state_dir / "account-password"

    @property
    def server_log(self) -> Path:
        """Everything the server process itself said."""
        return self.state_dir / "server.log"

    @property
    def octoprint_log(self) -> Path:
        """OctoPrint's own log, which is where the connection it opened is recorded."""
        return self.basedir / "logs" / "octoprint.log"

    @property
    def record(self) -> Path:
        """What `up` wrote about the running instance, for `down` and for callers."""
        return self.state_dir / "instance.json"


def note(message: str) -> None:
    """Say what is happening, on the stream the JSON answer is not on."""
    print(f"octoprint-env: {message}", file=sys.stderr, flush=True)


def _spawn(
    argv: list[str],
    *,
    stdout: int | IO[str] | None = None,
    stderr: int | None = None,
    start_new_session: bool = False,
) -> subprocess.Popen[str]:
    """Start a program by absolute path, never through a shell.

    Every subprocess this script starts goes through here, so `S603` has one
    reviewable site rather than one per caller, and `S607` cannot be
    reintroduced: every `argv[0]` below is an absolute path.
    """
    return subprocess.Popen(  # noqa: S603
        argv,
        text=True,
        stdout=stdout,
        stderr=stderr,
        start_new_session=start_new_session,
    )


def _run(argv: list[str], *, timeout: int = 900) -> subprocess.CompletedProcess[str]:
    """Run a program to completion and hand back everything it said."""
    process = _spawn(argv, stdout=subprocess.PIPE, stderr=subprocess.STDOUT)
    output, _ = process.communicate(timeout=timeout)
    return subprocess.CompletedProcess(argv, process.returncode, output, "")


def _uv() -> str:
    """The absolute path of `uv`, which provisions the instance's environment.

    Raises:
        FileNotFoundError: If `uv` is not on PATH.
    """
    found = shutil.which("uv")
    if found is None:
        message = "uv is not on PATH; it is what provisions the instance's environment"
        raise FileNotFoundError(message)
    return found


# ~~ provisioning


def install(instance: Instance) -> None:
    """Provision the instance, doing nothing that is already done."""
    instance.state_dir.mkdir(parents=True, exist_ok=True)
    instance.basedir.mkdir(parents=True, exist_ok=True)
    # Read first: a state directory whose configuration cannot be read is said
    # so in a second, rather than after an install that will not be used.
    read_config(instance)
    _install_octoprint(instance)
    _provision_api_key(instance)
    # Written last, because creating the account writes a configuration of its
    # own that this one is composed over.
    _write_config(instance)


def _install_octoprint(instance: Instance) -> None:
    """Put the pinned OctoPrint in the instance's own environment, once.

    Raises:
        RuntimeError: If the environment or the install does not come up, with
            everything `uv` said about why.
    """
    if instance.executable.is_file():
        note(f"{OCTOPRINT_REQUIREMENT} is already installed in {instance.venv}")
        return
    note(f"installing {OCTOPRINT_REQUIREMENT} on Python {OCTOPRINT_PYTHON} in {instance.venv}")
    created = _run([_uv(), "venv", "--python", OCTOPRINT_PYTHON, str(instance.venv)])
    if created.returncode != 0:
        message = f"could not create {instance.venv}:\n{created.stdout}"
        raise RuntimeError(message)
    installed = _run(
        [
            _uv(),
            "pip",
            "install",
            "--python",
            str(instance.venv / "bin" / "python"),
            OCTOPRINT_REQUIREMENT,
        ]
    )
    if installed.returncode != 0:
        message = f"could not install {OCTOPRINT_REQUIREMENT}:\n{installed.stdout}"
        raise RuntimeError(message)


def managed_config(connection: Connection) -> dict[str, Any]:
    """The configuration this script owns, for one connection.

    Everything outside the connection is identical between the two modes: the
    first-run wizard is already answered, the API is authenticated, and every
    call OctoPrint would otherwise make to the internet is off, because a
    fixture that had to reach the internet to start would fail for reasons that
    have nothing to do with the printer.
    """
    return {
        "api": {"allowCrossOrigin": False},
        "plugins": {
            "_disabled": ["announcements", "softwareupdate", "tracking", "achievements"],
            "virtual_printer": {"enabled": connection.is_virtual},
        },
        "serial": {
            "additionalPorts": [] if connection.is_virtual else [connection.device],
            "autoconnect": True,
            "baudrate": connection.baudrate,
            "port": connection.device,
        },
        "server": {
            # `firstRun` is what makes the instance unattended: left set,
            # OctoPrint sends every caller to the browser wizard instead.
            "firstRun": False,
            "host": HOST,
            "onlineCheck": {"enabled": False},
            "pluginBlacklist": {"enabled": False},
            "pythonEolCheck": {"enabled": False},
        },
    }


def _merge(over: dict[str, Any], under: dict[str, Any]) -> dict[str, Any]:
    """`over` wins, key by key, all the way down."""
    merged = dict(under)
    for key, value in over.items():
        beneath = merged.get(key)
        if isinstance(value, dict) and isinstance(beneath, dict):
            merged[key] = _merge(value, beneath)
        else:
            merged[key] = value
    return merged


def read_config(instance: Instance) -> dict[str, Any]:
    """Read the configuration OctoPrint wrote, or nothing on a fresh directory.

    Raises:
        StartupError: If a configuration is there and cannot be read.
    """
    if not instance.config_file.is_file():
        return {}
    try:
        loaded = yaml.safe_load(instance.config_file.read_text(encoding="utf-8"))
    except (OSError, yaml.YAMLError) as error:
        raise StartupError(
            "config-unreadable", f"{instance.config_file} could not be read: {error}"
        ) from error
    if loaded is None:
        return {}
    if not isinstance(loaded, dict):
        raise StartupError(
            "config-unreadable",
            f"{instance.config_file} holds {type(loaded).__name__}, not a mapping of settings",
        )
    return loaded


def _write_config(instance: Instance) -> None:
    """Write this script's settings over whatever the instance already had."""
    merged = _merge(managed_config(instance.connection), read_config(instance))
    # The global API key. It is deprecated, it stops working in OctoPrint 1.13,
    # and this instance authenticates with the user key `install` provisioned,
    # so nothing here carries it over: leaving it would make the two modes'
    # configurations differ by a random value rather than by the connection.
    # OctoPrint generates one for itself when the server starts; it is random,
    # local to the instance, and nothing here uses it.
    api = merged.get("api")
    if isinstance(api, dict):
        api.pop("key", None)
    instance.config_file.write_text(
        yaml.safe_dump(merged, default_flow_style=False, sort_keys=True), encoding="utf-8"
    )
    note(f"configured {instance.config_file} for the {instance.connection.mode} connection")


def _provision_api_key(instance: Instance) -> str:
    """Create the account if it is absent, and give it a key if it has none.

    Raises:
        RuntimeError: If the account cannot be created or is not there after it
            has been, with everything OctoPrint said about why.
    """
    if not instance.users_file.is_file():
        note(f"creating the `{ACCOUNT}` account")
        passphrase = secrets.token_hex(16)
        created = _run(
            [
                str(instance.executable),
                "--basedir",
                str(instance.basedir),
                "user",
                "add",
                "--password",
                passphrase,
                "--admin",
                ACCOUNT,
            ],
            timeout=300,
        )
        if created.returncode != 0:
            message = f"could not create the `{ACCOUNT}` account:\n{created.stdout}"
            raise RuntimeError(message)
        # Written down rather than thrown away: this is the account a person
        # logs into the web interface with on the machine beside the printer.
        instance.account_password_file.write_text(f"{passphrase}\n", encoding="utf-8")
        instance.account_password_file.chmod(0o600)

    users = yaml.safe_load(instance.users_file.read_text(encoding="utf-8")) or {}
    account = users.get(ACCOUNT)
    if not isinstance(account, dict):
        message = f"{instance.users_file} carries no `{ACCOUNT}` account"
        raise RuntimeError(message)
    key = account.get("apikey")
    if not key:
        key = secrets.token_hex(16)
        account["apikey"] = key
        instance.users_file.write_text(yaml.safe_dump(users, sort_keys=True), encoding="utf-8")
        note(f"provisioned an API key for `{ACCOUNT}`")
    instance.api_key_file.write_text(f"{key}\n", encoding="utf-8")
    instance.api_key_file.chmod(0o600)
    return str(key)


def api_key(instance: Instance) -> str:
    """The API key this instance authenticates with."""
    return instance.api_key_file.read_text(encoding="utf-8").strip()


# ~~ the API this script talks to the instance over


def call(
    url: str,
    key: str | None,
    path: str,
    *,
    method: str = "GET",
    body: bytes | None = None,
    content_type: str | None = None,
    timeout: float = 15.0,
) -> tuple[int, object]:
    """One API call, answering with the status and the decoded body.

    A plain HTTP connection rather than a URL opener: the scheme, the host and
    the port are this script's own, so there is no URL to audit.

    Synchronous, and the standard library rather than a client: every call here
    reaches an OctoPrint this script itself provisioned and started, on loopback
    at a port it claimed itself, and each one is a step of a strictly ordered
    bring-up whose answer is acted on before the next request is made — so there
    is nothing for an async client to overlap. The boundary where an async typed
    client does belong is the product's, and the product reaches OctoPrint from
    the `printobserver-octoprint` crate rather than from here.
    """
    parts = urllib.parse.urlsplit(url)
    headers = {}
    if key is not None:
        headers["X-Api-Key"] = key
    if content_type is not None:
        headers["Content-Type"] = content_type
    # llmlint: ignore[async_typed_clients_at_boundaries] one step of a sequential bring-up
    connection = http.client.HTTPConnection(
        parts.hostname or HOST, parts.port or 80, timeout=timeout
    )
    try:
        connection.request(method, path, body=body, headers=headers)
        answer = connection.getresponse()
        return answer.status, _decode(answer.read())
    finally:
        connection.close()


def _decode(payload: bytes) -> object:
    """The body of an answer, as JSON where it is JSON and as text where it is not."""
    text = payload.decode("utf-8", errors="replace")
    try:
        return json.loads(text)
    except json.JSONDecodeError:
        return text


def multipart(fields: dict[str, str], file_name: str, content: bytes) -> tuple[bytes, str]:
    """One multipart body carrying a file and some fields, and its content type."""
    boundary = f"----printobserver{secrets.token_hex(8)}"
    parts: list[bytes] = [
        (
            f'--{boundary}\r\nContent-Disposition: form-data; name="{name}"\r\n\r\n{value}\r\n'
        ).encode()
        for name, value in fields.items()
    ]
    parts.append(
        (
            f"--{boundary}\r\n"
            f'Content-Disposition: form-data; name="file"; filename="{file_name}"\r\n'
            f"Content-Type: text/plain\r\n\r\n"
        ).encode()
    )
    parts.append(content)
    parts.append(f"\r\n--{boundary}--\r\n".encode())
    return b"".join(parts), f"multipart/form-data; boundary={boundary}"


# ~~ starting, waiting and stopping


def free_port() -> int:
    """A port nothing is listening on, so two runs on one host do not collide."""
    with socket.socket(socket.AF_INET, socket.SOCK_STREAM) as probe:
        probe.bind((HOST, 0))
        return int(probe.getsockname()[1])


def claim_port(port: int) -> None:
    """Refuse a port something else is already listening on.

    Raises:
        StartupError: If the port cannot be bound.
    """
    with socket.socket(socket.AF_INET, socket.SOCK_STREAM) as probe:
        try:
            probe.bind((HOST, port))
        except OSError as error:
            raise StartupError(
                "port-in-use", f"nothing else can listen on {HOST}:{port}: {error}"
            ) from error


def claim_device(connection: Connection) -> None:
    """Refuse a serial device that cannot be opened, before anything is started.

    Raises:
        StartupError: If the named device cannot be opened.
    """
    if connection.is_virtual:
        return
    try:
        handle = os.open(connection.device, os.O_RDWR | os.O_NOCTTY | os.O_NONBLOCK)
    except OSError as error:
        raise StartupError(
            "serial-device-unopenable", f"{connection.device} could not be opened: {error}"
        ) from error
    os.close(handle)


def alive(pid: int) -> bool:
    """Whether a process this script started is still running."""
    try:
        os.kill(pid, 0)
    except OSError:
        return False
    return True


def start_server(instance: Instance, port: int) -> int:
    """Start the server in a session of its own, and answer with its process id."""
    argv = [
        str(instance.executable),
        "--basedir",
        str(instance.basedir),
        "serve",
        "--host",
        HOST,
        "--port",
        str(port),
    ]
    if os.geteuid() == 0:
        # Not a recommendation. A container that has nothing but root is
        # somewhere this environment has to come up anyway.
        argv.append("--iknowwhatimdoing")
    with instance.server_log.open("a", encoding="utf-8") as log:
        process = _spawn(argv, stdout=log, stderr=subprocess.STDOUT, start_new_session=True)
    note(f"started OctoPrint on {HOST}:{port} as process {process.pid}")
    return process.pid


def wait_for_api(instance: Instance, url: str, pid: int, timeout: float) -> None:
    """Answer only once the instance answers its own API.

    Raises:
        StartupError: If it never does, or stops before it does.
    """
    key = api_key(instance)
    deadline = time.monotonic() + timeout
    while time.monotonic() < deadline:
        if not alive(pid):
            raise StartupError(
                "never-answered",
                f"the server exited before it answered; read {instance.server_log}",
            )
        try:
            status, _ = call(url, key, "/api/version", timeout=5.0)
        except OSError, http.client.HTTPException:
            time.sleep(1.0)
            continue
        if status == 200:
            note(f"{url} is answering its own API")
            return
        time.sleep(1.0)
    raise StartupError(
        "never-answered",
        f"{url} did not answer within {timeout:.0f}s; read {instance.server_log}",
    )


def wait_for_printer(instance: Instance, url: str, timeout: float) -> str:
    """Answer only once the instance reports a connected printer.

    Raises:
        StartupError: If it never reports one.
    """
    key = api_key(instance)
    deadline = time.monotonic() + timeout
    state = "unknown"
    while time.monotonic() < deadline:
        status, body = call(url, key, "/api/connection")
        if status == 200 and isinstance(body, dict):
            state = str(body.get("current", {}).get("state", "unknown"))
            if state in {"Operational", "Printing", "Paused"}:
                note(f"the printer on {instance.connection.device} is {state.lower()}")
                return state
        time.sleep(1.0)
    raise StartupError(
        "printer-not-connected",
        f"the printer on {instance.connection.device} is `{state}` after {timeout:.0f}s; "
        f"read {instance.octoprint_log}",
    )


def hold_print(instance: Instance, url: str, timeout: float) -> str:
    """Upload the hold print, select it, start it, and see it running.

    Raises:
        StartupError: If the instance does not report a running print.
    """
    gcode = Path(__file__).resolve().parent / "gcode" / "hold.gcode"
    body, content_type = multipart(
        {"select": "true", "print": "true"}, gcode.name, gcode.read_bytes()
    )
    status, answer = call(
        url,
        api_key(instance),
        "/api/files/local",
        method="POST",
        body=body,
        content_type=content_type,
        timeout=60.0,
    )
    if status not in {200, 201}:
        raise StartupError(
            "print-not-running", f"uploading {gcode.name} was refused with {status}: {answer}"
        )

    deadline = time.monotonic() + timeout
    state = "unknown"
    while time.monotonic() < deadline:
        _, job = call(url, api_key(instance), "/api/job")
        if isinstance(job, dict):
            state = str(job.get("state", "unknown"))
            if state.startswith("Printing"):
                note(f"{gcode.name} is printing, and keeps printing for {HOLD_SECONDS}s")
                return state
        time.sleep(1.0)
    raise StartupError(
        "print-not-running",
        f"{gcode.name} was uploaded but the job is `{state}` after {timeout:.0f}s; "
        f"read {instance.server_log}",
    )


def stop(instance: Instance) -> dict[str, Any]:
    """Stop whatever this state directory started, leaving no process behind."""
    if not instance.record.is_file():
        note(f"no instance is recorded under {instance.state_dir}")
        return {"state_dir": str(instance.state_dir), "stopped": False, "pid": None}
    record = json.loads(instance.record.read_text(encoding="utf-8"))
    pid = int(record["pid"])
    if alive(pid):
        _terminate(pid)
    instance.record.unlink()
    note(f"stopped process {pid}")
    return {"state_dir": str(instance.state_dir), "stopped": True, "pid": pid}


def _terminate(pid: int) -> None:
    """Signal the session the server was started in, and wait for it to go."""
    try:
        group = os.getpgid(pid)
    except OSError:
        return
    os.killpg(group, signal.SIGTERM)
    deadline = time.monotonic() + 30.0
    while time.monotonic() < deadline:
        if not alive(pid):
            return
        time.sleep(0.5)
    os.killpg(group, signal.SIGKILL)


# ~~ the commands


def up(instance: Instance, port: int | None, *, printing: bool, timeout: float) -> dict[str, Any]:
    """Provision, start, and answer only once the instance is usable.

    Raises:
        StartupError: If it does not come up, having stopped what it started.
    """
    running = _already_running(instance)
    if running is not None:
        note(f"an instance is already running on {running['url']}")
        return running

    # Before anything is provisioned: a device that cannot be opened is not a
    # reason to install OctoPrint first.
    claim_device(instance.connection)
    install(instance)
    chosen = free_port() if port is None else port
    claim_port(chosen)

    url = f"http://{HOST}:{chosen}"
    pid = start_server(instance, chosen)
    summary: dict[str, Any] = {
        "state_dir": str(instance.state_dir),
        "url": url,
        "port": chosen,
        "pid": pid,
        "mode": instance.connection.mode,
        "device": instance.connection.device,
        "baudrate": instance.connection.baudrate,
        "api_key_file": str(instance.api_key_file),
        "server_log": str(instance.server_log),
        "hold_seconds": HOLD_SECONDS,
        "printing": False,
    }
    instance.record.write_text(json.dumps(summary, indent=2) + "\n", encoding="utf-8")

    try:
        wait_for_api(instance, url, pid, timeout)
        wait_for_printer(instance, url, timeout)
        if printing:
            hold_print(instance, url, timeout)
            summary["printing"] = True
    except StartupError:
        # A start that failed leaves nothing running: the diagnosis is in the
        # logs, which are files, rather than in a process nobody will stop.
        stop(instance)
        raise

    instance.record.write_text(json.dumps(summary, indent=2) + "\n", encoding="utf-8")
    return summary


def _already_running(instance: Instance) -> dict[str, Any] | None:
    """What this state directory already has running, if it still answers."""
    if not instance.record.is_file() or not instance.api_key_file.is_file():
        return None
    record = json.loads(instance.record.read_text(encoding="utf-8"))
    if not alive(int(record["pid"])):
        return None
    try:
        status, _ = call(str(record["url"]), api_key(instance), "/api/version", timeout=5.0)
    except OSError, http.client.HTTPException:
        return None
    return record if status == 200 else None


def summarize_install(instance: Instance) -> dict[str, Any]:
    """What `install` provisioned, for a caller to read."""
    return {
        "state_dir": str(instance.state_dir),
        "basedir": str(instance.basedir),
        "config_file": str(instance.config_file),
        "api_key_file": str(instance.api_key_file),
        "mode": instance.connection.mode,
        "device": instance.connection.device,
        "baudrate": instance.connection.baudrate,
        "hold_seconds": HOLD_SECONDS,
    }


def argument_parser() -> argparse.ArgumentParser:
    """The command surface, whose defaults every recipe and caller shares."""
    parser = argparse.ArgumentParser(prog="octoprint-env", description=__doc__)
    parser.add_argument("command", choices=("install", "up", "down"))
    parser.add_argument(
        "--state-dir",
        default=os.environ.get("OCTOPRINT_ENV_STATE_DIR", DEFAULT_STATE_DIR),
        help="the directory this environment lives under (default: %(default)s)",
    )
    parser.add_argument(
        "--mode",
        choices=("virtual", "serial"),
        default=os.environ.get("OCTOPRINT_ENV_MODE", "virtual"),
        help="the virtual printer, or a serial device (default: %(default)s)",
    )
    parser.add_argument(
        "--device",
        default=os.environ.get("OCTOPRINT_ENV_DEVICE", ""),
        help="the serial device to connect to, in `--mode serial`",
    )
    parser.add_argument(
        "--baudrate",
        type=int,
        default=int(os.environ.get("OCTOPRINT_ENV_BAUDRATE", DEFAULT_BAUDRATE)),
        help="the baud rate to connect at (default: %(default)s)",
    )
    parser.add_argument(
        "--port",
        default=os.environ.get("OCTOPRINT_ENV_PORT", "auto"),
        help="the port to listen on, or `auto` for a free one (default: %(default)s)",
    )
    parser.add_argument(
        "--print",
        dest="printing",
        action=argparse.BooleanOptionalAction,
        default=None,
        help=(
            "start the hold print once connected; on by default in `--mode virtual` "
            "and off by default in `--mode serial`, because a real printer moves"
        ),
    )
    parser.add_argument(
        "--start-timeout",
        type=float,
        default=float(os.environ.get("OCTOPRINT_ENV_START_TIMEOUT", "180")),
        help="seconds to wait for each step of starting (default: %(default)s)",
    )
    return parser


def connection_of(arguments: argparse.Namespace) -> Connection:
    """The connection the arguments ask for.

    Raises:
        StartupError: If `--mode serial` names no device.
    """
    if arguments.mode == "virtual":
        return Connection("virtual", VIRTUAL_DEVICE, arguments.baudrate)
    if not arguments.device:
        raise StartupError(
            "serial-device-unopenable",
            "`--mode serial` names no device: pass `--device /dev/ttyACM0`, or set "
            "OCTOPRINT_ENV_DEVICE",
        )
    return Connection("serial", arguments.device, arguments.baudrate)


def main(argv: list[str] | None = None) -> int:
    """Run one command, reporting every failure by name and next action."""
    arguments = argument_parser().parse_args(argv)
    try:
        connection = connection_of(arguments)
        instance = Instance(Path(arguments.state_dir), connection)
        if arguments.command == "install":
            install(instance)
            answer = summarize_install(instance)
        elif arguments.command == "down":
            answer = stop(instance)
        else:
            printing = connection.is_virtual if arguments.printing is None else arguments.printing
            port = None if str(arguments.port) == "auto" else int(arguments.port)
            answer = up(instance, port, printing=printing, timeout=arguments.start_timeout)
    except StartupError as failure:
        print(f"octoprint-env: failed: {failure.failure_class}", file=sys.stderr)
        print(f"  what happened: {failure.detail}", file=sys.stderr)
        print(f"  next action: {failure.next_action}", file=sys.stderr)
        return 1
    except Exception as error:
        print(
            "octoprint-env: failed outside the declared failure classes "
            f"({', '.join(FAILURE_CLASSES)}), with the underlying error's own text:",
            file=sys.stderr,
        )
        print(f"  {type(error).__name__}: {error}", file=sys.stderr)
        return 1
    print(json.dumps(answer, indent=2))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
