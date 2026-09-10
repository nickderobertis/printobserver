# release-artifacts

The constraints this project is held to, and why each one is worth its cost.
What each tier *proves* and when it runs is `docs/reference/testing.md`, and the
machine-readable half of the rules below is `repo-policy.toml`'s
`[install_proof]`, enforced by `just check-repo` — neither is restated here.

This project builds every artifact this repository publishes and carries the two
proofs over them. `AGENTS.md`'s "The registry install-path proof" is the
repository-level fact that the second of those is outside the gate, when it runs
and how a person runs it by hand; everything below is the judgment behind how it
is keyed and what it is run against, which belongs beside the code that carries
it.

## Two proofs, and what each one can say

`just prove-route-*` proves an artifact **built from the committed tree**. It is
the only proof a change can run before anything is published — and it is green
over a repository nothing can install, which is the state this one was in for
two days.

`just prove-registry-*` proves what each registry actually serves, which is what
a user meets. It can say nothing about the change under review: it reads the
real registries, so over a change it could only ever report what was published
before that change. One recipe per route, because the three routes are
alternatives and a reader chasing a failure wants the one that failed.

## Which version is proven is never the number in this tree

That number is whatever release automation last wrote into the workspace, and
what a user gets is whatever the registry is serving.
`PRINTOBSERVER_PROOF_VERSION` names the version under test, and every version
reaching this project from outside — a registry's metadata, a forge's release
list, or that variable — is narrowed in one place before anything is asked to
install it.

## The sequencing trap

A release's own proof is keyed on the `release-plz` workflow **having
finished**, not on the GitHub Release being published. That workflow cuts the
release before the two jobs that build and publish its artifacts, so a proof
keyed on the release itself measures the version before it.

## A release-time run proves the release IT cut

Never the newest the forge lists, and the tag left at that run's own commit is
what binds the two — a forge listing says which releases exist and not which run
cut which. `release_always` finishes a run on every push to `main` and all but
the release ones cut nothing, so two runs are in flight often; "the newest" is
then somebody else's release, which a run keyed on it goes green over while its
own goes unproven.

## A run that cut a release and failed to publish it is proven, not skipped

That is the one state this tier exists to find, and such a run concludes as a
failure — so gating the proof on the triggering run's *conclusion* skips exactly
it, and the missing publish is reported by nothing at all. Keyed on the release
that run cut, it is proven and answers that nothing serves it.

## Nothing here publishes to a registry in order to prove a point

`PRINTOBSERVER_PROOF_REGISTRIES` points all three registries somewhere else, and
`standin.py` is what answers there: a Python index serving real wheels, a
JavaScript registry serving real packages, and a forge listing real releases with
their artifacts and digests. The real `pip`, the real `npm` and the committed
install script do the installing, so every outcome is driven end to end without a
publish — which is the only way the failing directions can be falsified at all.

## A proof that passed says so in one line

Everything a reader of a pass needs is on it: which version was proven, where
that version came from, what installed it and what the program said. A **failing**
proof prints every fact the run knew, one to a line, because what a reader of a
failure is doing is finding out which repair it is — an artifact that does not
work, a publish that did not happen, or a registry that could not be reached at
all. Those are different next actions, so they are different answers rather than
one failure with a message.
