//! `read_gcode` streams a file's whole G-code into a sink, as it arrives.
//!
//! The file is a real `PrusaSlicer` recording held by the scripted `OctoPrint`
//! and downloaded from it over a real socket, so what is asserted is what the
//! far side served: the bytes the sink was handed, how many downloads the
//! stand-in answered from the first byte, and how many bytes it wrote.

use core::ops::ControlFlow;

use printobserver_octoprint::standin::ScriptedOctoPrint;
use printobserver_octoprint::{OctoPrintConfig, OctoPrintPrinter};
use printobserver_printer_api::{GcodeSink, PrinterError, PrinterPort as _};
use printobserver_types::FileName;

use crate::block_on::block_on;

/// A recorded `PrusaSlicer` print: this crate's plain copy, because the cases
/// under `tests/real-prints/` are Git LFS pointers until the real-print suite
/// fetches them.
const RECORDED: &[u8] = include_bytes!("../fixtures/real-prints/under-extrusion-lace.clean.gcode");

/// A sink keeping every piece it is handed, and how many pieces there were,
/// that stops once it holds `enough` bytes when it is given a bound.
struct Kept {
    /// What it was handed, in order.
    bytes: Vec<u8>,
    /// How many pieces it was handed.
    pieces: usize,
    /// How many bytes it wants at most.
    enough: Option<usize>,
}

impl Kept {
    /// A sink keeping everything.
    const fn everything() -> Self {
        Self {
            bytes: Vec::new(),
            pieces: 0,
            enough: None,
        }
    }
}

impl GcodeSink for Kept {
    fn take(&mut self, piece: &[u8]) -> ControlFlow<()> {
        self.bytes.extend_from_slice(piece);
        self.pieces += 1;
        match self.enough {
            Some(enough) if self.bytes.len() >= enough => ControlFlow::Break(()),
            _ => ControlFlow::Continue(()),
        }
    }
}

/// A printer configured against the stand-in.
fn printer_of(standin: &ScriptedOctoPrint) -> OctoPrintPrinter {
    OctoPrintPrinter::new(
        OctoPrintConfig::new(&standin.base_url(), standin.api_key()).expect("a configuration"),
    )
}

/// A file name.
fn named(name: &str) -> FileName {
    FileName::new(name).expect("a file name")
}

/// The whole recording reaches the sink byte for byte, in more than one piece,
/// through one download the stand-in answered from the first byte.
#[test]
fn a_recorded_file_is_streamed_whole_into_the_sink() {
    let standin = ScriptedOctoPrint::start();
    standin.holds_file("lace-test.gcode", RECORDED.to_vec(), 1_000.0);
    let mut sink = Kept::everything();

    let read = block_on(printer_of(&standin).read_gcode(named("lace-test.gcode"), &mut sink));

    assert_eq!(read, Ok(()));
    assert!(
        sink.bytes == RECORDED,
        "the sink was handed {} bytes of a {}-byte file, or different ones",
        sink.bytes.len(),
        RECORDED.len()
    );
    assert!(
        sink.pieces > 1,
        "the file arrived in one piece, which is a file held whole"
    );
    assert_eq!(standin.whole_downloads("lace-test.gcode"), 1);
    assert_eq!(standin.download_bytes(), RECORDED.len());
}

/// A sink that has enough stops the read: it is handed no piece after it said
/// so, and the rest of the file is never asked for again.
#[test]
fn a_sink_that_has_enough_stops_the_read() {
    let standin = ScriptedOctoPrint::start();
    standin.holds_file("lace-test.gcode", RECORDED.to_vec(), 1_000.0);
    let mut sink = Kept {
        enough: Some(1),
        ..Kept::everything()
    };

    let read = block_on(printer_of(&standin).read_gcode(named("lace-test.gcode"), &mut sink));

    assert_eq!(read, Ok(()));
    assert_eq!(
        sink.pieces, 1,
        "the sink was handed a piece after it stopped"
    );
    assert!(sink.bytes.len() < RECORDED.len());
    assert_eq!(standin.whole_downloads("lace-test.gcode"), 1);
}

/// A file the instance does not hold is refused with the instance's own
/// status, and the sink is handed nothing.
#[test]
fn a_file_the_instance_does_not_hold_is_refused_and_the_sink_handed_nothing() {
    let standin = ScriptedOctoPrint::start();
    let mut sink = Kept::everything();

    let read = block_on(printer_of(&standin).read_gcode(named("absent.gcode"), &mut sink));

    assert!(
        matches!(read, Err(PrinterError::Refused { status: 404, .. })),
        "{read:?}"
    );
    assert!(sink.bytes.is_empty());
    assert_eq!(standin.whole_downloads("absent.gcode"), 0);
}
