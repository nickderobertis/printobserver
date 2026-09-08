//! The effective bounds: the configured envelope intersected with the manifest.
//!
//! A manifest may only narrow. The rule has three arms and one refusal, and
//! every one of them is here rather than spread over the callers:
//!
//! * An adjustable the manifest names **narrower** than the envelope takes the
//!   manifest's range.
//! * An adjustable the manifest names **wider** than the envelope is narrowed
//!   to the envelope's, and the narrowing is recorded on the print so nobody
//!   has to wonder later which bound applied.
//! * An adjustable the manifest is **silent** about takes the envelope's own
//!   range, because inheriting the envelope is what silence means.
//! * An adjustable the **envelope** does not name is no bound at all: it is not
//!   one this printer has, and a request for it is refused as such rather than
//!   acquiring a bound from nowhere.

use std::collections::BTreeMap;

use printobserver_types::{
    Adjustable, EffectiveBounds, JobManifest, ManifestNarrowing, Range, SafetyEnvelope,
};

/// The bounds in force for one print, and what the manifest asked for.
#[derive(Debug, Clone, PartialEq)]
pub struct Bounds {
    /// The range each adjustable may be set to, inclusive.
    pub effective: EffectiveBounds,
    /// Every manifest range that was wider than the envelope's, in order.
    pub narrowings: Vec<ManifestNarrowing>,
}

impl Bounds {
    /// The range one adjustable may be set to, or absent when this printer has
    /// no such adjustable.
    #[must_use]
    pub fn range(&self, adjustable: Adjustable) -> Option<Range> {
        self.effective.allowed.get(&adjustable).copied()
    }
}

/// The envelope intersected with the manifest, and the narrowings it took.
///
/// The envelope is the whole set of adjustables this printer has: an entry the
/// manifest names and the envelope does not is deliberately absent from the
/// answer rather than carried into it.
#[must_use]
pub fn effective_bounds(envelope: &SafetyEnvelope, manifest: Option<&JobManifest>) -> Bounds {
    let mut allowed = BTreeMap::new();
    let mut narrowings = Vec::new();
    for (adjustable, permitted) in &envelope.allowed {
        let Some(asked_for) = manifest.and_then(|held| held.allowed.get(adjustable)) else {
            allowed.insert(*adjustable, *permitted);
            continue;
        };
        let applied = Range::new(
            permitted.min.max(asked_for.min),
            permitted.max.min(asked_for.max),
        );
        if asked_for.min < permitted.min || asked_for.max > permitted.max {
            narrowings.push(ManifestNarrowing {
                adjustable: *adjustable,
                requested: *asked_for,
                applied,
            });
        }
        allowed.insert(*adjustable, applied);
    }
    Bounds {
        effective: EffectiveBounds { allowed },
        narrowings,
    }
}
