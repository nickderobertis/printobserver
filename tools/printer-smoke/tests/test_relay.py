"""The relay's own guarantees: one process serves every command, and its count survives.

`relay.py` stands in front of the program to stop answering on demand. It is
one process a `World` starts and stops, and what the smoke runs per command is
its compiled client, so no command pays for an interpreter's start-up. That is
proven here by what the programs it runs can see: the relay that ran them.

What the tests in `test_cleanup.py` read afterwards is the count the relay
left, so the count has to be there whatever stopped the relay. A kill cannot be
landed inside a write on purpose, so what is driven here is what one leaves
behind: the state file as the last completed write left it, and beside it the
staging file an interrupted write got as far as.

Every relay here is started through `World`, the way the suite starts one, in
front of a program that is the interpreter itself told what to answer. Every
command relayed here goes through its client, the way the smoke drives it, so
those tests are also the drift check between the relay's two halves:
`relay.py` and `relay_client.rs` each write their own side of one wire, and a
change to either that the other did not follow fails here. The tests of a
message the client never sends — one cut short, malformed, or past a bound —
write that wire over a raw socket instead, the way a misbehaving client would.
"""

from __future__ import annotations

import contextlib
import io
import json
import os
import re
import socket
import subprocess
import sys
import threading
import time
from collections.abc import Callable
from pathlib import Path

import pytest
import world as worlds
from printer_smoke import PROGRAM_ENV
from relay import (
    ADDRESS,
    AFTER,
    LISTEN_ON,
    MOST_ARGUMENTS,
    MOST_FRAME_BYTES,
    PROGRAM,
    SERVED_BY,
    STAGING_SUFFIX,
    STATE,
    RelayState,
)
from repo_checks.expect import absent, contains, equal, failing, passing, truth
from repo_checks.shell import PROGRAM_NOT_FOUND, run, start
from world import (
    RELAY_CLIENT,
    RELAY_CLIENT_SOURCE,
    REPO_ROOT,
    RelayProcess,
    World,
    build_the_relay_client,
    relay_client,
)

#: The program the relay stands in front of, whose declared exits the client's
#: own must stay clear of.
EXITS_SOURCE = REPO_ROOT / "crates" / "printobserver" / "src" / "failure.rs"

#: The command the relay is told to count. It is never given reason to hang.
COUNTED = "set-feedrate-factor"

ECHO = "import json, sys; print(json.dumps(sys.argv[1:]))"

SERVED = f"import os; print(os.environ[{SERVED_BY!r}])"

ANSWERING = (
    "import sys; sys.stdout.write('fed ' + sys.stdin.read()); "
    "sys.stderr.write('said on stderr'); sys.exit(3)"
)

HEARTBEAT = (
    "import sys, time\n"
    "from pathlib import Path\n"
    "beat = 0\n"
    "while True:\n"
    "    beat += 1\n"
    "    Path(sys.argv[1]).write_text(str(beat))\n"
    "    time.sleep(0.05)\n"
)

SETTLE_S = 30.0

#: How long a stand-in relay is given to be ready before its bound begins. It
#: only fails a test whose host cannot start an interpreter at all.
READY_S = 300.0


def relayed(
    world: World,
    *,
    hang_on: str = COUNTED,
    after: int = 99,
    armed_by: str = "",
    program: Path = Path(sys.executable),
) -> dict[str, str]:
    """Start `world`'s relay in front of the interpreter, and answer the environment it needs.

    Args:
        world: The world to start it in.
        hang_on: The command it hangs.
        after: How many of that command it answers first.
        armed_by: The command that arms the hang.
        program: What it passes commands to.

    Returns:
        The environment its client is run under, as the smoke's commands are.
    """
    return world.environment(
        world.relaying(hang_on=hang_on, after=after, armed_by=armed_by, program=program)
    )


def relay(
    environment: dict[str, str], *arguments: str, stdin: str | None = None
) -> subprocess.CompletedProcess[str]:
    """Run one command through the relay's client, as the smoke runs one.

    Args:
        environment: What `relayed` answered.
        *arguments: What the smoke would have run the program with.
        stdin: What to feed the command.

    Returns:
        The completed run: the relay's exit, and what the program printed.
    """
    return run(
        [str(RELAY_CLIENT), *arguments], cwd=REPO_ROOT, env=environment, timeout=60, stdin=stdin
    )


def unanswered_exit() -> int:
    """The exit the client gives a command its relay never answered, read from its source."""
    found = re.search(
        r"const UNANSWERED: i32 = (\d+);", RELAY_CLIENT_SOURCE.read_text(encoding="utf-8")
    )
    truth(found is not None, describing="the client's exit for an unanswered command")
    return int(found.group(1)) if found else -1


def connected(environment: dict[str, str]) -> socket.socket:
    """A raw connection to the relay `environment` names, as a client that misbehaves makes."""
    host, _, port = environment[ADDRESS].rpartition(":")
    return socket.create_connection((host, int(port)), timeout=SETTLE_S)


def received(connection: socket.socket) -> bytes:
    """Everything the relay sends on `connection` before it closes it."""
    chunks = []
    while chunk := connection.recv(65536):
        chunks.append(chunk)
    return b"".join(chunks)


def a_closed_address() -> str:
    """A loopback address nothing listens on: a port the system handed out, then let go."""
    with socket.create_server(LISTEN_ON) as listener:
        host, port = listener.getsockname()[:2]
    return f"{host}:{port}"


def until(condition: Callable[[], bool], *, describing: str) -> None:
    """Wait for `condition()` to hold, failing naming what never did."""
    deadline = time.monotonic() + SETTLE_S
    while not condition():
        if time.monotonic() > deadline:
            truth(False, describing=describing)
        time.sleep(0.05)


def test_one_relay_process_serves_every_command(world: World) -> None:
    """Several commands, one relay: each is run by the same process, still running after.

    What the smoke runs is the compiled client rather than an interpreter, and
    every program the relay ran names the one relay that ran it.
    """
    environment = relayed(world)

    served = [relay(environment, "-c", SERVED, "status") for _ in range(3)]

    for answered in served:
        passing(answered, describing="a command passed through the relay")
    equal(
        {answered.stdout.strip() for answered in served},
        {served[0].stdout.strip()},
        describing="the relay each command was served by",
    )
    truth(served[0].stdout.strip().isdigit(), describing="a relay's process id to be named")
    equal(
        environment[PROGRAM_ENV],
        str(RELAY_CLIENT),
        describing="the program the smoke runs per command, which is the compiled client",
    )
    relay_process = world.relay.process if world.relay else None
    truth(
        relay_process is not None and relay_process.poll() is None,
        describing="the relay to be running still, after serving every command",
    )


def test_the_answer_is_the_program_s_own(world: World) -> None:
    """What the client is fed reaches the program, and what the program answers comes back."""
    environment = relayed(world)

    answered = relay(environment, "-c", ANSWERING, stdin="through the relay")

    equal(answered.returncode, 3, describing="the exit the program chose")
    equal(answered.stdout, "fed through the relay", describing="what it wrote on stdout")
    equal(answered.stderr, "said on stderr", describing="what it wrote on stderr")


def test_a_write_interrupted_before_its_move_costs_the_count_nothing(world: World) -> None:
    """The state file holds the last completed count, whatever a kill left beside it.

    A kill inside a write lands after the staging file was opened — empty, or
    part-written — and before it was moved over the state file. The state file
    still holds the previous count, so the next invocation counts on from it
    and its own write replaces the leftover.
    """
    state_file = world.relay_state_file
    RelayState(armed=True, seen=3).write(state_file)
    leftover = state_file.with_name(state_file.name + STAGING_SUFFIX)
    leftover.write_text("", encoding="utf-8")

    answered = relay(relayed(world), "-c", ECHO, COUNTED, "--json")

    passing(answered, describing="the relay, over a leftover of an interrupted write")
    equal(
        RelayState.read(state_file, armed=True),
        RelayState(armed=True, seen=4),
        describing="the count, carried on from the last completed write",
    )
    equal(leftover.exists(), False, describing="whether the leftover survived the next write")
    # Compared as the text it printed rather than decoded: an answer that is
    # not JSON is then named by the comparison instead of raised by the decoder.
    equal(
        answered.stdout.strip(),
        json.dumps([COUNTED, "--json"]),
        describing="what the program was run with, which the relay passes through whole",
    )


def test_a_command_the_relay_does_not_count_writes_nothing(world: World) -> None:
    """An uncounted command is passed through and leaves the count as it was."""
    state_file = world.relay_state_file
    RelayState(armed=True, seen=3).write(state_file)
    written_at = state_file.stat().st_mtime_ns

    answered = relay(relayed(world), "-c", ECHO, "status", "--json")

    passing(answered, describing="the relay, over a command it does not count")
    equal(state_file.stat().st_mtime_ns, written_at, describing="when the state was last written")
    equal(
        RelayState.read(state_file, armed=True),
        RelayState(armed=True, seen=3),
        describing="the count",
    )


@pytest.mark.parametrize(
    "held", [{"armed": "yes", "seen": 3}, {"armed": True, "seen": -1}], ids=["armed", "seen"]
)
def test_a_state_the_relay_did_not_write_stops_it_before_the_program(
    world: World, held: dict[str, object]
) -> None:
    """A file holding anything but a relay state is a defect to stop on, not to count from.

    Nothing but the relay writes its state file, so what is not a state there
    is not a count to carry on from; the relay refuses naming the file and what
    it held, and the program is never run.
    """
    state_file = world.relay_state_file
    state_file.write_text(json.dumps(held), encoding="utf-8")

    answered = relay(relayed(world), "-c", ECHO, COUNTED, "--json")

    failing(answered, naming=f"{state_file} holds")
    contains(answered.stderr, "rather than a relay state", describing="what it said")
    absent(answered.stdout, COUNTED, describing="what the program printed, which never ran")


def test_stopping_the_world_leaves_nothing_the_relay_started_running(
    world: World, tmp_path: Path
) -> None:
    """The relay ends with its world, releasing what it held and stopping what it ran.

    One command is held unanswered and one program never ends when the world is
    stopped. Afterwards the relay has exited, the held client has been let go,
    and the program has stopped rewriting its file.
    """
    environment = relayed(world, hang_on="hold", after=0)
    relay_process = world.relay.process if world.relay else None
    held = start([str(RELAY_CLIENT), "hold"], cwd=REPO_ROOT, env=environment)
    beating = tmp_path / "heartbeat"
    running = start(
        [str(RELAY_CLIENT), "-c", HEARTBEAT, str(beating)], cwd=REPO_ROOT, env=environment
    )
    until(beating.is_file, describing="the relay to have started the program that never ends")
    until(
        lambda: RelayState.read(world.relay_state_file, armed=True).seen == 1,
        describing="the relay to be holding the command it hangs",
    )

    world.stop()

    truth(
        relay_process is not None and relay_process.poll() is not None,
        describing="the relay to have exited with its world",
    )
    for client, which in ((held, "held"), (running, "running")):
        try:
            client.communicate(timeout=SETTLE_S)
        except subprocess.TimeoutExpired:
            client.kill()
            client.communicate()
            truth(False, describing=f"the {which} command's client to have been let go")
        truth(client.returncode != 0, describing=f"the {which} command to have gone unanswered")
    last = beating.read_text(encoding="utf-8")
    time.sleep(1.0)
    equal(
        beating.read_text(encoding="utf-8"),
        last,
        describing="the program's heartbeat, which a program the relay left running would go on",
    )


def test_a_program_that_is_not_there_is_answered_as_one(world: World, tmp_path: Path) -> None:
    """The relay answers a program it cannot start as the smoke's own runner does: not found."""
    environment = relayed(world, program=tmp_path / "no-such-printobserver")

    answered = relay(environment, "status", "--json")

    equal(answered.returncode, PROGRAM_NOT_FOUND, describing="the exit of a program not found")
    contains(answered.stderr, "no-such-printobserver", describing="what it said")
    relay_process = world.relay.process if world.relay else None
    truth(
        relay_process is not None and relay_process.poll() is None,
        describing="the relay to be serving still, after a program it could not start",
    )


@pytest.mark.parametrize("reachable", ["closed", "unnamed", "elsewhere", "unparsable"])
def test_a_client_that_reaches_no_relay_says_so(world: World, reachable: str) -> None:
    """A client with no relay to hand its command to exits as unanswered, saying why.

    That is a command that never answered, which is what the smoke's bound
    reports too, rather than a program exit the smoke would read as an answer.
    """
    environment = world.environment()
    said = "the smoke's relay did not answer"
    match reachable:
        case "closed":
            environment[ADDRESS] = a_closed_address()
        case "unnamed":
            environment.pop(ADDRESS, None)
        case "elsewhere":
            environment[ADDRESS] = "192.0.2.1:9"
            said = "not a loopback host:port"
        case _:
            environment[ADDRESS] = "a relay"
            said = "not a loopback host:port"

    answered = relay(environment, "status", "--json")

    equal(
        answered.returncode,
        unanswered_exit(),
        describing="the exit of a command the relay never answered",
    )
    contains(answered.stderr, said, describing="what it said")
    equal(answered.stdout, "", describing="what it printed, which is no answer")


def test_an_input_the_client_cannot_read_is_not_forwarded_as_its_end(
    world: World, tmp_path: Path
) -> None:
    """A standard input the client cannot read leaves the command unanswered, saying why.

    Forwarded as the end of the input, it would run the program on less than
    the smoke handed the command and report whatever that answered. The input
    is one the host refuses to read: a directory where a directory can be
    opened, and a file opened only for writing on Windows, where one cannot.
    """
    environment = relayed(world)
    fed = tmp_path / "fed"
    unreadable = (
        f"open({str(tmp_path / 'written')!r}, 'wb')"
        if sys.platform == "win32"
        else f"os.open({str(tmp_path)!r}, os.O_RDONLY)"
    )
    # A parent that hands the client that input, as the smoke's runner would
    # hand it the input it was given.
    handing = (
        f"import os, subprocess, sys; sys.exit(subprocess.call(sys.argv[1:], stdin={unreadable}))"
    )
    reading = (
        f"import sys; from pathlib import Path; Path({str(fed)!r}).write_text(sys.stdin.read())"
    )

    answered = run(
        [sys.executable, "-c", handing, str(RELAY_CLIENT), "-c", reading],
        cwd=REPO_ROOT,
        env=environment,
        timeout=60,
    )

    equal(
        answered.returncode,
        unanswered_exit(),
        describing="the exit of a command whose input could not be read",
    )
    contains(
        answered.stderr,
        "the smoke's relay did not answer: its standard input could not be read",
        describing="what it said",
    )
    equal(answered.stdout, "", describing="what it printed, which is no answer")
    passing(relay(environment, "-c", ECHO, "status"), describing="the next command")


@pytest.mark.skipif(
    sys.platform == "win32", reason="Windows arguments are UTF-16, so none is not UTF-8"
)
def test_an_argument_that_is_not_utf8_is_refused_rather_than_altered(world: World) -> None:
    """The client runs nothing with an argument it would have to change to send."""
    environment = relayed(world)

    answered = relay(environment, "-c", ECHO, os.fsdecode(b"\xffstatus"))

    equal(
        answered.returncode,
        unanswered_exit(),
        describing="the exit of a command the relay never answered",
    )
    contains(answered.stderr, "is not UTF-8", describing="what it said")
    equal(answered.stdout, "", describing="what the program printed, which never ran")


def test_a_relay_the_environment_does_not_describe_is_refused_by_the_world(world: World) -> None:
    """A relay that cannot start says why, and the world raises that rather than a bare wait."""
    with pytest.raises(RuntimeError, match=f"{AFTER} is '-1', which is not a count of commands"):
        world.relaying(hang_on=COUNTED, after=-1)

    equal(world.relay, None, describing="the relay the world holds after one failed to start")


def test_the_client_reads_the_address_and_the_frame_bound_the_relay_names() -> None:
    """The relay's address variable and frame bound are spelled once, and the client follows."""
    client = RELAY_CLIENT_SOURCE.read_text(encoding="utf-8")
    contains(
        client,
        f'const ADDRESS: &str = "{ADDRESS}";',
        describing="the relay's client, which must read the variable the relay's world sets",
    )
    bound = re.search(r"const MOST_FRAME_BYTES: u32 = (\d+) \* (\d+) \* (\d+);", client)
    equal(
        bound and int(bound[1]) * int(bound[2]) * int(bound[3]),
        MOST_FRAME_BYTES,
        describing="the most bytes one frame may carry, on the client's side of the wire",
    )


@pytest.mark.parametrize("missing", [PROGRAM, STATE])
def test_a_relay_missing_what_it_needs_says_so_before_listening(world: World, missing: str) -> None:
    """A relay started without a program or a state file refuses naming the variable."""
    environment = world.environment({PROGRAM: sys.executable, STATE: str(world.relay_state_file)})
    environment.pop(missing)

    with pytest.raises(RuntimeError, match=f"{missing} is not set"):
        RelayProcess.start(environment)


def test_a_client_that_goes_away_mid_message_costs_the_relay_nothing(world: World) -> None:
    """A connection closed partway through its arguments is dropped, and the next is served."""
    environment = relayed(world)
    with connected(environment) as connection:
        connection.sendall((1).to_bytes(4, "big") + (10).to_bytes(4, "big") + b"sta")

    answered = relay(environment, "-c", ECHO, "status")

    passing(answered, describing="the command after one that went away mid-message")
    equal(answered.stdout.strip(), json.dumps(["status"]), describing="what it answered")


def test_a_client_sending_more_arguments_than_any_is_refused(world: World) -> None:
    """An argument count past the most the relay takes is refused on the wire, naming it."""
    with connected(relayed(world)) as connection:
        connection.sendall((MOST_ARGUMENTS + 1).to_bytes(4, "big"))
        reply = received(connection)

    equal(int.from_bytes(reply[:4], "big", signed=True), 1, describing="the refusal's exit")
    contains(
        reply.decode("utf-8", errors="replace"),
        f"sent a length of {MOST_ARGUMENTS + 1}, over the most it takes",
        describing="what the relay said",
    )


BROKEN_REPLIES = {
    "oversized": (
        (0).to_bytes(4, "big") + (2**32 - 1).to_bytes(4, "big"),
        "over the most this takes",
    ),
    "truncated": ((0).to_bytes(4, "big"), "the smoke's relay did not answer"),
}


@pytest.mark.parametrize("broken", sorted(BROKEN_REPLIES))
def test_a_reply_the_client_cannot_read_leaves_the_command_unanswered(
    world: World, broken: str
) -> None:
    """A reply frame past the most the client takes, or one cut short, is no answer."""
    reply, said = BROKEN_REPLIES[broken]
    listener = socket.create_server(LISTEN_ON)
    host, port = listener.getsockname()[:2]

    def answer_brokenly() -> None:
        connection, _ = listener.accept()
        with connection:
            connection.sendall(reply)
            if broken == "oversized":
                with contextlib.suppress(OSError):
                    received(connection)

    answering = threading.Thread(target=answer_brokenly, daemon=True)
    answering.start()
    environment = world.environment({ADDRESS: f"{host}:{port}"})

    answered = relay(environment, "status", stdin="")
    answering.join(SETTLE_S)
    listener.close()

    equal(answered.returncode, unanswered_exit(), describing="the client's exit")
    contains(answered.stderr, said, describing="what the client said")
    equal(answered.stdout, "", describing="what it printed, which is no answer")


def test_input_past_the_most_the_relay_takes_stops_the_program_and_is_refused(
    world: World, tmp_path: Path
) -> None:
    """An input frame too long to take kills the program it was for, and the reply says why."""
    ran = tmp_path / "ran"
    reading = f"import sys; from pathlib import Path; sys.stdin.read(); Path({str(ran)!r}).touch()"
    argv = [b"-c", reading.encode()]
    with connected(relayed(world)) as connection:
        connection.sendall(
            len(argv).to_bytes(4, "big")
            + b"".join(len(argument).to_bytes(4, "big") + argument for argument in argv)
            + (MOST_FRAME_BYTES + 1).to_bytes(4, "big")
        )
        reply = received(connection)

    equal(int.from_bytes(reply[:4], "big", signed=True), 1, describing="the refusal's exit")
    contains(
        reply.decode("utf-8", errors="replace"),
        f"sent a length of {MOST_FRAME_BYTES + 1}, over the most it takes",
        describing="what the relay said",
    )
    equal(ran.exists(), False, describing="whether the program ran on past its input")


def test_input_refused_after_a_program_that_ended_first_is_still_refused(world: World) -> None:
    """An input frame the relay refuses is refused even when the program ended before it.

    The program exits without reading, so the frame before the refused one
    cannot be written to it; the reply still waits for that refusal rather
    than answering with the program's own exit.
    """
    unread = b"x" * (1024 * 1024)
    argv = [b"-c", b"pass"]
    with connected(relayed(world)) as connection:
        connection.sendall(
            len(argv).to_bytes(4, "big")
            + b"".join(len(argument).to_bytes(4, "big") + argument for argument in argv)
            + len(unread).to_bytes(4, "big")
            + unread
            + (MOST_FRAME_BYTES + 1).to_bytes(4, "big")
        )
        reply = received(connection)

    equal(int.from_bytes(reply[:4], "big", signed=True), 1, describing="the refusal's exit")
    contains(
        reply.decode("utf-8", errors="replace"),
        f"sent a length of {MOST_FRAME_BYTES + 1}, over the most it takes",
        describing="what the relay said",
    )


def test_a_relay_that_will_not_stop_when_asked_is_killed(world: World) -> None:
    """A relay still running once its stop is overdue is killed rather than waited on."""
    deaf = start(
        [sys.executable, "-c", "import time; time.sleep(600)"],
        cwd=REPO_ROOT,
        env=world.environment(),
    )
    relay_process = RelayProcess(process=deaf, address=f"{LISTEN_ON[0]}:1")

    relay_process.stop(within_s=0.5)

    truth(deaf.poll() is not None, describing="the relay that would not stop to have ended")


def test_the_client_s_unanswered_exit_is_none_of_the_program_s_own() -> None:
    """The exit that means the relay never answered is one the program it relays never gives."""
    declared = {
        int(status)
        for status in re.findall(r"Self::\w+ => (\d+),", EXITS_SOURCE.read_text(encoding="utf-8"))
    }

    truth(bool(declared), describing="the program's declared exits, read from failure.rs")
    absent(declared, unanswered_exit(), describing="the program's declared exits")


@pytest.mark.parametrize("stream", ["stdout", "stderr"])
def test_output_past_the_most_one_reply_carries_is_refused(world: World, stream: str) -> None:
    """A program answering more than the client would take is refused, naming the stream."""
    flooding = f"import sys; sys.{stream}.buffer.write(b'x' * {MOST_FRAME_BYTES + 1})"

    answered = relay(relayed(world), "-c", flooding)

    failing(answered, naming=f"more than {MOST_FRAME_BYTES} bytes on {stream}, over the most")
    equal(answered.stdout, "", describing="what reached the smoke of the flood")
    truth(len(answered.stderr) < 1024, describing="the refusal, rather than the flood, on stderr")


def test_an_argument_frame_that_is_not_utf8_is_refused_by_the_relay(world: World) -> None:
    """An argument the relay could only decode by altering it is refused on the wire."""
    with connected(relayed(world)) as connection:
        connection.sendall((1).to_bytes(4, "big") + (1).to_bytes(4, "big") + b"\xff")
        reply = received(connection)

    equal(int.from_bytes(reply[:4], "big", signed=True), 1, describing="the refusal's exit")
    contains(
        reply.decode("utf-8", errors="replace"),
        "can't decode byte 0xff",
        describing="what the relay said",
    )


def launching_once_ready(ready: Path, readied: list[float]) -> Callable[..., subprocess.Popen[str]]:
    """A launch that starts the real program and answers it only once `ready` exists.

    It stands where `RelayProcess.start` starts its relay, so the bound that
    method gives the relay to announce itself begins once the stand-in has
    demonstrably started rather than when its interpreter was asked to. Its
    output is left unread for that method to read. When it answered is
    appended to `readied`.
    """

    def launch(
        argv: list[str],
        *,
        cwd: Path | None = None,
        env: dict[str, str] | None = None,
        own_group: bool = False,
    ) -> subprocess.Popen[str]:
        process = start(argv, cwd=cwd, env=env, own_group=own_group)
        deadline = time.monotonic() + READY_S
        while not ready.is_file() and process.poll() is None:
            if time.monotonic() > deadline:
                process.kill()
                process.communicate()
                truth(False, describing=f"the stand-in relay to be ready inside {READY_S}s")
            time.sleep(0.05)
        truth(ready.is_file(), describing="the stand-in relay to be ready before it exited")
        readied.append(time.monotonic())
        return process

    return launch


@pytest.mark.parametrize("starting_s", [0.0, 1.5], ids=["promptly", "slowly"])
@pytest.mark.parametrize(
    ("announcing", "announced"),
    [(None, "''"), ("a relay", "'a relay'"), ("192.0.2.1:9", "'192.0.2.1:9'")],
    ids=["silent", "unparsable", "elsewhere"],
)
def test_a_relay_that_does_not_say_where_it_listens_is_refused_and_stopped(
    world: World,
    tmp_path: Path,
    monkeypatch: pytest.MonkeyPatch,
    announcing: str | None,
    announced: str,
    starting_s: float,
) -> None:
    """A relay silent past its bound, or announcing no loopback address, is killed and reported.

    How long the stand-in's interpreter takes to start is the host's, so the
    bound it is given starts only once it is ready: its first heartbeat
    written and, when it announces, that line flushed. `starting_s` makes it
    slower to start than that bound, which then changes nothing.
    """
    beating = tmp_path / "heartbeat"
    ready = tmp_path / "ready"
    impostor = tmp_path / "impostor_relay.py"
    impostor.write_text(
        f"import time\ntime.sleep({starting_s!r})\n"
        f"from pathlib import Path\nPath({str(beating)!r}).write_text('0')\n"
        + ("" if announcing is None else f"print({announcing!r}, flush=True)\n")
        + f"Path({str(ready)!r}).touch()\n"
        + HEARTBEAT.replace("sys.argv[1]", repr(str(beating))),
        encoding="utf-8",
    )
    readied: list[float] = []

    with monkeypatch.context() as patching:
        patching.setattr(worlds, "start", launching_once_ready(ready, readied))
        with pytest.raises(RuntimeError, match=f"announced {re.escape(announced)} rather than"):
            RelayProcess.start(world.environment(), within_s=0.5, relay=impostor)

    truth(
        time.monotonic() - readied[0] < SETTLE_S,
        describing="the refusal to come inside the bound",
    )
    truth(beating.is_file(), describing="the refused relay to have been running")
    last = beating.read_text(encoding="utf-8")
    time.sleep(1.0)
    equal(
        beating.read_text(encoding="utf-8"),
        last,
        describing="the refused relay's heartbeat, which a relay left running would go on",
    )


@pytest.mark.parametrize("oversized", ["one", "together"])
def test_arguments_past_the_most_the_relay_takes_are_refused(world: World, oversized: str) -> None:
    """One argument frame past the bound, or arguments past it together, are refused."""
    half = MOST_FRAME_BYTES // 2 + 1
    if oversized == "one":
        sent = (1).to_bytes(4, "big") + (MOST_FRAME_BYTES + 1).to_bytes(4, "big")
        said = f"sent a length of {MOST_FRAME_BYTES + 1}, over the most it takes"
    else:
        sent = (2).to_bytes(4, "big") + ((half).to_bytes(4, "big") + b"a" * half) * 2
        said = f"sent arguments over the most it takes ({MOST_FRAME_BYTES})"
    with connected(relayed(world)) as connection:
        connection.sendall(sent)
        reply = received(connection)

    equal(int.from_bytes(reply[:4], "big", signed=True), 1, describing="the refusal's exit")
    contains(reply.decode("utf-8", errors="replace"), said, describing="what the relay said")


def test_output_that_is_not_text_reaches_the_smoke_byte_for_byte(world: World) -> None:
    """What the program writes, text or not, is what the client writes on each stream."""
    writing = (
        "import sys; sys.stdout.buffer.write(bytes([0xff, 0xfe, 0x00, 0x0a])); "
        "sys.stderr.buffer.write(bytes([0x80, 0x0d, 0x0a]))"
    )
    client = start([str(RELAY_CLIENT), "-c", writing], cwd=REPO_ROOT, env=relayed(world))
    if client.stdin:
        client.stdin.close()
    output, error = (
        stream.buffer.read() if isinstance(stream, io.TextIOWrapper) else b""
        for stream in (client.stdout, client.stderr)
    )
    client.wait(timeout=SETTLE_S)

    equal(client.returncode, 0, describing="the program's exit")
    equal(output, bytes([0xFF, 0xFE, 0x00, 0x0A]), describing="the bytes on stdout")
    equal(error, bytes([0x80, 0x0D, 0x0A]), describing="the bytes on stderr")


def test_a_changed_client_source_is_given_a_compiled_file_of_its_own(tmp_path: Path) -> None:
    """A changed client source names a compiled file of its own, never a stale build."""
    changed = tmp_path / RELAY_CLIENT_SOURCE.name
    changed.write_text(
        RELAY_CLIENT_SOURCE.read_text(encoding="utf-8") + "// changed\n", encoding="utf-8"
    )

    equal(relay_client(), RELAY_CLIENT, describing="where the committed source is compiled to")
    equal(build_the_relay_client(), RELAY_CLIENT, describing="what the build answers")
    truth(RELAY_CLIENT.is_file(), describing="the compiled client to be there")
    truth(
        relay_client(changed) != RELAY_CLIENT,
        describing="a changed source to be compiled to a file of its own",
    )


def test_relaying_again_replaces_the_relay_with_one_under_the_new_configuration(
    world: World,
) -> None:
    """A second relay stops the first, and it is the second that serves, as configured."""
    before = relayed(world)
    first = world.relay.process if world.relay else None

    after = relayed(world, hang_on="hold", after=0)

    truth(
        first is not None and first.poll() is not None, describing="the first relay to have ended"
    )
    failing(relay(before, "status"), naming="the smoke's relay did not answer")
    passing(relay(after, "-c", ECHO, "status"), describing="a command through the second relay")
    held = start([str(RELAY_CLIENT), "hold"], cwd=REPO_ROOT, env=after)
    until(
        lambda: RelayState.read(world.relay_state_file, armed=True).seen == 1,
        describing="the second relay to hold the command it was configured to hang",
    )
    truth(held.poll() is None, describing="the held command to be unanswered")
    held.kill()
    held.communicate()


def test_a_client_that_goes_away_while_feeding_input_ends_the_program_s_input(
    world: World, tmp_path: Path
) -> None:
    """A client gone mid-input closes the program's input, and the relay serves the next command."""
    fed = tmp_path / "fed"
    reading = (
        f"import sys; from pathlib import Path; Path({str(fed)!r}).write_text(sys.stdin.read())"
    )
    argv = [b"-c", reading.encode()]
    environment = relayed(world)
    with connected(environment) as connection:
        connection.sendall(
            len(argv).to_bytes(4, "big")
            + b"".join(len(argument).to_bytes(4, "big") + argument for argument in argv)
            + len(b"partial").to_bytes(4, "big")
            + b"partial"
        )

    until(fed.is_file, describing="the program to have been given the end of its input")
    equal(fed.read_text(encoding="utf-8"), "partial", describing="what the program was fed")
    passing(relay(environment, "-c", ECHO, "status"), describing="the next command")
