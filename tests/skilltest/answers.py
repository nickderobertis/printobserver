"""What each stubbed `printobserver` command answers, in the program's own renderings.

The program prints the document the supervisor sent: as labelled lines by
default, or as the document itself under `--json`
(`crates/printobserver/src/render.rs`). So every answer here is composed once,
as a document in the server's answer shape, and both renderings are made from
it. [`field_names`] is how a deterministic test holds a composed document to
the generated example of its operation in
`skills/printobserver/reference/common-operations.md`, which `just
docs-generate` writes from the real program.

A stub answers with fixed text, so an action's answer cannot echo the reason
the agent gave. What it can enumerate it echoes, by a stub per variant
(`scenario.py`): each disposition of each alert an acknowledgement names, and
each value on an adjustment's grid.
"""

from __future__ import annotations

import json
import re
from collections.abc import Iterable
from itertools import pairwise
from typing import Any

from surface import separators

PATH_SEPARATOR, LABEL_SEPARATOR = separators()

# The array of event records inside each answer that carries one. A record's
# fields depend on its kind and on the producer that sent it, so the shape of
# these is the history's (`HistoryAnswer.json`) rather than an example's.
EVENT_LISTS = ("context.recent_events", "events", "arrived")


def fields(document: object, at: str = "") -> list[tuple[str, str]]:
    """Every leaf of a document by the path it sits at, in path order, as `render.rs` walks it.

    A branch with nothing under it is a leaf of its own.
    """

    def under(name: str) -> str:
        return f"{at}{PATH_SEPARATOR}{name}" if at else name

    match document:
        case dict() if document:
            return [
                leaf for name in sorted(document) for leaf in fields(document[name], under(name))
            ]
        case list() if document:
            return [
                leaf
                for index, held in enumerate(document)
                for leaf in fields(held, under(str(index)))
            ]
        case dict():
            return [(at, "{}")]
        case list():
            return [(at, "[]")]
        case str():
            return [(at, document)]
        case _:
            return [(at, json.dumps(document, ensure_ascii=False))]


def labelled(document: object) -> str:
    """The labelled-lines rendering the program prints by default."""
    return "".join(f"{at}{LABEL_SEPARATOR}{value}\n" for at, value in fields(document))


def machine(document: object) -> str:
    """The `--json` rendering: the document itself, keys in order, pretty-printed."""
    return json.dumps(document, indent=2, sort_keys=True, ensure_ascii=False) + "\n"


def _parsed(value: str) -> object:
    if value in {"true", "false", "null", "[]", "{}"}:
        return json.loads(value)
    if re.fullmatch(r"-?\d+", value):
        return int(value)
    if re.fullmatch(r"-?\d+\.\d+(?:e-?\d+)?", value):
        return float(value)
    return value


def from_labelled(text: str) -> dict[str, Any]:
    """A document read back from its labelled rendering, as a recorded answer is replayed.

    Each line is a leaf at a dotted path; a numeric segment is an index into a
    list. The program prints every leaf, so this loses nothing it printed.

    Raises:
        ValueError: If a line is not a labelled leaf, or two lines disagree on
            what one path holds.
    """
    root: dict[str, Any] = {}
    for number, line in enumerate(text.splitlines(), start=1):
        if not line.strip():
            continue
        path, separator, value = line.partition(LABEL_SEPARATOR)
        segments = path.split(PATH_SEPARATOR)
        if not separator or not all(segments):
            msg = f"line {number} is not a labelled leaf: {line!r}"
            raise ValueError(msg)
        holder: dict[str, Any] | list[Any] = root
        for segment, following in pairwise(segments):
            holder = _branch(holder, segment, [] if following.isdigit() else {}, number)
        _place(holder, segments[-1], _parsed(value), number)
    return root


def _held(holder: dict[str, Any] | list[Any], segment: str, number: int) -> object:
    """What a path's segment holds so far, a list grown to reach it."""
    match holder:
        case list():
            if not segment.isdigit():
                msg = f"line {number} names {segment!r} inside a list"
                raise ValueError(msg)
            holder.extend([None] * (int(segment) + 1 - len(holder)))
            return holder[int(segment)]
        case dict():
            return holder.get(segment)


def _put(holder: dict[str, Any] | list[Any], segment: str, value: object) -> None:
    match holder:
        case list():
            holder[int(segment)] = value
        case dict():
            holder[segment] = value


def _branch(
    holder: dict[str, Any] | list[Any], segment: str, empty: dict[str, Any] | list[Any], number: int
) -> dict[str, Any] | list[Any]:
    held = _held(holder, segment, number)
    if held is None:
        _put(holder, segment, empty)
        return empty
    if isinstance(held, dict | list) and type(held) is type(empty):
        return held
    msg = f"line {number} reads {segment!r} as a branch another line gave a value"
    raise ValueError(msg)


def _place(holder: dict[str, Any] | list[Any], segment: str, value: object, number: int) -> None:
    if _held(holder, segment, number) is not None:
        msg = f"line {number} gives {segment!r} a value another line already gave it"
        raise ValueError(msg)
    _put(holder, segment, value)


def _normalized(path: str) -> str | None:
    """A leaf's path with its data-dependent segments named once, or none inside an event list.

    An index is any index, an adjustable under `allowed` is any adjustable, and
    an actor is one field whatever shape it takes (an example's `operator`, an
    agent's session). A trailing index is dropped, so an empty list and a list
    with entries name the same field.
    """
    for events in EVENT_LISTS:
        if path == events or path.startswith(events + PATH_SEPARATOR):
            return None
    segments = path.split(PATH_SEPARATOR)
    named: list[str] = []
    for index, segment in enumerate(segments):
        if segment.isdigit() or (index > 0 and segments[index - 1] == "allowed"):
            named.append("*")
        else:
            named.append(segment)
        if segment == "actor":
            break
    while named and named[-1] == "*":
        named.pop()
    return PATH_SEPARATOR.join(named)


def field_names(lines: Iterable[str]) -> set[str]:
    """The field names a labelled answer carries, outside its event lists."""
    names = set()
    for line in lines:
        path = line.partition(LABEL_SEPARATOR)[0]
        name = _normalized(path)
        if name is not None:
            names.add(name)
    return names


def example_answers(markdown: str) -> dict[str, list[list[str]]]:
    """Each worked example's output lines in `common-operations.md`, by operation.

    An operation's entry may run several commands; each command's output is the
    lines between it and the next command or the end of the block. What it
    printed to standard error, and a shell's `echo $?`, are not answer fields.
    """
    examples: dict[str, list[list[str]]] = {}
    for entry in re.finditer(
        r"^### (?P<name>\w+)\n(?P<body>.*?)(?=^### |\Z)", markdown, re.MULTILINE | re.DOTALL
    ):
        operation = entry["name"].replace("_", "-")
        for block in re.finditer(r"```console\n(.*?)```", entry["body"], re.DOTALL):
            current: list[str] | None = None
            for line in block.group(1).splitlines():
                if line.startswith("$ "):
                    current = None
                    if line.startswith(f"$ printobserver {operation} ") or line == (
                        f"$ printobserver {operation}"
                    ):
                        current = []
                        examples.setdefault(operation, []).append(current)
                    continue
                if current is not None and re.match(r"^[\w.:]+: ", line):
                    current.append(line)
    return examples
