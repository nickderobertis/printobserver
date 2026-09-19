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

#: The half of a client's proof that needs no route — the client taken from its
#: registry and its smoke check run where it was installed — driven where the
#: install path does not target this host, since the proof by the route is
#: skipped there and the code the two share would otherwise be run on no such
#: host at all. Where the route targets the host the whole proof runs instead,
#: and this is what is skipped.
WITHOUT_ROUTE_PROOF = pytest.mark.skipif(
    HERE.no_route_proof is None,
    reason="the install path targets this host, so the client is proven by the route",
)
