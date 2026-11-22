# repo-e2e-independent

The end-to-end tier's journeys whose subject behaves the same on every
platform: they drive this repository's own tooling, workflows and records —
`just check-repo` and `just lint-workflows` over defect copies, the committed
workflows under the forge's rules (`tests/actions.py`), and `release-plz`, which
only the Linux release job runs. The project carries `platform:independent`,
the tag `repo-policy.toml`'s `[gate] platform_independent_tag` declares, so CI
may run it once rather than on every platform.

`tests/repo-e2e/AGENTS.md` holds the rules both projects' journeys are held to;
this project takes its hooks, fixtures and `journey` helpers from there. A
journey whose subject depends on the platform — an installer, a service
manager, a path, the shipped program — belongs to `tests/repo-e2e` instead, and
`tests/test_end_to_end_projects.py`'s `ASSIGNED` is the record of which file is
whose.
