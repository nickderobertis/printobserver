"""A budget whose figure a test recorded: `python -m repo_checks telemetry_budget <file> <figure>`.

Some figures are measured by a test that drives the real thing — the bytes a
fixture's LFS remote served a clone, say — and a budget's command reads that
measurement rather than taking a second one. The test writes one JSON document,

    {"figures": {"<figure>": <number>, ...}, "detail": {...}}

and this reports `figures[<figure>]` as the budget's value through the
onebudgetspec SDK, with `detail` as that result's detail. It never compares the
figure with a threshold: that is the check's, which reads the budget. A file
that is not there, does not parse, or records no number under that figure is an
error result — a non-zero exit naming what is missing — never a figure of zero.
"""

from __future__ import annotations

import json
import math
import sys
from pathlib import Path

from onebudgetspec_sdk import report

#: The exit statuses: the figure was reported, or it could not be read.
REPORTED, UNREADABLE = 0, 1


class TelemetryError(ValueError):
    """Why a recorded figure could not be read, naming what is missing."""


def figure_of(path: Path, figure: str) -> tuple[float, str | None]:
    """The figure `path` records under `figure`, and its detail as JSON text.

    Raises:
        TelemetryError: If the file is missing or unreadable, or records no
            finite number under `figure`.
    """
    try:
        document = json.loads(path.read_text(encoding="utf-8"))
    except FileNotFoundError as error:
        msg = f"{path} is not there: run the test that records it first"
        raise TelemetryError(msg) from error
    except (OSError, ValueError) as error:
        msg = f"{path} could not be read as JSON: {error}"
        raise TelemetryError(msg) from error
    figures = document.get("figures") if isinstance(document, dict) else None
    if not isinstance(figures, dict):
        msg = f"{path} records no `figures` object"
        raise TelemetryError(msg)
    value = _finite(figures.get(figure))
    if value is None:
        recorded = ", ".join(sorted(figures)) or "none"
        msg = f"{path} records no finite number under `{figure}` (it records: {recorded})"
        raise TelemetryError(msg)
    detail = document.get("detail")
    return value, None if detail is None else json.dumps(detail, sort_keys=True)


def _finite(recorded: object) -> float | None:
    """`recorded` as a finite float, or none when it is no number or none a float can hold.

    JSON carries `NaN` and `Infinity` as Python reads it, and an integer too
    large for a float, which `float` refuses with `OverflowError`: each is
    refused here rather than reported or let out as an exception.
    """
    if isinstance(recorded, bool) or not isinstance(recorded, int | float):
        return None
    try:
        value = float(recorded)
    except OverflowError:
        return None
    return value if math.isfinite(value) else None


def telemetry_budget(path: Path, figure: str) -> int:
    """Report one recorded figure as the running budget's value, or say why it cannot."""
    try:
        value, detail = figure_of(path, figure)
    except TelemetryError as error:
        print(f"telemetry_budget: {error}", file=sys.stderr)
        return UNREADABLE
    report(value, detail)
    print(f"telemetry_budget: {figure} = {value:g}")
    return REPORTED
