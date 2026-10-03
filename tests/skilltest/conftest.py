"""The opt-in switch for this project's harness-driven skill tests.

Each live test runs the `printobserver` skill through a real coding harness and
spends real model turns, so it must never fire by accident. Two mechanisms keep
it from doing so, for two different callers:

* **The project graph** keeps it out of the gate. This directory is its own Nx
  project declaring a `skilltest` target, and no gate recipe fans out over that
  name, so `just check` never runs the live tests. Its `test` target runs only
  the deterministic tests, which build every scenario without a model.
* **This conftest** is for a developer who types a bare `pytest` over the tree:
  a test marked ``@pytest.mark.skilltest_e2e`` is skipped unless
  ``SKILLTEST_E2E`` is set, which the `skilltest` target (and `just skilltest`)
  does.
"""

from __future__ import annotations

import os

import pytest

_TRUTHY = {"1", "true", "yes", "on"}


def pytest_collection_modifyitems(config: pytest.Config, items: list[pytest.Item]) -> None:
    """Skip every live skill test unless the run opted in."""
    del config
    if os.environ.get("SKILLTEST_E2E", "").strip().lower() in _TRUTHY:
        return
    skip = pytest.mark.skip(
        reason="opt-in skill test: set SKILLTEST_E2E=1 (or run `just skilltest`)"
    )
    for item in items:
        if "skilltest_e2e" in item.keywords:
            item.add_marker(skip)
