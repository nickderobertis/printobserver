# printobserver-octoprint

The constraints this crate is held to, and why. The root `AGENTS.md` holds the
repository-wide rules; this file holds the ones only this subtree carries.
`repo-policy.toml`'s `[octoprint]` table is the machine-readable half, and
`just check-repo` enforces it from there rather than from this file.

## This is the only crate that may construct an OctoPrint request

The workspace's dependency rule holds one level down, over vocabulary rather
than over edges: **`printobserver-octoprint` is the only crate that may
construct an `OctoPrint` request.** Everything above it is written as though
printers were normal, so the moment a second crate spells an OctoPrint path or
its authentication header there are two places one vendor's own surface has to
be kept right.

`repo-policy.toml`'s `[octoprint]` names the permitted crate and what
constructing such a request looks like in a Rust source; `just check-repo`
refuses one of those markers on a line of any other crate, exempting a
comment-only line so a crate may *say* `/api/job` while no crate but the adapter
may *build* one — and refuses a tree in which this crate itself constructs none,
because a rule guarding a boundary nothing is on has stopped being a rule.

So a new OctoPrint endpoint, header or request shape lands here and is exposed
upward through `printobserver-printer-api`'s port, never spelt in the crate that
needs it.
