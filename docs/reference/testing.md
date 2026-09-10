# Testing

Every tier this repository declares, what each one proves, and — for the tiers
that are not in the gate — why they are outside it and what runs them instead.
This document covers the tiers the gate runs and the tiers outside the gate.

The set of tiers is not written down here. It is `repo-policy.toml`'s
`gate.tiers` together with every other table of that file that declares a `tier`
recipe, and a check reads that declaration beside this document: a tier with no
entry and an entry with no tier are both refused. A tier declared there joins
this document's inventory by being declared, so one nobody thought to write up
is a failing check rather than a gap.

`just check` is the whole gate, and it is strict in all five senses —
formatting, linting, type checking and tests each fail the build on an issue,
coverage is measured on the test run, and the coverage tier fails the build
below the floors `repo-policy.toml` records. There is no warnings-only mode.

## The tiers the gate runs

### format-check

Refuses a source file that is not in its language's canonical format — `cargo
fmt`, `ruff format` and `biome`, each over the projects that language owns.

### lint

Lints every project with its language's linter, failing on any finding. Rust is
`clippy` with `all` and `pedantic` denied at the workspace level and
`unsafe_code` forbidden outright.

### typecheck

Type-checks every project with its language's type checker: `cargo check` over
all targets, `ty` over the Python projects, and `tsc` over the Node ones.

### test

Runs every project's tests, recording line coverage as they run. This is where
the unit tests, the contract tests over the generated schemas, and the journeys
that drive the real binary against a real server live — everything except the
three tiers below and the printer's own integration binary.

### coverage

Fails the build below the line-coverage floors `repo-policy.toml` records: 95%
per ecosystem, measured on the run the `test` tier just did. Each ecosystem
measures its own suite, because one blended figure lets one language's coverage
pay for another's.

### build

Builds every project that produces an artifact, so that a tree whose tests pass
and whose product does not compile for release is refused here rather than
after a merge.

### lint-workflows

Validates the committed workflows: `actionlint` parses them, `shellcheck` reads
the committed shell scripts, and this repository's own workflow check refuses an
unpinned action, a secret outside the manifest and a command outside the
allowlist.

### check-repo

This repository's own deterministic checks over the committed tree — the
dependency rule, the vendor-vocabulary boundary, the platform matrices, the
install path, the merge model, the suppression allowlist, and the documentation
checks behind this document and its siblings.
`tools/repo-checks/src/repo_checks/registry.py` names every one of them, and
each can be run on its own by name.

### test-e2e

The end-to-end tier, which drives this repository's own gate: it runs
`just bootstrap` in a fresh copy carrying no build products, and it assembles
copies of the tree carrying one defect each and asserts the gate refuses every
one of them.

## The tiers outside the gate

### lint-llm-diff

**What it proves.** That this change reads the way this repository asks code to
read, judged by an LLM against rules a deterministic linter cannot express —
whether a script's output is signal, whether a contract has one source, whether
a test drives the real artifact. The rules are not written here either: they
come from the plugin packs `llmlint.yml` pins by version, and every finding
carries the rationale behind it. `just lint-llm-diff` judges the merge-base diff
against `origin/main`, which is the form continuous integration runs;
`just lint-llm` is the same judge over the whole tree, which nothing in
continuous integration runs. `just lint-llm-validate` is the tier's
deterministic half — the config, the pinned plugins and the suppression
allowlist — and it runs first, so a slip in any of those fails without spending
a harness call.

**Why it is outside the gate.** Two reasons, and each alone would be enough. It
is non-deterministic: the same diff can come back with different findings, and a
tier that does that cannot be the one a developer runs to know whether their
change is ready. And it needs a harness credential, so a gate that could not run
without one would stop being the gate that runs anywhere. `just check` therefore
does not invoke it, and a check refuses a tree in which it does.

**When it runs.** On every change, as a continuous-integration job of its own —
the `llmlint` job of `.github/workflows/ci.yml`, which fires on every pull
request and on every push to `main`, and which is a required check. It carries no
platform matrix: a second cell over a text diff would be a second verdict from a
non-deterministic judge rather than a second platform. The judge is an agent the
runner does not otherwise carry, so `just setup-llmlint` installs one before the
tier runs.

### test-integration

**What it proves.** Every client command and the whole server loop against a
*real* OctoPrint — the one `just octoprint-up` provisions, with OctoPrint's own
virtual printer and a hold print running on it. The same cross-product the fast
tier runs against a socket runs here against a machine that takes its own time,
so a command that worked because nothing had to move is refused.

**Why it is outside the gate.** It installs OctoPrint into a virtual environment
of its own, starts it, and waits a print out. That is minutes of provisioning
every gate run would otherwise pay for.

**When it runs.** On every change, as a continuous-integration job of its own,
on every platform the supported-platform list names. `just octoprint-up` and
`just octoprint-down` bracket it.

### test-obico

**What it proves.** That the recorded Obico failure-alert sample still matches
what a live Obico produces. It stands up a self-hosted Obico from that project's
own development composition, causes a real failure alert through Obico's own
detection pipeline, captures the body that stack posts, fetches the image that
captured body points at, and compares the *shape* of what was captured with the
committed sample — reporting every field that moved.

**Why it is outside the gate.** It builds Obico's own images, one of which
carries a machine-learning model, starts four containers, and then waits a real
failure alert out: tens of minutes on a cold runner.

**When it runs.** On a schedule and on a manual invocation, and on no trigger
that fires on a change. The schedule is the cron `17 4 * * 1` —
`.github/workflows/obico.yml` is where it is declared, and `AGENTS.md` records
the same cron so the prose cannot drift from what fires. `just obico-up` and
`just obico-down` bracket it.

A divergence this tier finds is a finding to report rather than a defect of this
repository: the sample is a checked-in contract, and moving it is a deliberate
change.
