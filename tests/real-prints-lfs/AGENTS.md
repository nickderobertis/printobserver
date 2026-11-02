# real-prints-lfs

The rules the real-print cases' LFS proof is held to, and why. The root
`AGENTS.md` holds the repository-wide ones.

- **Prove it at the fixture's scale, never the set's.** The journeys push a few
  kilobytes to a local LFS remote at a file URL and spend none of the account's
  bandwidth. The full-scale figures are the budgets' arithmetic over
  `fixtures/full-set.tsv`, which is pointers alone.
- **Count what the remote serves, not what the client says.** git-lfs reports
  no download progress for a `file://` remote. `counting_remote.py` is the
  standalone transfer agent that `lfs.standalonetransferagent` puts in front of
  the remote's own store. It records every object it serves, and everything on
  the client side stays git-lfs's own.
- **The sandbox host carries the filter globally,** as `git lfs install`,
  hosted runners and Git for Windows all do. Without it a clone would leave
  pointers whatever `.lfsconfig` says, and the zero a new checkout measures
  would prove nothing. `test_without_the_exclusion_a_clone_downloads_every_object`
  is the control that keeps that zero honest.
- **The recipes are the committed justfile's,** run against a fixture
  repository through `JUST_JUSTFILE` and `JUST_WORKING_DIRECTORY`. So a recipe
  here must work in whatever repository `just` runs it in, and read nothing
  of this tree but its own scripts.
- **The CI steps are one committed list,** because the action-pin rule admits
  no local composite action. The suite's job pastes `ci-steps.yml` in verbatim,
  `real-prints-lfs-ci` holds it there, and `run_ci_steps` executes that same
  file. A step or an expression that runner does not know fails the journey, so
  the list cannot grow past what is proven.
- **Telemetry, not a second measurement.** The journeys write
  `telemetry/lfs.json`, which is ignored by git, and the two zero-byte budgets
  read it through `repo_checks telemetry_budget`. No test here asserts a
  budget's bound, because that is the budgets' check.
