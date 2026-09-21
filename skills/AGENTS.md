# Skills

`printobserver/` is the one Agent Skill this repository ships: `SKILL.md` and
the `reference/` documents it links to. `gh skill install
nickderobertis/printobserver printobserver` installs that directory and nothing
beside it, and drops every symlink, so everything the skill links must be a
regular file inside `printobserver/`, linked by a relative path that stays
there. `just check-repo` refuses a symlink under it and a link that leaves it.

`SKILL.md` opens with a frontmatter block (`name`, `description`, `license`);
the server sends the prose after it as every turn's system prompt, and runs the
agent from the skill's directory so its `reference/` links resolve.

Do not hand-edit `reference/surface.json`, `reference/schemas.md`, or the
console examples in `reference/common-operations.md`. `just docs-generate`
regenerates all three; `just check-repo` checks their declarations and drift.

Examples use `$ ` commands, stdout before stderr, and `$ echo $?` for exits.
Keep identifiers and instants as the existing placeholders so repeated runs
compare byte-for-byte. The `printobserver` documentation journeys execute the
examples and resolve the workflow solely through the skill and its links.
