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
# repository is made from a clone cut fresh for it, where the `pre-push` hook
# runs the whole gate. So the gate installs what it needs rather than requiring
# a person to run `just bootstrap` in a directory nothing hands them.
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
    bunx nx run-many -t test-integration --output-style=stream

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
