//! What a slicer wrote about a file at its end: the material each tool is
//! sliced for.
//!
//! `PrusaSlicer` writes every setting it sliced with as a block of comments at
//! the end of the file, between `; prusaslicer_config = begin` and
//! `; prusaslicer_config = end`, one `; <key> = <value>` per line. A setting
//! with one value per tool separates them with `;`, so
//! `; filament_type = PLA;PETG` is tool 0 sliced for PLA and tool 1 for PETG.
//!
//! The block is read from the file's **tail** alone: [`TAIL_BYTES`] is how
//! much of the end is asked for, which the recorded prints under
//! `tests/real-prints/` hold to — their whole settings block is under 17 KiB,
//! and `filament_type` under 12 KiB from the end.

use core::ops::ControlFlow;

/// How many bytes from a file's end its settings are read from.
pub const TAIL_BYTES: usize = 64 * 1024;

/// The setting naming the material each tool is sliced for.
const FILAMENT_TYPE: &str = "filament_type";

/// The last bytes of a file, collected as they arrive and never more than a
/// bound: a body longer than the bound is stopped rather than read whole.
#[derive(Debug, Clone, Default)]
pub struct Tail {
    /// The bound: a body longer than this many bytes is stopped, never read.
    limit: usize,
    /// The bytes taken so far, in the order they arrived.
    bytes: Vec<u8>,
    /// Whether more than the bound arrived, so reading stopped.
    overflowed: bool,
}

impl Tail {
    /// An empty tail holding at most `limit` bytes.
    #[must_use]
    pub const fn new(limit: usize) -> Self {
        Self {
            limit,
            bytes: Vec::new(),
            overflowed: false,
        }
    }

    /// Take the next piece; `Break` once more than the bound has arrived.
    pub fn take(&mut self, piece: &[u8]) -> ControlFlow<()> {
        if self.bytes.len() + piece.len() > self.limit {
            self.overflowed = true;
            return ControlFlow::Break(());
        }
        self.bytes.extend_from_slice(piece);
        ControlFlow::Continue(())
    }

    /// Whether more than the bound arrived.
    #[must_use]
    pub const fn overflowed(&self) -> bool {
        self.overflowed
    }

    /// What it holds.
    #[must_use]
    pub fn bytes(&self) -> &[u8] {
        &self.bytes
    }
}

/// The material each tool is sliced for, tool 0 first, by the last
/// `; filament_type = …` line of a file's tail: `None` for a tool the line
/// leaves blank, and empty when it names none at all.
///
/// `starts_mid_line` says the tail does not begin at the file's first byte,
/// so its first line may be the end of a longer one and is not read.
#[must_use]
pub fn filament_types_in(tail: &[u8], starts_mid_line: bool) -> Vec<Option<String>> {
    let text = String::from_utf8_lossy(tail);
    let mut lines = text.lines();
    if starts_mid_line {
        lines.next();
    }
    lines
        .rev()
        .find_map(setting_value)
        .map(|value| {
            value
                .split(';')
                .map(str::trim)
                .map(|named| (!named.is_empty()).then(|| named.to_owned()))
                .collect::<Vec<_>>()
        })
        .filter(|types| types.iter().any(Option::is_some))
        .unwrap_or_default()
}

/// The value of one `; filament_type = <value>` comment line.
fn setting_value(line: &str) -> Option<&str> {
    let comment = line.trim_start().strip_prefix(';')?;
    let (key, value) = comment.split_once('=')?;
    (key.trim() == FILAMENT_TYPE).then_some(value)
}
