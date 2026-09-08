//! What a sliced job says about itself, and the bounds it narrows to.

use std::collections::BTreeMap;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::adjustable::Adjustable;
use crate::reported::Range;

/// What a sliced job declares about itself and about what may be adjusted.
///
/// An adjustable the manifest does not name takes the envelope's own range; a
/// manifest range wider than the envelope's is narrowed to the envelope's and
/// the narrowing is recorded on the print. A manifest may only narrow.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct JobManifest {
    /// The file this manifest is about, as the slicer named it.
    pub file_name: String,
    /// The material the job is sliced for.
    pub material: String,
    /// The nozzle the job is sliced for, in millimetres.
    pub nozzle_diameter_mm: f64,
    /// The slicer profile the job was sliced with.
    pub slicer_profile: String,
    /// The range each named adjustable may take, inclusive.
    pub allowed: BTreeMap<Adjustable, Range>,
    /// Whatever else the slicer recorded.
    pub metadata: BTreeMap<String, String>,
}
