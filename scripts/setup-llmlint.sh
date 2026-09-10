#!/usr/bin/env bash
# Idempotent setup for the optional `llmlint` LLM-judge tier (oneharness + llmlint).
#
# Wired into the Claude Code SessionStart hook (.claude/settings.json) so web/cloud
# sessions can run `just lint-llm` / `just lint-llm-diff` with no manual steps; also
# safe to run by hand (`just setup-llmlint`) or from a terminal. Every step
# tolerates failure and the script always exits 0 — a flaky install must never
# break session startup.
#
# What it does, and why:
#   1. Installs the `llmlint` binary from PyPI via `uv tool`. `llmlint-cli` wraps
#      the prebuilt binary and depends on `oneharness-cli`, so one dependency
#      resolution fetches both wheels — no Rust toolchain and no github.com
#      reachability (works in restricted-egress sessions where PyPI is reachable).
#      `uv tool` links only the *requested* package's executable onto PATH, but
#      llmlint >= 0.3.23 finds `oneharness` beside its own binary in the tool venv —
#      so this one install is a complete setup; no separate oneharness install /
#      PATH entry. `--upgrade` bumps an older cached tool, honouring the floor below
#      (`just lint-llm-diff` needs the changed-file-scoped `--diff` and three-dot
#      `--diff-base` default; `just lint-llm-validate` needs the `validate` gate).
#   2. Installs an agent harness when the host carries none oneharness can spawn.
#      Step 1 installs `llmlint` and `oneharness` but neither of the agent binaries
#      oneharness drives, so on a host that has no harness at all — a continuous-
#      integration runner — every candidate in the fallback chain is skipped as
#      uninstalled and the tier errors with "all harnesses in the fallback chain
#      failed", having judged nothing. Which chain to look for is read back out of
#      oneharness's own effective configuration rather than restated here, so
#      `oneharness.toml` stays the one source of it.
#   3. Persists PATH so the freshly installed binaries resolve for whatever runs
#      next: into CLAUDE_ENV_FILE in a Claude Code session, and into GITHUB_PATH in
#      a workflow step, whose next step is a new shell that would not otherwise see
#      `$HOME/.local/bin`.
#
# Harness selection: the committed `oneharness.toml` is in fallback mode (codex +
# gpt-5.5 primary, claude-code + opus-4.8 secondary), so llmlint runs the primary
# for a contributor with Codex authenticated and falls through to claude-code in a
# Claude Code session where codex is absent — no `ONEHARNESS_*` override needed
# (one would only clobber the fallback list). If your fallback order can't select
# the right harness for some environment, set ONEHARNESS_HARNESSES there.
# llmlint: ignore-file[tool_output_is_signal, boundary_inputs_validated] deliberate for a session-startup installer (see header): success stays quiet while failures log-and-continue rather than block startup; and every external input is a named package from a registry the ecosystem authenticates — the `llmlint-cli` wheel from PyPI (Trusted Publishing + PEP 740 attestations) and the `@anthropic-ai/claude-code` package from npm (provenance attestations) — each a constant in this file rather than anything a caller supplies, so no unvalidated external input is executed.
set -uo pipefail

# Version floor, as a PyPI constraint (the `llmlint-cli` package version tracks the
# wrapped binary version). `uv tool install --upgrade` installs the newest release
# satisfying it; oneharness comes along transitively at a compatible version.
# llmlint >= 0.3.23 finds `oneharness` beside its own executable (so a lone
# `uv tool install llmlint-cli` works), gives the whole-tree default the composed
# llmlint.yml relies on (it omits `files.include`), restricts `--diff` to the
# changed files (skipping empty diffs) so `just lint-llm-diff` judges only the
# branch's changes, treats a plain `--diff-base <ref>` as three-dot/merge-base
# (0.3.15), and ships the deterministic `validate` gate — config structure +
# `llmlint: ignore` directives + fragment version bumps — that `just
# lint-llm-validate` runs with no model call (0.3.17), and bundles config_lint v1.2
# so `line_localizable_rules_require_attribution` is enforced (0.3.23).
readonly LLMLINT_MIN="0.3.23"
# The package `@anthropic-ai/claude-code` publishes; installing it puts the `claude`
# binary oneharness spawns for the `claude-code` harness onto PATH. It is the member
# of the fallback chain whose credential the llmlint continuous-integration job
# wires (ANTHROPIC_API_KEY), which is why it is this script's choice of harness to
# install rather than the chain's primary.
readonly HARNESS_PACKAGE="@anthropic-ai/claude-code"
# `npm --prefix` writes binaries to `$PREFIX_DIR/bin`, which is the directory this
# script already puts on PATH and persists — so one prefix serves both installs.
readonly PREFIX_DIR="$HOME/.local"
readonly BIN_DIR="$PREFIX_DIR/bin"

log() { printf 'setup-llmlint: %s\n' "$*" >&2; }

# Install llmlint from PyPI via uv (the repo's Python package manager). uv is a
# clean-clone prerequisite; if it is somehow absent, log an actionable pointer and
# leave any already-installed binary in place rather than aborting startup.
ensure_toolchain() {
  if ! command -v uv >/dev/null 2>&1; then
    log "uv not found; cannot install llmlint (install uv: https://docs.astral.sh/uv/)"
    return 0
  fi
  # llmlint-cli pulls oneharness-cli as a dependency into the same tool venv, where
  # llmlint discovers the `oneharness` binary beside its own — no separate install.
  log "installing llmlint-cli >= $LLMLINT_MIN via uv tool"
  uv tool install --upgrade "llmlint-cli>=$LLMLINT_MIN" >&2 \
    || log "llmlint-cli install failed (continuing)"
}

# Where `oneharness` actually is. `uv tool` links only the *requested* package's
# executable onto PATH, so oneharness is not on it: it sits beside `llmlint` inside
# the tool venv, which is where llmlint itself resolves it.
oneharness_binary() {
  local llmlint_path venv_bin
  if command -v oneharness >/dev/null 2>&1; then
    command -v oneharness
    return 0
  fi
  llmlint_path=$(command -v llmlint 2>/dev/null) || return 1
  venv_bin=$(dirname "$(readlink -f "$llmlint_path")")
  [ -x "$venv_bin/oneharness" ] || return 1
  printf '%s\n' "$venv_bin/oneharness"
}

# The fallback chain, read back out of oneharness's own effective configuration
# (`oneharness config` reports every field's value and where it came from). Asking
# oneharness rather than listing the harnesses here keeps `oneharness.toml` — and
# any ONEHARNESS_HARNESSES override layered over it — the one source of the chain,
# so this script cannot drift from the file that decides what the tier drives.
configured_chain() {
  local python
  python=$(command -v python3 2>/dev/null) || python=$(command -v python 2>/dev/null) || return 1
  "$1" config 2>/dev/null | "$python" -c 'import json, sys
print(" ".join(json.load(sys.stdin)["harnesses"]["value"] or []))' 2>/dev/null
}

# Zero when oneharness can already spawn one of the configured harnesses, so this
# host needs no install. `detect --require-available` is oneharness's own answer to
# "is this harness installed", reported as an exit status. Every way of failing to
# find out returns non-zero: an unanswerable question installs a harness rather than
# leaving the tier with none, which is the failure this step exists to prevent.
a_configured_harness_is_available() {
  local oneharness chain_text id
  local -a chain
  oneharness=$(oneharness_binary) || return 1
  chain_text=$(configured_chain "$oneharness") || return 1
  read -ra chain <<< "$chain_text"
  for id in "${chain[@]}"; do
    if "$oneharness" detect --harness "$id" --require-available >/dev/null 2>&1; then
      log "harness \`$id\` is installed; installing none"
      return 0
    fi
  done
  return 1
}

# Install a harness when the host carries none oneharness can spawn. Without this
# the tier errors with "all harnesses in the fallback chain failed" and judges
# nothing — which is what a continuous-integration runner does, because nothing
# else on it installs an agent.
ensure_harness() {
  a_configured_harness_is_available && return 0
  if ! command -v npm >/dev/null 2>&1; then
    log "no harness installed and npm absent; cannot install $HARNESS_PACKAGE"
    return 0
  fi
  log "no harness oneharness can spawn; installing $HARNESS_PACKAGE via npm"
  npm install -g --prefix "$PREFIX_DIR" "$HARNESS_PACKAGE" >&2 \
    || log "$HARNESS_PACKAGE install failed (continuing)"
}

# Persist env for the rest of the session via CLAUDE_ENV_FILE (Claude Code sources
# it into every later Bash call). PATH so the freshly installed binaries resolve.
# No-op outside a session.
persist_session_env() {
  [ -n "${CLAUDE_ENV_FILE:-}" ] || { log "no CLAUDE_ENV_FILE (not a session); skipping env"; return 0; }
  {
    case ":${PATH}:" in *":${BIN_DIR}:"*) ;; *) printf 'export PATH=%q\n' "${BIN_DIR}:${PATH}";; esac
    # No ONEHARNESS_* override: oneharness.toml's fallback mode selects the harness
    # (codex primary, claude-code secondary), so a Claude Code session — where codex
    # is absent — falls through to claude-code on its own. Set ONEHARNESS_HARNESSES
    # here only if a specific environment's fallback order can't pick correctly.
  } >> "$CLAUDE_ENV_FILE"
  log "exported PATH"
}

# Persist PATH for the rest of a workflow job via GITHUB_PATH. Each step of a job
# is a fresh shell, so the export below reaches this script and nothing after it —
# and the step that runs the tier is the one that has to find the harness binary.
# No-op outside a workflow.
persist_ci_env() {
  [ -n "${GITHUB_PATH:-}" ] || { log "no GITHUB_PATH (not a workflow); skipping env"; return 0; }
  printf '%s\n' "$BIN_DIR" >> "$GITHUB_PATH"
  log "added $BIN_DIR to GITHUB_PATH"
}

export PATH="${BIN_DIR}:${PATH}"
ensure_toolchain
ensure_harness
persist_session_env
persist_ci_env
# `llmlint doctor` confirms the sibling `oneharness` is reachable (it is not on
# PATH — llmlint resolves it beside its own binary), so report via doctor.
if command -v llmlint >/dev/null 2>&1; then
  log "ready (llmlint: $(llmlint --version 2>/dev/null || echo unknown))"
  llmlint doctor >&2 2>&1 || log "llmlint doctor reported an issue (see above)"
else
  log "llmlint not installed"
fi
exit 0
