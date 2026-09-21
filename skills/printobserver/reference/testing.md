# Testing

Every tier this repository declares, what each one proves, and — for the tiers
that are not in the gate — why they are outside it and what runs them instead.
This document covers the tiers the gate runs, the tiers outside the gate, and
the test nothing selects.

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

Which platforms a tier runs on is stated under each tier, and every answer is
derived from one list: `AGENTS.md`'s supported-platform list, which names each
platform's runner and service manager. A tier that runs on every platform runs
on exactly the platforms that list names, in one continuous-integration cell
per platform, so a platform joining or leaving the list joins or leaves every
such tier at once; a tier that runs once carries no platform matrix, and the
reason it does not is recorded beside it. Nothing below names a platform the
list does not carry, and a check refuses a document that does.

## The tiers the gate runs

Every tier in this section runs on every supported platform: the `gate` job of
`.github/workflows/ci.yml` is one cell per platform on the list, and each cell
runs `just bootstrap` and then `just check`, which is all of them. On the two
Windows cells `just` runs its recipes under Git's own bash, and the gate reads
the same tree there because every text file is checked out with the line
endings it was committed with.

### format-check

Refuses a source file that is not in its language's canonical format — `cargo
fmt`, `ruff format` and `biome`, each over the projects that language owns.

**Where it runs.** Every supported platform, in the gate's cell for it.

### lint

Lints every project with its language's linter, failing on any finding. Rust is
`clippy` with `all` and `pedantic` denied at the workspace level and
`unsafe_code` forbidden outright.

**Where it runs.** Every supported platform, in the gate's cell for it. On a
Unix host it lints every crate twice — natively, and once more for the Windows
target `repo-policy.toml`'s `toolchain.windows_lint` names — so a finding in
`cfg(windows)` code is reported before a push rather than by a Windows cell at
the end of a round; on the Windows cells the native pass is that pass.

### typecheck

Type-checks every project with its language's type checker: `cargo check` over
all targets, `ty` over the Python projects, and `tsc` over the Node ones.

**Where it runs.** Every supported platform, in the gate's cell for it.

### test

Runs every project's tests, recording line coverage as they run. This is where
the unit tests, the contract tests over the generated schemas, and the journeys
that drive the real binary against a real server live — everything except the
three tiers below and the printer's own integration binary.

**Where it runs.** Every supported platform, in the gate's cell for it. The
suites are one suite on every platform; what differs by platform is how a
journey observes the host — which program traces an invocation, which service
manager a service is driven through, how a serial device is named — and each
of those is chosen by the host rather than by a test that only runs on one.

### coverage

Fails the build below the line-coverage floors `repo-policy.toml` records: 95%
per ecosystem, measured on the run the `test` tier just did. Each ecosystem
measures its own suite, because one blended figure lets one language's coverage
pay for another's.

**Where it runs.** Every supported platform, in the gate's cell for it, with
one recorded exemption: on `windows-aarch64` the Rust toolchain cannot read the
profiles its own instrumentation writes, so `repo-policy.toml` exempts that
cell's Rust coverage report alone — the tests still run there, and the floor
is still required everywhere profiles are readable. `AGENTS.md`'s
"Supported platforms" records the toolchain defect and what removes the
exemption.

### build

Builds every project that produces an artifact, so that a tree whose tests pass
and whose product does not compile for release is refused here rather than
after a merge.

**Where it runs.** Every supported platform, in the gate's cell for it, each
building for its own Rust target.

### lint-workflows

Validates the committed workflows: `actionlint` parses them, `shellcheck` reads
the committed shell scripts, and this repository's own workflow check refuses an
unpinned action, a secret outside the manifest and a command outside the
allowlist.

**Where it runs.** Every supported platform, in the gate's cell for it.

### check-repo

This repository's own deterministic checks over the committed tree — the
dependency rule, the vendor-vocabulary boundary, the platform matrices, the
install path, the merge model, the suppression allowlist, and the documentation
checks behind this document and its siblings.
`tools/repo-checks/src/repo_checks/registry.py` names every one of them, and
each can be run on its own by name.

**Where it runs.** Every supported platform, in the gate's cell for it. The
platform checks are what hold every matrix in the committed workflows to the
supported-platform list, so they run on each platform that list names.

### test-e2e

The end-to-end tier, which drives this repository's own gate: it runs
`just bootstrap` in a fresh copy carrying no build products, and it assembles
copies of the tree carrying one defect each and asserts the gate refuses every
one of them.

**Where it runs.** Every supported platform, in the gate's cell for it. Its
journey over the installed service drives the real service manager on the
platforms that have one it can reach — systemd on the Linux cells, the
service control manager on the Windows cells — and skips the macOS cell,
where the same journey over launchd is `crates/printobserver/tests/service_manager.rs`'s,
in the `test` tier.

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

**When it runs.** On every change, as a continuous-integration job of its own —
the `integration` job of `.github/workflows/ci.yml`, one cell per platform the
supported-platform list names, each of which is a required check. `just
octoprint-up` and `just octoprint-down` bracket it. OctoPrint's virtual printer
is a bundled pure-Python plugin that needs no hardware, so no platform is
excluded; `AGENTS.md`'s "Virtual printer availability" is where one would be
recorded, with its reason.

### test-install-proof

**What it proves.** That each of the three end-user routes can be taken from
its own registry, on every platform the supported-platform list names: that the
registry serves the version under test, that what it serves installs into an
environment holding no copy of these sources and with no Rust toolchain on the
path, and that the program the install put there runs and reports that version.
It answers one of three outcomes and only the first is a pass — `SERVED AND
PROVEN`, `SERVED AND NOT PROVEN` (an artifact that does not work) and `NOT
SERVED` (a publish that did not happen) — because those are two different
repairs. The version under test is the one the caller names in
`PRINTOBSERVER_PROOF_VERSION`, the newest release the forge published where that
says `release`, and otherwise the newest the registry itself serves; never the
number in the committed tree, which is whatever release automation last wrote
there. `just prove-registry-pypi`, `prove-registry-npm` and
`prove-registry-script` are the three routes on their own;
`prove-registry-client-rust`, `prove-registry-client-python` and
`prove-registry-client-node` take each client from the registry a dependent
takes it from and run its smoke check against the supervisor the same
release's asset carries; and `just test-install-proof` is all six.

**Why it is outside the gate.** It reads the real package registries, so over a
change it could only ever report what was published *before* that change, and it
can say nothing at all about the artifact under review. What proves an artifact
built from the committed tree is `just prove-route-*` and `just prove-client-*`,
which the end-to-end tier drives inside the gate on every change and which
contact no registry. No workflow job runs one of those: a job proving a shipped
artifact takes it from its registry.

**When it runs.** After a release, on a schedule and on a manual invocation, and
on no trigger that fires on a change — so nothing a developer does selects it,
and a change to this tree is never what runs it. The schedule is the cron
`0 6 * * 1`, and `.github/workflows/install-path.yml` is where all three
triggers are declared. The release-time one is the `release-plz` workflow
*having finished* rather than the GitHub Release being published: that release
is cut before its artifacts are built and published, so a proof keyed on it
would measure the version before it. Each of the six proofs is one cell per
platform the supported-platform list answers `install path: yes` for, which
today is every platform on it; by hand, `just test-install-proof` runs all six
on the host it is run on, for the version `PRINTOBSERVER_PROOF_VERSION` names.

Nothing here publishes to a registry in order to prove a point:
`PRINTOBSERVER_PROOF_REGISTRIES` points all three registries at one stand-in
address, which `release-artifacts standin` stands up — a Python index serving
real wheels, a JavaScript registry serving real packages, and a forge listing
releases with their artifacts and digests. That is what this repository's own
suites drive every outcome through, with the real `pip`, the real `npm` and the
committed install script doing the installing:
`tests/repo-e2e/tests/test_registry_proof_journey.py` drives the recipes and
`tools/release-artifacts/tests/test_registries.py` the proof beneath them.

### test-skill-install

**What it proves.** That the Agent Skill this repository publishes installs
whole. From a copy of the tree's own files it runs the real `gh skill install`
in an environment holding no GitHub credentials, and holds what arrives to what
is committed: no symlink, the same files byte for byte but for `SKILL.md`, whose
prose is the committed prose, and every link in the installed skill opening
from the installed directory. It then runs `gh skill publish --dry-run` and
reads its verdict from what it prints, since it exits zero either way.

**Why it is outside the gate.** It needs GitHub CLI at the release
`repo-policy.toml` holds, which the gate's runners are not given; reached
without it, it refuses rather than skips.

**When it runs.** On every change, as the `skill-install` job of
`.github/workflows/ci.yml`, on one Linux runner with no platform matrix, after
`just install-gh` installs that release from its own verified archive. The
layout it depends on is checked on every platform inside the gate.

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
`.github/workflows/obico.yml` is where it is declared. `just obico-up` and
`just obico-down` bracket it. It runs once, on one Linux runner, and carries no
platform matrix: it proves an external producer's payload shape by standing
Obico up from that project's own Linux container composition, so it is not a
printer-host tier, and the hosted macOS and Windows runners do not run Linux
containers. By hand it needs Docker and its Compose plugin, the one
prerequisite `just bootstrap` does not install.

A divergence this tier finds is a finding to report rather than a defect of this
repository: the sample is a checked-in contract, and moving it is a deliberate
change.

## The test nothing selects

One test in this repository is not a tier at all, and deliberately: the
real-printer smoke test, `just test-printer-smoke`, which drives the installed
`printobserver` command against a running supervisor and, through it, a real
printer on a named serial port. It is not one of the gate's tiers, no graph
target reaches it, and no workflow runs it on a change or on a schedule,
because a print is hours of filament and an unattended test that starts one
ruins a print nobody was watching. Two things together select it and one alone
does not: the `--run` flag on its recipe, and `PRINTOBSERVER_SMOKE_DEVICE`
naming the serial device — spelled the way the host names one, `/dev/ttyACM0`
on Linux, `/dev/cu.usbmodem1101` on macOS, `COM3` on Windows. Absent either it
says which was missing and runs nothing. It runs on whichever supported
platform is beside the printer; every precondition it checks fails closed, and
refusing to run is a pass. `repo-policy.toml`'s `[smoke]` declares the recipe,
the flag and the variable, and names `AGENTS.md`'s "The real-printer smoke
test" as the test's account — the section `just check-repo`'s
`smoke-selection` refuses a tree without, or one that does not name all three.
