"""This project's place in a sharded gate's split of the end-to-end tier.

A gate split across runners runs the end-to-end tier in parts, each named in
`PRINTOBSERVER_E2E_PART`, and `repo_checks.gate_shards` decides which test file
each part collects, by its name, so every test is collected by exactly one part.
This project's files are held to the same partition as the tier's other
projects', read from that module rather than restated here.
"""

from __future__ import annotations

import os
from pathlib import Path

import pytest
from repo_checks import gate_shards

#: The directory whose files this project's hooks decide for; any other item
#: collected beside them is another project's, and left to its own hooks.
OURS = Path(__file__).parent

#: How many of this project's tests the run left to another part.
LEFT_TO_ANOTHER_PART = pytest.StashKey[int]()


def pytest_collection_modifyitems(config: pytest.Config, items: list[pytest.Item]) -> None:
    """Deselect this project's files another part of a split end-to-end tier runs."""
    named = os.environ.get(gate_shards.END_TO_END_PART_VARIABLE, "")
    if not named:
        return
    part = gate_shards.Shard.named(named)
    kept: list[pytest.Item] = []
    dropped: list[pytest.Item] = []
    for item in items:
        ours = item.path.is_relative_to(OURS)
        elsewhere = gate_shards.end_to_end_part(item.path.name, part.count) != part.index
        (dropped if ours and elsewhere else kept).append(item)
    config.stash[LEFT_TO_ANOTHER_PART] = config.stash.get(LEFT_TO_ANOTHER_PART, 0) + len(dropped)
    if dropped:
        config.hook.pytest_deselected(items=dropped)
        items[:] = kept


def pytest_sessionfinish(session: pytest.Session, exitstatus: int) -> None:
    """A part that left every one of this project's tests to another part ran its share.

    That is an empty share rather than a selection matching nothing; a run
    that collected nothing at all with nothing left elsewhere still fails.
    """
    if (
        exitstatus == pytest.ExitCode.NO_TESTS_COLLECTED
        and session.config.stash.get(LEFT_TO_ANOTHER_PART, 0) > 0
    ):
        session.exitstatus = pytest.ExitCode.OK
