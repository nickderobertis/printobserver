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
that is the interpreter itself told what to answer.
"""

from __future__ import annotations

import json
import subprocess
import sys
import time
from collections.abc import Callable
from pathlib import Path

from relay import SERVED_BY, STAGING_SUFFIX, RelayState
from repo_checks.expect import absent, contains, equal, failing, passing, truth
from repo_checks.shell import run, start
from world import RELAY_CLIENT, REPO_ROOT, World

#: The command the relay is told to count. It is never given reason to hang.
COUNTED = "set-feedrate-factor"

#: What the program answers: the arguments the relay ran it with.
ECHO = "import json, sys; print(json.dumps(sys.argv[1:]))"

#: What the program answers: the process id of the relay that ran it.
SERVED = f"import os; print(os.environ[{SERVED_BY!r}])"

#: A program that answers with every stream and an exit of its own, and echoes
#: what it was fed.
ANSWERING = (
    "import sys; sys.stdout.write('fed ' + sys.stdin.read()); "
    "sys.stderr.write('said on stderr'); sys.exit(3)"
)

#: A program that never ends, and says it is still running by rewriting a file.
HEARTBEAT = (
    "import sys, time\n"
    "from pathlib import Path\n"
    "beat = 0\n"
    "while True:\n"
    "    beat += 1\n"
    "    Path(sys.argv[1]).write_text(str(beat))\n"
    "    time.sleep(0.05)\n"
)

#: How long a test here waits for something the relay is doing to show.
SETTLE_S = 30.0


def relayed(
    world: World, *, hang_on: str = COUNTED, after: int = 99, armed_by: str = ""
) -> dict[str, str]:
    """Start `world`'s relay in front of the interpreter, and answer the environment it needs.

    Args:
        world: The world to start it in.
        hang_on: The command it hangs.
        after: How many of that command it answers first.
        armed_by: The command that arms the hang.

    Returns:
        The environment its client is run under, as the smoke's commands are.
    """
    return world.environment(
        world.relaying(
            hang_on=hang_on, after=after, armed_by=armed_by, program=Path(sys.executable)
        )
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


def test_a_state_the_relay_did_not_write_stops_it_before_the_program(world: World) -> None:
    """A file holding anything but a relay state is a defect to stop on, not to count from.

    Nothing but the relay writes its state file, so what is not a state there
    is not a count to carry on from; the relay refuses naming the file and what
    it held, and the program is never run.
    """
    state_file = world.relay_state_file
    state_file.write_text(json.dumps({"armed": "yes", "seen": 3}), encoding="utf-8")

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
