"""The platform-table condition shared by end-user install-route proofs."""

from pathlib import Path

import pytest
from repo_checks import platforms
from repo_checks.model import Repo

REPO_ROOT = Path(__file__).resolve().parents[3]

#: This host's own entry in AGENTS.md's supported-platform list.
HERE = platforms.host(Repo(REPO_ROOT))

#: A proof of an end-user install route, run where the install path targets this
#: host's platform and skipped where it does not, for the reason the platform's
#: own descriptor gives.
ROUTE_PROOF = pytest.mark.skipif(HERE.no_route_proof is not None, reason=HERE.no_route_proof or "")
