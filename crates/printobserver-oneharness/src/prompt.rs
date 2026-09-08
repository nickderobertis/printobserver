//! The committed prompt template, and the three slots one turn fills.
//!
//! The template is the whole of what a prompt may say. This module fills its
//! slots and composes nothing beside them, which is why a rule about what the
//! agent may be told is a rule about one committed file rather than about code
//! spread over a turn.

use core::fmt;

/// The slot the triggering event is written into.
pub const EVENT_SLOT: &str = "{{event}}";
/// The slot the materialized path of the event's image is written into.
pub const IMAGE_SLOT: &str = "{{image_path}}";
/// The slot the command that reads the print's context is written into.
pub const CONTEXT_COMMAND_SLOT: &str = "{{context_command}}";

/// Every slot the template declares, and there is no other.
pub const SLOTS: [&str; 3] = [EVENT_SLOT, IMAGE_SLOT, CONTEXT_COMMAND_SLOT];

/// What the image slot carries when the event arrived with no image.
///
/// The slot is filled either way, because a template with a slot left empty is
/// a second prompt shape nobody reads.
pub const NO_IMAGE: &str = "(this event arrived with no image)";

/// Why a file is not the prompt template.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TemplateError {
    /// A slot the template must declare is absent.
    SlotMissing {
        /// The slot that is absent.
        slot: &'static str,
    },
    /// A slot the template must declare once is declared more than once.
    SlotRepeated {
        /// The slot that is repeated.
        slot: &'static str,
    },
}

impl fmt::Display for TemplateError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::SlotMissing { slot } => {
                write!(formatter, "the prompt template declares no {slot} slot")
            }
            Self::SlotRepeated { slot } => {
                write!(
                    formatter,
                    "the prompt template declares {slot} more than once"
                )
            }
        }
    }
}

impl core::error::Error for TemplateError {}

/// The committed template, read once and filled per turn.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PromptTemplate {
    /// The literal text between the slots, in the order the template writes
    /// them: one more segment than there are slots.
    segments: Vec<String>,
    /// The slots, in the order the template writes them.
    order: Vec<&'static str>,
}

impl PromptTemplate {
    /// Read a template, requiring each slot exactly once.
    ///
    /// # Errors
    ///
    /// Returns [`TemplateError`] when a slot is absent or repeated.
    pub fn parse(text: &str) -> Result<Self, TemplateError> {
        let mut positions: Vec<(usize, &'static str)> = Vec::with_capacity(SLOTS.len());
        for slot in SLOTS {
            let mut found = text.match_indices(slot);
            let Some((at, _)) = found.next() else {
                return Err(TemplateError::SlotMissing { slot });
            };
            if found.next().is_some() {
                return Err(TemplateError::SlotRepeated { slot });
            }
            positions.push((at, slot));
        }
        positions.sort_unstable();

        let mut segments = Vec::with_capacity(positions.len() + 1);
        let mut order = Vec::with_capacity(positions.len());
        let mut cursor = 0;
        for (at, slot) in positions {
            segments.push(text[cursor..at].to_owned());
            order.push(slot);
            cursor = at + slot.len();
        }
        segments.push(text[cursor..].to_owned());
        Ok(Self { segments, order })
    }

    /// The prompt for one turn: this template, with its three slots filled and
    /// nothing else.
    ///
    /// Every slot is filled in one pass over the template's own segments, so
    /// text a filling happens to carry is never read back as a slot.
    #[must_use]
    pub fn fill(&self, event: &str, image_path: &str, context_command: &str) -> String {
        let mut filled = String::new();
        for (index, segment) in self.segments.iter().enumerate() {
            filled.push_str(segment);
            match self.order.get(index) {
                Some(&EVENT_SLOT) => filled.push_str(event),
                Some(&IMAGE_SLOT) => filled.push_str(image_path),
                Some(&CONTEXT_COMMAND_SLOT) => filled.push_str(context_command),
                Some(_) | None => {}
            }
        }
        filled
    }
}
