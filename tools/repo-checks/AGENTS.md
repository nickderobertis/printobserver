# repo-checks

The rules this project's release probe, `src/repo_checks/release-probe.sh`, is
held to, and why. The root `AGENTS.md` holds the repository-wide rules; the
script's own header says what it asks each registry.

## Three answers, under onevcs's "What a probe prints"

One version line and exit 0 is **released**; nothing and exit 0 is **no
release**; a non-zero exit with the reason on stderr is **not answered**. A
consumer holds on not answered and moves on no release, so no release is only
ever a registry saying so: crates.io's null `max_stable_version`, a 404 from PyPI
or npm, or github.com sending `releases/latest` to the bare releases page. A
server error, a body it cannot read, and an `id` no `[[target]]` declares are not
answered.

## It runs as onevcs spawns it

A direct subprocess from the repository root, with `PATH` and `HOME` alone and no
credential, so it reads only what an unauthenticated caller can.

## The GitHub Release is read off its redirect, never the REST API

An engine polls a held probe every sixty seconds, and the unauthenticated REST
limit is sixty an hour for the whole host. The tag is answered without its `v`.

## A release is not released until its `SHA256SUMS` is up

A consumer installs the program from that release, and `scripts/install.sh`
refuses every artifact it cannot verify against that file, so until it is there
the answer is not answered.

## Its readings are reconciled against the real registries

The journey proves the script against registries doubled at HTTP, which cannot
notice a producer changing its answer. `just probe-registries` is the
reconciliation: every declared target is published, so it fails unless each
answers a version.
