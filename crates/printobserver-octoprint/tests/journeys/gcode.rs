//! The filament a file's own G-code feeds each tool, up to a byte of it.
//!
//! Two kinds of evidence. Small hand-written programs, each carrying one rule
//! the reader has to get right, with the expected figure worked by hand. And
//! every real `PrusaSlicer` file under `tests/real-prints/`, whose own trailing
//! `; filament used [mm]` is a figure computed by the slicer rather than by
//! this reader — an independent one to agree with.

use std::path::{Path, PathBuf};

use printobserver_octoprint::{ExtrudedTo, extruded_to};
use printobserver_printer_api::ToolFilament;

/// How far a whole-file sum may stand from the slicer's own figure.
const SLICER_TOLERANCE: f64 = 0.005;

/// How far apart two sums of the same floats taken in a different order may be.
const ROUNDING: f64 = 1e-9;

/// Each tool's net feed, as `(tool, millimetres)`.
fn lengths(fed: &[ToolFilament]) -> Vec<(i64, f64)> {
    fed.iter().map(|tool| (tool.tool, tool.length_mm)).collect()
}

/// The net feed matches, tool for tool, to within rounding.
fn feeds(found: &[ToolFilament], expected: &[(i64, f64)]) {
    let found = lengths(found);
    assert_eq!(
        found.len(),
        expected.len(),
        "fed {found:?}, expected {expected:?}"
    );
    for ((tool, length), (want_tool, want_length)) in found.iter().zip(expected) {
        assert_eq!(tool, want_tool, "fed {found:?}, expected {expected:?}");
        assert!(
            (length - want_length).abs() < ROUNDING,
            "tool {tool} fed {length} mm, expected {want_length} mm"
        );
    }
}

/// The whole of a text.
fn whole(text: &str) -> Vec<ToolFilament> {
    extruded_to(text.as_bytes(), i64::MAX)
}

/// Absolute and relative `E`, with a `G92 E0` reset between.
#[test]
fn absolute_and_relative_extrusion_across_a_reset() {
    let text = "M82 ; absolute\nG92 E0\nG1 X1 E2.0\nG1 X2 E5.0\nG92 E0\nG1 X3 E1.5\n\
                M83 ; relative\nG1 X4 E2\nG1 X5 E0.5\n";

    // 2 + 3, then 1.5 from the reset, then 2 + 0.5 relative.
    feeds(&whole(text), &[(0, 9.0)]);
}

/// `G91` makes `E` relative, and `G90` puts back the absolute `M82` set.
#[test]
fn relative_axes_carry_extrusion_with_them() {
    let text = "M82\nG92 E0\nG91\nG1 E2\nG1 E2\nG90\nG1 E5\n";

    // 2 + 2 relative, then absolute 5 from a position of 4.
    feeds(&whole(text), &[(0, 5.0)]);
}

/// A retraction and its unretraction net to nothing, in either mode.
#[test]
fn a_retraction_and_its_unretraction_net_to_zero() {
    feeds(
        &whole("M83\nG1 X1 E1\nG1 E-0.8\nG0 X9\nG1 E0.8\n"),
        &[(0, 1.0)],
    );
    feeds(
        &whole("M82\nG92 E0\nG1 X1 E5\nG1 E4.2\nG0 X9\nG1 E5\n"),
        &[(0, 5.0)],
    );
}

/// Up to a retraction, the retraction is counted: the figure is net.
#[test]
fn a_position_between_a_retraction_and_its_unretraction_counts_the_retraction() {
    let text = "M83\nG1 X1 E1\nG1 E-0.8\nG1 E0.8\n";
    let after_retraction =
        i64::try_from(text.find("G1 E0.8").expect("the unretraction")).expect("a position");

    feeds(&extruded_to(text.as_bytes(), after_retraction), &[(0, 0.2)]);
}

/// Two tools are charged separately across a `T1`, each with a position of its
/// own.
#[test]
fn two_tools_are_charged_separately() {
    let relative = "M83\nT0\nG1 X1 E5\nT1\nG1 X2 E3\nG1 X3 E4\nT0\nG1 X4 E1\n";
    feeds(&whole(relative), &[(0, 6.0), (1, 7.0)]);

    let absolute = "M82\nT0\nG92 E0\nG1 E5\nT1\nG92 E0\nG1 E3\nT0\nG1 E6\n";
    feeds(&whole(absolute), &[(0, 6.0), (1, 3.0)]);
}

/// Line numbers and checksums are not words, spaced or not.
#[test]
fn checksummed_and_line_numbered_lines_are_read() {
    let text = "N1 M83*17\nN2 G1 X1 E2*96\nN3 G1X2E1.5*12\nN4 G1 E0.5 *99\nn5 g1 e1*3\n";

    feeds(&whole(text), &[(0, 5.0)]);
}

/// Neither kind of comment is a command, and a word inside one is not read.
#[test]
fn comments_are_not_commands() {
    let text = "M83\n; G1 E100\nG1 X1 E1 ; E50\nG1 X2 (E50) E1\n(G1 E100)\n;T1\nG1 E1\n";

    feeds(&whole(text), &[(0, 3.0)]);
}

/// Arcs and rapid moves feed too, and `M221` changes nothing here.
#[test]
fn arcs_feed_and_flow_factors_are_ignored() {
    let text = "M83\nM221 S50\nG2 X1 Y1 I1 J0 E2\nG3 X0 Y0 I-1 J0 E1\nG0 X5 E0.5\n";

    feeds(&whole(text), &[(0, 3.5)]);
}

/// A line counts once its newline is before the position; a position inside
/// a line does not count it; one beyond the end is the whole file.
#[test]
fn a_position_counts_the_lines_wholly_before_it() {
    // Lines end at bytes 4, 10, 16 and 22.
    let text = "M83\nG1 E1\nG1 E2\nG1 E4\n";
    let at = |position| lengths(&extruded_to(text.as_bytes(), position));

    assert_eq!(at(9), vec![], "the first move's newline is not yet read");
    assert_eq!(at(10), vec![(0, 1.0)]);
    assert_eq!(at(13), vec![(0, 1.0)], "a position mid-line");
    assert_eq!(at(16), vec![(0, 3.0)]);
    assert_eq!(at(22), vec![(0, 7.0)]);
    assert_eq!(at(1_000), vec![(0, 7.0)], "a position past the end");
    assert_eq!(at(-5), vec![], "a negative position reads nothing");
}

/// A last line with no newline counts once the position reaches the end.
#[test]
fn a_last_line_without_a_newline_counts_at_the_end() {
    let text = "M83\nG1 E1\nG1 E4";

    assert_eq!(lengths(&extruded_to(text.as_bytes(), 14)), vec![(0, 1.0)]);
    assert_eq!(lengths(&extruded_to(text.as_bytes(), 15)), vec![(0, 5.0)]);
}

/// A line longer than any G-code is not read, and does not swallow the next.
#[test]
fn an_overlong_line_is_not_a_command() {
    let text = format!("M83\nG1 E1 {}\nG1 E2\n", "X".repeat(10_000));

    feeds(&whole(&text), &[(0, 2.0)]);
}

/// The real files under `tests/real-prints/`.
fn real_prints() -> Vec<PathBuf> {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .join("tests")
        .join("real-prints");
    let mut files = Vec::new();
    for case in std::fs::read_dir(&root).expect("tests/real-prints") {
        let case = case.expect("a case").path();
        if !case.is_dir() {
            continue;
        }
        for entry in std::fs::read_dir(&case).expect("a case directory") {
            let path = entry.expect("an entry").path();
            if path
                .extension()
                .is_some_and(|extension| extension == "gcode")
            {
                files.push(path);
            }
        }
    }
    files.sort();
    files
}

/// The slicer's own figure for a whole file.
fn slicer_figure(text: &str) -> f64 {
    text.lines()
        .find_map(|line| line.strip_prefix("; filament used [mm] = "))
        .expect("PrusaSlicer writes the filament it used")
        .trim()
        .parse()
        .expect("the figure is a number")
}

/// Every real file's whole-file sum agrees with its slicer, and half of its
/// bytes feed strictly part of it.
#[test]
fn every_real_print_agrees_with_its_slicer() {
    let files = real_prints();
    assert!(
        files.len() >= 4,
        "found only {files:?} under tests/real-prints"
    );
    for path in files {
        let bytes = std::fs::read(&path).expect("the file");
        let figure = slicer_figure(&String::from_utf8_lossy(&bytes));
        let length = i64::try_from(bytes.len()).expect("a length");

        let all = lengths(&extruded_to(&bytes, length));
        assert_eq!(all.len(), 1, "{} feeds one tool: {all:?}", path.display());
        let sum = all[0].1;
        assert!(
            ((sum - figure) / figure).abs() <= SLICER_TOLERANCE,
            "{} sums to {sum} mm and its slicer says {figure} mm",
            path.display()
        );

        let half = lengths(&extruded_to(&bytes, length / 2));
        let partial = half.first().map_or(0.0, |(_, length)| *length);
        assert!(
            partial > 0.0 && partial < figure,
            "{} feeds {partial} mm by half its bytes, against {figure} mm in all",
            path.display()
        );
    }
}

/// Fed a piece at a time, the reader answers what it answers fed whole —
/// whatever byte a piece happens to end on.
#[test]
fn a_file_fed_in_pieces_reads_as_one_fed_whole() {
    let path = real_prints().into_iter().next().expect("a real print");
    let bytes = std::fs::read(&path).expect("the file");
    let position = i64::try_from(bytes.len() / 3).expect("a position");

    let whole = lengths(&extruded_to(&bytes, position));
    for piece in [1, 7, 4096] {
        let mut reader = ExtrudedTo::new(position);
        let mut stopped = false;
        for chunk in bytes.chunks(piece) {
            if reader.take(chunk).is_break() {
                stopped = true;
                break;
            }
        }
        assert!(
            stopped,
            "the reader read past its position in {piece}-byte pieces"
        );
        assert_eq!(lengths(&reader.fed()), whole, "in {piece}-byte pieces");
    }
}

/// A bare `G92` puts the selected tool's position back to zero.
#[test]
fn a_bare_reset_zeroes_the_position() {
    feeds(&whole("M82\nG1 E4\nG92\nG1 E1\n"), &[(0, 5.0)]);
}

/// A piece ending exactly on a position inside a line leaves the reader asking
/// whether the line goes on: the next byte says it does, so it is not counted,
/// and the end of the file says it does not, so it is.
#[test]
fn a_piece_ending_mid_line_at_the_position_waits_for_the_next_byte() {
    let text = b"M83\nG1 E1\nG1 E4";
    let mut goes_on = ExtrudedTo::new(12);
    assert!(goes_on.take(&text[..12]).is_continue());
    assert!(goes_on.take(&[]).is_continue(), "nothing has arrived yet");
    assert!(goes_on.take(&text[12..]).is_break());
    assert_eq!(lengths(&goes_on.fed()), vec![(0, 1.0)]);

    let mut ends = ExtrudedTo::new(15);
    assert!(ends.take(text).is_continue());
    ends.end();
    assert_eq!(lengths(&ends.fed()), vec![(0, 5.0)]);
}
