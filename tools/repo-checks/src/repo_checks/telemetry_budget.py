"""`python -m repo_checks telemetry_budget <file> <figure>`: one recorded figure, reported.

A budget whose figure a test records rather than one its command measures is
registered with this one command. The test writes a JSON object to a file under
its project's `telemetry/` directory:

    {"figures": {"<figure>": <number>, ...}, "detail": {...}}

and this reads that file, takes `figures[<figure>]`, and reports it through the
onebudgetspec SDK with the file's `detail` — rendered as JSON — as the result's
detail. It never compares the figure with a threshold: `onebudgetspec check`
reads the result and is the only judge.

A file that is absent or unreadable, a document of another shape, or a figure
the file does not record, or records as something other than a finite number,
is refused naming which: nothing is reported, the command exits non-zero, and
`onebudgetspec check` answers the budget as an error rather than as a figure.
"""

from __future__ import annotations

import json
import math
import sys
from pathlib import Path

from onebudgetspec_sdk import report

#: The exit status of a figure reported.
REPORTED = 0
#: The exit status of a file or figure that could not be read, nothing reported.
REFUSED = 2


class TelemetryError(Exception):
    """A telemetry file, or the figure asked of it, that cannot be reported."""


def figure_of(path: Path, figure: str) -> tuple[float, str]:
    """The figure a telemetry file records under one name, and its detail as JSON text.

    Raises:
        TelemetryError: If the file is absent, is not JSON, is not the
            `{"figures": {...}, "detail": {...}}` shape, or records no finite
            number under `figure`.
    """
    try:
        text = path.read_text(encoding="utf-8")
    except FileNotFoundError:
        msg = f"{path} is absent: the test that records it has not written it"
        raise TelemetryError(msg) from None
    except OSError as error:
        msg = f"{path} could not be read: {error}"
        raise TelemetryError(msg) from error
    except UnicodeDecodeError as error:
        msg = f"{path} is not UTF-8 text: {error}"
        raise TelemetryError(msg) from error
    try:
        document = json.loads(text)
    # A JSONDecodeError is a ValueError, and so is an integer too long to read.
    except ValueError as error:
        msg = f"{path} is not JSON this can read: {error}"
        raise TelemetryError(msg) from error
    if not isinstance(document, dict):
        msg = f"{path} is not a JSON object"
        raise TelemetryError(msg)
    figures = document.get("figures")
    if not isinstance(figures, dict):
        msg = f"{path} carries no `figures` object"
        raise TelemetryError(msg)
    if figure not in figures:
        recorded = ", ".join(sorted(str(name) for name in figures)) or "none"
        msg = f"{path} records no figure `{figure}`; it records {recorded}"
        raise TelemetryError(msg)
    value = figures[figure]
    refused = f"{path} records `{figure}` as {value!r}, which is not a finite number"
    # A bool is an int to Python, and is no figure.
    if isinstance(value, bool) or not isinstance(value, int | float):
        raise TelemetryError(refused)
    try:
        # What is reported is a float, so the figure is one only where it is a
        # finite float: an integer past a float's range is not.
        reported = float(value)
    except OverflowError:
        raise TelemetryError(refused) from None
    if not math.isfinite(reported):
        raise TelemetryError(refused)
    detail = document.get("detail", {})
    if not isinstance(detail, dict):
        msg = f"{path} carries a `detail` that is not a JSON object"
        raise TelemetryError(msg)
    return reported, json.dumps(detail, sort_keys=True)


def measure(path: str | None, figure: str | None) -> int:
    """The budget's command: report one recorded figure and its detail, or refuse naming why."""
    if not path or not figure:
        print(
            "telemetry_budget: refused: give the telemetry file and the figure to report",
            file=sys.stderr,
        )
        return REFUSED
    try:
        value, detail = figure_of(Path(path), figure)
    except TelemetryError as error:
        print(f"telemetry_budget: refused: {error}", file=sys.stderr)
        return REFUSED
    print(f"telemetry_budget: {figure} = {value} from {path}", file=sys.stderr)
    report(value, detail)
    return REPORTED
