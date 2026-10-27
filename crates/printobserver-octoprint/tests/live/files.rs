//! The files the instance holds, and the filament a print of one uses, against
//! a real printer.
//!
//! The walk uploads this crate's own extrusion fixture, waits for
//! `OctoPrint`'s analysis of it, prints it, reads the running job's position
//! and filament, asks what the file's G-code fed up to that position, cancels
//! it, and reads the tally the cancel left. It then prints a copy inside a
//! folder, so the listing's folders — which carry a tally of their own — are
//! told apart from files by the live producer rather than by a recording.
//! What the instance reported is read from its own JSON by a reader sharing no
//! parsing with the adapter, and every figure is compared with it: lengths,
//! volumes, sizes, tallies and the last print's date.
//!
//! It removes what it uploads before it begins, so a second run against one
//! instance starts from files never printed, and again when it is done, so the
//! instance holds what the bring-up left it holding — and it homes the head
//! its prints moved, so the machine reports the position the bring-up left.

use std::time::Duration;

use printobserver_printer_api::{FileFacts, PrinterPort as _, PrinterState, ToolFilament};
use printobserver_types::{FileName, Timestamp};

use crate::block_on::block_on;
use crate::env::Scripted;
use crate::raw;
use crate::wait;

/// The name the fixture is uploaded under.
const FIXTURE: &str = "printobserver-extrusion.gcode";

/// The file the scripted environment uploads and prints, and leaves selected.
const HOLD_FILE: &str = "hold.gcode";

/// The folder a second copy is printed from.
const FOLDER: &str = "printobserver-folder";

/// The name of the copy inside it.
const NESTED: &str = "printobserver-nested.gcode";

/// The fixture itself: absolute extrusion along a square, dwelling between
/// moves so that a virtual printer streams it for about a minute.
const FIXTURE_BYTES: &[u8] = include_bytes!("../fixtures/extrusion.gcode");

/// How long `OctoPrint`'s analysis queue, a start or a cancel may take.
const REACHED: Duration = Duration::from_secs(90);

/// Reading files, a running job's filament, and the feed up to its position.
///
/// Expects no print running, which is how the acting walk leaves the instance,
/// and leaves none running, with the hold print selected.
///
/// # Panics
///
/// Panics when a read does not carry what the instance reported.
pub fn walk(instance: &Scripted) {
    let printer = instance.printer();
    let name = FileName::new(FIXTURE).expect("a file name");
    remove_what_this_walk_uploads(instance);
    raw::upload_local(instance, None, FIXTURE, FIXTURE_BYTES);

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
    let analysed = analysed_filament(&reported["gcodeAnalysis"]["filament"]);
    assert!(
        !analysed.is_empty(),
        "the analysis names no tool: {reported}"
    );
    assert_eq!(
        facts.filament.as_ref(),
        Some(&analysed),
        "lengths and volumes"
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
    let whole = analysed[0].length_mm;
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

    // printed_extent — read from the same download at the same position, and
    // exactly what this fixture's own G-code laid down by then: the square at
    // Z 0, inside the corners its moves name.
    let extent = block_on(printer.printed_extent(name.clone(), second))
        .expect("the extent")
        .expect("the fixture extruded by then");
    assert_eq!(
        Some(extent),
        printobserver_octoprint::printed_extent(FIXTURE_BYTES, second),
        "the download is the file that was uploaded"
    );
    assert!(extent.tallest_mm.abs() < f64::EPSILON, "{extent:?}");
    assert!(
        extent.footprint.max_x <= 30.0 && extent.footprint.max_y <= 30.0,
        "{extent:?}"
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
    tallied_once_and_failed(
        instance,
        &tallied,
        &format!("/api/files/local/{FIXTURE}"),
        0,
    );

    print_inside_a_folder(instance);

    remove_what_this_walk_uploads(instance);
    put_the_head_back(instance);
}

/// Put the virtual head back at the origin the bring-up left it at.
///
/// This walk is the only one in the tier that moves the machine, and a print
/// cancelled mid-move leaves the virtual printer reporting a fractional
/// position — sometimes in scientific notation, `X:2.67e-14`. `OctoPrint`
/// records the position when a print pauses and does not read that notation,
/// so every later pause waits out its position timeout before it lands, about
/// ten seconds late: long enough for the next tier on this machine to read a
/// print still on its way to paused as running and leave it paused. Draining
/// the moves still buffered, then homing, puts every axis and the extruder back
/// at exactly zero.
fn put_the_head_back(instance: &Scripted) {
    raw::post_json(
        instance,
        "/api/printer/command",
        &serde_json::json!({ "commands": ["M400", "G28"] }),
    );
}

/// Remove the fixture and the folder, selecting the environment's own hold
/// print first: `OctoPrint` refuses to remove a selected file, or a folder
/// holding one, and a cancelled print stays selected.
fn remove_what_this_walk_uploads(instance: &Scripted) {
    raw::post_json(
        instance,
        &format!("/api/files/local/{HOLD_FILE}"),
        &serde_json::json!({ "command": "select", "print": false }),
    );
    raw::delete_local(instance, FIXTURE);
    raw::delete_local(instance, FOLDER);
}

/// A printed file inside a folder is listed, and the folder — which the
/// instance reports with a tally of its own — is not.
fn print_inside_a_folder(instance: &Scripted) {
    let printer = instance.printer();
    raw::create_folder(instance, FOLDER);
    raw::upload_local(instance, Some(FOLDER), NESTED, FIXTURE_BYTES);
    // The port starts only a file at the top of the storage, so the copy in
    // the folder is started the way an operator would start it.
    let nested_path = format!("/api/files/local/{FOLDER}/{NESTED}");
    // `OctoPrint` keeps a removed folder's metadata cached, so a copy uploaded
    // into a folder recreated under the same name can arrive carrying the last
    // run's tally: the claim is that this print adds one to it.
    let failed_before = raw::get(instance, &nested_path)["prints"]["failure"]
        .as_i64()
        .unwrap_or(0);
    raw::post_json(
        instance,
        &nested_path,
        &serde_json::json!({ "command": "select", "print": true }),
    );
    wait::until("the copy in the folder to print", REACHED, || {
        let state = raw::get(instance, "/api/job")["state"]
            .as_str()
            .unwrap_or_default()
            .to_owned();
        if state.starts_with("Printing") {
            Ok(())
        } else {
            Err(format!("`{state}`"))
        }
    });
    block_on(printer.cancel()).expect("the copy cancels");
    wait::until("the copy's cancel to finish", REACHED, || {
        let state = block_on(printer.job()).map_err(|e| e.to_string())?.state;
        if state == PrinterState::Operational {
            Ok(())
        } else {
            Err(format!("{state:?}"))
        }
    });

    let tallied = wait::until("the copy's cancel to be tallied", REACHED, || {
        let listed = block_on(printer.printed_files()).map_err(|e| e.to_string())?;
        assert!(
            listed.iter().all(|facts| facts.file_name != FOLDER),
            "a folder is listed as a printed file: {listed:?}"
        );
        listed
            .into_iter()
            .find(|facts| facts.file_name == NESTED && facts.failures > failed_before)
            .ok_or_else(|| "no failure tallied inside the folder yet".to_owned())
    });
    let listing = raw::get(instance, "/api/files?recursive=true");
    let folder = listing["files"]
        .as_array()
        .into_iter()
        .flatten()
        .find(|entry| entry["name"] == FOLDER)
        .unwrap_or_else(|| panic!("the instance does not list {FOLDER}: {listing}"));
    assert_eq!(folder["type"], "folder");
    assert!(
        folder.get("prints").is_some(),
        "the folder carries no tally, so leaving it out proves nothing: {folder}"
    );
    tallied_once_and_failed(instance, &tallied, &nested_path, failed_before);
}

/// One running reading of the job, past a position, bracketed by the
/// instance's own figure either side of it.
fn running_position(instance: &Scripted, analysed: &[ToolFilament], past: i64) -> i64 {
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
        assert_eq!(job.filament, analysed, "the job's filament is the analysis");
        Ok(position)
    })
}

/// The cancel this walk took added one failure to the file's tally and none to
/// its successes, and is its last print — dated when the instance dates it.
fn tallied_once_and_failed(
    instance: &Scripted,
    printed: &FileFacts,
    path: &str,
    failed_before: i64,
) {
    assert_eq!(
        (printed.successes, printed.failures),
        (0, failed_before + 1),
        "{printed:?}"
    );
    let last = printed.last_print.as_ref().expect("a last print");
    assert!(
        !last.success,
        "a cancelled print is not a success: {printed:?}"
    );
    let reported = raw::get(instance, path);
    let date = reported["prints"]["last"]["date"]
        .as_f64()
        .unwrap_or_else(|| panic!("the instance dates no last print: {reported}"));
    let at = Timestamp::from_unix_seconds(
        format!("{:.0}", date.round())
            .parse()
            .expect("a whole second"),
    )
    .expect("a timestamp");
    assert_eq!(last.at, at, "the instance dated the last print {date}");
}

/// Each tool's length, in tool order.
fn lengths(filament: &[ToolFilament]) -> Vec<(i64, f64)> {
    filament
        .iter()
        .map(|tool| (tool.tool, tool.length_mm))
        .collect()
}

/// The filament `OctoPrint`'s own analysis reports, in tool order.
fn analysed_filament(filament: &serde_json::Value) -> Vec<ToolFilament> {
    let mut tools: Vec<ToolFilament> = filament
        .as_object()
        .into_iter()
        .flatten()
        .filter_map(|(name, tool)| {
            Some(ToolFilament {
                tool: name.strip_prefix("tool")?.parse().ok()?,
                length_mm: tool["length"].as_f64()?,
                volume_cm3: tool["volume"].as_f64(),
            })
        })
        .collect();
    tools.sort_by_key(|tool| tool.tool);
    tools
}
