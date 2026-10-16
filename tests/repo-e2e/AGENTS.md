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
- **A sharded gate runs it in two parts, by test file.**
  `PRINTOBSERVER_E2E_PART` names the part a run collects and
  `repo_checks.gate_shards.END_TO_END_SECOND_PART` is the second part's files;
  a file it does not name is the first part's, so a new journey is never left
  out. Keep the `repo-root-release-build` group's files in one part: they
  share a release build another runner would pay for again.
