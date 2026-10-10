//! The arithmetic of filament, and the one rule for which command clears which
//! mark on a spool.
//!
//! Pure functions, with no store and no printer behind them, so that the
//! store, the supervision domain and every reader of a charge work a figure out
//! the same way rather than each restating it.

use printobserver_types::serde::{Deserialize, Serialize};

use crate::config::SpoolPolicy;
use crate::records::{SpoolTrust, UnverifiedMark, UnverifiedReason, UnverifiedReasonKind};

/// The diameter a spool is registered with when none is given, in millimetres.
pub const DEFAULT_DIAMETER_MM: f64 = 1.75;

/// The density of each material a spool may be registered as without giving
/// one, in grams per cubic centimetre.
pub const MATERIAL_DENSITIES: [(&str, f64); 7] = [
    ("PLA", 1.24),
    ("PETG", 1.27),
    ("ABS", 1.04),
    ("ASA", 1.07),
    ("TPU", 1.21),
    ("PC", 1.20),
    ("PA", 1.14),
];

/// The known density of a material, matched without regard to case.
#[must_use]
pub fn density_for(material: &str) -> Option<f64> {
    MATERIAL_DENSITIES
        .iter()
        .find(|(name, _)| name.eq_ignore_ascii_case(material.trim()))
        .map(|(_, density)| *density)
}

/// The grams a length of filament weighs: its cross-section times its length,
/// in cubic centimetres, times its density.
#[must_use]
pub fn grams_for_length(length_mm: f64, diameter_mm: f64, density_g_cm3: f64) -> f64 {
    let radius_mm = diameter_mm / 2.0;
    std::f64::consts::PI * radius_mm * radius_mm * length_mm / 1_000.0 * density_g_cm3
}

/// The margin asked for beyond a requirement: the policy's percentage of it,
/// or its minimum, whichever is more.
#[must_use]
pub fn buffer_g(required_g: f64, policy: &SpoolPolicy) -> f64 {
    (required_g * policy.buffer_percent / 100.0).max(policy.buffer_min_g)
}

/// The spool command a mark is being cleared by.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(crate = "printobserver_types::serde", rename_all = "snake_case")]
pub enum MarkClearing {
    /// The spool's figure was corrected.
    Adjusted,
    /// The spool was swapped off its tool for another.
    SwappedOut,
    /// The spool already on a tool was confirmed as never having been swapped.
    ConfirmedInPlace,
}

/// The marks one command clears: a correction clears every one; a swap clears
/// the outgoing spool's unconfirmed filament change, but not the doubt over a
/// position that could not be read; and a confirmation in place clears both
/// filament-change marks. Nothing else clears a mark.
#[must_use]
pub const fn marks_cleared_by(clearing: MarkClearing) -> &'static [UnverifiedReasonKind] {
    match clearing {
        MarkClearing::Adjusted => &[
            UnverifiedReasonKind::UnchargedPrint,
            UnverifiedReasonKind::UnaccountedPrint,
            UnverifiedReasonKind::FilamentChangeUnconfirmed,
            UnverifiedReasonKind::FilamentChangeUnconfirmedPosition,
            UnverifiedReasonKind::TallyReset,
        ],
        MarkClearing::SwappedOut => &[UnverifiedReasonKind::FilamentChangeUnconfirmed],
        MarkClearing::ConfirmedInPlace => &[
            UnverifiedReasonKind::FilamentChangeUnconfirmed,
            UnverifiedReasonKind::FilamentChangeUnconfirmedPosition,
        ],
    }
}

impl SpoolTrust {
    /// This trust with one more mark, unless a mark for that same reason is
    /// already held.
    #[must_use]
    pub fn marked(self, mark: UnverifiedMark) -> Self {
        let mut marks = self.into_marks();
        if !marks.iter().any(|held| held.reason == mark.reason) {
            marks.push(mark);
        }
        Self::Unverified { marks }
    }

    /// This trust with every mark of these kinds removed, and the reasons it
    /// removed, oldest first; verified once no mark remains.
    #[must_use]
    pub fn cleared(self, kinds: &[UnverifiedReasonKind]) -> (Self, Vec<UnverifiedReason>) {
        let (removed, kept): (Vec<UnverifiedMark>, Vec<UnverifiedMark>) = self
            .into_marks()
            .into_iter()
            .partition(|mark| kinds.contains(&mark.reason.kind()));
        let trust = if kept.is_empty() {
            Self::Verified
        } else {
            Self::Unverified { marks: kept }
        };
        (trust, removed.into_iter().map(|mark| mark.reason).collect())
    }

    /// The mark held longest, when any is held.
    #[must_use]
    pub fn oldest_mark(&self) -> Option<&UnverifiedMark> {
        match self {
            Self::Verified => None,
            Self::Unverified { marks } => marks.first(),
        }
    }

    /// Every mark held, oldest first.
    fn into_marks(self) -> Vec<UnverifiedMark> {
        match self {
            Self::Verified => Vec::new(),
            Self::Unverified { marks } => marks,
        }
    }
}
