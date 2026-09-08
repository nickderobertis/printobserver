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

That tier needs the *harness* as well as its credential, and the two are not the
same thing. `llmlint` and `oneharness` drive a separate agent binary that
neither of them carries, so on a host with none — a runner, where nothing else
installs an agent — oneharness skips every candidate in `oneharness.toml`'s
chain as uninstalled and the tier errors having judged nothing. It reads as a
broken toolchain rather than as a missing agent, and on a push to `main` it
hides completely, because that diff is empty and no rule runs to need one. So
`just setup-llmlint` installs an agent when the host carries none, and asks
`oneharness` which ones would count rather than restating the chain.

Recipes delegate to `nx run-many` rather than looping over packages, so a new
project joins the gate by declaring the target names every other project uses.

Every recipe that reaches Nx runs `just node-modules` — the locked `bun install
--frozen-lockfile` — before it gets there. `bunx nx` fails outright in a clone
whose JavaScript dependencies have never been installed, and the `pre-push` hook
runs the whole gate in exactly such a clone every time this repository is
published from a fresh one, so the gate heals that state rather than needing a
person to run `just bootstrap` in a directory nothing hands them. Being locked,
the install can neither resolve nor record anything `bun.lock` does not already
describe, and it reinstalls nothing when the tree already matches. Only the
JavaScript side needs this, because `uv run` syncs its own environment on every
invocation. `just check-repo` refuses a recipe that reaches Nx without it.

That same path hands the gate git's own hook environment, in which `GIT_DIR`
names the repository being pushed and everything the gate starts inherits it.
`repo_checks.shell.run` drops the variables that name a repository, so a
subprocess works on the directory it was given; without that, the suites that
build repositories in temporary directories commit into the one being pushed
instead. Both of these are proven by `tests/repo-e2e` driving the committed
`pre-push` hook over a copy carrying neither installed dependencies nor a clean
environment — where they fail, rather than argued about here.

## Supported platforms

This is the one source every one of this repository's continuous-integration
matrices is derived from; `just check-repo` refuses a matrix that names a
platform this list does not, or omits one it does. No matrix can be narrowed
independently of this list.

A matrix belongs to a job whose behaviour depends on the platform, and
`repo-policy.toml`'s `workflows.platform_dependent_kinds` is that set. Jobs of
those kinds are held to this list; `just check-repo` refuses a platform matrix on
any other job, because a second cell over the non-deterministic judged tier would
not be a second platform but a second, independent verdict on one diff — free to
pass and fail the same content while both are required checks.

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

## The scripted OctoPrint environment

The printer integration tier drives a real OctoPrint, because a fake one would
prove a fake. `tools/octoprint-env/octoprint_env.py` is that environment: one
script, two modes, and no container. The real printer is reached over a USB
serial port, so a containerized OctoPrint would be a second, different
installation beside the one that actually drives the machine — the same script
serves the tier and the board beside the Prusa.

- **Two modes, one flag apart.** `--mode virtual` enables OctoPrint's own
  virtual printer and connects to it, which is what the tier drives.
  `--mode serial --device /dev/ttyACM0 --baudrate 115200` connects to a real
  USB device, which is what the printer uses. The two compose the same
  configuration and differ in the connection alone; the script's
  `CONNECTION_KEYS` names exactly which keys that is, and a journey asserts the
  two configurations differ in those and in nothing else.
- **Provisioned, not assumed.** `install` is idempotent and unattended: a
  pinned OctoPrint in a virtual environment of its own (this repository's Python
  is newer than anything OctoPrint supports), the first-run wizard already
  answered so nothing waits on a browser, API authentication left **enabled**,
  and the provisioned key written to a path the script names on its own output.
  Turning authentication off would prove a configuration nobody runs.
- **Started, not raced.** `up` answers only once the instance answers its own
  API *and* reports a connected printer, and `--port auto` takes a free port so
  two runs on one host do not collide. In `--mode virtual` it also uploads
  `tools/octoprint-env/gcode/hold.gcode`, selects it and starts it, and states
  in `HOLD_SECONDS` the minimum that print keeps running for — which is what
  gives the tier something to act on. It does **not** start a print in
  `--mode serial` unless asked: a real printer moves.
- **Diagnosed, not timed out.** Every way starting can fail is one of the
  script's `FAILURE_CLASSES`, reported by name with a next action; anything
  outside that closed set is reported with the underlying error's own text. One
  diagnosed failure path and a bare timeout everywhere else is the shape that
  reads as diagnostics without being any.

`just octoprint-up` and `just octoprint-down` bracket `just test-integration`,
which is deliberately **not** one of `just check`'s tiers: it installs
OctoPrint, starts it and waits a print out, so it is a continuous-integration
job of its own rather than something every gate run pays for. The `integration`
job runs it on every change, on every platform the supported-platform list above
names except those excluded below. `repo-policy.toml`'s `[integration]` names
the three recipes, and `just check-repo` refuses a job that runs a recipe the
set does not declare, omits the bring-up or bring-down recipe, omits a platform
with no exclusion recorded, or is narrowed below every change.

### Virtual printer availability

Every platform the supported-platform list names for which OctoPrint's virtual
printer is unavailable, with the reason it is. A platform excluded here is one
the integration job does not run on; `just check-repo` refuses an exclusion with
no reason, and refuses a job that skips a platform no exclusion names.

[//]: # (BEGIN virtual-printer-exclusions)
No platform is excluded. OctoPrint's virtual printer is a bundled pure-Python
plugin that needs no hardware, so it is available on every platform the list
above names, and the integration job runs on all of them.
[//]: # (END virtual-printer-exclusions)

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

The status contexts required to be green before a pull request can merge. These
are **check-run names, not job keys**: a branch-protection rule names a check by
the name GitHub reports it under, and a matrixed job reports one check run per
cell. That is why the gate appears twice — its `name` carries the cell's platform
so the two are distinguishable — and why the judged tier appears once, with no
platform in its name at all.

[//]: # (BEGIN required-checks)
- `gate (linux-x86_64)`
- `gate (linux-aarch64)`
- `llmlint`
- `pr-title`
[//]: # (END required-checks)

`just check-repo` derives those contexts from the committed workflows and refuses
a record that disagrees with them. Applying the settings themselves to the
repository is a person's action through GitHub.

**Which subjects release.** `repo-policy.toml`'s `commits.release_types` is the
source: **`feat`, `fix` and `perf`** cut a release (and `!` / `BREAKING CHANGE`
raises the bump); `docs`, `test`, `chore`, `ci`, `refactor`, `build`, `style`
and `revert` are valid subjects that release nothing. `release-plz.toml`'s
`release_commits` regex and the committed `commit-msg` hook are held to that
same list by `just check-repo`. Pre-1.0 Cargo rules apply: `feat`/`fix`/`perf`
bump the patch, `!`/`BREAKING CHANGE` bumps the minor.

That rule is over subjects a *person* writes. Publishing a branch merges the base
into it first, and git writes that merge commit's subject itself — `Merge
remote-tracking branch 'origin/main' into <branch>`. The hook admits that commit
by its **state** rather than by its wording: `MERGE_HEAD` is in the git directory
exactly while git is completing a merge, and absent for an ordinary commit
whatever its subject says. Do not read the subject instead — `Merge branch 'main'`
typed by a person is textually identical to what git writes, so a rule matching
the wording hands anybody a bypass of the whole convention by typing one word. A
subject somebody types is held to the type list exactly as before, that wording
included. The exemption is the hook's alone: `just check-pr-title` has none,
because a title is always typed and no merge commit reaches `main` under
squash-merge. Without it no branch of this repository could be published once
`main` had moved under it, which is a failure that arrives after the work is
finished.

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

A defect that has to *outweigh* the tree is computed from what the copy measures
rather than written down. The coverage journey sizes its block of uncovered Rust
from the Rust its copy will carry, and it proves that sizing by running over a
copy grown by a substantial well-covered block as well as over the tree as it
stands. A fixed block stopped sinking the tree the moment a few well-covered
crates landed, and a journey that can no longer make the floor fail has stopped
checking that the floor is enforced at all.

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
