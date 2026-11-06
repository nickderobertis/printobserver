"""What a replay runs as the harness in this project's tests.

It stands where the agent's process stands: it runs the commands scripted for its
turn, through the program a real turn runs them through and with the environment
a real turn is given, writes down everything it could observe, and hands the
provider's answer to oneharness's own published responder. So everything from the
run request down is real, and only the paid model is not.

Given the script file and then whatever the harness was asked. Each run in the
script is `{"commands": [[word, ...], ...], "usage": {...}, "compact": bool}`, and
`"unreadable": true` answers something the engine must re-prompt. In a command's
words, `{print}`, `{actor}` and `{event}` stand for what the prompt named.
"""

from __future__ import annotations

import json
import os
import re
import sys
from pathlib import Path
from typing import Any, Literal, NotRequired, TypedDict

# Started with the environment a turn is given, which names nothing of this tree.
sys.path.insert(0, str(Path(__file__).resolve().parents[2] / "tools" / "repo-checks" / "src"))

from repo_checks.shell import run

Confidence = Literal["low", "medium", "high"]


class Assessment(TypedDict):
    """What a turn answers with, in the shape the assessment schema asks for."""

    summary: str
    confidence: Confidence
    should_continue: bool
    did: str
    why: str
    escalating: bool


TIMEOUT_S = 120
SEEN_FILE = "seen.jsonl"
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
}


class ScriptedRun(TypedDict):
    """One harness run of a script."""

    commands: NotRequired[list[list[str]]]
    usage: NotRequired[dict[str, int]]
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


def _run_is_whole(run_of: object) -> bool:
    """Whether one scripted run holds commands as lists of words, and flags as flags."""
    if not isinstance(run_of, dict):
        return False
    commands = run_of.get("commands", [])
    return (
        isinstance(commands, list)
        and all(isinstance(words, list) and words for words in commands)
        and all(isinstance(word, str) for words in commands for word in words)
        and isinstance(run_of.get("usage", {}), dict)
        and all(isinstance(count, int) for count in run_of.get("usage", {}).values())
        and isinstance(run_of.get("compact", False), bool)
        and isinstance(run_of.get("unreadable", False), bool)
        and isinstance(run_of.get("streamed_twice", False), bool)
        and isinstance(run_of.get("reads", []), list)
        and all(isinstance(path, str) for path in run_of.get("reads", []))
    )


def _prompt(arguments: list[str]) -> str:
    return next((word for word in arguments if "printobserver context" in word), "")


def _filled(word: str, prompt: str) -> str:
    print_id = re.search(r"printobserver context --print-id (\S+)", prompt)
    actor = re.search(r"--actor '([^']+)'", prompt)
    event = re.search(r'"id": "([^"]+)"', prompt)
    return (
        word.replace("{print}", print_id[1] if print_id else "")
        .replace("{actor}", actor[1] if actor else "")
        .replace("{event}", event[1] if event else "")
    )


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


def _ran(words: list[str]) -> CommandResult:
    # llmlint: ignore[async_typed_clients_at_boundaries] See suppressions.toml.
    done = run(words, timeout=TIMEOUT_S)
    return {"words": words, "status": done.returncode, "out": done.stdout, "err": done.stderr}


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
    counter = directory / COUNTER_FILE
    number = int(counter.read_text(encoding="utf-8")) if counter.is_file() else 0
    counter.write_text(str(number + 1), encoding="utf-8")
    scripted = script["turns"][min(number, len(script["turns"]) - 1)]
    prompt = _prompt(arguments)
    ran = [
        _ran([_filled(word, prompt) for word in words]) for words in scripted.get("commands", [])
    ]
    assessment: Assessment = {**ASSESSMENT, "summary": f"scripted run {number}"}
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
    session = f"mock-session-{script['session']}"
    usage = scripted.get("usage", {"input_tokens": 10, "output_tokens": 5})
    transcript = Path(os.environ["CLAUDE_CONFIG_DIR"]) / "projects" / "workspace"
    transcript.mkdir(parents=True, exist_ok=True)
    # llmlint: ignore[contracts_have_one_source_or_a_drift_gate] See suppressions.toml.
    lines: list[dict[str, Any]] = []
    if scripted.get("compact"):
        # llmlint: ignore[contracts_have_one_source_or_a_drift_gate] See suppressions.toml.
        lines.append({"type": "system", "subtype": "compact_boundary"})
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
                        "input": {"command": " ".join(words)},
                    }
                    for index, words in enumerate(scripted.get("commands", []))
                ),
                # llmlint: ignore[contracts_have_one_source_or_a_drift_gate] See suppressions.toml.
                *(
                    {
                        "type": "tool_use",
                        "id": f"read-{number}-{index}",
                        "name": "Read",
                        "input": {"file_path": path},
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
    )
    result = {
        "type": "result",
        "subtype": "success",
        "session_id": session,
        "usage": usage,
        "modelUsage": {MOCK_MODEL: usage},
        **answer,
    }
    responder = script["responder"]
    # llmlint: ignore[async_typed_clients_at_boundaries] See suppressions.toml.
    return run(
        [responder, *arguments],
        env={**os.environ, "MOCK_STDOUT": json.dumps(result)},
        capture=False,
    ).returncode


if __name__ == "__main__":
    raise SystemExit(main(sys.argv[1], sys.argv[2:]))
