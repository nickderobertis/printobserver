# printobserver — one command surface. `just --list` is the index.
#
# `just check` is the whole gate: it invokes every tier `repo-policy.toml`
# declares, the end-to-end tier included. Nothing here is a placeholder.

set shell := ["bash", "-euo", "pipefail", "-c"]

# `repo_checks` is a plain package rather than a distribution, so that no
# hand-maintained version string enters the tree. This is how the recipes and
# the graph targets reach it.
export PYTHONPATH := "tools/repo-checks/src"

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

# Install the tools `repo-policy.toml` declares, skipping any already on PATH.
install-tools:
    uv run -q python -m repo_checks install-tools

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

# Lint every project with its language's linter, failing on any finding.
lint:
    just node-modules
    bunx nx run-many -t lint --output-style=stream

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

# Validate the committed workflows: parse, pinned actions, allowlisted commands.
lint-workflows:
    uv run -q actionlint
    uv run -q shellcheck --severity=style scripts/*.sh
    uv run -q python -m repo_checks workflows

# Run this repository's own deterministic checks over the committed tree.
check-repo:
    uv run -q python -m repo_checks all

# The end-to-end tier: journeys that drive the real gate, checks and bootstrap.
test-e2e:
    just node-modules
    bunx nx run-many -t test-e2e --output-style=stream

# Bring up the scripted OctoPrint environment the integration tier drives.
#
# The virtual printer, a free port and the hold print, under `.octoprint-env`.
# `OCTOPRINT_ENV_MODE=serial OCTOPRINT_ENV_DEVICE=/dev/ttyACM0` is the same
# environment against the machine beside the printer.
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
    PYTHONPATH="tools/octoprint-env:$PYTHONPATH" uv run -q python tools/printer-smoke/printer_smoke.py {{flags}}

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
