"""What a replay runs as the harness in this project's tests.

It stands where the agent's process stands: it runs the commands scripted for its
turn, through the program a real turn runs them through and with the environment
a real turn is given, writes down everything it could observe, and hands the
provider's answer to oneharness's own published responder. So everything from the
run request down is real, and only the paid model is not.

Given the script file and then whatever the harness was asked. Each turn in the
script is `{"commands": [[word, ...], ...], "usage": {...}, "compact": bool}`. In
a command's words, `{print}`, `{actor}` and `{event}` stand for what the prompt
named.
"""

from __future__ import annotations

import json
import os
import re
import sys
from pathlib import Path
from typing import Any

# Started with the environment a turn is given, which names nothing of this tree.
sys.path.insert(0, str(Path(__file__).resolve().parents[2] / "tools" / "repo-checks" / "src"))

from repo_checks.shell import run

TIMEOUT_S = 120
SEEN_FILE = "seen.jsonl"
COUNTER_FILE = "turns.count"
#: The model the responder is scripted to say it is.
MOCK_MODEL = "mock-model"
ASSESSMENT = {
    "summary": "scripted",
    "confidence": "high",
    "should_continue": True,
    "did": "scripted",
    "why": "scripted",
    "escalating": False,
}


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


def _ran(words: list[str]) -> dict[str, Any]:
    done = run(words, timeout=TIMEOUT_S)
    return {"words": words, "status": done.returncode, "out": done.stdout, "err": done.stderr}


def main(script_path: str, arguments: list[str]) -> int:
    """Take one turn of the script, and answer through the published responder."""
    directory = Path(script_path).parent
    script = json.loads(Path(script_path).read_text(encoding="utf-8"))
    counter = directory / COUNTER_FILE
    number = int(counter.read_text(encoding="utf-8")) if counter.is_file() else 0
    counter.write_text(str(number + 1), encoding="utf-8")
    scripted = script["turns"][min(number, len(script["turns"]) - 1)]
    prompt = _prompt(arguments)
    ran = [
        _ran([_filled(word, prompt) for word in words]) for words in scripted.get("commands", [])
    ]
    seen = {
        "turn": number,
        "prompt": prompt,
        "environment": dict(os.environ),
        "cwd": str(Path.cwd()),
        "workspace": sorted(path.name for path in Path.cwd().iterdir()),
        "arguments": arguments,
        "ran": ran,
    }
    with (directory / SEEN_FILE).open("a", encoding="utf-8") as log:
        log.write(json.dumps(seen) + "\n")
    session = f"mock-session-{script['session']}"
    usage = scripted.get("usage", {"input_tokens": 10, "output_tokens": 5})
    transcript = Path(os.environ["CLAUDE_CONFIG_DIR"]) / "projects" / "workspace"
    transcript.mkdir(parents=True, exist_ok=True)
    lines: list[dict[str, Any]] = []
    if scripted.get("compact"):
        lines.append({"type": "system", "subtype": "compact_boundary"})
    lines.append(
        {
            "type": "assistant",
            "message": {
                "model": MOCK_MODEL,
                "content": [
                    {"type": "tool_use", "name": "Bash", "input": {"command": " ".join(words)}}
                    for words in scripted.get("commands", [])
                ],
            },
        }
    )
    with (transcript / f"{session}.jsonl").open("a", encoding="utf-8") as kept:
        kept.writelines(json.dumps(line) + "\n" for line in lines)
    result = {
        "type": "result",
        "subtype": "success",
        "session_id": session,
        "result": json.dumps(ASSESSMENT),
        "structured_output": ASSESSMENT,
        "usage": usage,
        "modelUsage": {MOCK_MODEL: usage},
    }
    responder = script["responder"]
    return run(
        [responder, *arguments],
        env={**os.environ, "MOCK_STDOUT": json.dumps(result)},
        capture=False,
    ).returncode


if __name__ == "__main__":
    raise SystemExit(main(sys.argv[1], sys.argv[2:]))
