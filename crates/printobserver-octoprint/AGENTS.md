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

So a new OctoPrint endpoint, header or request shape lands here and is exposed
upward through `printobserver-printer-api`'s port, never spelt in the crate that
needs it.
