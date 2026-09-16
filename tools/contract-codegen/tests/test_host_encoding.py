"""The generator runs on a host whose own text encoding is not UTF-8.

A Windows interpreter's default encoding is its ANSI code page, not UTF-8, and
the generated clients carry characters that code page cannot hold. So the
generator's own command is run here as a Windows host runs it: with UTF-8 mode
off and a locale that is not UTF-8, which on Linux is the C locale with the
interpreter told not to coerce it — and on Windows is what every run is.
"""

from __future__ import annotations

import os
import sys

from conftest import REPO_ROOT
from repo_checks.expect import equal
from repo_checks.shell import run


def test_the_generators_check_runs_where_the_locale_is_not_utf8() -> None:
    """`check` hands every client to its formatter and reads the answer back."""
    environment = {
        **os.environ,
        "PYTHONUTF8": "0",
        "PYTHONCOERCECLOCALE": "0",
        "LC_ALL": "C",
        "PYTHONPATH": os.pathsep.join(
            str(REPO_ROOT / root)
            for root in ("tools/repo-checks/src", "tools/contract-codegen/src")
        ),
    }

    checked = run(
        [sys.executable, "-m", "contract_codegen", "check"],
        cwd=REPO_ROOT,
        env=environment,
        timeout=600,
    )

    equal(
        checked.returncode,
        0,
        describing=f"`contract_codegen check` under a non-UTF-8 locale: {checked.stderr[-2000:]}",
    )
