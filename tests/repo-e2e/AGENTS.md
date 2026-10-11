# repo-e2e

The rules the end-to-end tier is held to when its journeys run beside each
other. The root `AGENTS.md` holds the repository-wide rules; this file holds the
ones only this suite carries.

- **It runs under pytest-xdist.** `test-e2e` runs `pytest -n auto --dist
  loadgroup`, and `tests/workers.py` answers `auto`: a worker per logical
  processor, one fewer on Windows, as many as the host's memory holds. The log
  says which figures it chose from.
- **A journey that shares something declares it.** What a journey touches
  beyond its own copy, its own temporary directory and a stand-in bound to port
  0 — a fixed path under the repository root, a build into that root's
  `target`, a host service, a program its own deadline bounds that another
  journey would slow — is one of `tests/journey.py`'s xdist groups, set as the
  module's `pytestmark`. A group keeps its journeys on one worker, one after
  another. A journey is never made to pass beside another by loosening what it
  asserts or how long it waits. Something it only reads is no group's, and
  `journey.py` says what that covers.
- **It waits on one build.** The target `dependsOn` `printobserver:build`, the
  debug program the journeys run, and on nothing else, so the gate starts it
  beside the test tier wherever the root `justfile`'s `check` overlaps them.
  Its `implicitDependencies` are what selects it at the affected tier, and
  `test_gate_tier_journey.py` drives a change to each.
