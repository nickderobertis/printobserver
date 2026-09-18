# What the macOS and Windows runners said

A working note, not reference documentation. It records what this repository's
own deterministic tiers did the first time they ran on the four platforms
`AGENTS.md`'s supported-platform list gained with every cell of theirs excluded:
`macos-aarch64`, Intel macOS (since cut as a platform, and named on that list no
longer), `windows-x86_64` and `windows-aarch64`. The
platform nodes that bring those platforms up are written against this record
instead of against a guess, and the documentation node of that plan deletes this
file together with the probe once the `platform-exclusions` block is empty.

## The probe

`.github/workflows/platform-probe.yml` is **temporary scaffolding**. It runs on
a manual dispatch alone — it ran on every pull request until the macOS bring-up
took its runner's cells into the gate, after which a probe of that runner on
every change was a second macOS job per push saying nothing the gate did not —
and it cannot fail its run: one job per runner still being brought up, each
naming its runner directly and carrying no platform matrix,
each setting the toolchain up, running `just bootstrap`, then running each of the
gate's deterministic tiers as a step of its own with every step continuing on
error. Its last step writes every step's `outcome` into the run's summary. It
runs no `just check`, no OctoPrint bring-up, no `prove-` recipe and no judged
tier, so the printer integration tier is outside what it says. **The
documentation node of the platform bring-up plan deletes it**, together with
this note.

Where `extractions/setup-just` has no prebuilt `just` for the runner, the probe
builds it with `cargo install just --locked` so that the tiers after it say
something about the tree; the setup action's own failure stays in the summary.

## The runs this was read from

Every entry below cites the run and the job its outcome and its quotation were
read from. The outcome is the one the job's summary states; the quotation is
from that same step's own log in that job.

- **Run 2** —
  [35087431232](https://github.com/nickderobertis/printobserver/actions/runs/35087431232),
  pull request #55 at `91d41f3`, the tree carrying the four list entries and
  their exclusions. This is the run every entry below is read from.
  - `probe-macos-aarch64` — job
    [104765467330](https://github.com/nickderobertis/printobserver/actions/runs/35087431232/job/104765467330)
  - the Intel macOS probe job, which the probe no longer carries — job
    [104765467622](https://github.com/nickderobertis/printobserver/actions/runs/35087431232/job/104765467622)
  - `probe-windows-x86_64` — job
    [104765467588](https://github.com/nickderobertis/printobserver/actions/runs/35087431232/job/104765467588)
  - `probe-windows-aarch64` — job
    [104765467478](https://github.com/nickderobertis/printobserver/actions/runs/35087431232/job/104765467478)
- **Run 1** —
  [35077754091](https://github.com/nickderobertis/printobserver/actions/runs/35077754091),
  at `d55c331`: the probe alone, over a list that did not yet name the four
  platforms, and before the probe built `just` from source. It is cited only
  where it says something run 2 does not.

## Across all four

Read these first; most per-tier entries below are one of them.

- **No cell came up whole.** On both macOS cells the format,
  lint, typecheck, build, workflow-lint and repository-check tiers came up and
  the test, coverage and end-to-end tiers did not. On both Windows cells nothing
  after the toolchain setup came up.
- **Windows: `PYTHONPATH` is joined with `:`.** The justfile's
  `export PYTHONPATH := "tools/repo-checks/src:tools/contract-codegen/src:..."`
  is one unreadable entry to a Windows interpreter, which separates entries with
  `;`, so every recipe reaching `repo_checks` fails before doing anything —
  bootstrap, coverage and check-repo among them.
- **Windows: the checked-out files carry carriage returns.** The formatter's
  diff ends every line in `␍`, `shellcheck` reports `SC1017 (error): Literal
  carriage return`, and `rustfmt` refuses generated code with `stream did not
  contain valid UTF-8`.
- **Windows: POSIX-only interfaces.** Rust reaching `std::os::unix`, Python
  reaching `os.uname`, `os.geteuid`, `os.getpgid`, `os.killpg`, `os.openpty`.
- **Windows: a quoted `nextest` filter reaches the program with its quotes.**
- **macOS: `strace` could not run**, and the command-line crate's journeys run
  every invocation under it.
- **macOS: a documented state-directory path is printed with a `/private`
  prefix**, so the documentation journey no longer matches it.
- **Both families: the release-artifact and install-route tests assume Linux** —
  a `manylinux` wheel tag, a program called `printobserver`, two tarballs, and a
  platform map naming only the two Linux identifiers.

## `macos-aarch64` — runner `macos-15`

Run 2, job
[104765467330](https://github.com/nickderobertis/printobserver/actions/runs/35087431232/job/104765467330).
Every setup step came up; `extractions/setup-just` had a build, so the
from-source step was skipped.

| Tier | Came up |
| --- | --- |
| `just bootstrap` | yes |
| `just format-check` | yes |
| `just lint` | yes |
| `just typecheck` | yes |
| `just test` | **no** |
| `just coverage` | **no** |
| `just build` | yes |
| `just lint-workflows` | yes |
| `just check-repo` | yes |
| `just test-e2e` | **no** |

### `just test` — did not come up

Failed tasks `printobserver:test`, `release-artifacts:test`,
`printer-smoke:test` and `repo-checks:test`. The command-line crate's journeys
(`printobserver: Summary [ 0.707s] 23/60 tests run: 20 passed, 3 failed`):

```text
printobserver:     `strace` could not run, and this tier's whole claim about which endpoints an invocation reaches rests on it: No such file or directory (os error 2)
```

```text
printobserver:     thread 'every_documented_example_prints_what_the_document_shows' (32142) panicked at crates/printobserver/tests/journeys/documenting.rs:436:5:
printobserver:     the documentation no longer shows what its examples print:
printobserver:     docs/reference/common-operations.md:114 shows `printobserver context --print-id PRINT_ID` printing
...
printobserver:     image_path: STATE_DIR/images/10/106326ff23f8c012db471960fb919d702d7de21f86dd3170d6760b975d2d4674
printobserver:     and it printed
...
printobserver:     image_path: /privateSTATE_DIR/images/10/106326ff23f8c012db471960fb919d702d7de21f86dd3170d6760b975d2d4674
```

The release artifacts (`release-artifacts: 13 failed, 189 passed`):

```text
release-artifacts: FAILED tools/release-artifacts/tests/test_artifacts.py::test_the_python_route_carries_a_platform_tag_and_a_runnable_program - AssertionError: expected printobserver_cli-0.2.0-py3-none-macosx_15_7_arm64.whl to state the platform it was built for
release-artifacts: FAILED tools/release-artifacts/tests/test_artifacts.py::test_the_wheel_tag_states_the_library_the_program_was_built_against - AssertionError: expected 'manylinux_2_39_aarch64'; got 'macosx_2_39_arm64'
release-artifacts: FAILED tools/release-artifacts/tests/test_publishing.py::test_the_checksum_file_lists_every_platforms_tarball - AssertionError: expected 2 for one line per tarball; got 3
```

The smoke test's own suite (`printer-smoke: 1 failed, 48 passed`):

```text
printer-smoke: FAILED tools/printer-smoke/tests/test_cleanup.py::test_an_interrupted_run_leaves_the_machine_as_it_found_it - AssertionError: expected 'interrupted' in the str of what the interrupted run said:
printer-smoke: ''
```

The repository checks (`repo-checks: 1 failed, 809 passed`):

```text
repo-checks: FAILED tools/repo-checks/tests/test_platform_descriptor.py::test_this_hosts_own_baseline_is_read_off_the_host - AssertionError: expected 'manylinux_' in the str of this Linux host's wheel platform tag:
```

In run 1, over a list that did not name this platform, the release-artifact
failures read instead `repo_checks.platforms.PlatformError: this host is
`Darwin/arm64`, which AGENTS.md's supported-platform list does not name` (job
[104734084016](https://github.com/nickderobertis/printobserver/actions/runs/35077754091/job/104734084016)).

### `just coverage` — did not come up

It measured what the failed test run left behind:

```text
TOTAL                                                13098              1245    90.49%        1437               151    89.49%        9545               616    93.55%           0                 0         -

Rust line coverage is below the 95% floor. Add tests that drive the uncovered lines, or explain the floor change in AGENTS.md.
error: recipe `coverage` failed on line 95 with exit code 1
```

### `just test-e2e` — did not come up

Collection stopped at the first module:

```text
repo-e2e:     PLATFORM = {"x86_64": "linux-x86_64", "aarch64": "linux-aarch64"}[os.uname().machine]
repo-e2e: E   KeyError: 'arm64'
repo-e2e: ERROR tests/repo-e2e/tests/test_install_script_journey.py - KeyError: 'arm64'
```

## Intel macOS — runner `macos-15-intel`

Cut as a platform on 2026-09-18, for hosted-runner cost, before its bring-up
finished; `repo-policy.toml`'s `platforms.retired` is the record. Run 2, job
[104765467622](https://github.com/nickderobertis/printobserver/actions/runs/35087431232/job/104765467622),
said the same as `macos-aarch64` above tier for tier — the same four failed
tasks under `just test`, in the same words, the same coverage floor under
`just coverage`, and the same collection error under `just test-e2e` — so
what that section records is what this runner said too, and nothing of its
own is kept here.

## `windows-x86_64` — runner `windows-2025`

Run 2, job
[104765467588](https://github.com/nickderobertis/printobserver/actions/runs/35087431232/job/104765467588).
Every setup step came up; `extractions/setup-just` had a build, so the
from-source step was skipped.

| Tier | Came up |
| --- | --- |
| `just bootstrap` | **no** |
| `just format-check` | **no** |
| `just lint` | **no** |
| `just typecheck` | **no** |
| `just test` | **no** |
| `just coverage` | **no** |
| `just build` | **no** |
| `just lint-workflows` | **no** |
| `just check-repo` | **no** |
| `just test-e2e` | **no** |

### `just bootstrap` — did not come up

`cargo fetch`, `uv sync` and the locked JavaScript install all finished; the
first recipe line reaching `repo_checks` did not:

```text
uv run -q python -m repo_checks install-tools
D:\a\printobserver\printobserver\.venv\Scripts\python.exe: No module named repo_checks
error: recipe `bootstrap` failed on line 23 with exit code 1
```

### `just format-check` — did not come up

Failed task `printobserver-sdk-node:format-check`, over every file of the Node
client, each line of the diff ending in a carriage return (`␍`):

```text
printobserver-sdk-node: npm\printobserver-sdk\integration\falsifying.test.ts format ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
printobserver-sdk-node:   × Formatter would have printed the following content:
printobserver-sdk-node:       1     │ - /**␍
...
printobserver-sdk-node: Found 20 errors.
```

### `just lint` — did not come up

Failed tasks `printobserver-oneharness:lint`, `printobserver-server:lint` and
`printobserver:lint`:

```text
printobserver-oneharness: error[E0433]: cannot find `unix` in `os`
printobserver-oneharness:    --> crates\printobserver-oneharness\src\sign_in.rs:126:22
printobserver-oneharness: 126 |         use std::os::unix::fs::PermissionsExt as _;
printobserver-oneharness:     |                      ^^^^ could not find `unix` in `os`
...
printobserver-oneharness: error[E0599]: no method named `mode` found for struct `std::fs::DirBuilder` in the current scope
```

### `just typecheck` — did not come up

Failed tasks `octoprint-env:typecheck`, `printer-smoke:typecheck`,
`obico-env:typecheck` and `repo-e2e:typecheck`:

```text
printer-smoke: error[unresolved-attribute]: Module `os` has no member `openpty`
octoprint-env: error[unresolved-attribute]: Module `os` has no member `O_NOCTTY`
octoprint-env: error[unresolved-attribute]: Module `os` has no member `geteuid`
octoprint-env: error[unresolved-attribute]: Module `os` has no member `getpgid`
octoprint-env: error[unresolved-attribute]: Module `os` has no member `killpg`
```

### `just test` — did not come up

The recipe ended `error: recipe `test` failed on line 91 with exit code 130`,
listing sixteen failed tasks. The Rust crates that compile fail their schema
drift test on line endings, and the crates reaching `std::os::unix` do not
compile, as under lint:

```text
printobserver-supervisor-api:     the checked-in schemas have drifted:
printobserver-supervisor-api:     D:\a\printobserver\printobserver\crates\printobserver-supervisor-api\..\..\schemas\printobserver-supervisor-api\TurnRequest.json is not what the types generate
```

The quoted `nextest` filter reaches `nextest` with its quotes:

```text
printobserver-octoprint:   error: expected expression
printobserver-octoprint:  1 │ 'not
printobserver-octoprint: error: failed to parse filterset
```

The Obico adapter's unreachable-host test:

```text
printobserver-obico:     thread 'a_host_nothing_answers_on_is_unreachable' at crates\printobserver-obico\tests\ingress.rs:737:5:
printobserver-obico:     expected an unreachable host, found the source took too long
```

(`printobserver-obico: Summary [ 2.372s] 17/41 tests run: 16 passed, 1 failed`).

The Python suites (`contract-codegen: 16 failed, 48 passed`,
`release-artifacts: 38 failed, 160 passed, 4 errors`, `repo-checks: 44 failed,
766 passed`):

```text
contract-codegen: FAILED tools/contract-codegen/tests/test_correspondence.py::test_the_committed_clients_are_what_the_generator_writes - contract_codegen.generate.FormatterError: `rustfmt` refused what was generated for crates/printobserver-sdk/src/contract.rs (1): stream did not contain valid UTF-8
release-artifacts: FAILED tools/release-artifacts/tests/test_artifacts.py::test_the_script_route_publishes_an_artifact_and_the_digest_it_is_verified_by - AssertionError: expected ['printobserver'] for what the release artifact carries; got ['printobserver.exe']
release-artifacts: FAILED tools/release-artifacts/tests/test_artifacts.py::test_the_wheel_tag_states_the_library_the_program_was_built_against - AssertionError: expected 'manylinux_2_39_x86_64'; got 'win_amd64'
repo-checks: FAILED tools/repo-checks/tests/test_cli.py::test_all_over_the_committed_tree_reports_nothing - AssertionError: expected 0; got 1
```

`printobserver-sdk-node:test` is among the failed tasks although that client's
own output reads `54 pass` and `0 fail`; what failed it is not in the step's log.

### `just coverage` — did not come up

```text
uv run -q python -m repo_checks coverage
D:\a\printobserver\printobserver\.venv\Scripts\python.exe: No module named repo_checks
error: recipe `coverage` failed on line 95 with exit code 1
```

### `just build` — did not come up

Failed tasks `printobserver-oneharness:build`, `printobserver-server:build` and
`printobserver:build`, on the same `std::os::unix` errors as lint:

```text
printobserver:    --> crates\printobserver-oneharness\src\sign_in.rs:163:40
printobserver: 163 |             std::fs::DirBuilder::new().mode(0o700).create(path)
printobserver:     |                                        ^^^^ method not found in `DirBuilder`
printobserver: error: could not compile `printobserver-oneharness` (lib) due to 4 previous errors
```

### `just lint-workflows` — did not come up

`actionlint` passed; `shellcheck` read the checked-out scripts' carriage returns:

```text
In scripts/setup-llmlint.sh line 79:
  log "installing llmlint-cli >= $LLMLINT_MIN via uv tool"
                                                          ^-- SC1017 (error): Literal carriage return. Run script through tr -d '\r' .
error: recipe `lint-workflows` failed on line 243 with exit code 1
```

### `just check-repo` — did not come up

```text
uv run -q python -m repo_checks all
D:\a\printobserver\printobserver\.venv\Scripts\python.exe: No module named repo_checks
error: recipe `check-repo` failed on line 248 with exit code 1
```

### `just test-e2e` — did not come up

```text
repo-e2e:     PLATFORM = {"x86_64": "linux-x86_64", "aarch64": "linux-aarch64"}[os.uname().machine]
repo-e2e: E   AttributeError: module 'os' has no attribute 'uname'. Did you mean: 'name'?
repo-e2e: ERROR tests/repo-e2e/tests/test_install_script_journey.py - AttributeError: module 'os' has no attribute 'uname'. Did you mean: 'name'?
```

## `windows-aarch64` — runner `windows-11-arm`

Run 2, job
[104765467478](https://github.com/nickderobertis/printobserver/actions/runs/35087431232/job/104765467478).

**The toolchain setup did not fully come up.** `extractions/setup-just` has no
build for this runner:

```text
##[error]failed to determine any valid targets; arch = arm64, platform = win32
```

In run 1, with nothing else to put `just` on the path, every tier then failed
before starting — `C:\a\_temp\5f14147a-c1cc-45cf-8363-f88a430270d5.sh: line 1:
just: command not found` (job
[104734084061](https://github.com/nickderobertis/printobserver/actions/runs/35077754091/job/104734084061)).
In run 2 the probe built it instead — `Installed package `just v1.58.0`
(executable `just.exe`)` — and every other setup step came up, so the tiers
below ran. The runner can install the toolchain; the action the gate uses cannot
install `just` on it.

| Tier | Came up |
| --- | --- |
| `just bootstrap` | **no** |
| `just format-check` | **no** |
| `just lint` | **no** |
| `just typecheck` | **no** |
| `just test` | **no** |
| `just coverage` | **no** |
| `just build` | **no** |
| `just lint-workflows` | **no** |
| `just check-repo` | **no** |
| `just test-e2e` | **no** |

Every tier failed the way it failed on `windows-x86_64`, in the same tool's
words; what differs is below each.

### `just bootstrap` — did not come up

```text
uv run -q python -m repo_checks install-tools
C:\a\printobserver\printobserver\.venv\Scripts\python.exe: No module named repo_checks
error: recipe `bootstrap` failed on line 23 with exit code 1
```

### `just format-check` — did not come up

Failed task `printobserver-sdk-node:format-check`, ending
`printobserver-sdk-node: Found 20 errors.`, on the same carriage returns.

### `just lint` — did not come up

Failed tasks `printobserver-oneharness:lint`, `printobserver-server:lint` and
`printobserver:lint`:

```text
printobserver-oneharness: error[E0433]: cannot find `unix` in `os`
```

### `just typecheck` — did not come up

Failed tasks `octoprint-env:typecheck`, `printer-smoke:typecheck`,
`obico-env:typecheck` and `repo-e2e:typecheck`, on the same `os` members.

### `just test` — did not come up

```text
contract-codegen: 16 failed, 48 passed in 69.76s (0:01:09)
release-artifacts: 38 failed, 160 passed, 4 errors in 189.92s (0:03:09)
repo-checks: 44 failed, 766 passed in 900.48s (0:15:00)
error: recipe `test` failed on line 91 with exit code 130
```

The same sixteen failed tasks, among them `printobserver-supervisor-api:test`
(`Summary [ 0.482s] 16/17 tests run: 15 passed, 1 failed`) and
`printobserver-obico:test` (`Summary [ 2.678s] 17/41 tests run: 16 passed, 1
failed`).

### `just coverage` — did not come up

```text
uv run -q python -m repo_checks coverage
C:\a\printobserver\printobserver\.venv\Scripts\python.exe: No module named repo_checks
```

### `just build` — did not come up

Failed tasks `printobserver-oneharness:build`, `printobserver-server:build` and
`printobserver:build`, on `error[E0433]: cannot find `unix` in `os``.

### `just lint-workflows` — did not come up

```text
         ^-- SC1017 (error): Literal carriage return. Run script through tr -d '\r' .
```

### `just check-repo` — did not come up

```text
uv run -q python -m repo_checks all
C:\a\printobserver\printobserver\.venv\Scripts\python.exe: No module named repo_checks
```

### `just test-e2e` — did not come up

```text
repo-e2e: E   AttributeError: module 'os' has no attribute 'uname'. Did you mean: 'name'?
repo-e2e: ERROR tests/repo-e2e/tests/test_install_script_journey.py - AttributeError: module 'os' has no attribute 'uname'. Did you mean: 'name'?
```
