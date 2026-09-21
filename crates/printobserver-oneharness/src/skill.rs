//! The skill file, read as the system prompt it carries.
//!
//! The skill is an Agent Skill: a `SKILL.md` that opens with a frontmatter
//! block naming and describing it, followed by the prose the agent reads. The
//! frontmatter is for whatever installs and lists skills — and `gh skill
//! install` rewrites it, adding its own `metadata` and reordering the keys — so
//! what a turn sends is the prose alone, split off here the same way whatever
//! that block holds.

/// Why a skill file's prose cannot be told apart from its frontmatter.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct UnclosedFrontmatter;

impl core::fmt::Display for UnclosedFrontmatter {
    fn fmt(&self, formatter: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        formatter.write_str(
            "it opens a frontmatter block with a `---` line and no later line closes it, \
             so nothing in it can be told apart from the prose",
        )
    }
}

impl std::error::Error for UnclosedFrontmatter {}

/// The prose of one skill file: what a turn sends as its system prompt.
///
/// A file whose first line is not exactly `---` carries no frontmatter, and
/// its prose is the whole text. Otherwise the prose is every byte after the
/// next line that is exactly `---`, whatever keys and order the block between
/// the two holds — so a skill as committed and the same skill as a `gh skill
/// install` rewrote it yield the same prose. A line ending in `\r\n` is read as
/// the same line.
///
/// # Errors
///
/// Returns [`UnclosedFrontmatter`] when the first line opens a block and no
/// later line closes it. Such a file is refused rather than sent whole, because
/// the agent would be handed installer metadata as its instructions.
pub fn skill_prose(text: &str) -> Result<&str, UnclosedFrontmatter> {
    let mut lines = text.split_inclusive('\n');
    let Some(first) = lines.next() else {
        return Ok(text);
    };
    if !is_fence(first) {
        return Ok(text);
    }
    let mut consumed = first.len();
    for line in lines {
        consumed += line.len();
        if is_fence(line) {
            return Ok(&text[consumed..]);
        }
    }
    Err(UnclosedFrontmatter)
}

/// Whether one line, with its terminator, is exactly `---`.
fn is_fence(line: &str) -> bool {
    let bare = line
        .strip_suffix('\n')
        .map_or(line, |rest| rest.strip_suffix('\r').unwrap_or(rest));
    bare == "---"
}
