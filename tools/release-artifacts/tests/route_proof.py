"""The platform-table condition shared by end-user install-route proofs."""

from pathlib import Path

import pytest
from repo_checks import platforms
from repo_checks.model import Repo

REPO_ROOT = Path(__file__).resolve().parents[3]

#: This host's own entry in AGENTS.md's supported-platform list.
HERE = platforms.host(Repo(REPO_ROOT))

#: A proof of an end-user install route, run where the install path targets this
#: host's platform and skipped where it does not.
#:
#: Keyed off the platform's own `install path` answer rather than off which
#: platform this is: that answer is the record a platform's install-route
#: delivery flips, so flipping it runs these proofs there with nobody editing a
#: test, and a platform the list says the install path targets never skips one.
ROUTE_PROOF = pytest.mark.skipif(
    not HERE.install_path,
    reason=(
        f"a proof of an end-user install route, and AGENTS.md's supported-platform list "
        f"answers `install path: no` for `{HERE.id}`: {HERE.install_path_reason}"
    ),
)
