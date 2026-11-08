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
use crate::stub::{Reply, Stub};

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

/// A file's facts naming its download, as `OctoPrint` answers them.
const FACTS: &str = r#"{"name": "held.gcode", "origin": "local", "type": "machinecode",
    "size": 4096, "refs": {"download": "http://ignored/downloads/files/local/held.gcode"}}"#;

/// A stub answering a file's facts as `OctoPrint` does, and its download as
/// one journey scripts it.
fn downloading(download: Reply) -> Stub {
    Stub::start(move |request| {
        if request.path.starts_with("/downloads/") {
            download.clone()
        } else {
            Reply::Body(200, FACTS.to_owned())
        }
    })
}

/// A printer configured against a stub.
fn printer_of_stub(stub: &Stub) -> OctoPrintPrinter {
    OctoPrintPrinter::new(
        OctoPrintConfig::new(&stub.base_url(), "test-key").expect("a configuration"),
    )
}

/// A download the instance refuses is answered as that refusal, in the
/// instance's own words, and the sink is handed nothing.
#[test]
fn a_refused_download_is_answered_as_the_instances_refusal() {
    let stub = downloading(Reply::Body(500, "the disk is gone".to_owned()));
    let mut sink = Kept::everything();

    let read = block_on(printer_of_stub(&stub).read_gcode(named("held.gcode"), &mut sink));

    assert!(
        matches!(&read, Err(PrinterError::Refused { status: 500, detail }) if detail.contains("the disk is gone")),
        "{read:?}"
    );
    assert!(sink.bytes.is_empty());
}

/// A download that ends before the length it declared is answered as the
/// connection dropping part-way, naming how much of the file arrived, rather
/// than as a whole file.
#[test]
fn a_download_cut_short_is_answered_as_the_connection_dropping() {
    let stub = downloading(Reply::Raw(
        b"HTTP/1.1 200 OK\r\nContent-Length: 4096\r\nConnection: close\r\n\r\nG28\nG1 X1".to_vec(),
    ));
    let mut sink = Kept::everything();

    let read = block_on(printer_of_stub(&stub).read_gcode(named("held.gcode"), &mut sink));

    assert!(
        matches!(&read, Err(PrinterError::Unreachable { detail }) if detail.contains("promised 4096 bytes and delivered 9")),
        "{read:?}"
    );
}

/// An instance nobody is listening at is answered as unreachable.
#[test]
fn an_instance_that_is_not_there_is_unreachable() {
    let address = {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("a loopback port");
        listener.local_addr().expect("the bound address")
    };
    let printer = OctoPrintPrinter::new(
        OctoPrintConfig::new(&format!("http://{address}"), "test-key").expect("a configuration"),
    );
    let mut sink = Kept::everything();

    let read = block_on(printer.read_gcode(named("held.gcode"), &mut sink));

    assert!(
        matches!(read, Err(PrinterError::Unreachable { .. })),
        "{read:?}"
    );
}
