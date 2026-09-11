<!-- llmlint: ignore[instruction_layer_localized] Reviews of this subtree are routed: `.github/CODEOWNERS` assigns every path, this one included, with `* @nickderobertis`. A run over a diff is handed only the files that changed and that file is not one of them, so a judge of this diff can read this project's file and not the routing it asks for. -->
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

## A publish that stopped partway is finished by running it again

Nothing is cleaned up first and nothing is moved by hand: the recovery is the
forge's own re-run of the `publish` job, so the publisher decides per artifact
rather than per run. Each artifact's registry is asked — through the same
documents the install-path proof reads — whether it already serves exactly
that artifact, and one it serves is reported rather than sent again; a refusal
is recorded and every artifact after it, on that registry and the others, is
still attempted; the release assets go up whether or not a package registry
refused; and the run fails at the end naming every refusal in the registry's
own words. A publisher that stopped at the first refusal would leave every
artifact after it for a person to send, which is the hand-run this forbids.

## A dispatched publish is the workflow's own, for a release that exists

Two commands serve the release workflow's second shape, and both are held to
the same answer shape a push produces so that the artifact jobs read one gate.
`dispatched` answers `released=<tag>` for a tag only after three things hold —
it is a tag release automation writes, the checkout carries it, and the
workspace manifest at that tag declares the version the tag names — and it
refuses everything else with nothing printed and nothing written, because a
tag that reaches the artifact jobs unverified is another release's tree built
and published under this one's name. The shallow, tagless checkout is the
refusal worth naming: an existing tag is invisible to it, so the refusal says
the checkout carries no tags and what fetches them, rather than that the
release does not exist. `recorded` reads back the one line `dispatched` wrote,
and refuses a record it cannot read for the reason `answered` refuses an
unreadable answer: the proof after the run skips on an empty field, and a
proof skipped over an unreadable record is a publish nobody checked.

## The version published is handed in on a dispatch, and `dist` is held to it

A push publishes at the workspace's own version, which is the tree the run
checked out. A dispatch builds an existing tag's tree and publishes with the
publisher at `main`, whose workspace has moved on, so the version arrives in
`PRINTOBSERVER_PUBLISH_VERSION` instead. Whichever it is, every wheel's file
name and every package's manifest in `dist` is held to it before any write:
the release tarballs carry no version in their names, so nothing else stops
one tag's artifacts landing on another's release, and that is the one state
nothing downstream can tell from a correct publish.

## No tier writes to the real forge

The forge upload is a direct call under `RELEASE_PLZ_TOKEN`, and GitHub
publishes no document its asset routes could be generated from, so the one
thing that could reconcile them is uploading to a real release — which nothing
here may do to prove a point. Every journey drives the publisher against the
stand-in in `standin.py`, which takes each registry's write as the real one
takes it, and a route or a media type the forge stopped accepting shows on the
next release as assets it does not carry and an install-path proof saying so.
