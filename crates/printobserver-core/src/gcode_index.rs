//! What the slicer planned to print at each height, indexed once per print
//! from the print's own G-code.
//!
//! A supervising agent judging a frame needs to know what *should* be there:
//! a support tower, a bridge or an overhang the slicer planned looks, to a
//! camera, much like a defect. The slicer wrote that plan into the file, and
//! this module reads it: one streamed pass over the whole file builds a
//! [`GcodeIndex`], and every check after that reads the plan at the print's
//! current file position from the index rather than from the file. The agent
//! is handed only what is derived — a [`SlicerPlan`] — and never a line of
//! G-code.
//!
//! # What is read
//!
//! * `;LAYER_CHANGE` starts a layer and the `;Z:` after it is its height: the
//!   markers `PrusaSlicer` writes. The last layer's height is the final one.
//! * `;TYPE:<name>` names what the moves after it extrude — `Perimeter`,
//!   `Support material`, `Bridge infill` — and a layer's types are those under
//!   which any move of that layer laid material. `Custom` is left out: it
//!   labels the slicer's own start and end G-code rather than the part.
//! * `M73 P<percent> R<minutes>` is the slicer's own estimate of the time
//!   left, in the normal mode. The silent mode's `M73 Q<percent> S<minutes>`
//!   is ignored.
//! * `M221 S<percent>` sets the flow factor and `M106 S<0–255>` the part
//!   cooling fan, which `M107` turns off; a fan named by a `P` other than `0`
//!   is not the part-cooling fan.
//! * `M486 S<n>` starts laying down object `n`, `M486 S-1` stops, and
//!   `M486 A<name>` names the object last started. An object's bed area is the
//!   rectangle every move laying material for it reached — an arc's the
//!   extremes of the part of its circle it sweeps, so a footprint is never
//!   smaller than what was laid and a nearly straight arc about a far centre
//!   is no larger than the line it nearly is.
//!
//! Everything else is a move or nothing to this reader: `G0`–`G3` with
//! `G90`/`G91`, `M82`/`M83`, `G92` and `G28` are followed so that what laid
//! material, and where, is known; a comment is not a command, a leading
//! `N<line number>` is not a word, and `*<checksum>` ends the line.
//!
//! # Where a position falls
//!
//! A line counts when every byte of it, its newline included, lies before the
//! position — `OctoPrint`'s file position is the offset after the last line it
//! read. So a marker the printer has read is in force, and one it is part-way
//! through is not yet.
//!
//! # Once per print
//!
//! [`GcodeIndexes`] holds each print's index in memory for as long as this
//! process runs. The first check or context read after a print starts builds
//! it; a read arriving while it is being built waits for that build rather
//! than starting a second, and no read after it touches the file. A read that
//! failed builds nothing, so the next one tries again; a file whose markers
//! are missing builds an index that says so ([`PlanFact`]), which is not a
//! failure.

use core::future::Future;
use core::ops::ControlFlow;
use core::pin::Pin;
use core::task::{Context, Poll, Waker};
use std::collections::BTreeMap;
use std::sync::{Arc, Mutex, PoisonError};

use printobserver_printer_api::GcodeSink;
use printobserver_types::PrintId;
use printobserver_types::contract::Sample;
use printobserver_types::schemars::JsonSchema;
use printobserver_types::serde::{Deserialize, Serialize};

/// The longest line this reader holds; anything longer is no line it reads.
///
/// G-code lines are tens of bytes. A bound keeps a file that is not G-code
/// from being held in memory a line at a time.
const LINE_LIMIT: usize = 4096;

/// The most distinct extrusion types one index names; a file naming more is
/// no slicer's output, and its further types are not recorded.
const TYPE_LIMIT: usize = 64;

/// The most objects one index names, for the same reason.
const OBJECT_LIMIT: usize = 1024;

/// The longest object name recorded, in characters.
const NAME_LIMIT: usize = 120;

/// The largest axis word read as a place, in millimetres: ten metres, far
/// past any machine's travel. A word beyond it is no place a head can be.
const AXIS_LIMIT: f64 = 1e4;

/// The largest number any word is read as: ten kilometres of filament as an
/// absolute `E`, far past any spool. A word beyond it is no figure a file sets.
const WORD_LIMIT: f64 = 1e7;

/// The most words one command is read as; G-code commands carry a handful.
const WORD_COUNT: usize = 8;

/// The XY travel below which a move counts as no travel at all.
const TRAVEL_MM: f64 = 1e-6;

/// The type label naming the slicer's own start and end G-code.
const NOT_THE_PART: &str = "Custom";

/// What `M106` sets the fan to with no `S` word, and what full speed is.
const FAN_FULL: f64 = 255.0;

/// One layer, from its `;LAYER_CHANGE` to the next.
#[derive(Debug, Clone, PartialEq)]
struct Layer {
    /// The offset just past its `;LAYER_CHANGE` line.
    at: u64,
    /// Its height, from the `;Z:` after it.
    z_mm: Option<f64>,
    /// The types any of its moves laid material under, as bits of the index's
    /// type table.
    types: u64,
}

/// A value a file set, and the offset just past the line that set it.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Mark<T> {
    /// The offset just past the line.
    at: u64,
    /// What it set.
    value: T,
}

/// An object as the reader has seen it so far.
#[derive(Debug, Clone, Default, PartialEq)]
struct ObjectDraft {
    /// Its name, from `M486 A`.
    name: Option<String>,
    /// The rectangle its material reached: least X, least Y, most X, most Y.
    bounds: Option<[f64; 4]>,
}

/// One file's slicer plan, by file position.
#[derive(Debug, Clone, PartialEq)]
pub struct GcodeIndex {
    /// How many bytes were read.
    bytes: u64,
    /// How many lines were read.
    lines: u64,
    /// Every layer, in file order.
    layers: Vec<Layer>,
    /// The extrusion types, in the order the file first names them.
    type_names: Vec<String>,
    /// Whether the file names any extrusion type.
    labelled_types: bool,
    /// The slicer's normal-mode estimates of the minutes left.
    time_left: Vec<Mark<i64>>,
    /// The flow factors the file sets, one meaning one hundred percent.
    flow: Vec<Mark<f64>>,
    /// The part-cooling fan speeds the file sets, in percent.
    fan: Vec<Mark<f64>>,
    /// Every object the file labels and lays material for, by its number.
    objects: Vec<PlannedObject>,
    /// Whether the file labels any object.
    labelled_objects: bool,
}

/// What a file set one adjustable to, as of a file position: the value, and
/// the offset just past the line that set it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SetInFile {
    /// What it set.
    pub value: f64,
    /// The offset just past the line that set it.
    pub at: u64,
}

impl GcodeIndex {
    /// How many bytes the index was built from.
    #[must_use]
    pub const fn bytes(&self) -> u64 {
        self.bytes
    }

    /// How many lines the index was built from.
    #[must_use]
    pub const fn lines(&self) -> u64 {
        self.lines
    }

    /// How many layers the file plans.
    #[must_use]
    pub const fn layer_count(&self) -> usize {
        self.layers.len()
    }

    /// How many entries the index holds: layers, time marks, flow and fan
    /// settings, and objects.
    #[must_use]
    pub const fn entries(&self) -> usize {
        self.layers.len()
            + self.time_left.len()
            + self.flow.len()
            + self.fan.len()
            + self.objects.len()
    }

    /// How many bytes the index itself holds on the heap and inline, which is
    /// what keeping it in memory for a print costs.
    #[must_use]
    pub fn resident_bytes(&self) -> usize {
        let names: usize = self.type_names.iter().map(String::capacity).sum();
        let objects: usize = self
            .objects
            .iter()
            .map(|object| object.name.as_ref().map_or(0, String::capacity))
            .sum();
        core::mem::size_of::<Self>()
            + self.layers.capacity() * core::mem::size_of::<Layer>()
            + self.type_names.capacity() * core::mem::size_of::<String>()
            + names
            + self.time_left.capacity() * core::mem::size_of::<Mark<i64>>()
            + self.flow.capacity() * core::mem::size_of::<Mark<f64>>()
            + self.fan.capacity() * core::mem::size_of::<Mark<f64>>()
            + self.objects.capacity() * core::mem::size_of::<PlannedObject>()
            + objects
    }

    /// The most extrusion types any one layer lays material under, and the
    /// file position just past that layer's `;LAYER_CHANGE`: where the plan
    /// names the most.
    #[must_use]
    pub fn busiest_layer(&self) -> Option<(usize, i64)> {
        self.layers
            .iter()
            .map(|layer| {
                (
                    usize::try_from(layer.types.count_ones()).unwrap_or(usize::MAX),
                    i64::try_from(layer.at).unwrap_or(i64::MAX),
                )
            })
            .reduce(|best, next| if next.0 > best.0 { next } else { best })
    }

    /// The flow factor the file had set by a position, when it had set one.
    #[must_use]
    pub fn flow_at(&self, position: i64) -> Option<SetInFile> {
        last_at(&self.flow, position).map(|mark| SetInFile {
            value: mark.value,
            at: mark.at,
        })
    }

    /// The part-cooling fan speed the file had set by a position, in percent,
    /// when it had set one.
    #[must_use]
    pub fn fan_at(&self, position: i64) -> Option<SetInFile> {
        last_at(&self.fan, position).map(|mark| SetInFile {
            value: mark.value,
            at: mark.at,
        })
    }

    /// The slicer's plan at one file position, with no change made through
    /// this program beside what the file set.
    #[must_use]
    pub fn plan_at(&self, position: i64) -> SlicerPlan {
        self.plan_with(position, None, None)
    }

    /// The slicer's plan at one file position, with the flow factor and fan
    /// speed changed through this program since the file last set each, when
    /// there was such a change.
    #[must_use]
    pub fn plan_with(
        &self,
        position: i64,
        flow_changed: Option<f64>,
        fan_changed: Option<f64>,
    ) -> SlicerPlan {
        let mut unknown = Vec::new();
        let layer_index = self
            .layers
            .iter()
            .rposition(|layer| reached(layer.at, position));
        let final_z_mm = self.layers.last().and_then(|layer| layer.z_mm);
        if self.layers.is_empty() {
            unknown.push(PlanFact::Layer);
        }
        if final_z_mm.is_none() {
            unknown.push(PlanFact::FinalZ);
        }
        if !self.labelled_types {
            unknown.push(PlanFact::Types);
        }
        let types = layer_index.map_or_else(Vec::new, |index| {
            let bits = self.layers[index].types;
            self.type_names
                .iter()
                .enumerate()
                .filter(|(bit, _)| bits & (1 << bit) != 0)
                .map(|(_, name)| name.clone())
                .collect()
        });
        let minutes_left = last_at(&self.time_left, position).map(|mark| mark.value);
        if minutes_left.is_none() {
            unknown.push(PlanFact::MinutesLeft);
        }
        let flow_factor = set_value(
            self.flow_at(position).map(|set| set.value),
            flow_changed,
            PlanFact::FlowFactor,
            &mut unknown,
        );
        let fan_percent = set_value(
            self.fan_at(position).map(|set| set.value),
            fan_changed,
            PlanFact::FanPercent,
            &mut unknown,
        );
        if !self.labelled_objects {
            unknown.push(PlanFact::Objects);
        }
        unknown.sort_unstable();
        SlicerPlan {
            layer: layer_index.map(|index| index + 1),
            layers: (!self.layers.is_empty()).then_some(self.layers.len()),
            z_mm: layer_index.and_then(|index| self.layers[index].z_mm),
            final_z_mm,
            types,
            minutes_left,
            flow_factor,
            fan_percent,
            objects: self.objects.clone(),
            unknown,
        }
    }
}

/// One adjustable as the file set it and as this program changed it since,
/// recording the fact as unknown when the file had set nothing.
fn set_value(
    file: Option<f64>,
    changed: Option<f64>,
    fact: PlanFact,
    unknown: &mut Vec<PlanFact>,
) -> Option<SetValue> {
    if file.is_none() {
        unknown.push(fact);
    }
    (file.is_some() || changed.is_some()).then_some(SetValue { file, changed })
}

/// Whether a line ending at `at` lies wholly before `position`.
fn reached(at: u64, position: i64) -> bool {
    u64::try_from(position).is_ok_and(|position| at <= position)
}

/// The last mark a position has reached.
fn last_at<T: Copy>(marks: &[Mark<T>], position: i64) -> Option<Mark<T>> {
    let Ok(position) = u64::try_from(position) else {
        return None;
    };
    let reached = marks.partition_point(|mark| mark.at <= position);
    reached.checked_sub(1).map(|index| marks[index])
}

/// One adjustable as the print's file set it at the current position, and as
/// a change made through this program since then left it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(crate = "printobserver_types::serde", deny_unknown_fields)]
#[schemars(crate = "printobserver_types::schemars")]
pub struct SetValue {
    /// What the file's own commands set it to, by the current position.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub file: Option<f64>,
    /// What the latest change made through this program set it to, when one
    /// was made after the file last set it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub changed: Option<f64>,
}

/// One object the print's file lays down, and the bed area it covers.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(crate = "printobserver_types::serde", deny_unknown_fields)]
#[schemars(crate = "printobserver_types::schemars")]
pub struct PlannedObject {
    /// Its name, as the file labels it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// The least X its material reaches, in millimetres.
    pub x_min_mm: f64,
    /// The most X its material reaches, in millimetres.
    pub x_max_mm: f64,
    /// The least Y its material reaches, in millimetres.
    pub y_min_mm: f64,
    /// The most Y its material reaches, in millimetres.
    pub y_max_mm: f64,
}

/// A fact of the plan the print's file does not state.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, JsonSchema,
)]
#[serde(crate = "printobserver_types::serde", rename_all = "snake_case")]
#[schemars(crate = "printobserver_types::schemars")]
pub enum PlanFact {
    /// The file marks no layers.
    Layer,
    /// The file states no final height.
    FinalZ,
    /// The file labels no extrusion types.
    Types,
    /// The file carries no estimate of the time left, by the current position.
    MinutesLeft,
    /// The file sets no flow factor, by the current position.
    FlowFactor,
    /// The file sets no part-cooling fan speed, by the current position.
    FanPercent,
    /// The file labels no objects.
    Objects,
}

/// What the slicer planned at the print's current file position: the shape
/// and place of what should be printing, read from the print's own G-code.
///
/// These are the slicer's plan rather than an observation: a feature named
/// here is part of the design at this height, and nothing here says how well
/// it printed.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(crate = "printobserver_types::serde", deny_unknown_fields)]
#[schemars(crate = "printobserver_types::schemars")]
pub struct SlicerPlan {
    /// The layer the print is in, counting the first as 1; absent before the
    /// first.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub layer: Option<usize>,
    /// How many layers the file plans.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub layers: Option<usize>,
    /// The current layer's height, in millimetres.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub z_mm: Option<f64>,
    /// The final layer's height, in millimetres.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub final_z_mm: Option<f64>,
    /// What the current layer lays material as, in the order the file first
    /// names each: `Perimeter`, `Support material`, `Bridge infill`.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub types: Vec<String>,
    /// The slicer's own estimate of the minutes left, at the current position.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub minutes_left: Option<i64>,
    /// The flow factor, one meaning one hundred percent.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub flow_factor: Option<SetValue>,
    /// The part-cooling fan speed, in percent.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fan_percent: Option<SetValue>,
    /// Every object the file lays down, each with the bed area it covers.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub objects: Vec<PlannedObject>,
    /// The facts the file does not state, which are therefore unknown rather
    /// than absent.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub unknown: Vec<PlanFact>,
}

impl Sample for SlicerPlan {
    fn sample_full() -> Self {
        Self {
            layer: Some(12),
            layers: Some(150),
            z_mm: Some(2.4),
            types: vec![
                "Perimeter".to_owned(),
                "External perimeter".to_owned(),
                "Support material".to_owned(),
            ],
            final_z_mm: Some(30.0),
            minutes_left: Some(42),
            flow_factor: Some(SetValue {
                file: Some(1.0),
                changed: Some(0.95),
            }),
            fan_percent: Some(SetValue {
                file: Some(100.0),
                changed: None,
            }),
            objects: vec![PlannedObject {
                name: Some("benchy.stl".to_owned()),
                x_min_mm: 100.0,
                x_max_mm: 150.0,
                y_min_mm: 99.0,
                y_max_mm: 117.0,
            }],
            unknown: vec![PlanFact::Objects],
        }
    }

    fn sample_minimal() -> Self {
        Self {
            layer: None,
            layers: None,
            z_mm: None,
            final_z_mm: None,
            types: Vec::new(),
            minutes_left: None,
            flow_factor: None,
            fan_percent: None,
            objects: Vec::new(),
            unknown: Vec::new(),
        }
    }
}

/// How a coordinate is read: as a place, or as a distance from the last one.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
enum Positioning {
    /// A place: `M82` for `E`, `G90` for the axes. What a firmware starts in.
    #[default]
    Absolute,
    /// A distance: `M83` for `E`, `G91` for the axes.
    Relative,
}

/// Builds a [`GcodeIndex`] from a file streamed into it, a piece at a time,
/// in one pass: the [`GcodeSink`] the printer port's whole-file read feeds.
#[derive(Debug, Default)]
pub struct IndexBuilder {
    /// The line being read, up to [`LINE_LIMIT`].
    line: Vec<u8>,
    /// Whether the line being read has run past [`LINE_LIMIT`].
    overlong: bool,
    /// How many bytes have been read.
    read: u64,
    /// How many lines have been read.
    lines: u64,
    /// Every layer so far.
    layers: Vec<Layer>,
    /// The extrusion types so far.
    type_names: Vec<String>,
    /// Whether any type was named.
    labelled_types: bool,
    /// The type the moves being read extrude, as a bit of the type table.
    current_type: Option<usize>,
    /// The time estimates so far.
    time_left: Vec<Mark<i64>>,
    /// The flow factors so far.
    flow: Vec<Mark<f64>>,
    /// The fan speeds so far.
    fan: Vec<Mark<f64>>,
    /// Every object labelled so far, by its number.
    objects: BTreeMap<i64, ObjectDraft>,
    /// The object the moves being read lay material for.
    current_object: Option<i64>,
    /// Whether any object was labelled.
    labelled_objects: bool,
    /// Where the head is in X, Y and Z.
    at: [f64; 3],
    /// The extruder's position.
    e: f64,
    /// What `G90`/`G91` last said.
    axes: Positioning,
    /// What `M82`/`M83` last said.
    extruder: Positioning,
}

impl GcodeSink for IndexBuilder {
    fn take(&mut self, piece: &[u8]) -> ControlFlow<()> {
        // Plain index loops rather than iterator adaptors throughout this
        // reader: it runs over every byte of a print, and is as fast in a
        // build without optimisation as in one with it only written so.
        let mut start = 0;
        let mut at = 0;
        while at < piece.len() {
            if piece[at] == b'\n' {
                let head = &piece[start..at];
                self.read += u64::try_from(at + 1 - start).unwrap_or(u64::MAX);
                if self.line.is_empty() && !self.overlong {
                    // The whole line is in this piece: read it where it lies.
                    self.lines += 1;
                    if head.len() <= LINE_LIMIT {
                        self.read_line(head);
                    }
                } else {
                    self.hold(head);
                    self.end_line();
                }
                start = at + 1;
            }
            at += 1;
        }
        let rest = &piece[start..];
        self.hold(rest);
        self.read += u64::try_from(rest.len()).unwrap_or(u64::MAX);
        ControlFlow::Continue(())
    }
}

impl IndexBuilder {
    /// A builder that has read nothing.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// The index of everything read, the file's last line counting whether or
    /// not it ended in a newline.
    #[must_use]
    pub fn finish(mut self) -> GcodeIndex {
        if !self.line.is_empty() || self.overlong {
            self.end_line();
        }
        let objects = self
            .objects
            .into_values()
            .filter_map(|draft| {
                draft
                    .bounds
                    .map(|[least_x, least_y, most_x, most_y]| PlannedObject {
                        name: draft.name,
                        x_min_mm: tenth(least_x),
                        x_max_mm: tenth(most_x),
                        y_min_mm: tenth(least_y),
                        y_max_mm: tenth(most_y),
                    })
            })
            .collect();
        GcodeIndex {
            bytes: self.read,
            lines: self.lines,
            layers: self.layers,
            type_names: self.type_names,
            labelled_types: self.labelled_types,
            time_left: self.time_left,
            flow: self.flow,
            fan: self.fan,
            objects,
            labelled_objects: self.labelled_objects,
        }
    }

    /// Hold more of the line being read, up to its bound.
    fn hold(&mut self, bytes: &[u8]) {
        if self.overlong {
            return;
        }
        if self.line.len() + bytes.len() > LINE_LIMIT {
            self.overlong = true;
            self.line.clear();
            return;
        }
        self.line.extend_from_slice(bytes);
    }

    /// Read the line held, which ended at the offset read so far.
    fn end_line(&mut self) {
        self.lines += 1;
        let line = core::mem::take(&mut self.line);
        if !self.overlong {
            self.read_line(&line);
        }
        self.overlong = false;
        self.line = line;
        self.line.clear();
    }

    /// Read one whole line, without its newline.
    fn read_line(&mut self, raw: &[u8]) {
        let raw = raw.trim_ascii();
        if let Some(comment) = raw.strip_prefix(b";") {
            if let Ok(comment) = core::str::from_utf8(comment) {
                self.comment(comment.trim_end());
            }
            return;
        }
        let held = words(raw);
        let mut words = held.as_slice();
        // A leading `N<line number>` numbers the line and is no word of it.
        if let Some((Word { letter: b'N', .. }, rest)) = words.split_first() {
            words = rest;
        }
        let Some(first) = words.first() else {
            return;
        };
        let code = if held.numbered {
            held.second_code
        } else {
            held.first_code
        };
        match (char::from(first.letter), code) {
            ('G', Some(code @ 0..=3)) => self.motion(code, &words[1..]),
            ('G', Some(28)) => self.home(&words[1..]),
            ('G', Some(90)) => self.axes = Positioning::Absolute,
            ('G', Some(91)) => self.axes = Positioning::Relative,
            ('G', Some(92)) => self.declare(&words[1..]),
            ('M', Some(82)) => self.extruder = Positioning::Absolute,
            ('M', Some(83)) => self.extruder = Positioning::Relative,
            ('M', Some(73)) => self.estimate(&words[1..]),
            ('M', Some(221)) => self.flow_factor(&words[1..]),
            ('M', Some(106)) => self.fan_speed(&words[1..], false),
            ('M', Some(107)) => self.fan_speed(&words[1..], true),
            ('M', Some(486)) => self.object(raw, &words[1..]),
            _ => {}
        }
    }

    /// A comment line: a layer change, a height, or an extrusion type.
    fn comment(&mut self, comment: &str) {
        if comment == "LAYER_CHANGE" {
            self.layers.push(Layer {
                at: self.read,
                z_mm: None,
                types: 0,
            });
        } else if let Some(z) = comment.strip_prefix("Z:") {
            if let Some(layer) = self.layers.last_mut()
                && layer.z_mm.is_none()
            {
                layer.z_mm = z.trim().parse::<f64>().ok().filter(|z| z.is_finite());
            }
        } else if let Some(name) = comment.strip_prefix("TYPE:") {
            let name = name.trim();
            self.labelled_types = true;
            self.current_type = if name == NOT_THE_PART {
                None
            } else {
                self.intern(name)
            };
        }
    }

    /// The bit of the type table one name is, adding it when there is room.
    fn intern(&mut self, name: &str) -> Option<usize> {
        if let Some(bit) = self.type_names.iter().position(|held| held == name) {
            return Some(bit);
        }
        if self.type_names.len() >= TYPE_LIMIT {
            return None;
        }
        self.type_names.push(name.to_owned());
        Some(self.type_names.len() - 1)
    }

    /// A move: where the head goes, and whether it laid material on the way.
    fn motion(&mut self, code: u32, words: &[Word]) {
        let start = self.at;
        // One pass over the words: a move carries a handful, and a print
        // carries millions of moves.
        let mut extruded = None;
        let mut at = 0;
        while at < words.len() {
            let Word { letter, value } = words[at];
            at += 1;
            if !value.is_finite() {
                continue;
            }
            let axis = match letter {
                b'X' => 0,
                b'Y' => 1,
                b'Z' => 2,
                b'E' if value.abs() <= WORD_LIMIT => {
                    extruded = Some(value);
                    continue;
                }
                _ => continue,
            };
            if value.abs() <= AXIS_LIMIT {
                self.at[axis] = match self.axes {
                    Positioning::Absolute => value,
                    Positioning::Relative => self.at[axis] + value,
                };
            }
        }
        let fed = extruded.map_or(0.0, |value| {
            if self.axes == Positioning::Relative || self.extruder == Positioning::Relative {
                self.e += value;
                value
            } else {
                let fed = value - self.e;
                self.e = value;
                fed
            }
        });
        let travel = (self.at[0] - start[0]).hypot(self.at[1] - start[1]);
        if fed <= 0.0 || (travel <= TRAVEL_MM && code < 2) {
            return;
        }
        if let (Some(layer), Some(bit)) = (self.layers.last_mut(), self.current_type) {
            layer.types |= 1 << bit;
        }
        let Some(object) = self.current_object else {
            return;
        };
        let end = self.at;
        if let Some(draft) = self.objects.get_mut(&object) {
            grow(&mut draft.bounds, start[0], start[1]);
            grow(&mut draft.bounds, end[0], end[1]);
            if code >= 2 {
                for [x, y] in arc_extremes(words, code == 2, start, end) {
                    grow(&mut draft.bounds, x, y);
                }
            }
        }
    }

    /// `G28`: the axes it names, or every axis when it names none, go home.
    fn home(&mut self, words: &[Word]) {
        let named: Vec<usize> = ['X', 'Y', 'Z']
            .into_iter()
            .enumerate()
            .filter(|(_, letter)| words.iter().any(|word| char::from(word.letter) == *letter))
            .map(|(axis, _)| axis)
            .collect();
        if named.is_empty() {
            self.at = [0.0; 3];
        } else {
            for axis in named {
                self.at[axis] = 0.0;
            }
        }
    }

    /// `G92`: where the named axes are, without moving them; bare, every axis
    /// and the extruder are at zero.
    fn declare(&mut self, words: &[Word]) {
        if words.is_empty() {
            self.at = [0.0; 3];
            self.e = 0.0;
            return;
        }
        for (axis, letter) in ['X', 'Y', 'Z'].into_iter().enumerate() {
            if let Some(value) = number(words, letter).filter(|value| value.abs() <= AXIS_LIMIT) {
                self.at[axis] = value;
            }
        }
        if let Some(value) = number(words, 'E') {
            self.e = value;
        }
    }

    /// `M73`: the normal mode's estimate of the minutes left, `P` and `R`.
    fn estimate(&mut self, words: &[Word]) {
        if number(words, 'P').is_none() {
            return;
        }
        if let Some(minutes) = number(words, 'R').filter(|minutes| *minutes >= 0.0) {
            self.time_left.push(Mark {
                at: self.read,
                value: whole(minutes),
            });
        }
    }

    /// `M221 S<percent>`: the flow factor.
    fn flow_factor(&mut self, words: &[Word]) {
        if let Some(percent) = number(words, 'S').filter(|percent| *percent >= 0.0) {
            push_changed(&mut self.flow, self.read, percent / 100.0);
        }
    }

    /// `M106 S<speed>` or `M107`: the part-cooling fan, in percent.
    fn fan_speed(&mut self, words: &[Word], off: bool) {
        if number(words, 'P').is_some_and(|fan| fan != 0.0) {
            return;
        }
        let speed = if off {
            0.0
        } else {
            number(words, 'S').unwrap_or(FAN_FULL)
        };
        let percent = (speed.clamp(0.0, FAN_FULL) / FAN_FULL * 100.0).round();
        push_changed(&mut self.fan, self.read, percent);
    }

    /// `M486`: an object starting, stopping, or being named.
    fn object(&mut self, raw: &[u8], words: &[Word]) {
        let command = String::from_utf8_lossy(raw);
        let command = command.split(';').next().unwrap_or_default();
        if let Some(name) = command
            .strip_prefix("M486")
            .map(str::trim_start)
            .and_then(|rest| rest.strip_prefix('A'))
        {
            self.labelled_objects = true;
            let name: String = name
                .trim()
                .trim_matches('"')
                .chars()
                .take(NAME_LIMIT)
                .collect();
            if let Some(object) = self.current_object
                && let Some(draft) = self.objects.get_mut(&object)
                && !name.is_empty()
            {
                draft.name = Some(name);
            }
            return;
        }
        let Some(number) = number(words, 'S') else {
            return;
        };
        self.labelled_objects = true;
        if number < 0.0 {
            self.current_object = None;
            return;
        }
        let id = whole(number);
        if !self.objects.contains_key(&id) && self.objects.len() >= OBJECT_LIMIT {
            self.current_object = None;
            return;
        }
        self.objects.entry(id).or_default();
        self.current_object = Some(id);
    }
}

/// Record a value a file set, where it set it — every time, the same value
/// included, because a command repeating a value still puts it back over a
/// change made since.
fn push_changed(marks: &mut Vec<Mark<f64>>, at: u64, value: f64) {
    marks.push(Mark { at, value });
}

/// The whole number nearest a value a file wrote, which every word is
/// bounded well inside [`i64`] by.
fn whole(value: f64) -> i64 {
    format!("{:.0}", value.round()).parse().unwrap_or_default()
}

/// A length rounded to a tenth of a millimetre, which is all a bed area is
/// read to.
fn tenth(value: f64) -> f64 {
    (value * 10.0).round() / 10.0
}

/// Grow a rectangle to reach a point.
fn grow(bounds: &mut Option<[f64; 4]>, x: f64, y: f64) {
    *bounds = Some(match *bounds {
        None => [x, y, x, y],
        Some([least_x, least_y, most_x, most_y]) => {
            [least_x.min(x), least_y.min(y), most_x.max(x), most_y.max(y)]
        }
    });
}

/// The points of an arc's circle at which it reaches furthest along X or Y,
/// of the part of the circle the arc sweeps: what an arc lays beyond its two
/// ends.
///
/// The centre is the one `I` and `J` name, or for the `R` form the one on the
/// side of the chord the direction and the radius's sign choose. `G2` sweeps
/// clockwise and `G3` counter-clockwise; an arc ending where it began is the
/// whole circle.
fn arc_extremes(
    words: &[Word],
    clockwise: bool,
    start: [f64; 3],
    end: [f64; 3],
) -> impl Iterator<Item = [f64; 2]> {
    let centre = number(words, 'R').map_or_else(
        || {
            Some([
                start[0] + number(words, 'I').unwrap_or(0.0),
                start[1] + number(words, 'J').unwrap_or(0.0),
            ])
        },
        |radius| centre_of_radius(radius, clockwise, start, end),
    );
    let reach = centre.map(|centre| {
        let radius = (start[0] - centre[0]).hypot(start[1] - centre[1]);
        let from = (start[1] - centre[1]).atan2(start[0] - centre[0]);
        let to = (end[1] - centre[1]).atan2(end[0] - centre[0]);
        let tau = core::f64::consts::TAU;
        // How far the arc turns, counter-clockwise from `from`, as a positive
        // sweep: a clockwise arc is the same sweep taken from its end.
        let (base, sweep) = if clockwise {
            (to, (from - to).rem_euclid(tau))
        } else {
            (from, (to - from).rem_euclid(tau))
        };
        let sweep = if sweep < 1e-9 { tau } else { sweep };
        (0..4).filter_map(move |quarter| {
            let angle = core::f64::consts::FRAC_PI_2 * f64::from(quarter);
            ((angle - base).rem_euclid(tau) <= sweep + 1e-9).then(|| {
                [
                    centre[0] + radius * angle.cos(),
                    centre[1] + radius * angle.sin(),
                ]
            })
        })
    });
    reach.into_iter().flatten()
}

/// The centre of an `R`-form arc: on the chord's perpendicular, on the side a
/// clockwise or counter-clockwise turn of a short arc puts it, and on the other
/// side for a negative radius, which asks for the long way round. Nothing when
/// the chord is longer than the circle allows, or the arc ends where it began.
fn centre_of_radius(
    radius: f64,
    clockwise: bool,
    start: [f64; 3],
    end: [f64; 3],
) -> Option<[f64; 2]> {
    let chord = [end[0] - start[0], end[1] - start[1]];
    let length = chord[0].hypot(chord[1]);
    if length <= TRAVEL_MM || length > 2.0 * radius.abs() {
        return None;
    }
    let rise = (radius * radius - length * length / 4.0).max(0.0).sqrt();
    let left = if clockwise { -1.0 } else { 1.0 };
    let side = left * radius.signum() * rise / length;
    Some([
        start[0] + chord[0] / 2.0 - chord[1] * side,
        start[1] + chord[1] / 2.0 + chord[0] * side,
    ])
}

/// The words of a command, each a letter, the text after it and the number
/// that text is when it is one, up to [`WORD_COUNT`] of them: up to the `;`
/// a comment starts at or the `*` a checksum does.
///
/// Each number is read as its bytes go by — an optional sign, digits and at
/// most one point, which is every number a slicer writes — with plain
/// comparisons rather than calls, because this runs over every byte of every
/// move of a print and is as fast in a build without optimisation as it is
/// written to be. The digits are summed exactly and divided once by a power of
/// ten, which is the correctly rounded value a full parse answers; anything
/// else a word carries is read by the full parse.
fn words(command: &[u8]) -> Words {
    let mut found = Words {
        held: [Word {
            letter: 0,
            value: f64::NAN,
        }; WORD_COUNT],
        len: 0,
        first_code: None,
        second_code: None,
        numbered: false,
    };
    let length = command.len();
    let mut at = 0;
    while at < length {
        let letter = command[at];
        if letter == b';' || letter == b'*' {
            break;
        }
        at += 1;
        let upper = match letter {
            b'a'..=b'z' => letter - (b'a' - b'A'),
            b'A'..=b'Z' => letter,
            _ => continue,
        };
        let start = at;
        let mut negative = false;
        if at < length && (command[at] == b'-' || command[at] == b'+') {
            negative = command[at] == b'-';
            at += 1;
        }
        let mut sum = 0.0_f64;
        let mut digits = 0_usize;
        let mut fraction = 0_usize;
        let mut point = false;
        let mut plain = true;
        while at < length {
            let byte = command[at];
            let digit = match byte {
                b'0' => 0.0,
                b'1' => 1.0,
                b'2' => 2.0,
                b'3' => 3.0,
                b'4' => 4.0,
                b'5' => 5.0,
                b'6' => 6.0,
                b'7' => 7.0,
                b'8' => 8.0,
                b'9' => 9.0,
                b'.' if !point => {
                    point = true;
                    at += 1;
                    continue;
                }
                b' ' | b'\t' | b';' | b'*' | b'A'..=b'Z' | b'a'..=b'z' => break,
                _ => {
                    plain = false;
                    at += 1;
                    continue;
                }
            };
            sum = sum * 10.0 + digit;
            digits += 1;
            if point {
                fraction += 1;
            }
            at += 1;
        }
        let text = &command[start..at];
        let value = if plain && digits > 0 && digits <= MANTISSA_DIGITS {
            let value = sum / TENTHS[fraction];
            if negative { -value } else { value }
        } else {
            decimal(text).unwrap_or(f64::NAN)
        };
        // A command's code is the first word's, or the second's after a
        // line number, read as the whole number it is written as.
        if found.len == 0 {
            found.first_code = whole_number(text);
            found.numbered = upper == b'N';
        } else if found.len == 1 {
            found.second_code = whole_number(text);
        }
        if found.len < WORD_COUNT {
            found.held[found.len] = Word {
                letter: upper,
                value,
            };
            found.len += 1;
        }
    }
    found
}

/// The most digits a number is summed from exactly: every integer up to
/// fifteen digits is a `f64` exactly.
const MANTISSA_DIGITS: usize = 15;

/// One word of a command: its letter, upper-cased, and the number it carries,
/// not a number when it carries none.
#[derive(Debug, Clone, Copy)]
struct Word {
    /// The letter.
    letter: u8,
    /// The number, `NaN` when the word's text is none.
    value: f64,
}

/// One command's words, held without allocating.
struct Words {
    /// The words, the first `len` of them read.
    held: [Word; WORD_COUNT],
    /// How many were read.
    len: usize,
    /// The first word's text, as a whole number when it is one.
    first_code: Option<u32>,
    /// The second word's text, as a whole number when it is one.
    second_code: Option<u32>,
    /// Whether the first word is a line number.
    numbered: bool,
}

impl Words {
    /// The words read.
    fn as_slice(&self) -> &[Word] {
        &self.held[..self.len]
    }
}

/// The whole number a command's code is, when it is digits alone.
fn whole_number(text: &[u8]) -> Option<u32> {
    let mut value: u32 = 0;
    let mut at = 0;
    while at < text.len() {
        let byte = text[at];
        if !byte.is_ascii_digit() {
            return None;
        }
        value = value.checked_mul(10)?.checked_add(u32::from(byte - b'0'))?;
        at += 1;
    }
    (!text.is_empty()).then_some(value)
}

/// The powers of ten a decimal's fraction is divided by.
const TENTHS: [f64; 16] = [
    1.0, 1e1, 1e2, 1e3, 1e4, 1e5, 1e6, 1e7, 1e8, 1e9, 1e10, 1e11, 1e12, 1e13, 1e14, 1e15,
];

/// The number a word's text is, by the full parse: what a word the fast read
/// in [`words`] does not take is read by.
fn decimal(text: &[u8]) -> Option<f64> {
    core::str::from_utf8(text).ok()?.parse::<f64>().ok()
}

/// The number one word carries, when the command has that word and it is a
/// number within reach.
fn number(words: &[Word], letter: char) -> Option<f64> {
    let mut at = 0;
    while at < words.len() {
        let Word {
            letter: word,
            value,
        } = words[at];
        if char::from(word) == letter {
            return Some(value).filter(|value| value.is_finite() && value.abs() <= WORD_LIMIT);
        }
        at += 1;
    }
    None
}

/// Build one file's index from its pieces, in order — what the whole-file
/// read does with the pieces it is handed.
#[must_use]
pub fn index_of(pieces: &[&[u8]]) -> GcodeIndex {
    let mut builder = IndexBuilder::new();
    for piece in pieces {
        let _ = builder.take(piece);
    }
    builder.finish()
}

/// One print's index, as the cache holds it.
#[derive(Debug, Default)]
enum Slot {
    /// Nothing is built and nothing is building.
    #[default]
    Empty,
    /// A build is under way, and these are waiting for it.
    Building(Vec<Waker>),
    /// Built.
    Ready(Arc<GcodeIndex>),
}

/// Every print's index, held for as long as this process runs.
#[derive(Debug, Default)]
pub struct GcodeIndexes {
    /// One slot per print that has been asked for.
    slots: Mutex<BTreeMap<PrintId, Slot>>,
}

/// What asking for one print's index answered.
#[derive(Debug)]
pub enum Claimed<'a> {
    /// It is built.
    Ready(Arc<GcodeIndex>),
    /// Nothing is building it, and this caller is to: it holds the claim.
    Build(BuildClaim<'a>),
}

/// The one caller building a print's index. Dropping it unfinished hands the
/// build to whoever asks next, and wakes everyone waiting.
#[derive(Debug)]
pub struct BuildClaim<'a> {
    /// The cache it was claimed from.
    indexes: &'a GcodeIndexes,
    /// The print it is building for.
    print_id: PrintId,
    /// Whether it has finished.
    done: bool,
}

impl BuildClaim<'_> {
    /// Hand the built index over, waking every caller waiting for it.
    #[must_use]
    pub fn finish(mut self, index: GcodeIndex) -> Arc<GcodeIndex> {
        let index = Arc::new(index);
        self.settle(Slot::Ready(Arc::clone(&index)));
        self.done = true;
        index
    }

    /// Put the slot into its next state and wake whoever waited.
    fn settle(&self, next: Slot) {
        let waiting = {
            let mut slots = self.indexes.held();
            let slot = slots.entry(self.print_id).or_default();
            match core::mem::replace(slot, next) {
                Slot::Building(waiting) => waiting,
                Slot::Empty | Slot::Ready(_) => Vec::new(),
            }
        };
        for waker in waiting {
            waker.wake();
        }
    }
}

impl Drop for BuildClaim<'_> {
    fn drop(&mut self) {
        if !self.done {
            self.settle(Slot::Empty);
        }
    }
}

impl GcodeIndexes {
    /// The slots, held for one read or write.
    fn held(&self) -> std::sync::MutexGuard<'_, BTreeMap<PrintId, Slot>> {
        self.slots.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// One print's index when it is built, or the claim to build it, waiting
    /// while another caller is building it.
    pub const fn claim(&self, print_id: PrintId) -> Claim<'_> {
        Claim {
            indexes: self,
            print_id,
        }
    }

    /// One print's index, when it is built.
    #[must_use]
    pub fn built(&self, print_id: PrintId) -> Option<Arc<GcodeIndex>> {
        match self.held().get(&print_id) {
            Some(Slot::Ready(index)) => Some(Arc::clone(index)),
            _ => None,
        }
    }
}

/// The future [`GcodeIndexes::claim`] answers.
#[derive(Debug)]
pub struct Claim<'a> {
    /// The cache.
    indexes: &'a GcodeIndexes,
    /// The print.
    print_id: PrintId,
}

impl<'a> Future for Claim<'a> {
    type Output = Claimed<'a>;

    fn poll(self: Pin<&mut Self>, context: &mut Context<'_>) -> Poll<Self::Output> {
        let mut slots = self.indexes.held();
        let slot = slots.entry(self.print_id).or_default();
        match slot {
            Slot::Ready(index) => Poll::Ready(Claimed::Ready(Arc::clone(index))),
            Slot::Building(waiting) => {
                if !waiting.iter().any(|held| held.will_wake(context.waker())) {
                    waiting.push(context.waker().clone());
                }
                Poll::Pending
            }
            Slot::Empty => {
                *slot = Slot::Building(Vec::new());
                Poll::Ready(Claimed::Build(BuildClaim {
                    indexes: self.indexes,
                    print_id: self.print_id,
                    done: false,
                }))
            }
        }
    }
}

/// The most indexes held at once. A print's index is a few tens of kilobytes,
/// and a supervisor watches one print at a time; this bounds what a server
/// running for months holds of prints long finished.
const HELD_INDEXES: usize = 16;

impl GcodeIndexes {
    /// Let go of finished indexes beyond [`HELD_INDEXES`], keeping `keep`.
    fn trim(&self, keep: PrintId) {
        let mut slots = self.held();
        let ready: Vec<PrintId> = slots
            .iter()
            .filter(|(print_id, slot)| **print_id != keep && matches!(slot, Slot::Ready(_)))
            .map(|(print_id, _)| *print_id)
            .collect();
        let over = slots.len().saturating_sub(HELD_INDEXES);
        for print_id in ready.into_iter().take(over) {
            slots.remove(&print_id);
        }
    }
}

impl crate::supervisor::Supervisor {
    /// What the slicer planned at the job's current file position, building
    /// the print's index first when this supervisor has none for it.
    ///
    /// Nothing when the print is over, the job is not this print's file, the
    /// job reports no position, or the file could not be read — a read that
    /// failed builds nothing, so the next one tries again.
    pub(crate) async fn slicer_plan(
        &self,
        print: &crate::records::PrintRecord,
        job: Option<&printobserver_printer_api::JobSnapshot>,
        interventions: &[crate::records::Intervention],
    ) -> Option<SlicerPlan> {
        let job = job?;
        let position = job.file_position_bytes?;
        let file = print.file_name.as_deref()?;
        if print.ended_at.is_some() || job.file_name.as_deref() != Some(file) {
            return None;
        }
        let index = self.index_of(print.id, file).await?;
        let (flow, fan) = self
            .changes_since(print.id, &index, position, interventions)
            .await;
        Some(index.plan_with(position, flow, fan))
    }

    /// One print's index, built from its file when there is none yet.
    async fn index_of(&self, print_id: PrintId, file: &str) -> Option<Arc<GcodeIndex>> {
        // A name no file could carry is no file to read.
        let file_name = printobserver_types::FileName::new(file).ok()?;
        let claim = match self.gcode_indexes().claim(print_id).await {
            Claimed::Ready(index) => return Some(index),
            Claimed::Build(claim) => claim,
        };
        let mut builder = IndexBuilder::new();
        self.read_gcode(file_name.clone(), &mut builder)
            .await
            .ok()?;
        let index = claim.finish(builder.finish());
        self.gcode_indexes().trim(print_id);
        if let Ok(body) = printobserver_types::EventBody::of(&crate::kinds::GcodeIndexedPayload {
            file_name,
            bytes: index.bytes(),
            layers: u64::try_from(index.layer_count()).unwrap_or(u64::MAX),
        }) {
            // The index is built whether or not the store takes the record of
            // it, and a check goes on without it.
            let _ = self.append_system_event(print_id, body).await;
        }
        Some(index)
    }

    /// The flow factor and the fan speed the latest change made through this
    /// program set, each when it was made after the file last set the same
    /// thing and still stands.
    ///
    /// A change is placed against the file by the position the print had last
    /// been read to have reached when it was made, at the file's start when no
    /// position had been read yet; a change whose bounded intervention has
    /// since expired stands no more.
    async fn changes_since(
        &self,
        print_id: PrintId,
        index: &GcodeIndex,
        position: i64,
        interventions: &[crate::records::Intervention],
    ) -> (Option<f64>, Option<f64>) {
        use printobserver_types::EventPayload as _;

        use crate::kinds::{ActionExecutedPayload, ActionRequestedPayload};
        use crate::records::PrintAction;

        let kinds = [ActionRequestedPayload::KIND, ActionExecutedPayload::KIND]
            .iter()
            .filter_map(|kind| printobserver_types::EventKind::new(*kind).ok())
            .collect();
        let Ok(events) = self
            .stores()
            .events
            .history(crate::store::HistoryQuery {
                print_id,
                kinds,
                since: None,
                until: None,
                limit: Some(crate::store::MAX_HISTORY_LIMIT),
            })
            .await
        else {
            return (None, None);
        };
        let requested: BTreeMap<crate::records::ActionId, PrintAction> = events
            .iter()
            .filter_map(printobserver_types::EventRecord::payload_as::<ActionRequestedPayload>)
            .filter_map(Result::ok)
            .map(|payload| (payload.action_id, payload.action))
            .collect();
        let mut flow = None;
        let mut fan = None;
        // Newest first, so the first change of each kind is the latest.
        for executed in events
            .iter()
            .filter_map(printobserver_types::EventRecord::payload_as::<ActionExecutedPayload>)
            .filter_map(Result::ok)
        {
            let standing = executed
                .intervention_id
                .is_none_or(|opened| interventions.iter().any(|active| active.id == opened));
            // Where in the file the change was made; one made before any
            // position was known is placed at the file's start.
            let made = executed
                .file_position_bytes
                .and_then(|made| u64::try_from(made).ok())
                .unwrap_or(0);
            let after = |set: Option<SetInFile>| set.is_none_or(|set| made >= set.at);
            match requested.get(&executed.action_id) {
                Some(PrintAction::SetFlowrateFactor { factor, .. }) if flow.is_none() => {
                    flow = Some((standing && after(index.flow_at(position))).then_some(*factor));
                }
                Some(PrintAction::SetFanPercent { percent, .. }) if fan.is_none() => {
                    fan = Some((standing && after(index.fan_at(position))).then_some(*percent));
                }
                _ => {}
            }
        }
        (flow.flatten(), fan.flatten())
    }
}
