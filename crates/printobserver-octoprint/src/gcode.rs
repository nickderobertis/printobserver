//! The filament a file's own G-code feeds each tool, and where it printed, up
//! to a byte of it.
//!
//! `OctoPrint` measures no extrusion. It knows the slicer's analysed length and,
//! while a job runs, how many bytes of the file it has sent, so the filament a
//! print that stopped part-way actually used is the filament the G-code it sent
//! asked for — which is read here, from the file, rather than guessed as a
//! fraction of its bytes.
//!
//! This is printer language rather than `OctoPrint`'s, and it lives in this
//! crate because the download it reads lives here and nothing else needs it.
//!
//! # What is read
//!
//! * `G0`, `G1`, `G2` and `G3` with an `E` word feed the selected tool.
//! * `M82` makes `E` absolute and `M83` relative. `G91` makes it relative too,
//!   and `G90` puts back whatever `M82`/`M83` last said — which is how
//!   `OctoPrint`'s own analysis reads them by default, and how the firmware this
//!   targets does.
//! * `G92` with an `E` word sets the selected tool's position; a bare `G92`
//!   sets it to zero.
//! * `T<n>` selects tool `n`; each tool keeps a position of its own.
//! * Comments — `;` to the end of the line, and `( … )` — are not commands, a
//!   leading `N<line number>` is not a word, and `*<checksum>` ends the line.
//!
//! Feeding is counted **net**: a retraction subtracts and the unretraction that
//! follows adds it back, so the figure is the filament that left the spool.
//!
//! # Where it printed
//!
//! Beside the feed, the reader tracks where the head is in X, Y and Z, so that
//! what a file printed up to a position has a height and a footprint
//! ([`PrintedExtent`]). `G90`/`G91` make the axes absolute or relative, `G92`
//! declares where the named axes are without moving them, and `G28` homes the
//! axes it names — every axis when it names none — to zero. A `G0`–`G3` that
//! feeds filament while it travels in X or Y laid material along its segment,
//! so the higher of its two ends counts towards the height and both ends
//! towards the footprint; a feed with no XY travel — an unretraction, a prime —
//! laid nothing along a path. An arc is bounded by its two ends, not by the
//! curve between them.
//!
//! # What is deliberately not read
//!
//! Flow-factor commands (`M221`) are ignored: the figure is the filament the
//! file asks for, and a charge that applies a flow factor states that as its
//! own method rather than having it folded in here.
//!
//! # Where a position falls
//!
//! A line counts when every byte of it, its newline included, lies before the
//! position — which is how `OctoPrint` reports a file position: the offset
//! after the last line it read. A line the position cuts through has not been
//! sent and does not count. A file's last line needs no newline to count once
//! the position reaches the end of the file.

use core::ops::ControlFlow;
use std::collections::BTreeMap;

use printobserver_printer_api::{Footprint, PrintedExtent, ToolFilament};

/// The longest line this reader holds; anything longer is no command it reads.
///
/// G-code lines are tens of bytes. A bound keeps a file that is not G-code from
/// being held in memory a line at a time.
const LINE_LIMIT: usize = 4096;

/// The largest `E` word read as a length, in millimetres: ten kilometres of
/// filament, far past any spool. A word beyond it is no position a file feeds
/// to, and summing such words could reach infinity rather than a figure.
const E_LIMIT: f64 = 1e7;

/// The largest axis word read as a place, in millimetres: ten metres, far
/// past any machine's travel. A word beyond it is no place a head can be.
const AXIS_LIMIT: f64 = 1e4;

/// The XY travel below which a move counts as no travel at all: a thousandth
/// of a micron, far under any step a printer can make.
const TRAVEL_MM: f64 = 1e-6;

/// The three axes this reader tracks, in the order it holds them.
const AXES: [char; 3] = ['X', 'Y', 'Z'];

/// How a coordinate is read: as a place, or as a distance from the last one.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
enum Positioning {
    /// A place: `M82` for `E`, `G90` for the axes. What a firmware starts in.
    #[default]
    Absolute,
    /// A distance: `M83` for `E`, `G91` for the axes.
    Relative,
}

/// The interpreter's state across a file: the tool, the modes, and every
/// tool's position and net feed.
#[derive(Debug, Default, Clone)]
pub struct Extrusion {
    /// The selected tool.
    tool: i64,
    /// What `M82`/`M83` last said about `E`.
    extruder: Positioning,
    /// What `G90`/`G91` last said; `G91` makes `E` relative whatever `M82`
    /// said, and `G90` hands `E` back to it.
    axes: Positioning,
    /// Every tool's current `E` position.
    positions: BTreeMap<i64, f64>,
    /// Every tool's net feed, in millimetres.
    fed: BTreeMap<i64, f64>,
    /// Where the head is in X, Y and Z, in millimetres.
    at: [f64; 3],
    /// What the extruding moves read so far laid down, when any did.
    extent: Option<PrintedExtent>,
}

impl Extrusion {
    /// An interpreter that has read nothing.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Read one line, without its newline.
    pub fn line(&mut self, raw: &[u8]) {
        let words = words(raw);
        let Some((letter, code)) = words.first() else {
            return;
        };
        let number = code.parse::<u32>().ok();
        match (*letter, number) {
            ('G', Some(0..=3)) => {
                let fed = self.feed(&words[1..]);
                self.travel(&words[1..], fed);
            }
            ('G', Some(28)) => self.home(&words[1..]),
            ('G', Some(90)) => self.axes = Positioning::Absolute,
            ('G', Some(91)) => self.axes = Positioning::Relative,
            ('G', Some(92)) => {
                self.reset(&words[1..]);
                self.declare(&words[1..]);
            }
            ('M', Some(82)) => self.extruder = Positioning::Absolute,
            ('M', Some(83)) => self.extruder = Positioning::Relative,
            ('T', Some(tool)) => self.tool = i64::from(tool),
            _ => {}
        }
    }

    /// The net feed of every tool that was fed, in tool order.
    #[must_use]
    pub fn fed(&self) -> Vec<ToolFilament> {
        self.fed
            .iter()
            .map(|(tool, length)| ToolFilament {
                tool: *tool,
                length_mm: *length,
                volume_cm3: None,
            })
            .collect()
    }

    /// What the extruding moves read so far laid down, when any did.
    #[must_use]
    pub const fn extent(&self) -> Option<PrintedExtent> {
        self.extent
    }

    /// One move's axis words: the head goes where they say, and a move that
    /// fed filament while it travelled in X or Y counts towards the extent.
    fn travel(&mut self, words: &[(char, String)], fed: f64) {
        let start = self.at;
        for (index, axis) in AXES.iter().enumerate() {
            let Some(value) = axis_of(words, *axis) else {
                continue;
            };
            let moved = match self.axes {
                Positioning::Relative => self.at[index] + value,
                Positioning::Absolute => value,
            };
            // A relative move summing past any travel is no place a head can
            // be, and is read as though it were absent.
            if moved.abs() <= AXIS_LIMIT {
                self.at[index] = moved;
            }
        }
        let travelled = (self.at[0] - start[0]).hypot(self.at[1] - start[1]) > TRAVEL_MM;
        if fed <= 0.0 || !travelled {
            return;
        }
        // A segment that descends while it extrudes laid material at its
        // start too, so the higher of its two ends counts.
        let high = start[2].max(self.at[2]);
        let footprint = self.extent.map_or(
            Footprint {
                min_x: start[0],
                min_y: start[1],
                max_x: start[0],
                max_y: start[1],
            },
            |extent| grown(extent.footprint, start[0], start[1]),
        );
        self.extent = Some(PrintedExtent {
            tallest_mm: self
                .extent
                .map_or(high, |extent| extent.tallest_mm.max(high)),
            footprint: grown(footprint, self.at[0], self.at[1]),
        });
    }

    /// `G28`: the axes it names go to zero, every axis when it names none.
    fn home(&mut self, words: &[(char, String)]) {
        let named: Vec<usize> = words
            .iter()
            .filter_map(|(letter, _)| AXES.iter().position(|axis| axis == letter))
            .collect();
        if named.is_empty() {
            self.at = [0.0; 3];
        }
        for index in named {
            self.at[index] = 0.0;
        }
    }

    /// `G92`: each axis it names is where it says, without moving.
    fn declare(&mut self, words: &[(char, String)]) {
        for (index, axis) in AXES.iter().enumerate() {
            if let Some(value) = axis_of(words, *axis) {
                self.at[index] = value;
            }
        }
    }

    /// One move's `E` word, when it has one, answering how far it fed.
    fn feed(&mut self, words: &[(char, String)]) -> f64 {
        let Some(e) = number_of(words, 'E') else {
            return 0.0;
        };
        let position = self.positions.entry(self.tool).or_insert(0.0);
        let delta = if self.extruder == Positioning::Relative || self.axes == Positioning::Relative
        {
            e
        } else {
            e - *position
        };
        *position += delta;
        *self.fed.entry(self.tool).or_insert(0.0) += delta;
        delta
    }

    /// `G92`: the selected tool's position is what it says, or zero.
    fn reset(&mut self, words: &[(char, String)]) {
        let to = if words.is_empty() {
            Some(0.0)
        } else {
            number_of(words, 'E')
        };
        if let Some(to) = to {
            self.positions.insert(self.tool, to);
        }
    }
}

/// The number one word carries, when the line has that word and it is a
/// length: a word that is no number, or one past [`E_LIMIT`], is skipped, and
/// the move is read as though it carried none.
fn number_of(words: &[(char, String)], letter: char) -> Option<f64> {
    words
        .iter()
        .find(|(found, _)| *found == letter)
        .and_then(|(_, value)| value.parse::<f64>().ok())
        .filter(|value| value.abs() <= E_LIMIT)
}

/// The place one axis word carries, when the line has that word and it is a
/// finite number within [`AXIS_LIMIT`].
fn axis_of(words: &[(char, String)], letter: char) -> Option<f64> {
    words
        .iter()
        .find(|(found, _)| *found == letter)
        .and_then(|(_, value)| value.parse::<f64>().ok())
        .filter(|value| value.abs() <= AXIS_LIMIT)
}

/// A footprint grown to hold one more point.
const fn grown(footprint: Footprint, x: f64, y: f64) -> Footprint {
    Footprint {
        min_x: footprint.min_x.min(x),
        min_y: footprint.min_y.min(y),
        max_x: footprint.max_x.max(x),
        max_y: footprint.max_y.max(y),
    }
}

/// One line's words, each a letter and the text after it, with its line
/// number, checksum and comments gone.
///
/// Read whether or not the words are spaced, because `G1X10E2` is a line a
/// firmware reads exactly as `G1 X10 E2`.
///
/// A number is one run of characters: a space ends it, so `E1 2` is not `E12`
/// but an `E` word that is no number, which the move is then read without.
fn words(raw: &[u8]) -> Vec<(char, String)> {
    /// What no number is spelled with, so that a value carrying it never parses.
    const BROKEN: char = ' ';
    let mut words: Vec<(char, String)> = Vec::new();
    let mut in_parenthesis = false;
    let mut ended = false;
    for &byte in raw {
        let c = char::from(byte);
        if in_parenthesis {
            in_parenthesis = c != ')';
            continue;
        }
        match c {
            ';' | '*' => break,
            '(' => in_parenthesis = true,
            c if c.is_ascii_alphabetic() => {
                words.push((c.to_ascii_uppercase(), String::new()));
                ended = false;
            }
            c if c.is_ascii_whitespace() => ended = true,
            c => {
                if let Some((_, value)) = words.last_mut() {
                    if ended && !value.is_empty() {
                        value.push(BROKEN);
                    }
                    value.push(c);
                }
                ended = false;
            }
        }
    }
    if words.first().is_some_and(|(letter, _)| *letter == 'N') {
        words.remove(0);
    }
    words
}

/// A file read a piece at a time, up to a byte position in it.
///
/// Fed whatever the download delivers, in order; it says when it has read as far
/// as the position, so the rest of the file need never arrive.
#[derive(Debug, Clone)]
pub struct ExtrudedTo {
    /// What every complete line before the position has been read into.
    extrusion: Extrusion,
    /// How many bytes before the position have not arrived yet.
    remaining: u64,
    /// The line being read, up to the bytes that have arrived.
    line: Vec<u8>,
    /// Whether the line being read is longer than any this reader holds.
    overlong: bool,
    /// Whether the position has been reached part-way through a line, so that
    /// one more byte says the line goes on past it and the end of the file says
    /// it does not.
    awaiting_end: bool,
}

impl ExtrudedTo {
    /// A reader stopping at a byte position; a negative one reads nothing.
    #[must_use]
    pub fn new(position_bytes: i64) -> Self {
        Self {
            extrusion: Extrusion::new(),
            remaining: u64::try_from(position_bytes).unwrap_or(0),
            line: Vec::new(),
            overlong: false,
            awaiting_end: false,
        }
    }

    /// Read the next bytes of the file, and say whether to go on.
    pub fn take(&mut self, bytes: &[u8]) -> ControlFlow<()> {
        if self.awaiting_end {
            // The line goes on past the position, so it was not sent.
            if !bytes.is_empty() {
                self.drop_line();
                return ControlFlow::Break(());
            }
            return ControlFlow::Continue(());
        }
        let budget = usize::try_from(self.remaining).unwrap_or(usize::MAX);
        let within = bytes.len().min(budget);
        for &byte in &bytes[..within] {
            self.push(byte);
        }
        self.remaining -= u64::try_from(within).unwrap_or(self.remaining);
        if self.remaining > 0 {
            return ControlFlow::Continue(());
        }
        if self.line.is_empty() && !self.overlong {
            return ControlFlow::Break(());
        }
        if bytes.len() > within {
            self.drop_line();
            return ControlFlow::Break(());
        }
        self.awaiting_end = true;
        ControlFlow::Continue(())
    }

    /// The file ended: a last line with no newline counts.
    pub fn end(&mut self) {
        self.finish_line();
    }

    /// The net feed of every tool, up to the position.
    #[must_use]
    pub fn fed(&self) -> Vec<ToolFilament> {
        self.extrusion.fed()
    }

    /// What the file printed up to the position, when any move extruded.
    #[must_use]
    pub const fn extent(&self) -> Option<PrintedExtent> {
        self.extrusion.extent()
    }

    /// One byte: a newline completes the line, and a line past
    /// [`LINE_LIMIT`] stops being held and is marked to be skipped.
    fn push(&mut self, byte: u8) {
        if byte == b'\n' {
            self.finish_line();
        } else if self.line.len() < LINE_LIMIT {
            self.line.push(byte);
        } else {
            self.overlong = true;
        }
    }

    /// The line being read is complete: read it, unless it outgrew
    /// [`LINE_LIMIT`] — only its first bytes were held, and a command read from
    /// a prefix could be a different command.
    fn finish_line(&mut self) {
        if !self.overlong {
            self.extrusion.line(&self.line);
        }
        self.drop_line();
    }

    /// Forget the line being read, whether it was read or never will be.
    fn drop_line(&mut self) {
        self.line.clear();
        self.overlong = false;
        self.awaiting_end = false;
    }
}

/// The net feed a whole text of G-code asks each tool for, up to a position.
///
/// The same reader the download is fed through, handed the text in one piece.
#[must_use]
pub fn extruded_to(text: &[u8], position_bytes: i64) -> Vec<ToolFilament> {
    let mut reader = ExtrudedTo::new(position_bytes);
    if reader.take(text).is_continue() {
        reader.end();
    }
    reader.fed()
}

/// What a whole text of G-code printed up to a position: the same reader the
/// download is fed through, handed the text in one piece.
#[must_use]
pub fn printed_extent(text: &[u8], position_bytes: i64) -> Option<PrintedExtent> {
    let mut reader = ExtrudedTo::new(position_bytes);
    if reader.take(text).is_continue() {
        reader.end();
    }
    reader.extent()
}
