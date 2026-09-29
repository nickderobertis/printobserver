"""A smoke run against a printer the test controls.

Nothing here stands in for the thing under test. `printer_smoke.py` runs as a
subprocess exactly as its recipe runs it, and it drives the `printobserver`
program this workspace builds; what is controlled is the far side of that
command — the supervisor and the machine — which is `machine.Machine`.

The serial device is one the test creates: a pseudo-terminal, which is a real
character device this user can open, so the device precondition is met by a
device rather than by a fixture pretending to be one. Windows has no
pseudo-terminal, and there the device is the null device — see
`a_serial_device`.
"""

from __future__ import annotations

import hashlib
import json
import os
import signal
import subprocess
import sys
import tempfile
import threading
import time
from collections.abc import Iterator
from contextlib import contextmanager
from dataclasses import dataclass, field
from pathlib import Path

from machine import Machine
from printer_smoke import CONSERVATIVE_ENVELOPE, FILE_NAME, address_of
from relay import ADDRESS as RELAY_ADDRESS
from relay import AFTER, ARMED_BY, HANG_ON, LISTEN_ON, STATE, RelayState
from relay import PROGRAM as RELAYED_PROGRAM
from repo_checks import platforms
from repo_checks.expect import truth
from repo_checks.model import Repo
from repo_checks.shell import run, start

REPO_ROOT = Path(__file__).resolve().parents[3]
SMOKE = REPO_ROOT / "tools" / "printer-smoke" / "printer_smoke.py"
RELAY = Path(__file__).resolve().parent / "relay.py"
RELAY_CLIENT_SOURCE = Path(__file__).resolve().parent / "relay_client.rs"

RELAY_START_S = 60.0
RELAY_STOP_S = 30.0

#: The device a Windows host names for the smoke: its null device, which the
#: system's device table carries on every Windows machine.
WINDOWS_DEVICE = "NUL"


def built_program() -> Path:
    """Where the workspace's debug build leaves the `printobserver` program on this host.

    Returns:
        That path, under the name this host's platform gives the program — which
        carries `.exe` on Windows.
    """
    return REPO_ROOT / "target" / "debug" / platforms.host(Repo(REPO_ROOT)).program


PROGRAM = built_program()


def relay_client() -> Path:
    """Where the relay's client is compiled to, which the smoke runs in front of a relay.

    It sits in the build directory beside the program, named the way this host
    names a program, and after the source it was compiled from: two suites
    compiling at once then produce the same file rather than racing to write
    one, and a source that changed is never answered by a stale build.
    """
    digest = hashlib.sha256(RELAY_CLIENT_SOURCE.read_bytes()).hexdigest()[:16]
    return REPO_ROOT / "target" / "printer-smoke" / f"relay-client-{digest}{PROGRAM.suffix}"


RELAY_CLIENT = relay_client()

#: What the smoke asks a machine for, and how long this harness lets it wait.
#: A socket answers at once, so a run against one needs no minute of patience.
SETTLE_S = "4"
DURATION_S = "1"

#: The bound a relayed command is given. A command that answers needs the relay's
#: compiled client started, one loopback round trip to the relay, the program
#: run and the machine asked; the relay's own interpreter is started once per
#: world, before the smoke is, and no command pays for it.
#:
# llmlint: ignore[comments_earn_their_place] suppressions.toml has the reason.
#: The rule: ten times the slowest ordinary relayed command measured under
#: parallel load, rounded up to a whole second. The measurement it was taken
#: from, on 2026-09-28: four copies of this whole suite at once on a 20-core
#: Linux host already loaded to between 44 and 60, timing each relayed command
#: the smoke ran from start to answer, over 520 that answered — median 0.040s,
#: 99th percentile 0.166s, slowest 0.224s. So 3s.
#: A bound that stops a command meant to answer is a hang the test did not ask
#: for, and every hang the test did ask for costs the whole bound.
RELAY_BOUND_S = "3"

#: The manifest the print carries, which is one for the smoke's own payload.
MANIFEST: dict[str, object] = {
    "file_name": FILE_NAME,
    "material": "PLA",
    "nozzle_diameter_mm": 0.4,
    "slicer_profile": "the profile the smoke's own payload was sliced with",
    "allowed": {},
    "metadata": {},
}

CREDENTIAL = "a-credential-this-harness-configures"


def build_the_program() -> Path:
    """Build the command the smoke drives, and answer where it is.

    Returns:
        The built `printobserver` binary.

    Raises:
        RuntimeError: If it could not be built, with everything cargo said.
    """
    built = run(["cargo", "build", "-p", "printobserver", "--locked"], cwd=REPO_ROOT, timeout=1800)
    if built.returncode != 0 or not PROGRAM.is_file():
        message = f"the smoke drives a program this workspace could not build:\n{built.stderr}"
        raise RuntimeError(message)
    return PROGRAM


def build_the_relay_client() -> Path:
    """Compile the relay's client, and answer where it is.

    It is one file of the standard library alone, so it is compiled by `rustc`
    directly — with the toolchain this workspace pins — rather than made a crate
    of the workspace: it is a test's instrument, not something this repository
    ships.

    Returns:
        The compiled client.

    Raises:
        RuntimeError: If it could not be compiled, with everything rustc said.
    """
    if RELAY_CLIENT.is_file():
        return RELAY_CLIENT
    RELAY_CLIENT.parent.mkdir(parents=True, exist_ok=True)
    # Compiled in a directory of this build's own and moved into place whole,
    # so a suite running beside this one never sees a part-written client.
    with tempfile.TemporaryDirectory(dir=RELAY_CLIENT.parent) as building:
        compiled = Path(building) / RELAY_CLIENT.name
        built = run(
            [
                "rustc",
                "--edition",
                "2021",
                "-C",
                "opt-level=2",
                "-o",
                str(compiled),
                str(RELAY_CLIENT_SOURCE),
            ],
            cwd=REPO_ROOT,
            timeout=600,
        )
        if built.returncode != 0 or not compiled.is_file():
            message = f"the relay's client could not be compiled:\n{built.stderr}"
            raise RuntimeError(message)
        if not RELAY_CLIENT.is_file():
            compiled.replace(RELAY_CLIENT)
    return RELAY_CLIENT


@dataclass
class RelayProcess:
    """One running relay: the process, and the address its client reaches it at."""

    process: subprocess.Popen[str]
    address: str

    @classmethod
    def start(
        cls, environment: dict[str, str], *, within_s: float = RELAY_START_S, relay: Path = RELAY
    ) -> RelayProcess:
        """Start a relay under `environment`, and answer it once it is listening.

        Args:
            environment: What it runs under, which says what it relays and
                hangs, and which every program it runs inherits.
            within_s: How long it is given to say where it listens.
            relay: The script it is, which a test replaces to make one that
                never says.

        Returns:
            The relay, listening.

        Raises:
            RuntimeError: If its first line is not the loopback address it
                listens on — it stopped first, said something else, or said
                nothing inside `within_s` — with that line and everything it
                said.
        """
        process = start([sys.executable, str(relay)], cwd=REPO_ROOT, env=environment)
        first: list[str] = []
        # llmlint: ignore[async_typed_clients_at_boundaries] suppressions.toml has the reason.
        reading = threading.Thread(
            target=lambda: first.append(process.stdout.readline() if process.stdout else ""),
            daemon=True,
        )
        reading.start()
        reading.join(within_s)
        announced = first[0].strip() if first else ""
        if not _is_loopback_address(announced):
            process.kill()
            reading.join()
            _, said = process.communicate(timeout=RELAY_START_S)
            message = (
                f"the relay announced {announced!r} rather than where it listens, and said:\n{said}"
            )
            raise RuntimeError(message)
        return cls(process=process, address=announced)

    def stop(self, *, within_s: float = RELAY_STOP_S) -> None:
        """Stop it by ending its input, and kill it if that does not within `within_s`.

        Ending its input is how it is asked: that releases every command it
        holds unanswered and stops every program it started before it exits.
        """
        try:
            self.process.communicate(timeout=within_s)
        except subprocess.TimeoutExpired:
            self.process.kill()
            self.process.communicate()


def _is_loopback_address(announced: str) -> bool:
    """Whether `announced` is a port on the loopback host the relay listens on."""
    host, _, port = announced.rpartition(":")
    return host == LISTEN_ON[0] and port.isdecimal() and 0 < int(port) < 2**16


@dataclass
class World:
    """One machine the test controls, and the smoke run pointed at it."""

    root: Path
    substitute: Machine
    device: str
    config: Path
    print_id: str
    program: Path
    state_dir: Path
    relay: RelayProcess | None = field(default=None)

    def stop(self) -> None:
        """Stop everything this world started: its relay, and its machine."""
        if self.relay is not None:
            self.relay.stop()
            self.relay = None
        self.substitute.stop()

    def environment(self, extra: dict[str, str] | None = None) -> dict[str, str]:
        """The environment a smoke run is started under.

        Args:
            extra: What this run changes about it, including a variable set to
                the empty string to take one away.

        Returns:
            The environment, with everything the smoke reads in it.
        """
        environment = dict(os.environ)
        environment.update(
            {
                "PRINTOBSERVER_SMOKE_DEVICE": self.device,
                "PRINTOBSERVER_SMOKE_CONFIG": str(self.config),
                "PRINTOBSERVER_SMOKE_PRINT_ID": self.print_id,
                "PRINTOBSERVER_SMOKE_PROGRAM": str(self.program),
                "PRINTOBSERVER_SMOKE_SETTLE_S": SETTLE_S,
                "PRINTOBSERVER_SMOKE_DURATION_S": DURATION_S,
                "OCTOPRINT_ENV_STATE_DIR": str(self.state_dir),
                "PRINTOBSERVER_SERVER": self.substitute.url,
                "PRINTOBSERVER_CREDENTIAL": CREDENTIAL,
                "PYTHONPATH": os.pathsep.join(
                    str(REPO_ROOT / part)
                    for part in ("tools/repo-checks/src", "tools/octoprint-env")
                ),
            }
        )
        for name, value in (extra or {}).items():
            if value:
                environment[name] = value
            else:
                environment.pop(name, None)
        return environment

    def smoke(
        self,
        *arguments: str,
        environment: dict[str, str] | None = None,
        timeout: float = 300,
    ) -> subprocess.CompletedProcess[str]:
        """Run the smoke the way its recipe runs it, and hand back what it said.

        Args:
            *arguments: What the recipe passed on, usually `--run`.
            environment: What this run changes about the environment.
            timeout: Seconds to let it take.

        Returns:
            The completed run.
        """
        return run(
            [sys.executable, str(SMOKE), *arguments],
            cwd=REPO_ROOT,
            env=self.environment(environment),
            timeout=timeout,
        )

    def relaying(
        self,
        *,
        hang_on: str,
        after: int = 0,
        armed_by: str = "",
        timeout_s: str = RELAY_BOUND_S,
        program: Path | None = None,
    ) -> dict[str, str]:
        """Put a relay in front of the program, which stops answering on one command.

        A command that never answers is the failure a controlled machine cannot
        be scripted into — it answers or it does not — so it is made here, by a
        stand-in that passes every invocation through except the ones named.
        The relay is one process, started here and stopped with this world;
        what the smoke runs per command is its compiled client.

        Args:
            hang_on: The client command it stops answering on.
            after: How many of that command to answer normally first.
            armed_by: A command that arms the hang, for a run that must reach
                its cleanup before it meets a machine it cannot read.
            timeout_s: The bound the smoke gives one command, in seconds. Every
                hang costs the whole of it, so a test that hangs many commands
                may shorten it — but not below what a command that does answer
                needs on a loaded host, or the bound stops the ones that were
                meant to answer.
            program: What the relay passes commands to, or the built program.

        Returns:
            What this changes about the environment the smoke runs under.
        """
        if self.relay is not None:
            self.relay.stop()
            self.relay = None
        self.relay = RelayProcess.start(
            self.environment(
                {
                    RELAYED_PROGRAM: str(self.program if program is None else program),
                    HANG_ON: hang_on,
                    AFTER: str(after),
                    ARMED_BY: armed_by,
                    STATE: str(self.relay_state_file),
                }
            )
        )
        return {
            "PRINTOBSERVER_SMOKE_PROGRAM": str(RELAY_CLIENT),
            "PRINTOBSERVER_SMOKE_COMMAND_TIMEOUT_S": timeout_s,
            RELAY_ADDRESS: self.relay.address,
        }

    @property
    def relay_state_file(self) -> Path:
        """Where the relay `relaying` put in front of the program counts."""
        return self.root / "relay-state.json"

    def relay_state(self, run: subprocess.CompletedProcess[str]) -> RelayState:
        """What the relay counted over `run`, as the state it left.

        Read after the run rather than trusted: the smoke's own bound stops
        every command the relay hangs, so what the relay left is the evidence
        that it counted the command before the command was stopped. A file that holds
        no state fails naming what it did hold, beside everything the run said,
        rather than failing inside the decoder with neither.

        Args:
            run: The completed smoke run the relay was counting over.

        Returns:
            The relay's state: whether it was armed, and what it had seen.
        """
        held = (
            self.relay_state_file.read_text(encoding="utf-8")
            if self.relay_state_file.is_file()
            else None
        )
        try:
            state = RelayState.of(json.loads(held)) if held is not None else None
        except json.JSONDecodeError:
            state = None
        truth(
            state is not None,
            describing="the relay to have left the state it counted; "
            f"it left {held!r}, and the run said:\n{run.stdout}",
        )
        return state if state is not None else RelayState(armed=False, seen=0)

    def interrupt_the_smoke(
        self, *, after: float = 8.0, duration_s: str = "30"
    ) -> subprocess.CompletedProcess[str]:
        """Run the smoke and interrupt it part-way, as a person at the machine would.

        The interrupt is a real one. On a Unix it is `SIGINT`, addressed to
        the smoke itself rather than to its process group: the group is where
        the `printobserver` commands the smoke is spawning at that instant
        live, so a run could be interrupted between spawning one and reading
        it, and what a person's own interrupt reaches is the program they
        started. On Windows it is the console break event, which is addressed
        to a group, so the smoke is started in one of its own. The bounded
        intervention is given long enough that the run is waiting it out when
        the interrupt lands, so what is interrupted is a run that has already
        started a print and already moved the machine.

        Delivered by this suite rather than by a `timeout` program: that one is
        GNU's, and macOS has none.

        Args:
            after: How long to let it run before interrupting it, in seconds.
            duration_s: How long its bounded intervention stands for.

        Returns:
            The completed run, including everything it said while cleaning up.
        """
        environment = self.environment({"PRINTOBSERVER_SMOKE_DURATION_S": duration_s})
        process = start(
            [sys.executable, str(SMOKE), "--run"],
            cwd=REPO_ROOT,
            env=environment,
            own_group=True,
        )
        time.sleep(after)
        process.send_signal(signal.CTRL_BREAK_EVENT if os.name == "nt" else signal.SIGINT)
        stdout, stderr = process.communicate(timeout=300)
        return subprocess.CompletedProcess(process.args, process.returncode, stdout + stderr, "")


@contextmanager
def a_serial_device() -> Iterator[str]:
    """A device this user can open, named the way the smoke's variable names one.

    On a POSIX host it is a pseudo-terminal, held open for as long as it is used:
    a real character device, at a path, with a mode this user can read.

    Windows has no pseudo-terminal, and no serial port a runner can conjure
    without a driver. What the smoke's device precondition asks of a Windows
    host is that the system's own device table carries the name, and the null
    device is a device every Windows machine's table carries — so the
    precondition is still met by a device rather than by a fixture pretending to
    be one.

    Yields:
        The device's name.
    """
    if sys.platform == "win32":
        yield WINDOWS_DEVICE
        return
    controller, device = os.openpty()
    try:
        yield os.ttyname(device)
    finally:
        os.close(controller)
        os.close(device)


def write_the_record(state_dir: Path, url: str, device: str, *, mode: str = "serial") -> None:
    """Write what a scripted OctoPrint bring-up records about itself.

    Args:
        state_dir: The directory `octoprint-env` keeps its state under.
        url: Where that instance answers.
        device: The serial device it connected to.
        mode: The mode it was brought up in.
    """
    state_dir.mkdir(parents=True, exist_ok=True)
    key_file = state_dir / "api-key"
    key_file.write_text("a-provisioned-key\n", encoding="utf-8")
    (state_dir / "instance.json").write_text(
        json.dumps(
            {
                "state_dir": str(state_dir),
                "url": url,
                "pid": os.getpid(),
                "mode": mode,
                "device": device,
                "api_key_file": str(key_file),
            },
            indent=2,
        ),
        encoding="utf-8",
    )


def write_the_configuration(
    path: Path,
    url: str,
    envelope: dict[str, tuple[float, float]] | None = None,
    *,
    octoprint_url: str | None = None,
    client_server: str | None = None,
) -> None:
    """Write the supervisor's own configuration file, which the smoke also reads.

    The server's own file rather than a client's: it says where the supervisor
    was told to listen and which OctoPrint that supervisor drives, which is the
    pair the smoke's binding precondition holds to the instance it verified.

    Args:
        path: Where to write it.
        url: Where the supervisor answers.
        envelope: The safety envelope it configures, or the conservative one.
        octoprint_url: The OctoPrint it says the supervisor drives, or `url`.
        client_server: A `[client] server` to write beside `listen`, when one
            is wanted — a configuration pointing a client somewhere else.
    """
    allowed = CONSERVATIVE_ENVELOPE if envelope is None else envelope
    lines = [
        f'listen = "{address_of(url)}"',
        "",
        "[octoprint]",
        f'url = "{url if octoprint_url is None else octoprint_url}"',
        "",
    ]
    if client_server is not None:
        lines.extend(["[client]", f'server = "{client_server}"', ""])
    lines.append("[safety.allowed]")
    lines.extend(
        f'"{name}" = {{ min = {low}, max = {high} }}' for name, (low, high) in allowed.items()
    )
    path.write_text("\n".join(lines) + "\n", encoding="utf-8")


def a_world(root: Path, *, device: str) -> World:
    """One world: a substitute, a device, a recorded environment and a configuration.

    Args:
        root: A directory of this test's own.
        device: The serial device the smoke is pointed at.

    Returns:
        The world, answering.
    """
    print_id = "01860d5a-4a8f-7c3d-9a4e-4d2f6b1c8e90"
    substitute = Machine(
        device=device, envelope=CONSERVATIVE_ENVELOPE, manifest=MANIFEST, print_id=print_id
    )
    state_dir = root / "octoprint-env"
    write_the_record(state_dir, substitute.url, device)
    config = root / "config.toml"
    write_the_configuration(config, substitute.url)
    return World(
        root=root,
        substitute=substitute,
        device=device,
        config=config,
        print_id=print_id,
        program=PROGRAM,
        state_dir=state_dir,
    )
