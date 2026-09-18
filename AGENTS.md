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
- `optional:pre-commit-framework` — excluded: what verifies a change here is the
  pull request's own required checks. A local framework that re-specifies those
  tools is a second, drifting copy of the gate, run before the one that decides.
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
lines, per ecosystem). There is no warnings-only mode, and one exemption from the
Rust floor: `windows-aarch64`, whose toolchain cannot read the profiles its own
instrumentation writes ([rust-lang/rust#150123](https://github.com/rust-lang/rust/issues/150123)).

`just lint` lints every crate twice on a Unix host: natively, and once more for
the Windows target `repo-policy.toml`'s `toolchain.windows_lint` names, so a
finding in `cfg(windows)` code — dead code to the native pass — is reported
before a push rather than by a Windows runner at the end of a two-hour round.
Clippy for another target still runs every dependency's build script, and two
of them compile C for it, so the pass hands cargo zig as the cross C compiler
through `repo_checks`'s own `zig-cc.sh` — obtained through `uv` from the `zig` dependency
group, off the default set so the Windows runners, whose native lint is that
pass, never fetch it. `just bootstrap` adds the target's standard library on
every host that is not Windows; where it is absent the pass fails naming the
`rustup target add`, because a lint that ran over none of the Windows code is
not one that passed.

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
whose JavaScript dependencies have never been installed, and every publication
of this repository is made from a clone cut fresh for it, so the gate heals that
state rather than needing a person to run `just bootstrap` in a directory
nothing hands them. Being locked,
the install can neither resolve nor record anything `bun.lock` does not already
describe, and it reinstalls nothing when the tree already matches. Only the
JavaScript side needs this, because `uv run` syncs its own environment on every
invocation. `just check-repo` refuses a recipe that reaches Nx without it.

git hands every hook it runs its own environment, in which `GIT_DIR` names the
repository being operated on and everything that hook starts inherits it.
`repo_checks.shell.run` drops the variables that name a repository, so a
subprocess works on the directory it was given; without that, the suites that
build repositories in temporary directories commit into the ambient one
instead. Both of these are proven by `tests/repo-e2e` driving a committed hook
over a copy carrying neither installed dependencies nor a clean environment —
where they fail, rather than argued about here.

## The agent's and the operator's surface

The `printobserver` command is the whole of both. There is no second surface: the
agent never touches `OctoPrint`, never sees G-code, and asks for nothing this
program does not have a command for — and the operator uses the same commands,
so nothing an agent can do is a path a person cannot audit.

**Nothing in the command-line crate lists what that surface is.** It is folded
out of what the server declares: one **client command** per operation
`printobserver-server`'s `OPERATIONS` serves, and each command's options out of
that operation's own `Operation::request` — whose body, for an action, is read at
run time from the contracts' own `PrintAction` schema. So a vocabulary that gains
a variant gains a command, a variant that gains a field gains an option, and
growing a declaration beside the parser grows nothing. Two commands sit beside
them, and they are the only ones that are not requests to an already-running
server: the **server command**, which runs the supervisor, and the **sign-in
command**, which signs that supervisor's harness in as the user the service runs
as.

<!-- llmlint: ignore[instruction_layer_localized] The task that added the sign-in command requires this section to describe it beside the server command, and this section is the root's one statement of the program's whole surface, which spans the adapter, server and command-line crates; no one crate's subtree owns it. suppressions.toml has the full reason. -->
The sign-in command exists because the service runs as a system user whose unit
hides every home. `printobserver-oneharness`'s `SIGN_INS` is the one table of the
harnesses it can sign in and of the variable selecting where each keeps its
sign-in, and `<state_dir>/harness/<identity>` is the one directory both the
sign-in and every supervision turn use; nothing else in the tree names those
variables. It reads `state_dir` and `supervisor.harness` alone, so it never needs
a reachable printer.

Four options are common to every command and there is no fifth:
`--json`, `--config <path>`, `--help` and `--version`. The configuration file's
path is one of them and the address and credential it carries are not, because
the closure that matters is the closure of the **action** surface: a path to a
file reaches no printer and cannot be composed into an action.

- **Where a server is and what authenticates to it are configuration.** No client
  command takes either as an argument or an option. They are read from a
  configuration file — which may be the server's own, since a client with no
  `[client]` table takes the address the server was told to listen on — and from
  `PRINTOBSERVER_SERVER` and `PRINTOBSERVER_CREDENTIAL`, which win. A credential
  has one accessor and no rendering that shows it.
- **Five exits, each a different thing to do next**: success, an unreachable
  server, a program nothing configured, an action the policy refused, and an
  image path that names no file on the host the command ran on. That last one
  prints the rest of the answer and a sentence saying why the path is not one it
  can open, rather than a file name that names nothing.
- **Images are a path, never bytes.** The server answers an absolute path on its
  **own** filesystem, and this program transports none.

`crates/printobserver/tests/journeys.rs` is the tier that holds all of it, and
`crates/printobserver/tests/integration.rs` runs the same cross-product against
the scripted `OctoPrint`.

## The agent-facing documentation

The skill is the supervising agent's whole initial context. Keep it short and
put reference material in the documents it links to. `[docs]` in
`repo-policy.toml` declares that surface and its enforced bounds.
References must remain reachable beside the materialized skill after install,
on a host with no checkout.

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

Each entry states four facts: the runner, the Rust target, the service manager
whose own command pair the install-path section below states, and whether the
end-user install path targets the platform at all. A platform's four other facts
— what the JavaScript registry selects a package by, what the program's own file
is called there, what the release asset is named, and how a wheel's platform tag
is spelled — are nobody's prose, and live in
`tools/repo-checks/src/repo_checks/platforms.py`, which hands a consumer all
eight at once. That module is the source every other copy of one of them derives
from.

[//]: # (BEGIN supported-platforms)
- `linux-x86_64` — runner `ubuntu-24.04`, Rust target `x86_64-unknown-linux-gnu`, service manager `systemd`, install path: yes
- `linux-aarch64` — runner `ubuntu-24.04-arm`, Rust target `aarch64-unknown-linux-gnu`, service manager `systemd`, install path: yes
<!-- llmlint: ignore[agents_md_durable_and_terse] the lever refuses `install path: no` without this reason, and the bring-up deletes it -->
- `macos-aarch64` — runner `macos-15`, Rust target `aarch64-apple-darwin`, service manager `launchd`, install path: no — bring-up owed; the macOS platform node flips this to `yes`
<!-- llmlint: ignore[agents_md_durable_and_terse] the lever refuses `install path: no` without this reason, and the bring-up deletes it -->
- `macos-x86_64` — runner `macos-15-intel`, Rust target `x86_64-apple-darwin`, service manager `launchd`, install path: no — bring-up owed; the macOS platform node flips this to `yes`
<!-- llmlint: ignore[agents_md_durable_and_terse] the lever refuses `install path: no` without this reason, and the bring-up deletes it -->
- `windows-x86_64` — runner `windows-2025`, Rust target `x86_64-pc-windows-msvc`, service manager `windows-service`, install path: no — the `windows-install-routes` node delivers the three install routes and flips this to `yes`
<!-- llmlint: ignore[agents_md_durable_and_terse] the lever refuses `install path: no` without this reason, and the bring-up deletes it -->
- `windows-aarch64` — runner `windows-11-arm`, Rust target `aarch64-pc-windows-msvc`, service manager `windows-service`, install path: no — the `windows-install-routes` node delivers the three install routes and flips this to `yes`
[//]: # (END supported-platforms)

The two Linux entries run under systemd, which is what the unit in the end-user
install path below is written for. `linux-aarch64` is not optional: the machine
beside the printer is usually a small ARM board, and it is the worst place to
discover an architecture was never built for.

Rust 1.97.1's bundled `llvm-profdata` cannot read the profiles its native
`aarch64-pc-windows-msvc` instrumentation writes: it reports both `malformed
instrumentation profile data: symbol name is empty` and `no profile can be
merged` ([rust-lang/rust#150123](https://github.com/rust-lang/rust/issues/150123)).
That gate still runs every Rust test, build, lint and end-to-end journey
natively; `repo-policy.toml` exempts only its unreadable Rust coverage report,
while the 95% floor remains required everywhere profiles are readable. No code
is compiled only for Windows aarch64. The exemption is removed when that
toolchain produces readable profiles.

<!-- llmlint: ignore[agents_md_durable_and_terse] States what the four entries above are and where their record is, which the list is unreadable without. suppressions.toml has the full reason. -->
The macOS and Windows entries are being brought up as first-class platforms, through the two levers below; `docs/platform-bring-up.md` records what their runners first said.

### The two levers a platform is brought up in stages by

Growing this list fans out instantly to every matrix derived from it, which on
its own would make adding a platform one impossible change. Two levers make it a
set of reviewable ones instead, and neither can be pulled quietly: both are
visible here and both are refused without a reason.

**`install path: yes|no`** is the first, and it is per platform. A platform
answered `no` is carried by no install-route, registry-proof or artifact-route
matrix; one answered `yes` is carried by every one of them. A `no` carries its
own reason — `install path: no — <reason>` — because a platform taken out of
every install tier by an unexplained opt-out is one nobody can put back.

**The block below** is the second, and it is per cell: one line per
platform-dependent job that does not run on a platform this list names, with the
reason it does not, in the shape `- \`<platform>\` on \`<job>\` — <reason>`.

[//]: # (BEGIN platform-exclusions)
<!-- llmlint: ignore[agents_md_durable_and_terse] the lever requires this cell's reason, and its bring-up deletes the line -->
- `macos-aarch64` on `gate` — bring-up owed; the macOS platform node removes this line
<!-- llmlint: ignore[agents_md_durable_and_terse] the lever requires this cell's reason, and its bring-up deletes the line -->
- `macos-aarch64` on `artifact-client-rust` — bring-up owed; the macOS platform node removes this line
<!-- llmlint: ignore[agents_md_durable_and_terse] the lever requires this cell's reason, and its bring-up deletes the line -->
- `macos-aarch64` on `artifact-client-python` — bring-up owed; the macOS platform node removes this line
<!-- llmlint: ignore[agents_md_durable_and_terse] the lever requires this cell's reason, and its bring-up deletes the line -->
- `macos-aarch64` on `artifact-client-node` — bring-up owed; the macOS platform node removes this line
<!-- llmlint: ignore[agents_md_durable_and_terse] the lever requires this cell's reason, and its bring-up deletes the line -->
- `macos-aarch64` on `artifacts` — bring-up owed; the macOS platform node removes this line
<!-- llmlint: ignore[agents_md_durable_and_terse] the lever requires this cell's reason, and its bring-up deletes the line -->
- `macos-aarch64` on `integration` — bring-up owed; the macOS platform node removes this line
<!-- llmlint: ignore[agents_md_durable_and_terse] the lever requires this cell's reason, and its bring-up deletes the line -->
- `macos-x86_64` on `gate` — bring-up owed; the macOS platform node removes this line
<!-- llmlint: ignore[agents_md_durable_and_terse] the lever requires this cell's reason, and its bring-up deletes the line -->
- `macos-x86_64` on `artifact-client-rust` — bring-up owed; the macOS platform node removes this line
<!-- llmlint: ignore[agents_md_durable_and_terse] the lever requires this cell's reason, and its bring-up deletes the line -->
- `macos-x86_64` on `artifact-client-python` — bring-up owed; the macOS platform node removes this line
<!-- llmlint: ignore[agents_md_durable_and_terse] the lever requires this cell's reason, and its bring-up deletes the line -->
- `macos-x86_64` on `artifact-client-node` — bring-up owed; the macOS platform node removes this line
<!-- llmlint: ignore[agents_md_durable_and_terse] the lever requires this cell's reason, and its bring-up deletes the line -->
- `macos-x86_64` on `artifacts` — bring-up owed; the macOS platform node removes this line
<!-- llmlint: ignore[agents_md_durable_and_terse] the lever requires this cell's reason, and its bring-up deletes the line -->
- `macos-x86_64` on `integration` — bring-up owed; the macOS platform node removes this line
<!-- llmlint: ignore[agents_md_durable_and_terse] the lever requires this cell's reason, and its bring-up deletes the line -->
- `windows-x86_64` on `install-route-pypi` — an end-user install route, owed; the `windows-install-routes` node removes this line
<!-- llmlint: ignore[agents_md_durable_and_terse] the lever requires this cell's reason, and its bring-up deletes the line -->
- `windows-x86_64` on `install-route-npm` — an end-user install route, owed; the `windows-install-routes` node removes this line
<!-- llmlint: ignore[agents_md_durable_and_terse] the lever requires this cell's reason, and its bring-up deletes the line -->
- `windows-x86_64` on `install-route-script` — an end-user install route, owed; the `windows-install-routes` node removes this line
<!-- llmlint: ignore[agents_md_durable_and_terse] the lever requires this cell's reason, and its bring-up deletes the line -->
- `windows-x86_64` on `prove-registry-pypi` — an end-user install route, owed; the `windows-install-routes` node removes this line
<!-- llmlint: ignore[agents_md_durable_and_terse] the lever requires this cell's reason, and its bring-up deletes the line -->
- `windows-x86_64` on `prove-registry-npm` — an end-user install route, owed; the `windows-install-routes` node removes this line
<!-- llmlint: ignore[agents_md_durable_and_terse] the lever requires this cell's reason, and its bring-up deletes the line -->
- `windows-x86_64` on `prove-registry-script` — an end-user install route, owed; the `windows-install-routes` node removes this line
<!-- llmlint: ignore[agents_md_durable_and_terse] the lever requires this cell's reason, and its bring-up deletes the line -->
- `windows-x86_64` on `artifact-route-pypi` — an end-user install route, owed; the `windows-install-routes` node removes this line
<!-- llmlint: ignore[agents_md_durable_and_terse] the lever requires this cell's reason, and its bring-up deletes the line -->
- `windows-x86_64` on `artifact-route-npm` — an end-user install route, owed; the `windows-install-routes` node removes this line
<!-- llmlint: ignore[agents_md_durable_and_terse] the lever requires this cell's reason, and its bring-up deletes the line -->
- `windows-x86_64` on `artifact-route-script` — an end-user install route, owed; the `windows-install-routes` node removes this line
<!-- llmlint: ignore[agents_md_durable_and_terse] the lever requires this cell's reason, and its bring-up deletes the line -->
- `windows-x86_64` on `artifacts` — the release artifacts the end-user install routes take, owed; the `windows-install-routes` node removes this line
<!-- llmlint: ignore[agents_md_durable_and_terse] the lever requires this cell's reason, and its bring-up deletes the line -->
- `windows-aarch64` on `install-route-pypi` — an end-user install route, owed; the `windows-install-routes` node removes this line
<!-- llmlint: ignore[agents_md_durable_and_terse] the lever requires this cell's reason, and its bring-up deletes the line -->
- `windows-aarch64` on `install-route-npm` — an end-user install route, owed; the `windows-install-routes` node removes this line
<!-- llmlint: ignore[agents_md_durable_and_terse] the lever requires this cell's reason, and its bring-up deletes the line -->
- `windows-aarch64` on `install-route-script` — an end-user install route, owed; the `windows-install-routes` node removes this line
<!-- llmlint: ignore[agents_md_durable_and_terse] the lever requires this cell's reason, and its bring-up deletes the line -->
- `windows-aarch64` on `prove-registry-pypi` — an end-user install route, owed; the `windows-install-routes` node removes this line
<!-- llmlint: ignore[agents_md_durable_and_terse] the lever requires this cell's reason, and its bring-up deletes the line -->
- `windows-aarch64` on `prove-registry-npm` — an end-user install route, owed; the `windows-install-routes` node removes this line
<!-- llmlint: ignore[agents_md_durable_and_terse] the lever requires this cell's reason, and its bring-up deletes the line -->
- `windows-aarch64` on `prove-registry-script` — an end-user install route, owed; the `windows-install-routes` node removes this line
<!-- llmlint: ignore[agents_md_durable_and_terse] the lever requires this cell's reason, and its bring-up deletes the line -->
- `windows-aarch64` on `artifact-route-pypi` — an end-user install route, owed; the `windows-install-routes` node removes this line
<!-- llmlint: ignore[agents_md_durable_and_terse] the lever requires this cell's reason, and its bring-up deletes the line -->
- `windows-aarch64` on `artifact-route-npm` — an end-user install route, owed; the `windows-install-routes` node removes this line
<!-- llmlint: ignore[agents_md_durable_and_terse] the lever requires this cell's reason, and its bring-up deletes the line -->
- `windows-aarch64` on `artifact-route-script` — an end-user install route, owed; the `windows-install-routes` node removes this line
<!-- llmlint: ignore[agents_md_durable_and_terse] the lever requires this cell's reason, and its bring-up deletes the line -->
- `windows-aarch64` on `artifacts` — the release artifacts the end-user install routes take, owed; the `windows-install-routes` node removes this line
[//]: # (END platform-exclusions)

An entry is a cell that **does not run yet**, not one nobody intends to run: it
names the bring-up owed and what removes it, and the block is meant to end empty.

The printer integration job is read against this block and against a narrowing
of its own — the "Virtual printer availability" block below, which records where
the virtual printer is unavailable. The two say different things: that block
records a platform the tier cannot run on, and this one a cell whose bring-up is
still owed, so either excuses a cell of that job and a matrix carrying a cell
either records is refused.

### The three jobs that carry no platform matrix

These run once per change rather than once per platform, and a matrix would say
nothing about any of them. Each carries the reason it has none — the record is
what makes a job running once a decision rather than an omission.

[//]: # (BEGIN unmatrixed-jobs)
- `llmlint` — the judged-lint tier reads one text diff and a non-deterministic judge rules on it, so a second cell is a second independent verdict on one change rather than a second platform: two required checks free to pass and fail the same content.
- `pr-title` — a pull-request title is one string, and linting it against Conventional Commits reads nothing at all of the host it runs on.
- `obico` — the scheduled Obico tier proves an EXTERNAL producer's webhook payload shape: it stands a self-hosted Obico up from that project's own Linux container composition, causes a real failure alert on it over HTTP, and compares the body that stack posts against the committed sample. It is not a printer-host tier, and the hosted macOS and Windows runners do not run Linux containers.
[//]: # (END unmatrixed-jobs)

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
job of its own. The gate also drives these recipes to prove process cleanup.
The `integration`
job runs it on every change, on every platform the supported-platform list above
names except those excluded below. `repo-policy.toml`'s `[integration]` names
the three recipes, and `just check-repo` refuses a job that runs a recipe the
set does not declare, omits the bring-up or bring-down recipe, omits a platform
with no exclusion recorded, or is narrowed below every change.

There is **one machine**, and every project's `test-integration` target drives
it: `just test-integration` runs the adapter's tier and this environment's own
suite against the one OctoPrint the bring-up started, and one of them cancels
the print the other asserts is running. So `nx.json` declares `test-integration`
unparallelisable, and a tier that acts on the print puts it back when it is
done, leaving the environment as the bring-up recipe left it. `just check-repo`
refuses a graph that leaves two of the tier's tasks free to run at once. It cost
a publication to learn: the tier that failed was the one that had done nothing
wrong, which is what a shared machine does to a diagnosis.

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

## The real-printer smoke test

Everything above this line is proven against a virtual printer, and a virtual
printer cannot tell you that a real Prusa refuses a flow adjustment mid-print or
that a restore lands on a machine that has already moved on.
`tools/printer-smoke/printer_smoke.py` is the one test that says the whole stack
works on the actual machine: it drives the installed `printobserver` command
against a running supervisor, and through it the printer on a named serial port.
It is the one place in this repository where a bug damages hardware, so it
prefers refusing to run over proceeding on a precondition it is unsure of.

**Nothing selects it, and two things together do.** The `--run` flag on its own
recipe, and `PRINTOBSERVER_SMOKE_DEVICE` naming the serial device. One alone
does not select it: absent either, the run says so and which was missing rather
than skipping silently or failing. The ordinary gate does not name it a tier,
`just check` does not invoke it, no graph target reaches it and no workflow runs
it — on a change or on a schedule — because a print is hours of filament and an
unattended test that starts one ruins a print nobody was watching.
`repo-policy.toml`'s `[smoke]` declares the recipe, the flag and the variable,
and `just check-repo`'s `smoke-selection` refuses a tree in which anything else
could reach it.

```console
PRINTOBSERVER_SMOKE_DEVICE=/dev/ttyACM0 just test-printer-smoke --run
```

**What a person does before running it**, on the machine beside the printer, and
in this order: bring the scripted `OctoPrint` up against the real device
(`OCTOPRINT_ENV_MODE=serial OCTOPRINT_ENV_DEVICE=/dev/ttyACM0 just
octoprint-up`); upload `tools/printer-smoke/gcode/smoke.gcode` to that instance
under its own name; put the conservative envelope below into the running
supervisor's configuration; and set a manifest for that file on the print the
smoke is to act on, with `printobserver manifest-set`. `PRINTOBSERVER_SMOKE_CONFIG`
names that configuration file — the server's own, `/etc/printobserver/config.toml`,
by default — and `PRINTOBSERVER_SMOKE_PRINT_ID` names the print, because a print
record is minted by the supervisor rather than by a caller.

**Every precondition fails closed, and refusing is a pass.** Each is checked
before anything is asked of the machine, an unmet one stops the run naming it,
and the run exits zero having sent the printer no command at all — because a
smoke test that could not satisfy itself that it was safe to run is not a defect
to investigate. They are, in the order they are checked: the `printobserver`
command is on this host; the named serial device is there and readable; the
scripted `OctoPrint` is connected to *that* device in real-serial mode rather
than to a virtual printer; the supervisor this run will drive is the one
attached to that instance; the print carries a manifest for the smoke's own
file; the printer reports itself operational; no job is running and none is
paused; and the safety envelope in the configuration is the conservative one
this test ships. `tools/printer-smoke/tests/test_preconditions.py` walks all
eight, making each unmet in turn, and fails when the smoke declares one that
walk does not cover.

**The machine checked and the machine driven are one machine.** That is the
fourth precondition above and it is the one an opt-in naming a device cannot
give you by itself: a valid instance record for the printer on `/dev/ttyACM0`
and a `PRINTOBSERVER_SERVER` naming a second supervisor satisfy every other
precondition independently, and the actions then land on a machine nothing here
verified. So two links are required and the run is refused unless both hold.
The configuration this smoke reads is the **supervisor's own**, so the
`OctoPrint` it names under `octoprint.url` has to be the instance whose serial
connection was just checked; and every place that names where a supervisor is —
that file's `listen`, its `[client]` table, and `PRINTOBSERVER_SERVER`, which
wins over both — has to name one address. Two of them naming different
supervisors is refused before anything is driven rather than resolved in favour
of whichever the client would have used.

**The conservative envelope is bounded by this document rather than by the
test.** A test that shipped a permissive envelope and then faithfully required
it would satisfy its own precondition while missing the point of having one, so
no range `CONSERVATIVE_ENVELOPE` declares may be wider than: feedrate factor 0.5
to 1.2, flowrate factor 0.9 to 1.1, any tool target 0 to 230 °C, bed target 0 to
70 °C, fan 0 to 100 percent. Those bounds accommodate PLA comfortably and
exclude by construction every material needing temperatures nobody has reviewed,
so a smoke run cannot be pointed at a filament this ceiling was not written for.
`tools/printer-smoke/tests/test_envelope.py` holds the shipped envelope to
exactly that ceiling.

**What it verifies**, in this order, reading the printer's own answer back at
every point and failing the run naming the point where that answer was not what
was required: the context read reports the printer, the job and effective bounds
no wider than the configuration allows; the smoke print starts; each adjustable
is set just inside its bound and read back from the machine; each is then asked
for just outside its bound, and the rejection is confirmed to have changed
nothing on the machine; a bounded intervention with a short duration puts the
prior value back at expiry; a pause and a resume are each taken; the print is
cancelled; and the history afterwards accounts for every action, decision and
outcome the run produced.

**It cleans up on every exit path it has** — a completed run as much as a failed
or an interrupted one, since a run that finishes without restoring what it
changed leaves the machine altered exactly as a crashed one does. The restore
comes *before* the cancel, and that ordering is load-bearing: an adjustment is
valid from a printing or a paused machine and from no other state, so a run that
cancelled first could never put back what it changed. One adjustable that cannot
be put back does not cost the ones after it: every one is attempted, and what
could not be restored is collected rather than raised at the first.

**And a run that could not put everything back says so and exits non-zero.**
What the cleanup managed is not taken on trust: afterwards the machine is read
once more, and every value still carrying this run's own — and a printer not
left operational — is printed as `LEFT CHANGED` and makes the run fail, whether
or not any verification point did. A green report over a machine still holding a
modified feedrate is the worst answer this program can give, and it is worse
than the failure it would be hiding. Where a verification point *did* fail, that
failure is what is reported first and the cleanup is reported beneath it: a
cleanup that could not finish never replaces the cause a reader needs.

**A command that never answers is that command's failure and nothing more.** A
run that hung, or one whose program could not be started at all, comes back as
an exit no answer carries rather than as an exception out of the middle of the
cleanup — because letting one out there would abandon the restorations after it
and the cancellation with them, on a machine this run has already moved. So the
bound one command is given (`PRINTOBSERVER_SMOKE_COMMAND_TIMEOUT_S`, two minutes
by default) is enforced where the command is run, a restoration that never
answers costs that adjustable and no other, and a cancellation is still asked
for over a machine whose state could not be read — a print that is not running
refuses it and nothing moves, while one that is running is this run's own and
must not be left behind. And where the last look at the machine is the thing
that did not answer, that is `UNVERIFIED` and it fails the run too: a machine
nothing could see is not one this test may report green on.

**What to watch while it runs.** Stay next to the machine — this is not a test
to start and walk away from. Watch the first layer go down after the print
starts; watch the nozzle and bed temperatures as the two heater adjustments are
made and again when they are put back; and keep a hand near the printer's own
power switch while the pause, the resume and the cancel are driven, because
those are the three moments the machine changes what it is doing on somebody
else's instruction. The run says what it is verifying before each step, so what
is about to happen is on the screen before it happens.

### The payload it prints

`tools/printer-smoke/gcode/smoke.gcode` is this repository's own: a 20 mm square
with two perimeters over five layers. Two properties of it are mechanical rather
than promised, and `just check-repo`'s `smoke-payload` is what makes them so.

It contains only commands drawn from a closed set — `G21`, `G90`, `G91`, `G92`,
`G28`, `G0`, `G1`, `M82`, `M83`, `M104`, `M109`, `M140`, `M190`, `M106`, `M107`
and `M84`: units, positioning and extrusion mode, homing, motion, the four heater
commands, the two fan commands, and disabling the steppers. Everything else is
outside it by construction, which is how the firmware writes (`M500`, `M502`),
the calibration routines (`M303`, `G29`) and the emergency stop (`M112`) stay
out — each is excluded because it is not in the set rather than because it is on
a list of its own. And it is no longer than **200 lines**: short enough that a
person reads the whole payload before it reaches a machine, and long enough for
an object with real perimeters rather than a purge line and two moves.
`repo-policy.toml`'s `smoke.safe_commands` and `smoke.max_lines` are the
machine-readable copy of both, and `tools/repo-checks/tests/test_smoke_payload.py`
holds the check to them.

The check refuses a payload carrying anything outside that set, one exceeding
that length, one carrying no command that moves the machine, and one that
neither is heat-free nor declares at its head the temperatures it needs and for
how long — and where it does declare them, one naming a temperature no command
in the file sets, or carrying a heating command the declaration does not account
for. The motion rule is there because an inert payload is otherwise
indistinguishable from a correct one: an empty file satisfies every safety
condition by containing nothing.

## The scheduled Obico tier

The vision port reads Obico's webhook notifications, and the fast tier replays a
recorded one. A recording is only as good as the last time somebody held it
against the thing being recorded — and Obico is an external project on its own
release cadence, so the day its payload gains or renames a field, every replayed
test goes on passing and the running system stops seeing failures. This tier is
the only thing here that notices.

**What it proves.** `tools/obico-env/obico_env.py` brings up a self-hosted Obico
in containers from that project's own development composition, pinned to a
revision; `tools/obico-env/obico_tier.py` then walks one path and reports one
verdict:

1. it listens on the address the stack's webhook notification plugin was
   configured to post to,
2. it causes a real failure alert on that stack — the snapshot goes in through
   Obico's own printer API and the alert is raised through Obico's own
   `alert_if_needed`, the function its detection pipeline calls the moment a
   frame scores as a failure, so everything downstream of the score is Obico's:
   its models, its queue, its worker and its own webhook plugin,
3. it captures the body that stack posts,
4. it fetches the image *that captured body* points at and reads its bytes,
5. it compares *that captured body* against the sample committed at
   `crates/printobserver-obico/samples/obico/failure-alert.json`,
6. and it reports a verdict naming every field that moved.

Nothing in it compares a body the capture did not produce — the alteration
tests in `tools/obico-env/tests/test_reconciliation.py` run each alteration
through the capture rather than past the comparator, which is what makes them
proof of that. What is compared is the *shape*: the set of fields and the JSON
type of each, because the ids, the file name and the instants differ on every
run by design and are not what the sample claims. A field the producer added,
renamed, removed or retyped moves the shape and fails the tier naming it.

A divergence found here is a **finding to report** rather than a defect of this
repository: the sample is the `contracts` node's file, and moving it is a
deliberate change to a checked-in contract.

One thing about step 2 a reader will otherwise meet as a mystery: Obico alerts on
a print **once** and suppresses every alert after it, which is right — a printer
that alerted on the same failed print every ten seconds would be unusable. So the
trigger finishes an already-alerted print and starts a fresh one, which is what
happens between two real failures anyway. A stack that has run this tier several
times therefore carries several finished prints, and a tier that skipped this
would capture nothing on its second run and blame the network.

**Why it is not in every run.** The tier builds Obico's images from Obico's own
sources — one of them carries a machine-learning model — starts four containers,
and then waits a real failure alert out. That is tens of minutes on a cold
runner. `repo-policy.toml`'s `gate.tiers` does not name it, `just check` does not
invoke it, and `just check-repo` refuses a tree in which either changes. Leaving
it out silently is what this repository forbids; running it on a schedule and
saying so here is what it asks for instead.

**The schedule it runs on.** `.github/workflows/obico.yml` declares this and a
manual `workflow_dispatch`, and no trigger that fires on a change at all. The
cron below and the workflow's own are checked against each other, so this
paragraph cannot drift from what actually fires.

[//]: # (BEGIN obico-tier-schedule)
- cron: `17 4 * * 1`
[//]: # (END obico-tier-schedule)

**How to run it by hand.** Three recipes, in order. Docker and its Compose plugin
are the one prerequisite `just bootstrap` does not install, because this is the
only thing here that needs them; a host without them is told so by name with the
next action rather than by a failure to start. `just obico-up` says what it is
about to start — naming each of the four services — and roughly how long that
takes, before it starts anything.

```console
just obico-up
just test-obico
just obico-down
```

`just obico-down` stops every container the bring-up created and is worth running
even after a failure: the bring-up stops what it started when it fails, but a
tier interrupted between the two leaves a stack up. It also hands the state
directory back to the user who ran it before stopping anything — Obico's own
composition bind-mounts its sources into containers that run as root, so without
that a developer needs `sudo` to delete `.obico-env` after their own bring-down.

## The Obico ingress answer bound

The vision port's ingress is an HTTP endpoint `Obico`'s webhook notification
plugin posts to, and that plugin posts **best-effort**: it calls `requests.post`
with a short timeout, raises on the answer, and **does not retry**. An alert it
gives up on is an alert this system never sees. So the server answers that
endpoint **before** its handling completes, and inside a bound this repository
declares in its own configuration with a stated default.

The timeout that bound has to stay below is Obico's rather than this
repository's, so it is written down here as a claim about an external producer,
with the release it was read from — and the scheduled tier above is what
reconciles that claim against a live Obico.

[//]: # (BEGIN obico-posting-timeout)
- posting timeout: `5000` ms
- source: `backend/notifications/plugins/webhook/__init__.py`, whose `execute_webhook` takes `timeout: float = 5.0`
- release: Obico at revision `49c0bc7001a3fd8d56297fc3032ba287bfe1d50b`, the one `tools/obico-env/obico_env.py` stands up
- retries: none. The plugin calls `raise_for_status` and the exception is logged; nothing posts that body a second time.
[//]: # (END obico-posting-timeout)

The default this repository ships is **1000 ms**, a fifth of that, and
`crates/printobserver-server/src/config.rs`'s `DEFAULT_INGRESS_ANSWER_BOUND_MS`
is where it is written. The margin is the whole point of the number rather than
caution about it: the supported host is a small ARM board beside the printer, and
a bound set just under the producer's is one the first slow moment on that board
exceeds — after which the alert is gone, because nothing posts it again. A
configured bound at or above the recorded timeout is refused where it is
configured, naming the field, and `just check-repo`'s `ingress-answer-bound`
holds the shipped default and the server's own copy of that timeout to the block
above.

## The end-user install path

This section is the authoritative source of the end-user install path. Every
other statement of it in this repository — the README, the continuous-
integration jobs that exercise it — is derived from here, and `just check-repo`
refuses any that differs.

The path is **one program obtained by any one of three alternative routes, and
then two commands in order**, with the agent's harness installed and signed in
as the service's own user between those two commands.

The three routes are three ways of *obtaining* the program, so a reader takes
one of them whatever platform they are on rather than one per platform. What is
per platform is the pair of commands after them: each service manager the
supported-platform list names states its own installer command and its own start
command, in that order, and a platform's pair is the one its own service-manager
column names. Route 3 goes one level further down, because a shell script is not
a route a Windows machine can take: it states one fetch command per install
script behind it, and every platform this list answers `install path: yes` for is
reached by exactly one of those scripts.

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

### Then, check what you installed

Whichever route you took, run the program. It prints the version it is, and
that is the one thing the three commands above cannot tell you: a package
manager that unpacked a program nobody can run exits zero, and so does an
install script that put a file on your path. This is where an install that
worked stops looking like one that did not.

```console
printobserver --version
```

### Then, in order — two commands

Both run as root, and which pair you run is your platform's own: the
supported-platform list's service-manager column names it. The first of a pair
puts the service in place and the second starts it, and one subsection below
states one pair, headed by the service manager it belongs to. Every service
manager that list names a platform the install path targets under has a pair
here, and no manager it does not name has one. A manager whose platforms all
answer `install path: no` may have one too: a pair is owed once the routes reach
a platform of that manager and permitted before they do, because a service is
proven on its platform before that platform's routes are, and the pair is the
same either way.

**Enabling and starting is a command of its own rather than something the
installer does, and the reason is that this service commands a 3D printer.**
Installing a package must not, as a side effect, start a process that can move a
machine, so starting the supervisor stays a decision somebody takes rather than
something that happens while they are installing. Do not fold these two steps
back together, in any pair.

`just check-repo`'s `service-install` reads this section beside the tree and
refuses one in which the service's name or the installer's path differs from
what is written below, or in which that installer enables or starts anything.
What it reads for a platform is that platform's own service-manager column, so
each service manager here is read against its own pair and its own rules —
`repo-policy.toml`'s `service.managers.<manager>` — rather than against
systemd's.

What makes the three routes executable is the `sdks` node, and what makes the two
commands executable is the `server` node — each held to this section.

#### systemd

The first is the installer the `server` node ships, committed at
`scripts/install-service.sh`: it puts the binary, the state directory, the
configuration and the systemd unit in place. The second enables and starts the
unit, whose name is `printobserver.service`. Made executable by the `server`
node.

```console
curl -fsSL https://raw.githubusercontent.com/nickderobertis/printobserver/main/scripts/install-service.sh | sudo sh
```

```console
sudo systemctl enable --now printobserver.service
```

#### windows-service

Both run in an elevated PowerShell — Windows PowerShell or a newer one. The first
is the Windows form of the same installer, committed at
`scripts/install-service.ps1`: it puts the program under `Program Files`, the
state directory and the configuration under `ProgramData`, and the service's
registration with the service control manager in place — registered to start
**on demand**, as its own virtual account `NT SERVICE\printobserver`, and to be
brought back by the manager five seconds after its process ends abruptly. The
second sets the service, whose name is `printobserver`, to start automatically
and starts it; it is the one command that enables anything, because the service
control manager has no way to declare an automatic start without taking it, and a
registration the installer wrote as automatic would start the supervisor at the
next reboot whether or not anybody ran this command. Made executable by the
`windows-service` node.

```powershell
irm https://raw.githubusercontent.com/nickderobertis/printobserver/main/scripts/install-service.ps1 | iex
```

```powershell
Set-Service -Name printobserver -StartupType Automatic -Status Running
```

### Between the two commands, sign in the agent's harness

On every event the service runs the harness `supervisor.harness` names, as its
own user, whose unit hides every home. So after the installer and before the
command that enables the service, install that harness's program where the
service user's path finds it — as root, one of these two — and sign it in once
as that user with `printobserver sign-in`, which keeps the sign-in under the
state directory, where every supervision turn reads it. It reads `state_dir` and
`supervisor.harness` alone, starts nothing and reaches no printer. Made
executable by the `printobserver` command's own `sign-in`.

```console
sudo npm install -g @anthropic-ai/claude-code
```

```console
sudo npm install -g @openai/codex
```

```console
sudo -u printobserver /usr/local/lib/printobserver/printobserver sign-in
```

On Windows the service's virtual account cannot be signed in to, and does not
need to be: the sign-in is kept under the state directory, which the installer
made the service account's to read, so it is taken from the same elevated
PowerShell the two commands run in — the harness installed for every account
with `npm install -g`, as above and without `sudo`, and then:

```powershell
& 'C:\Program Files\printobserver\printobserver.exe' sign-in
```

## The registry install-path proof

The one thing that says this repository is installable: each of the three
routes above taken from its own registry, and what it installed run.

```console
just test-install-proof
```

`PRINTOBSERVER_PROOF_VERSION` names the version under test — never the number in
this tree, because what a user gets is whatever the registry is serving.

**It is not one of `just check`'s tiers** and `just check` does not invoke it: it
reads the real registries, so over a change it could only report what was
published before that change. It runs after a release — for the version that
release cut, read off the tag at the run's commit, and after a dispatched
publish too, for the version that dispatch published, read off the record the
dispatched run uploads — on a manual invocation, and on this schedule:

[//]: # (BEGIN install-proof-schedule)
- cron: `0 6 * * 1`
[//]: # (END install-proof-schedule)

## Commits, releases, and merging

**Merge model.** A change reaches `main` through a pull request, squash-merged,
with auto-merge on and head branches deleted. **`main` takes no direct push.**
The pull request title becomes the squash subject, which is the commit release
automation reads, so it is linted against Conventional Commits as a required
check.

The status contexts required to be green before a pull request can merge. These
are **check-run names, not job keys**: a branch-protection rule names a check by
the name GitHub reports it under, and a matrixed job reports one check run per
cell. That is why the gate and the printer integration job each appear once per
platform, and every cell of a required job is required, because a job required on
one platform and not the other is a merge path the other never blocked — and why
the judged tier appears once, with no platform in its name at all.

The two matrixed jobs are spelled differently below, and the difference is
GitHub's rather than ours. The gate's own `name` interpolates the cell's
platform, so GitHub takes that name verbatim and the context is `gate
(<platform>)`. The integration job's `name` interpolates nothing, so GitHub
qualifies it instead, appending the cell's whole matrix entry — which puts the
runner in the context beside the platform. Neither is preferred; what matters is
that the record spells each the way the job is actually reported, because a
context named here that nothing reports blocks every pull request forever. Do not
"tidy" the integration contexts by qualifying that job's name: that is a rename,
and it strands the two contexts branch protection already requires.

[//]: # (BEGIN required-checks)
- `gate (linux-x86_64)`
- `gate (linux-aarch64)`
- `gate (windows-x86_64)`
- `gate (windows-aarch64)`
- `integration (linux-x86_64, ubuntu-24.04)`
- `integration (linux-aarch64, ubuntu-24.04-arm)`
- `integration (windows-x86_64, windows-2025)`
- `integration (windows-aarch64, windows-11-arm)`
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
push to `main`. `release-plz release-pr` opens the release pull request under
`RELEASE_PLZ_TOKEN` — the workflow's built-in token is deliberately not used,
because a pull request opened by it does not trigger the workflows that gate
it — and `release-plz release` tags, cuts the GitHub Release and publishes
every crate under `CARGO_REGISTRY_TOKEN`. Nobody hand-edits a version,
hand-tags, or runs a publish with their own credentials. The same workflow
dispatched by hand on `main` for an existing release tag is not that hand-run:
it builds the tag's own tree's artifacts and publishes them with the publisher
at the dispatched ref, through the same jobs, the same publisher and the same
secrets read in CI, with an operator's credentials nowhere — and it runs the
release program not at all, so a dispatch can finish a release and never cut
one.

**Publishing never waits on drafting, and the artifacts follow only a cut
release.** Drafting the next release's pull request compares each package
against the registry and can die doing it; that must not stop a publication
that is ready. And the publishing step exits zero having released nothing —
every ordinary push — while the Python and JavaScript registries refuse a
version they already serve, so the artifact build and publish are gated on what
that step *answered* it released, never on its exit status. `just check-repo`'s
`release-gating` reads the workflow for both, and
`tests/repo-e2e/tests/test_release_workflow_journey.py` runs the committed
workflow under the forge's scheduling rules to prove them.

**A publish that stopped partway has two recoveries, and neither is a
hand-run.** A failed `publish` job of a release published under the resumable
publisher is re-run — GitHub's own re-run, with nothing cleaned up and nothing
moved first. A release cut before that publisher existed, or one whose re-run
cannot reach the fixed publisher because the run checked out a commit that
predates it, is finished by dispatching the workflow for its tag, which builds
the tag's own tree and publishes with the publisher at `main`. Both are safe
because `just publish-artifacts` decides per artifact rather than per run — one
its registry already serves is skipped and reported, every artifact is
attempted whatever an earlier one answered, and the job fails at the end naming
each refusal in the registry's own words — and because a dispatch is held, at
`release`, to a run on `main` and a tag that exists whose tree is the release
it names. **The
forge upload is exercised only against the stand-in** in
`tools/release-artifacts/src/release_artifacts/standin.py`, because nothing in
this repository may write to the real forge; the install-path workflow that
runs after a release publishes is where it is first proven against GitHub.
`just check-repo`'s `release-gating` and `release-dispatch` hold the workflows
to both shapes.

`release-targets.toml` declares what this repository publishes.
`repo-policy.toml`'s `manifests.automation_owned` is the only set of manifests
permitted to carry a version field, and `just check-repo` enforces it: a version
anywhere else is one a person would have to hand-maintain.

<!-- llmlint: ignore[agents_md_durable_and_terse] Required content rather than history: the release-path repair had to record, beside the rule it departs from, why the workspace version was moved by hand, that it is one closed exception, and that every version after it is release automation's; the durable instruction — do not repeat this, fix the state instead — is unreadable without the exception it refuses to repeat. suppressions.toml has the full reason. -->
**The rule above has one recorded exception, and it is closed.** `0.1.0` →
`0.2.0` was written by hand on 2026-09-10, because `release-plz release-pr`
cannot draft while a tag names a version some publishable crate never published
— the state the first run left, with two scaffold crates at `0.1.0` and eleven
absent. Every version after it is release automation's. Do not repeat this to
unstick a run: `tests/repo-e2e/tests/test_release_path_journey.py` drives the
drafting tool against a stand-in registry and refuses a tree at a tagged
version, which is the state to fix instead.

**Where a release's tag and GitHub Release come from.** One of each per
workspace version, both named `v<version>`, and both created by the
`printobserver` package after its own `cargo publish` — which release ordering
places after every other publishable crate's, because that crate depends on all
of them. One package rather than twelve because release-plz tags and releases
*per package*, in release order, and under the one name every package renders
the second to publish dies creating a ref the first already created, leaving the
rest unpublished. So `release-plz.toml` turns creation off under `[workspace]`
and on for `printobserver` alone, keeping the name templates workspace-wide so
that every entry of the program's answer still carries `v<version>` for `just
release-answer` to fold into one. A `v<version>` tag left standing on the forge
over unpublished crates is the quieter failure: release-plz skips every package
whose tag exists *before* it asks the registry, so the next push releases
nothing, exits 0, answers `released=`, and skips the artifacts. The repair is
to delete the tag and the GitHub Release, never to move the version.
`tests/repo-e2e/tests/test_release_tag_journey.py` holds the configuration to
this.

**Secrets.** `gh-secrets.json` is the authoritative list of the Actions secrets
this repository holds. A workflow may reference no secret outside it, spelled as
that manifest spells it, and `just check-repo` enforces that too.

## The dependency rule

The rule is an edge table, not a set of layers. `repo-policy.toml`'s
`crates.may_depend_on` is the source (`crates.may_depend_on_in_tests` for the
edges a crate's tests alone may add), `just check-repo` holds every manifest to
it, and `docs/reference/architecture.md` says why — why the crates are cut by
domain, and why core names no implementation crate. The step that draws an edge
is the step that admits it in the table. Do not bring `printobserver-store-api`
back, yank it or republish it: it is no crate of this workspace and stays on
crates.io at `0.2.0`.

The same rule holds one level down, over vocabulary rather than over edges:
**`printobserver-octoprint` is the only crate that may construct an `OctoPrint`
request.** Everything above it is written as though printers were normal, so the
moment a second crate spells an OctoPrint path or its authentication header
there are two places one vendor's own surface has to be kept right.
`repo-policy.toml`'s `[octoprint]` names the permitted crate and what
constructing such a request looks like in a Rust source; `just check-repo`
refuses one of those markers on a line of any other crate, exempting a
comment-only line so a crate may *say* `/api/job` while no crate but the adapter
may *build* one — and refuses a tree in which the adapter itself constructs
none, because a rule guarding a boundary nothing is on has stopped being a rule.

## Tests are the only QA loop

Never mock the layer under test. Drive the real artifact across real boundaries
the way a user does: the compiled binary as a subprocess, the real recipes, the
real checks. "Done" means every user-facing journey, happy path **and**
failure/recovery — not one smoke test. Coverage is a floor, not the target.

The end-to-end tier lives in `tests/repo-e2e`. It drives bootstrap in a fresh
copy carrying no build products, dependency installation, hooks, repository
checks, release tooling, the opt-in smoke interface and the real OctoPrint
recipes. A gate that stops gating is not a silent failure: the next defective
change exposes it, so we do not run whole gates over defect copies to test that
the gate gates. Structural wiring has one deterministic proof instead:
`repo_checks.checks_repo.recipe_set`, reached by `just check-repo`, requires
`check` to invoke every tier in `repo-policy.toml` and refuses empty tiers.

<!-- llmlint: ignore[instruction_layer_localized] This is a repository-wide gate and merge-protection constraint spanning the e2e project and CI workflows; neither project subtree alone owns where the required gate runs. -->
**Keep the OctoPrint journey in the required gate.**
`test_octoprint_tier.py` asserts that bring-down leaves no process from bring-up.
The separate `integration` job runs the same three recipes but has no cleanup
assertion, so it does not catch that silent leak. These jobs run in parallel;
relocation saves only their critical-path difference. Keep the journey in the
required gate unless `integration` first becomes required through a coordinated
repository-setting and merge-path inventory change.

### The one journey, in three clients

The nine steps all three clients drive are declared once, in
`repo_checks.checks_journeys`, and each journey marks its own in its own
language's comments — `journey step 4: start`. `just check-repo`'s
`journey-completeness` holds every marker to the code beside it, so a step
deleted, renamed, or left with nothing under its marker is refused in whichever
client lost it.

**Only the orderings that are load-bearing are asked for**, and `ORDERINGS` is
the whole of them: the manifest is written before the print is started so the
print runs under it, both adjustments come after it has started and before
history is read so history has them to account for, and the print is cancelled
last. Everything else is the journey author's to arrange — which of the two
reads comes first, and where the call the client refuses to make is written,
prove the same thing in any order, and a check demanding one arrangement would
be a check over layout rather than over the walk.

**A call the client refuses to make is proven by a recording proxy in front of
the real supervisor**, and by nothing else. The seventh step is that a mutating
call with an empty reason reaches no server at all, and neither unchanged
history nor a client pointed at an address nothing listens on says that: a
request the server took and recorded nowhere leaves history unchanged, and a
client that can reach nothing reaches nothing whatever it is asked. What the
step asserts is that the proxy — which the same call *did* go through, one read
earlier — saw nothing.

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
