# repo-e2e

The rules the end-to-end tier is held to when its journeys run beside each
other. The root `AGENTS.md` holds the repository-wide rules; this file holds the
ones only this suite carries.

- **A journey that shares something declares it.** Journeys run beside each
  other under pytest-xdist. What a journey touches beyond its own copy, its own
  temporary directory and a stand-in bound to port 0 — a fixed path under the
  repository root, a build into that root's `target`, a host service, a program
  its own deadline bounds that another journey would slow — is one of
  `tests/journey.py`'s xdist groups, set as the module's `pytestmark`. A group keeps its journeys on one worker, one after
  another. A journey that cannot share the host with any other is marked
  `alone` instead, saying why, and runs after the parallel session. A journey is
  never made to pass beside another by loosening what it asserts or how long it
  waits. Something it only reads is no group's, and `journey.py` says what that
  covers.
- **It waits on one build.** The target `dependsOn` `printobserver:build`, the
  debug program the journeys run, and on nothing else, so the gate starts it
  beside the test tier wherever the root `justfile`'s `check` overlaps them.
  Its `implicitDependencies` are what selects it at the affected tier, and
  `test_gate_tier_journey.py` drives a change to each.
- **A journey that behaves the same on every platform goes next door.**
  `tests/repo-e2e-independent` is the tier's second project, tagged
  `platform:independent`, for journeys that drive only this repository's own
  tooling, workflows and records. A journey whose subject behaves differently
  by platform — an installer, a service manager, a path, the shipped program,
  the gate as a platform's own cell runs it — stays here. Both projects take
  their hooks and fixtures from `tests/journey_session.py`, and Nx runs their
  `test-e2e` targets one after the other (`nx.json`), so a group here still
  holds against the other project's journeys. Name a new file in
  `test_end_to_end_projects.py`'s `ASSIGNED`, which refuses one left out.
- **Each session ends by naming where its time went:** the slowest test files,
  each summed over its tests, under the `--durations` list. Choose a cut from
  that, not from a single test's figure.
