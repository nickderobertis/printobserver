//! The files the instance holds, and the filament a print of one uses, against
//! a real printer.
//!
//! The walk uploads this crate's own extrusion fixture, waits for
//! `OctoPrint`'s analysis of it, prints it, reads the running job's position
//! and filament, asks what the file's G-code fed up to that position, cancels
//! it, and reads the tally the cancel left. What the instance reported is read
//! from its own JSON by a reader sharing no parsing with the adapter.
//!
//! It removes the fixture before it begins, so a second run against one
//! instance starts from a file never printed, and removes it again when it is
//! done, so the instance holds what the bring-up left it holding.

use std::time::Duration;

use printobserver_printer_api::{FileFacts, PrinterPort as _, PrinterState, ToolFilament};
use printobserver_types::FileName;

use crate::block_on::block_on;
use crate::env::Scripted;
use crate::raw;
use crate::wait;

/// The name the fixture is uploaded under.
const FIXTURE: &str = "printobserver-extrusion.gcode";

/// The fixture itself: absolute extrusion along a square, dwelling between
/// moves so that a virtual printer streams it for about a minute.
const FIXTURE_BYTES: &[u8] = include_bytes!("../fixtures/extrusion.gcode");

/// How long `OctoPrint`'s analysis queue, a start or a cancel may take.
const REACHED: Duration = Duration::from_secs(90);

/// Reading files, a running job's filament, and the feed up to its position.
///
/// Expects no print running, which is how the acting walk leaves the instance.
///
/// # Panics
///
/// Panics when a read does not carry what the instance reported.
pub fn walk(instance: &Scripted) {
    let printer = instance.printer();
    let name = FileName::new(FIXTURE).expect("a file name");
    raw::delete_local(instance, FIXTURE);
    raw::upload_local(instance, FIXTURE, FIXTURE_BYTES);

    // file_facts — once the analysis appears: a filament per tool, no tallies.
    let facts = wait::until("OctoPrint's analysis of the fixture", REACHED, || {
        let facts = block_on(printer.file_facts(name.clone())).map_err(|e| e.to_string())?;
        if facts.filament.is_some() {
            Ok(facts)
        } else {
            Err("no analysis yet".to_owned())
        }
    });
    let reported = raw::get(instance, &format!("/api/files/local/{FIXTURE}"));
    let analysed = analysed_lengths(&reported["gcodeAnalysis"]["filament"]);
    assert!(
        !analysed.is_empty(),
        "the analysis names no tool: {reported}"
    );
    assert_eq!(
        lengths(facts.filament.as_deref().unwrap_or_default()),
        analysed
    );
    assert_eq!(facts.file_name, FIXTURE);
    assert_eq!(facts.origin, "local");
    assert_eq!(facts.size_bytes, reported["size"].as_i64());
    assert_eq!(
        (facts.successes, facts.failures, facts.last_print.as_ref()),
        (0, 0, None),
        "a file never printed carries no tallies"
    );

    // start, then a running job whose position grows and whose filament is the
    // analysis's.
    block_on(printer.start(name.clone())).expect("the fixture starts");
    let first = running_position(instance, &analysed, 0);
    let second = running_position(instance, &analysed, first);
    assert!(
        second > first,
        "the position did not grow: {first} then {second}"
    );

    // extruded_to — at the position the job reported, strictly part of the
    // whole, and exactly what this fixture's own G-code feeds by then.
    let fed = block_on(printer.extruded_to(name.clone(), second)).expect("the feed");
    let whole = analysed[0].1;
    let charged = lengths(&fed);
    assert_eq!(charged.len(), 1, "one tool fed: {fed:?}");
    assert!(
        charged[0].1 > 0.0 && charged[0].1 < whole,
        "{} mm fed by byte {second}, against {whole} mm analysed",
        charged[0].1
    );
    assert_eq!(
        fed,
        printobserver_octoprint::extruded_to(FIXTURE_BYTES, second),
        "the download is the file that was uploaded"
    );

    // cancel — and the instance clears the position.
    block_on(printer.cancel()).expect("the fixture cancels");
    let cleared = wait::until("the cancel to clear the job's position", REACHED, || {
        let job = block_on(printer.job()).map_err(|e| e.to_string())?;
        if job.state == PrinterState::Operational {
            Ok(job)
        } else {
            Err(format!("{:?} at {:?}", job.state, job.file_position_bytes))
        }
    });
    assert_eq!(
        raw::get(instance, "/api/job")["progress"]["filepos"],
        serde_json::Value::Null
    );
    assert_eq!(cleared.file_position_bytes, None);

    // printed_files — the fixture's failure tally, and a last print that failed.
    let tallied = wait::until("the cancel to be tallied", REACHED, || {
        let listed = block_on(printer.printed_files()).map_err(|e| e.to_string())?;
        listed
            .into_iter()
            .find(|facts| facts.file_name == FIXTURE && facts.failures > 0)
            .ok_or_else(|| "no failure tallied yet".to_owned())
    });
    tallied_once_and_failed(&tallied);

    raw::delete_local(instance, FIXTURE);
}

/// One running reading of the job, past a position, bracketed by the
/// instance's own figure either side of it.
fn running_position(instance: &Scripted, analysed: &[(i64, f64)], past: i64) -> i64 {
    let printer = instance.printer();
    wait::until("the running fixture to read further in", REACHED, || {
        let before = raw::get(instance, "/api/job")["progress"]["filepos"].as_i64();
        let job = block_on(printer.job()).map_err(|e| e.to_string())?;
        let after = raw::get(instance, "/api/job")["progress"]["filepos"].as_i64();
        let (Some(before), Some(position), Some(after)) = (before, job.file_position_bytes, after)
        else {
            return Err(format!(
                "positions {before:?}, {:?}, {after:?}",
                job.file_position_bytes
            ));
        };
        if position <= past {
            return Err(format!("still at {position}"));
        }
        assert!(
            before <= position && position <= after,
            "the port answered {position}, outside the instance's {before}..={after}"
        );
        assert_eq!(job.file_name.as_deref(), Some(FIXTURE));
        assert_eq!(job.state, PrinterState::Printing);
        assert_eq!(
            lengths(&job.filament),
            analysed,
            "the job's filament is the analysis"
        );
        Ok(position)
    })
}

/// The cancel this walk took is the fixture's one print, and it failed.
fn tallied_once_and_failed(printed: &FileFacts) {
    assert_eq!((printed.successes, printed.failures), (0, 1), "{printed:?}");
    let last = printed.last_print.as_ref().expect("a last print");
    assert!(
        !last.success,
        "a cancelled print is not a success: {printed:?}"
    );
}

/// Each tool's length, in tool order.
fn lengths(filament: &[ToolFilament]) -> Vec<(i64, f64)> {
    filament
        .iter()
        .map(|tool| (tool.tool, tool.length_mm))
        .collect()
}

/// The lengths `OctoPrint`'s own analysis reports, in tool order.
fn analysed_lengths(filament: &serde_json::Value) -> Vec<(i64, f64)> {
    let mut tools: Vec<(i64, f64)> = filament
        .as_object()
        .into_iter()
        .flatten()
        .filter_map(|(name, tool)| {
            Some((
                name.strip_prefix("tool")?.parse().ok()?,
                tool["length"].as_f64()?,
            ))
        })
        .collect();
    tools.sort_by_key(|(tool, _)| *tool);
    tools
}
