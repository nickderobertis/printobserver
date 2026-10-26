"""The address the tier hands the stack: the configured one with its own port held.

The capture binds before anything is told to post, so the address the trigger
is given is the configured webhook URL with the bound port put in place of the
one it named. Nothing else of that URL may move with it.
"""

from __future__ import annotations

import pytest
from obico_tier import held_at
from repo_checks.expect import equal


@pytest.mark.parametrize(
    ("configured", "held"),
    [
        ("http://host.docker.internal:0/alert", "http://host.docker.internal:4321/alert"),
        ("http://127.0.0.1:8080/alert?to=tier", "http://127.0.0.1:4321/alert?to=tier"),
        ("http://tier:word@127.0.0.1:0/alert", "http://tier:word@127.0.0.1:4321/alert"),
        ("http://[::1]:0/alert", "http://[::1]:4321/alert"),
        ("http://[fe80::1]/alert", "http://[fe80::1]:4321/alert"),
    ],
)
def test_only_the_port_moves(configured: str, held: str) -> None:
    """User information, an IPv6 literal's brackets, the path and the query stay."""
    equal(held_at(configured, 4321), held, describing=f"{configured} held on port 4321")
