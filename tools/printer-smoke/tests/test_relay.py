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

Every relay here is started through `World`, the way the suite starts one, and
driven through its client, the way the smoke drives it, in front of a program
that is the interpreter itself told what to answer. So every test here is also
the drift check between the relay's two halves: `relay.py` and
`relay_client.rs` each write their own side of one wire, and a change to either
that the other did not follow fails here.
"""

from __future__ import annotations

import contextlib
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
from world import RELAY_CLIENT, RELAY_CLIENT_SOURCE, REPO_ROOT, RelayProcess, World

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
        environment["PRINTOBSERVER_SMOKE_PROGRAM"],
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


@pytest.mark.parametrize("reachable", ["closed", "unnamed"])
def test_a_client_that_reaches_no_relay_says_so(world: World, reachable: str) -> None:
    """A client with no relay to hand its command to exits as unanswered, saying why.

    That is a command that never answered, which is what the smoke's bound
    reports too, rather than a program exit the smoke would read as an answer.
    """
    environment = world.environment()
    if reachable == "closed":
        environment[ADDRESS] = a_closed_address()
    else:
        environment.pop(ADDRESS, None)

    answered = relay(environment, "status", "--json")

    equal(
        answered.returncode,
        unanswered_exit(),
        describing="the exit of a command the relay never answered",
    )
    contains(answered.stderr, "the smoke's relay did not answer", describing="what it said")
    equal(answered.stdout, "", describing="what it printed, which is no answer")


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


def test_the_client_reads_the_address_the_relay_names() -> None:
    """The variable naming where the relay listens is spelled once, and the client follows it."""
    contains(
        RELAY_CLIENT_SOURCE.read_text(encoding="utf-8"),
        f'const ADDRESS: &str = "{ADDRESS}";',
        describing="the relay's client, which must read the variable the relay's world sets",
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


#: What a relay that misbehaves sends, and what the client says of it.
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
