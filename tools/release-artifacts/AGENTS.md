# release-artifacts

The constraints this project is held to, and why each one is worth its cost.
What the code currently *does* is in `repo-policy.toml`'s `[install_proof]`, the
graph targets and the suites; nothing here restates them.

## A release-time proof proves the release that run cut

Not the newest release the forge lists. That listing says which releases exist
and not which run cut which, so the binding is the tag release automation left
at the run's own commit. `release_always` finishes a run on every push to `main`
and all but the release ones cut nothing, so two runs are in flight often — and
"the newest" is then somebody else's release, which the earlier run goes green
over while its own goes unproven.

## No proof is gated on the triggering run's conclusion

A run that cut its release and then failed to publish the artifacts concludes as
a failure, and that is the one state a registry proof exists to find. Gated on
the conclusion it is skipped, and the publish that did not happen is reported by
nothing at all.

## Every version arriving from outside is narrowed in one place

A registry's metadata, a forge's release list and the variable a caller named
are arbitrary strings until then, and a registry may serve anything beside the
versions release automation writes — a pre-release, a yanked-and-renamed
distribution, a tag somebody typed. None of those may be picked as "the newest"
or handed to a package manager.

## A proof that passed is one line; a proof that failed is the whole report

A pass carries what a reader of a pass needs and stops: the version proven,
where it came from, what installed it and what the program said. A failure
carries every fact the run knew, one to a line, because what a reader of a
failure is doing is working out which of the repairs it is — and those repairs
differ, so they are different answers rather than one failure with a message.
