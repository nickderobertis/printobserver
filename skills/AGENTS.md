# Skills

`printobserver/` is the one Agent Skill this repository ships. `gh skill
install` takes that directory alone and drops symlinks, so everything the
skill links must be a regular file inside it, linked by a relative path that
stays there.

`SKILL.md` opens with its frontmatter (`name`, `description`, `license`); a
turn is sent the prose after it, from the skill's own directory.

Do not hand-edit `reference/surface.json`, `reference/schemas.md`, or the
console examples in `reference/common-operations.md`. `just docs-generate`
regenerates all three; `just check-repo` checks their declarations and drift.

Examples use `$ ` commands, stdout before stderr, and `$ echo $?` for exits.
Keep identifiers and instants as the existing placeholders so repeated runs
compare byte-for-byte. The `printobserver` documentation journeys execute the
examples and resolve the workflow solely through the skill and its links.
