# AGENTS

`printobserver` is a thin, provably-correct supervision layer between a 3D
printer and an agent that watches it. One installable artifact — the
`printobserver` command — carries everything: `printobserver server` runs the
supervisor, and the other subcommands are the operator's surface onto a running
one. The clients (Rust, Python, Node) talk to that server.

> `CLAUDE.md` is a symlink to this file (`ln -s AGENTS.md CLAUDE.md`). Edit
> `AGENTS.md` only; the two must never drift.

This repository runs on agents with no human reading every diff, so the suite
and the gate are the only quality loop there is. `repo-policy.toml` holds the
machine-readable half of these rules; `just check-repo` enforces them.

## Two standing goals on every task

The user drives features and their request is the priority — but carry two goals
into *every* task, folding either in when it is the lowest-error path to what
was asked and proposing it otherwise.

1. **Engineer the context for next time.** Realistic end-to-end tests over what
   a consumer observes, scripts that shrink output to signal, and terse notes
   here capturing what the code cannot make obvious.
2. **Engineer the codebase and environment.** Keep it clean, repeatable and
   automatically set up.

## Stack and composition

Composed with the `create-repo` skill's own composer, at
`~/.claude/skills/bootstrap/create-repo`:

```console
uv run --script scripts/compose_repo_plan.py --shape cli \
  --language rust --language python --language typescript --releasing \
  -o REPO_PLAN.md
```

- **Shape:** `cli`. The installable artifact is the `printobserver`
  command-line program. The server is a *subcommand* of it rather than a second
  artifact, which is why the shape is a CLI and not a library or a web app.
- **Languages:** `rust` (the workspace and the artifact), `python` and
  `typescript` (the client distributions and this repository's own tooling).
- **Ships a versioned artifact:** yes — `--releasing`, so `releasing.md`
  composed in. See "Commits, releases, and merging".
- **Auto-composed intersections:** `rust-cli` and `python-cli`.
- **Always applied, with no flag:** `base.md`, `project-graph.md`, `ci.md`.

Every optional piece of guidance the composer offers, with its disposition.
This record is this repository's claim about the create-repo skill as it stood
when the baseline was written; the skill's own baseline checker, run by a person
against the tree, is what reconciles it against a later version of that skill.

[//]: # (BEGIN composition-record)
- `shape:cli` — included: the deliverable is one installed command, so the CLI
  shape's entry-point e2e, output contract and asset-naming rules all apply.
- `shape:library` — excluded: the clients are thin generated surfaces over the
  server's HTTP API, not standalone libraries with their own product shape; the
  library guidance would duplicate the CLI shape's packaging rules with nothing
  added.
- `shape:web-app` — excluded: nothing here serves a browser. The server speaks
  HTTP to programs.
- `shape:react` — excluded: builds on `web-app`, which does not apply.
- `shape:nextjs` — excluded: builds on `react`, which does not apply.
- `shape:asdf-plugin` — excluded: this repository ships no asdf plugin.
- `shape:skills-repo` — excluded: this repository ships no agent skills.
- `language:rust` — included: the workspace, the artifact and every port and
  adapter are Rust.
- `language:python` — included: the Python client distribution, and this
  repository's own deterministic checks and end-to-end tier.
- `language:typescript` — included: the Node client distribution.
- `language:bash` — excluded: as a *product* language. The only shell in the
  tree is session and toolchain setup, plus the install scripts the `sdks` and
  `server` nodes ship. Its gate is `shellcheck` inside `just lint-workflows`
  rather than a project of its own, and the judged tier carries no bash rules.
- `language:terraform` — excluded: this repository provisions no infrastructure.
  The deployment target is one machine beside a printer, reached by the
  end-user install path below.
- `intersection:rust-cli` — included: auto-composed from `cli` + `rust`.
- `intersection:python-cli` — included: auto-composed from `cli` + `python`.
- `cross-cutting:releasing` — included: passed as `--releasing`, because this
  repository publishes versioned artifacts and release automation runs from
  Conventional Commits with no manual step.
- `cross-cutting:project-graph` — included: mandatory, with no flag to pass.
  Every crate, client and tool is a project in the Nx graph.
- `cross-cutting:ci` — included: mandatory, with no flag to pass. GitHub Actions
  proves the artifact on the supported platform matrix and proves the end-user
  install path.
- `cross-cutting:llmlint` — included: `llmlint.yml` and `oneharness.toml` are
  composed rather than hand-rolled, and the judged tier is a continuous-
  integration job of its own rather than a step inside the gate job.
- `optional:asdf` — included: `.tool-versions` pins `just`, Node and `uv` for
  asdf/mise users. The Rust toolchain is pinned by `rust-toolchain.toml`
  instead, because rustup reads that and asdf does not.
- `optional:direnv` — excluded: `just bootstrap` is the one entry point and it
  is explicit; an implicit environment that changes under `cd` would make the
  gate depend on where it was invoked from.
- `optional:src-layout` — included: for Python (`src/`) and Rust (`crates/`).
  The Node client keeps `src/` beside `test/`, which is that ecosystem's own
  layout rather than a departure from this one.
- `optional:pre-commit-framework` — excluded: the committed `.githooks` call the
  gate directly. A framework that re-specifies the tools the gate already runs
  is a second, drifting copy of the gate.
- `optional:notignored` — included: `.github/workflows/notignored.yml` posts the
  suppressions a change adds. It is a review artifact, deliberately not a
  required check.
- `optional:pr-template` — included: `.github/pull_request_template.md`, whose
  **What**/**Why** becomes the squash body release automation reads.
[//]: # (END composition-record)

Non-negotiable invariants are never on that list: a strict gate, realistic
un-mocked end-to-end coverage of every real journey, and CI that proves the
artifact are not optional and are not excluded with a rationale.

## Command surface

Use the `just` recipes; do not hand-roll equivalents. `just --list` is the
index, and `just check` is the whole gate.

`repo-policy.toml`'s `gate.tiers` declares every tier `just check` must invoke —
`format-check`, `lint`, `typecheck`, `test`, `coverage`, `build`,
`lint-workflows`, `check-repo` and the end-to-end `test-e2e`. `just check-repo`
refuses a `check` recipe that omits one, and refuses a recipe in that set whose
body would run nothing.

The gate is strict in all five senses: formatting, linting, type checking and
tests each fail the build on an issue, coverage is measured on the test run, and
`just coverage` fails the build below the floors `repo-policy.toml` records (95%
lines, per ecosystem). There is no warnings-only mode.

The judged-lint tier (`just lint-llm-diff`) is deliberately **not** in `just
check`: it is non-deterministic and needs a harness credential, so it is a
continuous-integration job of its own.

Recipes delegate to `nx run-many` rather than looping over packages, so a new
project joins the gate by declaring the target names every other project uses.

## Supported platforms

This is the one source both this repository's continuous-integration matrices
are derived from; `just check-repo` refuses a matrix that names a platform this
list does not, or omits one it does. Neither matrix can be narrowed
independently of this list.

[//]: # (BEGIN supported-platforms)
- `linux-x86_64` — runner `ubuntu-24.04`, Rust target `x86_64-unknown-linux-gnu`, service manager `systemd`, install path: yes
- `linux-aarch64` — runner `ubuntu-24.04-arm`, Rust target `aarch64-unknown-linux-gnu`, service manager `systemd`, install path: yes
[//]: # (END supported-platforms)

Both are Linux with systemd, which is what the unit in the end-user install path
below is written for. `linux-aarch64` is not optional: the machine beside the
printer is usually a small ARM board, and it is the worst place to discover an
architecture was never built for. macOS and Windows are deliberately absent —
they can host a *client*, and the clients land at the `sdks` node, which is
where the list grows if they need one.

## The end-user install path

This section is the authoritative source of the end-user install path. Every
other statement of it in this repository — the README, the continuous-
integration jobs that exercise it — is derived from here, and `just check-repo`
refuses any that differs.

The path is **one program obtained by any one of three alternative routes, and
then two commands in order**.

### The three routes

These three routes are alternatives reaching the same program: take one of them,
not all three. Which package manager the machine in front of you already has is
not something this repository gets to assume, and the third route needs neither
of them. Every route puts a program **already built for the platform** on your
path, so none of them needs a Rust toolchain on the target host and none of them
compiles anything there — the target host is the small machine beside the
printer, which is the worst place to compile a Rust workspace. (`cargo install
printobserver` is a developer path and is deliberately not one of these three.)

#### Route 1 — the Python package registry

An ordinary install of this repository's CLI-capable distribution
`printobserver-cli`: a wheel carrying the `printobserver` program already built
for the platform. Made executable by the `sdks` node.

```console
pip install printobserver-cli
```

#### Route 2 — the JavaScript package registry

An ordinary global install of the CLI-capable distribution of that registry —
the same distribution name and the same command — with the platform-specific
program carried in a per-platform artifact the package manager selects for the
caller's operating system and processor. Made executable by the `sdks` node.

```console
npm install -g printobserver-cli
```

#### Route 3 — the bundled install script

`scripts/install.sh`, committed in this repository beside the artifacts it
downloads and fetched by this one-line command. It detects the caller's
platform, downloads the prebuilt release artifact for it, verifies what it
downloaded, and puts the program on the caller's path. This is the route for a
machine that has neither package manager. Made executable by the `sdks` node.

```console
curl -fsSL https://raw.githubusercontent.com/nickderobertis/printobserver/main/scripts/install.sh | sh
```

It defaults to the newest release and a default directory, and also accepts a
pinned release and an install directory:

```console
curl -fsSL https://raw.githubusercontent.com/nickderobertis/printobserver/main/scripts/install.sh | sh -s -- --version v0.1.0 --to ~/.local/bin
```

### Then, in order — two commands

Both run as root. The first is the installer the `server` node ships, committed
at `scripts/install-service.sh`: it puts the binary, the state directory, the
configuration and the systemd unit in place. The second enables and starts the
unit, whose name is `printobserver.service`. Made executable by the `server`
node.

```console
curl -fsSL https://raw.githubusercontent.com/nickderobertis/printobserver/main/scripts/install-service.sh | sudo sh
```

```console
sudo systemctl enable --now printobserver.service
```

**Enabling and starting is a command of its own rather than something the
installer does, and the reason is that this service commands a 3D printer.**
Installing a package must not, as a side effect, start a process that can move a
machine, so starting the supervisor stays a decision somebody takes rather than
something that happens while they are installing. Do not fold these two steps
back together.

None of the three routes and neither of these two commands runs yet: the
distributions, the install script and the service installer do not exist at the
baseline. What makes the three routes executable is the `sdks` node, and what
makes the two commands executable is the `server` node — each held to this
section.

## Commits, releases, and merging

**Merge model.** A change reaches `main` through a pull request, squash-merged,
with auto-merge on and head branches deleted. **`main` takes no direct push.**
The pull request title becomes the squash subject, which is the commit release
automation reads, so it is linted against Conventional Commits as a required
check.

The jobs required to be green before a pull request can merge:

[//]: # (BEGIN required-checks)
- `gate`
- `llmlint`
- `pr-title`
[//]: # (END required-checks)

`just check-repo` refuses a required name with no job behind it. Applying these
settings to the repository itself is a person's action through GitHub — it needs
these jobs to exist first — and is tracked as its own task of this plan.

**Which subjects release.** `repo-policy.toml`'s `commits.release_types` is the
source: **`feat`, `fix` and `perf`** cut a release (and `!` / `BREAKING CHANGE`
raises the bump); `docs`, `test`, `chore`, `ci`, `refactor`, `build`, `style`
and `revert` are valid subjects that release nothing. `release-plz.toml`'s
`release_commits` regex and the committed `commit-msg` hook are held to that
same list by `just check-repo`. Pre-1.0 Cargo rules apply: `feat`/`fix`/`perf`
bump the patch, `!`/`BREAKING CHANGE` bumps the minor.

**How a release happens.** `.github/workflows/release-plz.yml` fires on every
push to `main` with no manual invocation. `release-plz release-pr` opens the
release pull request under `RELEASE_PLZ_TOKEN` — the workflow's built-in token
is deliberately not used, because a pull request opened by it does not trigger
the workflows that gate it — and `release-plz release` tags, cuts the GitHub
Release and publishes every crate under `CARGO_REGISTRY_TOKEN`. Nobody
hand-edits a version, hand-tags, or hand-dispatches a publish.

`release-targets.toml` declares what this repository publishes.
`repo-policy.toml`'s `manifests.automation_owned` is the only set of manifests
permitted to carry a version field, and `just check-repo` enforces it: a version
anywhere else is one a person would have to hand-maintain.

**Secrets.** `gh-secrets.json` is the authoritative list of the Actions secrets
this repository holds. A workflow may reference no secret outside it, spelled as
that manifest spells it, and `just check-repo` enforces that too.

## The dependency rule

`printobserver-core` may depend on `printobserver-types` and the four `*-api`
port crates, and on **no implementation crate**. **No implementation crate may
depend on another.** `printobserver-server` and the `printobserver` binary are
the composition roots and are the only crates allowed to name an implementation.
The roles are declared in `repo-policy.toml` and enforced by `just check-repo` —
the boundary is not a convention, it is a check.

## Tests are the only QA loop

Never mock the layer under test. Drive the real artifact across real boundaries
the way a user does: the compiled binary as a subprocess, the real recipes, the
real checks. "Done" means every user-facing journey, happy path **and**
failure/recovery — not one smoke test. Coverage is a floor, not the target.

The end-to-end tier lives in `tests/repo-e2e` and drives this repository's own
gate: it runs `just bootstrap` in a fresh copy carrying no build products, and
it assembles copies of the tree carrying one defect each and asserts the gate
refuses each one. Those copies omit `tests/repo-e2e` itself, because a gate that
ran the suite that runs the gate could not terminate.

## Suppressions

`suppressions.toml` is the only way to suppress a diagnostic here. Every
directive needs an entry naming the rule, the file, the site and a non-empty
reason, added in the same change. Nothing in the check asks whether a reason is
a *good* reason — that is for whoever reviews the change.

**Silencing a rule for a whole file is refused**, not allowlisted — in a
*configuration* file (a blanket `ignore`, a per-file glob, a crate lint set to
`allow`, a linter rule set to `off`) and by a file-level directive inside the
file itself (ruff's `noqa` file form, flake8's, mypy's `ignore-errors`,
`biome-ignore-all`, `eslint-disable` — each spelled here without its comment
marker so this document carries none). Both cover every line, including lines
written long after the reason was true, and neither names a site. Choosing which
rules run is not suppression and is left alone.

**One narrow exception, enumerated and enforced.** Every deterministic linter
and type checker here attributes each finding to a line, so a whole-file
directive from one of them is always broader than the finding it answers and is
refused with no exception at all. llmlint is the one tool with rules that are
about a file as a whole — whether a script's output is signal, whether its
inputs are validated — and for those a whole-file `ignore-file` directive is the
only shape that fits. `repo-policy.toml`'s `suppressions.whole_file_rules` names
exactly which rules that reaches; `just check-repo` refuses one naming any other
rule, refuses the same form from any other tool, and still requires the
directive's own allowlist entry and reason. Two such directives stand in the
tree today, both in `scripts/`, and both are listed in `suppressions.toml`.

Every other suppression is a single line answering a single finding. Where even
that is avoidable, avoid it: every subprocess goes through
`repo_checks.shell.run`, which resolves the executable against PATH, so `S607`
is fixed by construction and `S603` has one site rather than thirty-four. The
suites are written in the `repo_checks.expect` vocabulary rather than the
`assert` statement, so `S101` is enabled with nothing suppressing it anywhere.

## Keeping the allowlist current

The agent command allowlist is `.claude/settings.json`. `just check-repo`
derives the set of programs this repository's own recipes and graph targets
invoke and requires the allowlist to name exactly that set: no command missing,
and none beyond it. When a recipe starts calling a new program, the allowlist
entry lands in the same change.
