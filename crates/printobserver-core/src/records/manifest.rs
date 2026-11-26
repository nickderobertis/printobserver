//! What a sliced job says about itself, and the bounds it narrows to.

use std::collections::BTreeMap;

use printobserver_printer_api::Adjustable;
use printobserver_types::Range;
use printobserver_types::schemars::JsonSchema;
use printobserver_types::serde::{Deserialize, Deserializer, Serialize};

use super::ids::SpoolId;

/// What a sliced job declares about itself and about what may be adjusted.
///
/// An adjustable the manifest does not name takes the envelope's own range; a
/// manifest range wider than the envelope's is narrowed to the envelope's and
/// the narrowing is recorded on the print. A manifest may only narrow.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(crate = "printobserver_types::serde", deny_unknown_fields)]
#[schemars(crate = "printobserver_types::schemars")]
pub struct JobManifest {
    /// The file this manifest is about, as the slicer named it.
    pub file_name: String,
    /// The material the job is sliced for.
    pub material: String,
    /// The nozzle the job is sliced for, in millimetres.
    pub nozzle_diameter_mm: f64,
    /// The slicer profile the job was sliced with.
    pub slicer_profile: String,
    /// The build sheet the job goes down on, as one of the sheet ids the
    /// server's sheet table names, such as `smooth-pei` or `textured-pei`.
    ///
    /// Every start must declare one: a start whose manifest names none is
    /// refused, and no override lets it through. Optional here only so that a
    /// manifest stored before the field existed still loads.
    // llmlint: ignore[invalid_states_unrepresentable] suppressions.toml has the reason.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(extend("x-unchecked-text" = true))]
    pub sheet: Option<String>,
    /// The range each named adjustable may take, inclusive.
    pub allowed: BTreeMap<Adjustable, Range>,
    /// Whatever else the slicer recorded.
    pub metadata: BTreeMap<String, String>,
    /// The spool meant for each later stretch of a file with planned colour
    /// changes, which a start checks against that stretch's filament. A
    /// manifest naming none is checked as one without changes is.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub segment_spools: Vec<SegmentSpool>,
}

/// The spool meant for one stretch after a planned change.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(crate = "printobserver_types::serde", deny_unknown_fields)]
#[schemars(crate = "printobserver_types::schemars")]
pub struct SegmentSpool {
    /// The stretch, counting the one after the first change as 1: segment 0
    /// is the spool loaded on tool 0 at the start, so it is never named here.
    #[serde(deserialize_with = "after_a_change")]
    #[schemars(range(min = 1))]
    pub segment: u32,
    /// The spool meant for it.
    pub spool_id: SpoolId,
}

/// A segment number a manifest may name: one after a change, never 0.
fn after_a_change<'de, D: Deserializer<'de>>(deserializer: D) -> Result<u32, D::Error> {
    use printobserver_types::serde::de::Error as _;
    let segment = u32::deserialize(deserializer)?;
    if segment == 0 {
        return Err(D::Error::custom(
            "segment 0 is the spool on tool 0 at the start; a manifest names spools from \
             segment 1",
        ));
    }
    Ok(segment)
}
