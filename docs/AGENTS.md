# Documentation

Do not hand-edit `reference/surface.json`, `reference/schemas.md`, or the
console examples in `reference/common-operations.md`. `just docs-generate`
regenerates all three; `just check-repo` checks their declarations and drift.

Examples use `$ ` commands, stdout before stderr, and `$ echo $?` for exits.
Keep identifiers and instants as the existing placeholders so repeated runs
compare byte-for-byte. The `printobserver` documentation journeys execute the
examples and resolve the workflow solely through the skill and its links.

Skill links use relative `reference/` paths. The oneharness asset symlink and
bundled references must still resolve on an installed host without a checkout.
