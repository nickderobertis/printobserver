"""What a replay runs as the harness in this project's tests.

It stands where the agent's process stands: it runs the commands scripted for its
turn, through the program a real turn runs them through and with the environment
a real turn is given, writes down everything it could observe, and hands the
provider's answer to oneharness's own published responder. So everything from the
run request down is real, and only the paid model is not.

Given the script file and then whatever the harness was asked, its prompt on its
standard input as the engine hands one over. Each run in the script is
`{"commands": [[word, ...], ...], "usage": {...}, "compact": bool}`, and
`"unreadable": true` answers something the port must ask for again. `"calls"`
scripts the usage of each model call the run streams, one by default carrying its
`usage`, and `"disposition"` what its assessment answers with. In a command's
words, `{print}`, `{actor}` and `{event}` stand for what the prompt named, and in an
operator's message `{file}` and `{frame}` for the file it asks to start and the frame
it hands over; `{new_print}` is an identifier no print holds, and a file a run reads may
be `{looked}`, the frame its last look named. A command is a list of
words, run as written, or one string, which is a shell line as the agent wrote it.

The shell is the harness's to refuse, so a run started with permission rules takes
the one way Claude Code's `dontAsk` mode does: a shell line runs only when every
command it chains begins with a command a rule allows, and is otherwise refused and
answered as refused. A run whose prompt asks for no schema answers in words, its
`reply`.
"""

from __future__ import annotations

import json
import os
import re
import shlex
import shutil
import sys
import time
from pathlib import Path
from typing import Any, Literal, NotRequired, TypedDict

# Started with the environment a turn is given, which names nothing of this tree.
sys.path.insert(0, str(Path(__file__).resolve().parents[2] / "tools" / "repo-checks" / "src"))
sys.path.insert(0, str(Path(__file__).resolve().parents[2] / "tests" / "skilltest"))

from repo_checks.shell import run
from shell_lines import commands as run_by

Confidence = Literal["low", "medium", "high"]
Disposition = Literal["continue", "watch", "stop"]


class Assessment(TypedDict):
    """What a turn answers with, in the shape the assessment schema asks for."""

    summary: str
    confidence: Confidence
    should_continue: bool
    did: str
    why: str
    escalating: bool
    disposition: Disposition


TIMEOUT_S = 120
SEEN_FILE = "seen.jsonl"
#: The last turn prompt this conversation was handed, which a run asked again for its
#: answer is filled from: the engine's feedback names no print, and a real session
#: still holds the prompt that did.
CONVERSATION_FILE = "conversation.txt"
COUNTER_FILE = "turns.count"
#: The model the responder is scripted to say it is.
MOCK_MODEL = "mock-model"
ASSESSMENT: Assessment = {
    "summary": "scripted",
    "confidence": "high",
    "should_continue": True,
    "did": "scripted",
    "why": "scripted",
    "escalating": False,
    "disposition": "continue",
}
#: What a prompt held to the schema by its own words carries, in the engine's first
#: instruction and in its feedback asking again alike.
SCHEMA_INSTRUCTION = "JSON Schema"


class ScriptedRun(TypedDict):
    """One harness run of a script."""

    commands: NotRequired[list[list[str] | str]]
    #: What a run asked for no schema answers.
    reply: NotRequired[str]
    usage: NotRequired[dict[str, int]]
    #: Each model call the run streams, by its usage; one carrying `usage` by default.
    calls: NotRequired[list[dict[str, int]]]
    disposition: NotRequired[Disposition]
    #: How long the turn takes beyond its commands, in seconds.
    stall_s: NotRequired[float]
    #: Files the turn opens with its file tool.
    reads: NotRequired[list[str]]
    #: Whether the transcript writes each call twice, as one streamed over two messages.
    streamed_twice: NotRequired[bool]
    compact: NotRequired[bool]
    unreadable: NotRequired[bool]


class Script(TypedDict):
    """What a test scripts a mock replay with; the last run stands for every later one."""

    session: str
    responder: str
    turns: list[ScriptedRun]
    #: How a compaction of the session goes, when the port asks for one.
    compaction: NotRequired[Compaction]


class Compaction(TypedDict):
    """How a scripted compaction goes: the sizes it reports and how long it takes."""

    before: NotRequired[int]
    after: NotRequired[int]
    stall_s: NotRequired[float]
    #: The harness fails it, as a compaction Claude Code could not complete.
    fail: NotRequired[bool]


def _run_is_whole(run_of: object) -> bool:
    """Whether one scripted run holds commands as lists of words, and flags as flags."""
    if not isinstance(run_of, dict):
        return False
    commands = run_of.get("commands", [])
    return (
        isinstance(commands, list)
        and all(
            (isinstance(words, list) and words and all(isinstance(word, str) for word in words))
            or (isinstance(words, str) and words)
            for words in commands
        )
        and isinstance(run_of.get("reply", ""), str)
        and isinstance(run_of.get("usage", {}), dict)
        and all(isinstance(count, int) for count in run_of.get("usage", {}).values())
        and isinstance(run_of.get("calls", []), list)
        and all(
            isinstance(call, dict) and all(isinstance(count, int) for count in call.values())
            for call in run_of.get("calls", [])
        )
        and run_of.get("disposition", "continue") in {"continue", "watch", "stop"}
        and isinstance(run_of.get("compact", False), bool)
        and isinstance(run_of.get("unreadable", False), bool)
        and isinstance(run_of.get("streamed_twice", False), bool)
        and isinstance(run_of.get("stall_s", 0), (int, float))
        and 0 <= run_of.get("stall_s", 0) < float("inf")
        and isinstance(run_of.get("reads", []), list)
        and all(isinstance(path, str) for path in run_of.get("reads", []))
    )


#: An identifier in the shape the server mints a print's in, which no print holds.
NEW_PRINT = "01900000-0000-7000-8000-000000000000"
#: A Claude Code shell rule allowing every command that begins with its prefix.
SHELL_RULE = re.compile(r"^Bash\((?P<prefix>[^:()*]+):\*\)$")


def _prompt(arguments: list[str], stdin: str) -> str:
    """What the run was asked: its standard input, for a run naming an `--input-format`.

    A stream-json input is one user frame whose text blocks are the prompt; a text
    one is the prompt itself. A run naming none is asked on its argv, after `-p`.
    """
    if "--input-format" in arguments:
        texts = []
        for line in stdin.splitlines():
            try:
                frame = json.loads(line)
            except ValueError:
                continue
            content = frame.get("message", {}).get("content") if isinstance(frame, dict) else None
            texts.extend(
                block["text"]
                for block in (content if isinstance(content, list) else ())
                if isinstance(block, dict) and isinstance(block.get("text"), str)
            )
        return "\n".join(texts) if texts else stdin
    if "-p" in arguments and arguments.index("-p") + 1 < len(arguments):
        return arguments[arguments.index("-p") + 1]
    return next((word for word in arguments if "printobserver context" in word), "")


def _allowed_prefixes(arguments: list[str]) -> list[str] | None:
    """The command prefixes the run's rules allow the shell, or none when no rule narrows it."""
    if "--allowedTools" not in arguments:
        return None
    rules = []
    for word in arguments[arguments.index("--allowedTools") + 1 :]:
        if word.startswith("--"):
            break
        rules.append(word)
    return [found["prefix"] for rule in rules if (found := SHELL_RULE.match(rule))]


# llmlint: ignore[contracts_have_one_source_or_a_drift_gate] See suppressions.toml.
def _allowed(line: str, prefixes: list[str] | None) -> bool:
    """Whether every command a shell line runs begins with a command a rule allows."""
    if prefixes is None:
        return True
    commands = [" ".join(words) for words in run_by(line)]
    return bool(commands) and all(
        any(command == prefix or command.startswith(f"{prefix} ") for prefix in prefixes)
        for command in commands
    )


def _filled(word: str, prompt: str) -> str:
    print_id = re.search(r"printobserver context --print-id (\S+)", prompt) or re.search(
        r"You started the print (\S+) and follow it", prompt
    )
    actor = re.search(r"--actor '([^']+)'", prompt)
    event = re.search(r'"id": ?"([^"]+)"', prompt)
    file = re.search(r"start the print (\S+),", prompt)
    frame = re.search(r"as it is now is (\S+?)\. ", prompt)
    if "{new_print}" in word:
        word = word.replace("{new_print}", NEW_PRINT)
    for placeholder, found in (
        ("{print}", print_id),
        ("{actor}", actor),
        ("{event}", event),
        ("{file}", file),
        ("{frame}", frame),
    ):
        if placeholder in word:
            if found is None:
                msg = f"the prompt names nothing for {placeholder}"
                raise ValueError(msg)
            word = word.replace(placeholder, found[1])
    return word


class CommandResult(TypedDict):
    """One command a scripted turn ran, and what it answered."""

    words: list[str]
    status: int
    out: str
    err: str


class Seen(TypedDict):
    """What one harness run could observe, as this writes it down for a test."""

    turn: int
    prompt: str
    environment: dict[str, str]
    cwd: str
    workspace: list[str]
    arguments: list[str]
    ran: list[CommandResult]
    assessment: Assessment


#: The status a refused shell line is written down with: it never ran.
REFUSED = -1


def _ran(command: list[str] | str, prompt: str, prefixes: list[str] | None) -> CommandResult:
    """Run one scripted command, unless the run's rules refuse it as Claude Code would."""
    if isinstance(command, str):
        line = _filled(command, prompt)
        words = shlex.split(line)
    else:
        words = [_filled(word, prompt) for word in command]
        line = shlex.join(words)
    if not _allowed(line, prefixes):
        return {"words": words, "status": REFUSED, "out": "", "err": "permission denied"}
    argv = words
    if isinstance(command, str) and len(run_by(line)) > 1:
        # A line running several commands, every one of them allowed, is the shell's to run.
        shell = shutil.which("sh")
        if shell is None:
            return {"words": words, "status": REFUSED, "out": "", "err": "no shell to run it"}
        argv = [shell, "-c", line]
    # llmlint: ignore[async_typed_clients_at_boundaries] See suppressions.toml.
    done = run(argv, timeout=TIMEOUT_S)
    status, out, err = done.returncode, done.stdout, done.stderr
    return {"words": words, "status": status, "out": out, "err": err}


def _looked(ran: list[CommandResult]) -> str | None:
    """The frame the run's last look that answered named, if one did."""
    for result in reversed(ran):
        if result["status"] != 0 or result["words"][1:2] != ["look"]:
            continue
        try:
            answered = json.loads(result["out"])
        except ValueError:
            continue
        path = answered.get("image_path") if isinstance(answered, dict) else None
        if isinstance(path, str):
            return path
    return None


#: What a file a run reads is written as to stand for the frame its last look named.
LOOKED = "{looked}"


def _named_look(path: str, looked: str | None) -> str:
    """A file the run reads, `{looked}` standing for the frame its last look named."""
    if LOOKED not in path:
        return path
    if looked is None:
        msg = f"the run reads {path} and no look of it named a frame"
        raise ValueError(msg)
    return path.replace(LOOKED, looked)


def _line(command: list[str] | str) -> str:
    """A scripted command as the agent's shell call writes it."""
    return command if isinstance(command, str) else " ".join(command)


#: Where each compaction a run was started for is written down, one per line.
COMPACTIONS_FILE = "compactions.jsonl"


def _compacted(script: Script, directory: Path, arguments: list[str]) -> int:
    """Compact the session as Claude Code's `/compact` reports one, after `stall_s`.

    The script's `compaction` says the size it leaves and how long it takes; the
    session continues under a fork of its own.
    """
    compaction = script.get("compaction", {})
    fails = compaction.get("fail", False)
    if not isinstance(fails, bool):
        print(f"the script's compaction `fail` is not a boolean: {fails!r}", file=sys.stderr)
        return 2
    started = time.time()
    time.sleep(compaction.get("stall_s", 0))
    resumed = arguments[arguments.index("--resume") + 1] if "--resume" in arguments else ""
    fork = f"{resumed}-compacted"
    before, after = compaction.get("before", 150_000), compaction.get("after", 10_000)
    lines = [
        {"type": "system", "subtype": "status", "status": "compacting", "session_id": fork},
        {
            "type": "system",
            "subtype": "status",
            "status": None,
            "compact_result": "success",
            "session_id": fork,
        },
        {
            "type": "system",
            "subtype": "compact_boundary",
            "session_id": fork,
            "compact_metadata": {"trigger": "manual", "pre_tokens": before, "post_tokens": after},
        },
        {
            "type": "result",
            "subtype": "success",
            "is_error": False,
            "result": "",
            "session_id": fork,
            "model_usage_since_last_result": {
                MOCK_MODEL: {"inputTokens": 5, "outputTokens": 900, "cacheReadInputTokens": before}
            },
        },
    ]
    with (directory / COMPACTIONS_FILE).open("a", encoding="utf-8") as log:
        log.write(json.dumps({"resumed": resumed, "started": started, "ended": time.time()}) + "\n")
    if fails:
        print("the scripted compaction failed", file=sys.stderr)
        return 1
    # llmlint: ignore[async_typed_clients_at_boundaries] See suppressions.toml.
    return run(
        [script["responder"], *arguments],
        env={
            **os.environ,
            "MOCK_STDOUT": "\n".join(json.dumps(line) for line in lines),
            "MOCK_PRESERVE_STDOUT": "1",
        },
        capture=False,
    ).returncode


def main(script_path: str, arguments: list[str]) -> int:
    """Take one turn of the script, and answer through the published responder."""
    directory = Path(script_path).parent
    script: Script = json.loads(Path(script_path).read_text(encoding="utf-8"))
    if not isinstance(script, dict):
        print(f"{script_path} is no script", file=sys.stderr)
        return 2
    if not isinstance(script.get("turns"), list) or not script["turns"]:
        print(f"{script_path} scripts no run", file=sys.stderr)
        return 2
    if not all(_run_is_whole(run_of) for run_of in script["turns"]):
        print(f"{script_path} scripts a run that is no document", file=sys.stderr)
        return 2
    if not isinstance(script.get("session"), str) or not isinstance(script.get("responder"), str):
        print(f"{script_path} names no responder", file=sys.stderr)
        return 2
    if "/compact" in arguments:
        return _compacted(script, directory, arguments)
    counter = directory / COUNTER_FILE
    number = int(counter.read_text(encoding="utf-8")) if counter.is_file() else 0
    counter.write_text(str(number + 1), encoding="utf-8")
    scripted = script["turns"][min(number, len(script["turns"]) - 1)]
    stdin = sys.stdin.read() if "--input-format" in arguments else ""
    prompt = _prompt(arguments, stdin)
    conversation = directory / CONVERSATION_FILE
    if "printobserver context --print-id" in prompt:
        conversation.write_text(prompt, encoding="utf-8")
    held = conversation.read_text(encoding="utf-8") if conversation.is_file() else ""
    filling = prompt if "printobserver context --print-id" in prompt else f"{prompt}\n{held}"
    prefixes = _allowed_prefixes(arguments)
    ran = [_ran(command, filling, prefixes) for command in scripted.get("commands", [])]
    looked = _looked(ran)
    assessment: Assessment = {
        **ASSESSMENT,
        "summary": f"scripted run {number}",
        "disposition": scripted.get("disposition", "continue"),
    }
    seen: Seen = {
        "turn": number,
        "prompt": prompt,
        "environment": dict(os.environ),
        "cwd": str(Path.cwd()),
        "workspace": sorted(path.name for path in Path.cwd().iterdir()),
        "arguments": arguments,
        "ran": ran,
        "assessment": assessment,
    }
    with (directory / SEEN_FILE).open("a", encoding="utf-8") as log:
        log.write(json.dumps(seen) + "\n")
    time.sleep(scripted.get("stall_s", 0))
    # A run continuing a conversation is handed its id, and one opening it is given a new one.
    resumed = arguments.index("--resume") + 1 if "--resume" in arguments else len(arguments)
    session = (
        arguments[resumed]
        if resumed < len(arguments)
        else f"mock-session-{script['session']}-{number}"
    )
    usage = scripted.get("usage", {"input_tokens": 10, "output_tokens": 5})
    # llmlint: ignore[contracts_have_one_source_or_a_drift_gate] See suppressions.
    transcript = Path(os.environ["CLAUDE_CONFIG_DIR"]) / "projects" / "workspace"
    transcript.mkdir(parents=True, exist_ok=True)
    # llmlint: ignore[contracts_have_one_source_or_a_drift_gate] See suppressions.toml.
    lines: list[dict[str, Any]] = []
    if scripted.get("compact"):
        # llmlint: ignore[contracts_have_one_source_or_a_drift_gate] See suppressions.toml.
        lines.append({"type": "system", "subtype": "compact_boundary"})
    # llmlint: ignore[contracts_have_one_source_or_a_drift_gate] See suppressions.
    message = {
        "type": "assistant",
        "message": {
            "model": MOCK_MODEL,
            "content": [
                *(
                    {
                        "type": "tool_use",
                        "id": f"call-{number}-{index}",
                        "name": "Bash",
                        "input": {"command": _filled(_line(command), filling)},
                    }
                    for index, command in enumerate(scripted.get("commands", []))
                ),
                # llmlint: ignore[contracts_have_one_source_or_a_drift_gate] See suppressions.toml.
                *(
                    {
                        "type": "tool_use",
                        "id": f"read-{number}-{index}",
                        "name": "Read",
                        "input": {"file_path": _filled(_named_look(path, looked), filling)},
                    }
                    for index, path in enumerate(scripted.get("reads", []))
                ),
            ],
        },
    }
    lines.extend([message] * (2 if scripted.get("streamed_twice") else 1))
    with (transcript / f"{session}.jsonl").open("a", encoding="utf-8") as kept:
        kept.writelines(json.dumps(line) + "\n" for line in lines)
    answer: dict[str, Any] = (
        {"result": "no answer in the shape asked for"}
        if scripted.get("unreadable")
        else {"result": json.dumps(assessment), "structured_output": assessment}
        if "--json-schema" in arguments or SCHEMA_INSTRUCTION in prompt
        else {"result": scripted.get("reply", "scripted reply")}
    )
    # llmlint: ignore[contracts_have_one_source_or_a_drift_gate] See suppressions.
    result = {
        "type": "result",
        "subtype": "success",
        "session_id": session,
        "usage": usage,
        # llmlint: ignore[contracts_have_one_source_or_a_drift_gate] See suppressions.toml.
        "modelUsage": {MOCK_MODEL: usage},
        **answer,
    }
    stdout = json.dumps(result)
    if "stream-json" in arguments:
        # Claude Code streams one assistant message per model call before its result.
        streamed = [
            {
                "type": "assistant",
                "session_id": session,
                "message": {
                    "id": f"msg-{number}-{index}",
                    "model": MOCK_MODEL,
                    "content": [{"type": "text", "text": "scripted"}],
                    "usage": call,
                },
            }
            for index, call in enumerate(scripted.get("calls", [usage]))
        ]
        stdout = "\n".join(json.dumps(line) for line in [*streamed, result])
    responder = script["responder"]
    # llmlint: ignore[async_typed_clients_at_boundaries] See suppressions.toml.
    return run(
        [responder, *arguments],
        env={**os.environ, "MOCK_STDOUT": stdout},
        capture=False,
    ).returncode


if __name__ == "__main__":
    if len(sys.argv) < 2:
        sys.exit("usage: mock_turn.py <script> [what the harness was asked]")
    raise SystemExit(main(sys.argv[1], sys.argv[2:]))
