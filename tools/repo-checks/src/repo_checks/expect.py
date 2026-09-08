"""The assertion vocabulary this repository's own suites are written in.

These raise `AssertionError` directly rather than going through the `assert`
statement, for two reasons that point the same way.

`assert` is compiled out under `-O`, which is what `S101` exists to catch, and
the only way to keep that rule enabled and honest across a suite this size is to
stop using the statement rather than to silence the rule 243 times.

The messages are better for it. `refused(findings, "...")` prints every finding
the check returned when none of them names what was expected, and
`failing(result, naming=...)` prints the whole run — which is what a reader of a
failed gate journey actually needs, and more than a bare comparison would say.

Both suites import this one module, so the vocabulary cannot drift into two.
"""

from __future__ import annotations

import subprocess
from collections.abc import Container, Sequence

Run = subprocess.CompletedProcess[str] | tuple[int, str]


def _listing(findings: Sequence[str]) -> str:
    """Render a check's findings for a failure message."""
    return "\n  ".join(findings) if findings else "(none)"


def _outcome(result: Run) -> tuple[int, str]:
    """Normalize a completed process or an (exit status, output) pair."""
    if isinstance(result, tuple):
        return result
    return result.returncode, (result.stdout or "") + (result.stderr or "")


def accepted(findings: Sequence[str], *, describing: str = "the committed tree") -> None:
    """Fail unless the check returned no findings.

    Raises:
        AssertionError: If it returned any, naming every one.
    """
    if findings:
        message = f"expected {describing} to be accepted; it was refused:\n  {_listing(findings)}"
        raise AssertionError(message)


def refused(findings: Sequence[str], naming: str) -> None:
    """Fail unless some finding names `naming`.

    Raises:
        AssertionError: If none does, naming every finding there was.
    """
    if not any(naming in finding for finding in findings):
        message = f"expected a finding naming {naming!r}; got:\n  {_listing(findings)}"
        raise AssertionError(message)


def refused_naming(findings: Sequence[str], *parts: str) -> None:
    """Fail unless one single finding names every one of `parts`.

    Two separate findings that each name one part are not the same thing as one
    finding that names both, so this is not `refused` twice.

    Raises:
        AssertionError: If no single finding names them all.
    """
    if not any(all(part in finding for part in parts) for finding in findings):
        message = (
            f"expected one finding naming all of {list(parts)!r}; got:\n  {_listing(findings)}"
        )
        raise AssertionError(message)


def equal(actual: object, expected: object, *, describing: str = "") -> None:
    """Fail unless `actual` equals `expected`.

    Raises:
        AssertionError: If they differ, showing both.
    """
    if actual != expected:
        context = f" for {describing}" if describing else ""
        message = f"expected {expected!r}{context}; got {actual!r}"
        raise AssertionError(message)


def contains(whole: Container[object], part: object, *, describing: str = "") -> None:
    """Fail unless `part` is in `whole`.

    Raises:
        AssertionError: If it is not, showing what was searched.
    """
    if part not in whole:
        context = f" of {describing}" if describing else ""
        message = f"expected {part!r} in the {type(whole).__name__}{context}:\n{whole!r}"
        raise AssertionError(message)


def absent(whole: Container[object], part: object, *, describing: str = "") -> None:
    """Fail unless `part` is absent from `whole`.

    Raises:
        AssertionError: If it is present, showing what was searched.
    """
    if part in whole:
        context = f" of {describing}" if describing else ""
        message = f"expected {part!r} to be absent from the {type(whole).__name__}{context}"
        raise AssertionError(message)


def truth(condition: object, *, describing: str) -> None:
    """Fail unless `condition` holds, saying what was expected.

    Raises:
        AssertionError: If it does not hold.
    """
    if not condition:
        message = f"expected {describing}"
        raise AssertionError(message)


def passing(result: Run, *, describing: str = "the run") -> None:
    """Fail unless the run exited zero.

    Raises:
        AssertionError: If it did not, printing everything it said.
    """
    code, said = _outcome(result)
    if code != 0:
        message = f"expected {describing} to succeed; it exited {code}:\n{said}"
        raise AssertionError(message)


def failing(result: Run, *, naming: str) -> None:
    """Fail unless the run exited non-zero and said `naming`.

    Raises:
        AssertionError: If it succeeded, or failed without naming it.
    """
    code, said = _outcome(result)
    if code == 0:
        message = f"expected a failure naming {naming!r}; the run succeeded:\n{said}"
        raise AssertionError(message)
    if naming not in said:
        message = f"expected the failure to name {naming!r}; it exited {code} saying:\n{said}"
        raise AssertionError(message)
