"""The one place in this repository that starts a subprocess.

Every check, command and test helper runs external programs through `run`
below, and two things follow from there being exactly one such place.

`S607` — starting a process from a partial path — is *fixed* rather than
suppressed: the executable is resolved against PATH with `shutil.which` and run
by absolute path, so no caller can reintroduce the finding.

`S603` fires on the `subprocess.run` call itself whatever it runs, and no code
change silences it. Funnelling every caller here leaves it one reviewable site
carrying one allowlist entry, instead of one at every call in the tree.
"""

from __future__ import annotations

import shutil
import subprocess
from pathlib import Path

# The exit status a shell reports for a command it could not find.
PROGRAM_NOT_FOUND = 127


def run(
    argv: list[str],
    *,
    cwd: Path | None = None,
    env: dict[str, str] | None = None,
    timeout: float | None = None,
    check: bool = False,
    capture: bool = True,
) -> subprocess.CompletedProcess[str]:
    """Run a program by absolute path.

    Args:
        argv: The program and its arguments. Never a shell string — nothing here
            runs through a shell, so no argument can be interpreted as one.
        cwd: The directory to run in.
        env: The environment to run under, or the caller's when omitted.
        timeout: Seconds to wait before giving up.
        check: Raise on a non-zero exit rather than returning it.
        capture: Collect the output, or let it reach the caller's terminal when
            the program's own progress is what a reader needs.

    Returns:
        The completed process. A program that is not on PATH comes back with
        `PROGRAM_NOT_FOUND` and a message rather than an exception, so a caller
        that reports its own failure can do so.

    Raises:
        FileNotFoundError: If the program is absent and `check` is set.
    """
    program = shutil.which(argv[0])
    if program is None:
        if check:
            message = f"{argv[0]}: not found on PATH"
            raise FileNotFoundError(message)
        return subprocess.CompletedProcess(argv, PROGRAM_NOT_FOUND, "", f"{argv[0]}: not found\n")
    return subprocess.run(  # noqa: S603
        [program, *argv[1:]],
        cwd=cwd,
        env=env,
        timeout=timeout,
        capture_output=capture,
        text=True,
        check=check,
    )
