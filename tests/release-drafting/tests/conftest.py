"""This project's place in a sharded gate's split of the end-to-end tier.

A gate split across runners runs the end-to-end tier in parts, each named in
`PRINTOBSERVER_E2E_PART`, and `repo_checks.gate_shards` decides which test file
each part collects, by its name, so every test is collected by exactly one part.
This project's files are held to the same partition as the tier's other
projects', read from that module rather than restated here.
"""

from __future__ import annotations

import os

import pytest
from repo_checks import gate_shards


def pytest_collection_modifyitems(config: pytest.Config, items: list[pytest.Item]) -> None:
    """Deselect the files another part of a split end-to-end tier runs."""
    named = os.environ.get(gate_shards.END_TO_END_PART_VARIABLE, "")
    if not named:
        return
    part = gate_shards.Shard.named(named)
    kept: list[pytest.Item] = []
    dropped: list[pytest.Item] = []
    for item in items:
        elsewhere = gate_shards.end_to_end_part(item.path.name, part.count) != part.index
        (dropped if elsewhere else kept).append(item)
    if dropped:
        config.hook.pytest_deselected(items=dropped)
        items[:] = kept
