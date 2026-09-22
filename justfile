# printobserver — one command surface. `just --list` is the index.
#
# `just check` is the whole gate: it invokes every tier `repo-policy.toml`
# declares, the end-to-end tier included. Nothing here is a placeholder.

set shell := ["bash", "-euo", "pipefail", "-c"]

# `repo_checks` is a plain package rather than a distribution, so that no
# hand-maintained version string enters the tree. This is how the recipes and
# the graph targets reach it.
#
# `TOOL_PACKAGES` is the one list, written with `:`; the export joins it with
# the separator the interpreter on this host reads. A Windows interpreter splits
# a search path on `;` and reads a `:`-joined one as a single entry that names
# nothing, so without this every recipe reaching `repo_checks` fails there
# before doing anything. The separator is exported too, for a recipe that puts
# one more entry in front of the path.
TOOL_PACKAGES := "tools/repo-checks/src:tools/contract-codegen/src:tools/release-artifacts/src"
export PATH_SEPARATOR := if os_family() == "windows" { ";" } else { ":" }
export PYTHONPATH := replace(TOOL_PACKAGES, ":", PATH_SEPARATOR)

# Show the command surface.
default:
    @just --list

# Bring a clean clone to a state in which the gate runs.
bootstrap:
    rustup show active-toolchain
    cargo fetch --locked
    uv sync
    just node-modules
    uv run -q python -m repo_checks install-tools
    uv run -q python -m repo_checks install-hooks

# Install the tools `repo-policy.toml` declares, skipping any already on PATH
# at the release it holds them at, and replacing any at another. A tool it
# declares `bootstrap = false` is installed only when named: `just install-tools
# gh`.
install-tools *TOOL:
    uv run -q python -m repo_checks install-tools {{TOOL}}

# Install GitHub CLI at VERSION from that release's own archive, verified against
# the checksums file the release publishes, into ~/.local/bin. The job that runs
# `test-skill-install` passes the release `just tool-version gh` reads.
install-gh VERSION:
    uv run -q python -m repo_checks install-gh {{quote(VERSION)}}

# The release `repo-policy.toml` holds TOOL at, as `version=<release>`.
#
# What a workflow installing TOOL prebuilt through a step of its own rather than
# through `install-tools` appends to `GITHUB_OUTPUT` and installs, so that
# release is written in one place and a bump is one edit.
tool-version TOOL:
    @uv run -q python -m repo_checks tool-version {{quote(TOOL)}}

# The full gate: every tier `repo-policy.toml` declares, end-to-end included.
check:
    just format-check
    just lint
    just typecheck
    just test
    just coverage
    just build
    just lint-workflows
    just check-repo
    just test-e2e

# Install the workspace's JavaScript dependencies, exactly as `bun.lock` describes.
#
# Every recipe that reaches Nx runs this first, because `bunx nx` fails outright
# in a clone that has never been bootstrapped — and every publication of this
# repository is made from a clone cut fresh for it, which no one runs `just
# bootstrap` in. So a tier installs what it needs rather than requiring a person
# to run it in a directory nothing hands them.
#
# The install is the locked one: it can neither resolve nor record anything
# `bun.lock` does not already describe, and it reports `no changes` without
# reinstalling when the tree already matches. Only the JavaScript side needs
# this, because `uv run` syncs its own environment on every invocation.
node-modules:
    bun install --frozen-lockfile

# Rewrite every project's sources in its language's canonical format.
format:
    just node-modules
    bunx nx run-many -t format --output-style=stream

# Refuse a source file that is not in its language's canonical format.
format-check:
    just node-modules
    bunx nx run-many -t format-check --output-style=stream

# Lint every project with its language's linter, failing on any finding — and
# every crate once more for the Windows target on a Unix host, so a finding in
# `cfg(windows)` code is reported here rather than by a Windows runner.
lint:
    just node-modules
    bunx nx run-many -t lint --output-style=stream
    uv run -q python -m repo_checks lint-windows-target

# Type-check every project with its language's type checker.
typecheck:
    just node-modules
    bunx nx run-many -t typecheck --output-style=stream

# Run every project's tests, recording coverage as they run.
test:
    just node-modules
    cargo llvm-cov clean --workspace
    uv run -q coverage erase
    bunx nx run-many -t test --output-style=stream

# Fail the build below the coverage floors `repo-policy.toml` records.
coverage:
    uv run -q python -m repo_checks coverage

# Build every project that produces an artifact.
build:
    just node-modules
    bunx nx run-many -t build --output-style=stream

# Regenerate the three clients from the checked-in contract schemas.
#
# The generator writes in place, so running this over a tree already carrying
# what the schemas write changes nothing. `just check-repo` refuses a tree in
# which a generated client differs from what it writes, which is what makes
# three copies of one contract safe to have.
generate-clients:
    just node-modules
    uv run -q python -m contract_codegen write

# Build every artifact this repository publishes, into `dist/`.
#
# The three clients a dependent takes as a dependency, and the three
# alternative routes an end user gets the `printobserver` program by. Every one
# carries the version release automation wrote into the workspace, because no
# manifest in this tree carries one.
build-artifacts:
    uv run -q python -m release_artifacts build-all

# Publish every built artifact to the registry its own consumers install from.
#
# Run by release automation after `just build-artifacts`, never by hand. Each
# registry is authenticated by an API token carried in a repository secret,
# which `gh-secrets.json` is the authoritative list of.
publish-artifacts:
    uv run -q python -m release_artifacts publish

# Take each shipped artifact the way its own consumer takes it, and prove it.
#
# One recipe per artifact, because the six are separable and a reader chasing a
# failure wants the one that failed. Each builds from the committed tree,
# installs into a throwaway environment holding no copy of these sources, and
# then proves what it installed: a client by its own committed smoke check
# against a real supervisor, a route by the program it put on a path reporting
# its own version. The three routes are installed with a PATH holding no Rust
# toolchain at all, which is the whole reason they carry a program already
# built for the platform.
#
# These are a change's own proof of what it built, and the end-to-end tier is
# what runs them. No continuous-integration job runs one: a job proving a
# shipped artifact takes it from its registry, with the `prove-registry-*`
# recipes below, and `just check-repo` refuses a job running one of these.
prove-client-rust:
    uv run -q python -m release_artifacts prove --target crate:printobserver-sdk --into dist/proof/client-rust

prove-client-python:
    uv run -q python -m release_artifacts prove --target pypi:printobserver-sdk --into dist/proof/client-python

prove-client-node:
    uv run -q python -m release_artifacts prove --target npm:@printobserver/sdk --into dist/proof/client-node

prove-route-pypi:
    uv run -q python -m release_artifacts prove --target pypi:printobserver-cli --into dist/proof/route-pypi

prove-route-npm:
    uv run -q python -m release_artifacts prove --target npm:printobserver-cli --into dist/proof/route-npm

prove-route-script:
    uv run -q python -m release_artifacts prove --target release:printobserver --into dist/proof/route-script

# Prove one shipped artifact against what its own registry actually serves.
#
# One recipe per artifact, because the three routes are alternatives, the three
# clients are separable, and a reader chasing a failure wants the one that
# failed. PRINTOBSERVER_PROOF_VERSION names the version to prove and
# PRINTOBSERVER_PROOF_REGISTRIES points every registry somewhere other than the
# real ones; each exits zero only on a pass. A client is taken from its
# registry and its own smoke check runs against the supervisor the same
# release's asset carries, so nothing a proof here reaches is a build of this
# tree — which is why these, and not the `prove-client-*` and `prove-route-*`
# recipes above, are what a continuous-integration job runs.
#
# `@` because a proof that passed is ONE line — which version was proven, where
# it came from, what installed it and what the program said — and the echoed
# command line beside it is the only other thing such a run would print. A
# failing proof still prints its whole report, and `just` still names the recipe
# that failed, so nothing a reader chasing a failure needs is behind the echo.
prove-registry-pypi:
    @uv run -q python -m release_artifacts prove --registry --target pypi:printobserver-cli --into dist/proof/registry-pypi

prove-registry-npm:
    @uv run -q python -m release_artifacts prove --registry --target npm:printobserver-cli --into dist/proof/registry-npm

prove-registry-script:
    @uv run -q python -m release_artifacts prove --registry --target release:printobserver --into dist/proof/registry-script

prove-registry-client-rust:
    @uv run -q python -m release_artifacts prove --registry --target crate:printobserver-sdk --into dist/proof/registry-client-rust

prove-registry-client-python:
    @uv run -q python -m release_artifacts prove --registry --target pypi:printobserver-sdk --into dist/proof/registry-client-python

prove-registry-client-node:
    @uv run -q python -m release_artifacts prove --registry --target npm:@printobserver/sdk --into dist/proof/registry-client-node

# Which release the release-time run at COMMIT cut, as `version=<version>`.
#
# The one line the release-time job publishes its output from, so that the one
# concrete version reaches all three route proofs rather than each of them
# resolving "the newest" for itself. A run that cut no release — every push
# finding nothing unreleased — answers an empty field, and that is what those
# jobs are gated on. Needs a checkout carrying the commit and its tags.
release-version COMMIT ROOT:
    @uv run -q python -m release_artifacts released --commit {{quote(COMMIT)}} --root {{quote(ROOT)}}

# What the publishing job released, as `released=<tag>...`, read off ANSWER.
#
# ANSWER is the file `release-plz release --output json` wrote: the version and
# tag of every package it released, and `{"releases":[]}` where it released
# none — which is every ordinary push under `release_always`, and the program
# exits zero either way. The artifact build and the artifact publish are gated
# on this field being non-empty rather than on that exit status, because the
# Python and JavaScript registries refuse a version they already serve, and a
# publish on a push that cut nothing turns `main` red. An answer that is not
# that program's is refused rather than read as "none": a publish skipped over
# an unreadable answer is a release nobody can install and nothing reported.
release-answer ANSWER:
    @uv run -q python -m release_artifacts answered --answer {{quote(ANSWER)}}

# Which release a hand-dispatched run publishes, as `released=<tag>`, off TAG.
#
# The same line `release-answer` prints for a cut release, so the artifact
# build and the artifact publish are gated on one field whichever way the run
# was started. TAG is an existing release tag, `v<version>`; ROOT is a checkout
# carrying it — the whole history and its tags, which `fetch-depth: 0` gives
# and a shallow checkout does not; RECORD is where the one `version=<version>`
# line `release-version` prints is written, for the install-path proof to read
# after the run; and REF is the ref the run was started on, which must be the
# base branch `repo-policy.toml` declares, because that is where the publisher
# and the secrets it runs with are the ones a release trusts. A run on any
# other ref, a tag that is not release automation's, one the checkout cannot
# see, or one whose tree's workspace version is not the one it names is
# refused naming why, and nothing is printed or written: the job fails, and
# the jobs after it are skipped.
release-dispatched TAG ROOT RECORD REF:
    @uv run -q python -m release_artifacts dispatched --tag {{quote(TAG)}} --root {{quote(ROOT)}} --record {{quote(RECORD)}} --ref {{quote(REF)}}

# Which version a dispatched run recorded, as `version=<version>`, off RECORD.
#
# What the install-path proof's resolving job answers after a dispatched run,
# in place of reading a tag off the run's commit — a dispatch runs at `main`
# and publishes a tag elsewhere in the history. A record that is not there or
# holds anything but that one line is refused rather than read as an empty
# field, because the route proofs skip on that field and a proof skipped over
# an unreadable record is a publish nobody checked.
release-version-dispatched RECORD:
    @uv run -q python -m release_artifacts recorded --record {{quote(RECORD)}}

# The registry install-path proof: all three routes and all three clients,
# which is how a person runs this tier by hand. `AGENTS.md`'s "The registry
# install-path proof" is what it is, why it is not one of `just check`'s tiers,
# and when it runs.
# llmlint: ignore[tool_output_is_signal] suppressions.toml has the reason.
test-install-proof:
    @just prove-registry-pypi
    @just prove-registry-npm
    @just prove-registry-script
    @just prove-registry-client-rust
    @just prove-registry-client-python
    @just prove-registry-client-node

# Where one job of `ci.yml` or of `install-path.yml` runs for one supported
# platform, as `runner=<label>` and `source=<workflow>`: what the platform-
# dispatch workflow's `select` job appends to its outputs. JOB is a platform-
# matrixed job of either workflow and PLATFORM an identifier from `AGENTS.md`'s
# supported-platform list; a pair naming neither is refused, exit 2, before
# anything runs.
dispatch-resolve JOB PLATFORM:
    @uv run -q python -m repo_checks.dispatching resolve --job {{quote(JOB)}} --platform {{quote(PLATFORM)}}

# Run that job's own `run:` steps here, off the committed source workflow, as the
# platform-dispatch workflow's `run` job does on the runner `dispatch-resolve`
# answered: each step's command under bash, its condition and its waiver read
# off the source, and the exit the first unwaived failure's.
dispatch-run JOB PLATFORM:
    uv run -q python -m repo_checks.dispatching run --job {{quote(JOB)}} --platform {{quote(PLATFORM)}}

# Validate the committed workflows: parse, pinned actions, allowlisted commands.
lint-workflows:
    uv run -q actionlint
    uv run -q shellcheck --severity=style scripts/*.sh tools/repo-checks/src/repo_checks/*.sh
    uv run -q python -m repo_checks workflows

# Run this repository's own deterministic checks over the committed tree.
check-repo:
    uv run -q python -m repo_checks all

# The end-to-end tier: journeys that drive the real gate, checks and bootstrap.
#
# release-plz first, by name: `repo-policy.toml` declares it `bootstrap = false`
# — only this tier and the release workflow run it — and the three journeys that
# drive it skip where it is absent. Installing it here is what keeps those three
# from going silently skipped on a host, or a gate cell, that bootstrapped and
# nothing more. The install downloads one verified prebuilt archive, and skips
# even that where the held release is already on PATH.
#
# llmlint: ignore[external_service_suite_stays_out_of_the_affected_tier] suppressions.toml has the reason.
test-e2e:
    uv run -q python -m repo_checks install-tools release-plz
    just node-modules
    bunx nx run-many -t test-e2e --output-style=stream

# A real `gh skill install` of this repository's Agent Skill, from a copy of this
# tree and with no GitHub credentials, and `gh skill publish --dry-run` over it.
#
# Deliberately not one of `just check`'s tiers: it needs GitHub CLI at the
# release `repo-policy.toml` holds, which the gate's runners are not given, so
# it is the `skill-install` job of its own. Without that gh it refuses, naming
# what is missing, rather than skipping.
test-skill-install:
    just node-modules
    bunx nx run-many -t test-skill-install --output-style=stream

# Bring up the scripted OctoPrint environment the integration tier drives.
#
# The virtual printer, a free port and the hold print, under `.octoprint-env`.
# `OCTOPRINT_ENV_MODE=serial OCTOPRINT_ENV_DEVICE=/dev/ttyACM0` is the same
# environment against the machine beside the printer, the device named the
# way that host names one; a refusal says what that is.
octoprint-up:
    uv run -q python tools/octoprint-env/octoprint_env.py up

# Stop it, leaving no process behind.
octoprint-down:
    uv run -q python tools/octoprint-env/octoprint_env.py down

# The printer integration tier, against the environment `octoprint-up` started.
#
# Deliberately not one of `just check`'s tiers: it installs OctoPrint, starts
# it, and waits a print out, so it is a continuous-integration job of its own
# rather than something every gate run pays for.
test-integration:
    just node-modules
    bunx nx run-many -t test-integration --output-style=stream

# Rewrite every generated documentation artifact from what the tree declares.
#
# Three generators, and no artifact any of them writes is edited by hand: the
# command surface out of this program's own `surface()`, the schema document out
# of the schema tree the contracts' generation target writes, and every worked
# example out of what that command actually printed against a real supervisor.
# `just check-repo` refuses a tree in which any of the three has drifted, so this
# is what a change to the surface, the contracts or an example runs afterwards.
# llmlint: ignore[changed_behavior_has_e2e] Proving this composite repairs an incomplete generated documentation set requires the tree surgery this dispatch forbids. Its components retain the real schema-writing CLI test, runtime surface comparison, and every documented example executed against a real server.
docs-generate:
    just node-modules
    bunx nx run printobserver:docs-surface --output-style=stream
    uv run -q python -m repo_checks docs-schemas-write
    RUSTFLAGS=-Dwarnings PRINTOBSERVER_DOCS=write cargo test --locked -p printobserver --features test-fixtures --test journeys every_documented_example

# The real-printer smoke test, which nothing runs by accident.
#
# Two things together select it and one alone does not: the `--run` flag here,
# and PRINTOBSERVER_SMOKE_DEVICE naming the serial device the printer is on.
# Absent either, it says so and which was missing rather than skipping silently.
# Deliberately not a tier of `just check`, not a job of continuous integration
# and not on any schedule: a print is hours of filament, and an unattended test
# that starts one ruins a print nobody was watching.
#
#     PRINTOBSERVER_SMOKE_DEVICE=/dev/ttyACM0 just test-printer-smoke --run
#
# `AGENTS.md`'s "The real-printer smoke test" says what it requires, how to set
# a host up for it, and what to watch while it runs. Stay next to the machine.
test-printer-smoke *flags:
    PYTHONPATH="tools/octoprint-env$PATH_SEPARATOR$PYTHONPATH" uv run -q python tools/printer-smoke/printer_smoke.py {{flags}}

# Refuse a pull-request title that is not a Conventional Commit subject.
check-pr-title:
    uv run -q python -m repo_checks pr-title

# Update every lockfile, then re-run the gate on the upgraded versions.
upgrade:
    cargo update
    uv lock --upgrade
    bun update
    just check

# Install llmlint and the harness it judges through.
setup-llmlint:
    ./scripts/setup-llmlint.sh

# The judged-lint tier's deterministic half: config, plugins and suppressions.
lint-llm-validate:
    llmlint validate

# The judged-lint tier over this change's merge-base diff.
lint-llm-diff:
    llmlint --diff --diff-base "origin/main"

# The judged-lint tier over the whole tree.
lint-llm:
    llmlint

# Prove release-plz accepts this tree's release configuration, publishing nothing.
release-dry-run:
    release-plz release --dry-run

# Bring up the self-hosted Obico stack the scheduled reconciliation tier drives.
#
# Four containers under `.obico-env`, built from Obico's own sources at a pinned
# revision. It says on stderr what it is about to start and roughly how long that
# takes before it starts anything: about 25 minutes the first time and about 3
# once the images are built. `OBICO_ENV_WEBHOOK_URL` names the address the
# notification plugin is configured to post to; `auto` takes a free port.
obico-up:
    uv run -q python tools/obico-env/obico_env.py up

# Stop it, leaving no container of it running.
obico-down:
    uv run -q python tools/obico-env/obico_env.py down

# The scheduled Obico tier, against the stack `obico-up` started.
#
# Deliberately NOT one of `just check`'s tiers, and `just check-repo` refuses a
# tree in which it is: it builds Obico's own images, starts four containers and
# waits a real failure alert out. It is a scheduled workflow of its own —
# `.github/workflows/obico.yml` — rather than something every gate run pays for.
test-obico:
    just node-modules
    bunx nx run-many -t test-obico --output-style=stream
