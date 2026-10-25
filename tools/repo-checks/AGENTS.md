# repo-checks

Why this project's release probe, `src/repo_checks/release-probe.sh`, is held to
the rules it is. The root `AGENTS.md` says what it answers and how it is run;
the script's own header says what it asks each registry.

## No release is only ever a registry saying so

A consumer holds on not answered and moves on no release, so an answer the probe
cannot vouch for has to hold rather than release a wait early.

## It runs as onevcs spawns it

With `PATH` and `HOME` alone and no credential, so it reads only what an
unauthenticated caller can, and a reading that passes here passes there.

## The GitHub Release is read off its redirect, never the REST API

An engine polls a held probe every sixty seconds, and the unauthenticated REST
limit is sixty an hour for the whole host.

## A release is not released until its `SHA256SUMS` is up

A consumer installs the program from that release, and `scripts/install.sh`
refuses every artifact it cannot verify against that file.

## A body nested past a stated bound is unreadable

How deep Python's parser nests before refusing depends on the interpreter's
stack, so the same body would read on one host and not on another.

## Its readings are reconciled against the real registries, off the gate

The journey's doubled registries cannot notice a producer changing its answer,
so the real ones are asked — from the scheduled install proof, because no gate
tier may read the network, and never beside the gate's own stand-in proofs. No
release passes it, because a target declared before its first publication
honestly answers that; only not answered, or an answer that is not one version,
fails it, because either is a registry the probe can no longer read.
