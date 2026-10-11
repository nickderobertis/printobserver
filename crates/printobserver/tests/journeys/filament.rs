//! The filament check and the filament charge, against a real `OctoPrint`.
//!
//! Everything a spool's figure turns on here is the machine's own: the file is
//! uploaded to the instance `just octoprint-up` started the moment the journey
//! starts it, so the start waits for `OctoPrint`'s own analysis of it; the
//! figures the check is judged on are that analysis's; and what a cancelled
//! print is charged is what the file's own moves fed up to the byte the
//! machine reported it had reached. The program is driven as a subprocess and
//! every effect is read back through it.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, Instant};

use printobserver::failure::Exit;
use printobserver_types::serde_json::{Value, json};

use crate::machine::Reports;
use crate::traced::Ran;
use crate::walk;
use crate::world::World;

use super::running;

/// The reason every change here gives.
const REASON: &str = "the filament journey is checking a spool against a real machine";

/// What the fixture feeds tool 0, by its own moves and so by the analysis.
const FED_MM: f64 = 2_000.0;

/// How long the journey waits for the machine to reach a byte of the file.
const PATIENCE: Duration = Duration::from_secs(120);

/// The workspace root, from this crate's own directory.
fn workspace_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
}

/// Upload the fixture to the running instance under a name of its own, through
/// the environment's own script, and answer that name.
fn uploaded() -> String {
    let name = format!("filament-{}.gcode", printobserver_types::EventId::new());
    let fixture = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/filament.gcode");
    let ran = Command::new("uv")
        .current_dir(workspace_root())
        .args([
            "run",
            "-q",
            "python",
            "tools/octoprint-env/octoprint_env.py",
            "upload",
        ])
        .arg("--file")
        .arg(&fixture)
        .args(["--as", &name])
        .output()
        .expect("the environment's own script runs");
    assert!(
        ran.status.success(),
        "the fixture could not be uploaded: {}",
        String::from_utf8_lossy(&ran.stderr)
    );
    name
}

/// Run one change as the operator.
fn changed(world: &World, asked: &[&str]) -> Ran {
    let mut given = asked.to_vec();
    given.extend_from_slice(&["--actor", "operator", "--reason", REASON]);
    running::command(world, &given)
}

/// Run one change that has to succeed, and answer its document.
fn done(world: &World, asked: &[&str]) -> Value {
    let mut given = asked.to_vec();
    given.push("--json");
    let ran = changed(world, &given);
    assert_eq!(
        ran.code,
        Some(0),
        "{asked:?} did not succeed: {}",
        ran.said()
    );
    printobserver_types::serde_json::from_str(&ran.out)
        .unwrap_or_else(|error| panic!("{asked:?} answered {error}: {}", ran.out))
}

/// A start of one file through this program, overridden or not.
fn started(world: &World, file: &str, overridden: bool) -> Ran {
    let manifest = walk::manifest_of("the filament journey", 1.3, file).to_string();
    let print = world.print_id();
    let mut asked = vec![
        "start-print",
        "--print-id",
        &print,
        "--file-name",
        file,
        "--manifest",
        &manifest,
        "--json",
    ];
    if overridden {
        asked.extend_from_slice(&["--override-filament-check", "true"]);
    }
    changed(world, &asked)
}

/// Whether a figure is this one, to the last few bits a sum taken in another
/// order differs in.
fn close(carried: &Value, expected: f64) -> bool {
    carried
        .as_f64()
        .is_some_and(|carried| (carried - expected).abs() < 1e-9)
}

/// A start the loaded spool is too short for is refused with its figures and
/// what to do about it; the check read on its own answers the same figures and
/// records nothing; the start overridden goes ahead; and the print, cancelled
/// part-way, charges the spool exactly what the file's own moves fed.
pub fn a_short_spool_is_refused_overridden_and_charged(world: &World) {
    world.wants(Reports::Operational);
    let file = uploaded();
    let short = done(
        world,
        &[
            "register-spool",
            "--name",
            "PLA nearly empty",
            "--material",
            "PLA",
            "--initial-g",
            "1000",
            "--remaining-g",
            "1",
        ],
    )["spool"]["id"]
        .as_str()
        .expect("a registration answers its spool")
        .to_owned();
    done(world, &["load-spool", "--spool-id", &short, "--tool", "0"]);

    // Started the moment it was uploaded: the start waits for the analysis,
    // and is then judged on its figures.
    let refused = started(world, &file, false);
    assert_eq!(
        refused.code,
        Some(i32::from(Exit::Rejected.status())),
        "{}",
        refused.said()
    );
    let answer: Value = printobserver_types::serde_json::from_str(&refused.out)
        .unwrap_or_else(|error| panic!("the refusal is not a document ({error}): {}", refused.out));
    let short_of = answer
        .pointer("/record/decision/rejected/insufficient_filament")
        .cloned()
        .unwrap_or_else(|| panic!("the start was refused for something else: {answer}"));
    assert_eq!(
        short_of["spool_name"],
        json!("PLA nearly empty"),
        "{short_of}"
    );
    assert_eq!(short_of["tool"], json!(0), "{short_of}");
    assert_eq!(short_of["remaining_g"], json!(1.0), "{short_of}");
    let required = short_of["required_g"].as_f64().unwrap_or_default();
    assert!(
        close(
            &short_of["required_g"],
            printobserver_core::grams_for_length(
                FED_MM,
                printobserver_core::DEFAULT_DIAMETER_MM,
                printobserver_core::density_for("PLA").expect("PLA has a density"),
            )
        ),
        "the start was not judged on the analysis of what the file feeds: {short_of}"
    );
    for said in [
        "insufficient_filament".to_owned(),
        "Spool `PLA nearly empty` on tool 0 holds 1.0 g".to_owned(),
        format!("needs {required:.1} g"),
        format!(
            "{:.1} g buffer",
            short_of["buffer_g"].as_f64().unwrap_or_default()
        ),
        "Load a spool holding enough".to_owned(),
        "adjust-spool".to_owned(),
        "--override-filament-check true".to_owned(),
    ] {
        assert!(
            refused.err.contains(&said),
            "the note does not say {said:?}: {}",
            refused.err
        );
    }
    assert!(!refused.err.contains("Ask again inside"), "{}", refused.err);
    assert_eq!(
        world.reported_state(),
        Some(Reports::Operational),
        "a refused start started the machine"
    );

    the_check_read_alone_records_nothing(world, &file, &short_of);

    let overridden = started(world, &file, true);
    assert_eq!(overridden.code, Some(0), "{}", overridden.said());
    let started_answer: Value = printobserver_types::serde_json::from_str(&overridden.out)
        .expect("the start answers a document");
    world.follows(
        started_answer
            .pointer("/record/print_id")
            .and_then(Value::as_str)
            .expect("a start answers the print it opened"),
    );
    world.settles_at(Reports::Printing, "the overridden start");

    a_cancelled_print_is_charged_what_it_fed(world, &short, &file);

    // The spool off the tool, so the machine is put back printing the hold
    // print the environment's own suite expects, judged on no spool of this
    // journey's.
    done(world, &["unload-spool", "--spool-id", &short]);
    world.wants(Reports::Printing);
}

/// The check read on its own answers the figures the refused start was judged
/// on, and the printer's history and every spool read the same around it.
fn the_check_read_alone_records_nothing(world: &World, file: &str, short_of: &Value) {
    let history = || running::read(world, &["printer-history", "--limit", "1000"]);
    let spools = || running::read(world, &["spools", "--include-retired", "true"]);
    let (history_before, spools_before) = (history(), spools());

    let checked = running::read(world, &["preflight", "--file-name", file]);
    let tool = checked
        .pointer("/preflight/tools/0")
        .cloned()
        .unwrap_or_else(|| panic!("the check names no tool: {checked}"));
    for figure in ["required_g", "buffer_g", "remaining_g", "spool_name"] {
        assert_eq!(
            tool[figure], short_of[figure],
            "the check read alone answered another `{figure}`: {checked}"
        );
    }
    assert_eq!(tool["verdict"], json!("insufficient"), "{checked}");
    assert_eq!(
        history(),
        history_before,
        "a filament check wrote into the history"
    );
    assert_eq!(spools(), spools_before, "a filament check moved a spool");
    assert_eq!(world.reported_state(), Some(Reports::Operational));
}

/// The running print reaches a byte of its file, is cancelled through this
/// program, and charges the spool on its tool what the file's moves fed up to
/// there — the history carrying its start, its end and the charge, and the
/// spool holding exactly what that charge left.
fn a_cancelled_print_is_charged_what_it_fed(world: &World, spool: &str, file: &str) {
    let before = running::read(world, &["spool", "--spool-id", spool])["spool"]["remaining_g"]
        .as_f64()
        .expect("a spool holds a figure");
    let deadline = Instant::now() + PATIENCE;
    loop {
        let status = running::read(world, &["status", "--print-id", &world.print_id()]);
        let reached = status
            .pointer("/job/file_position_bytes")
            .and_then(Value::as_i64)
            .unwrap_or_default();
        if reached > 0 {
            break;
        }
        assert!(
            Instant::now() < deadline,
            "the machine never reported reaching a byte of {file}: {status}"
        );
        std::thread::sleep(Duration::from_millis(500));
    }
    std::thread::sleep(Duration::from_secs(4));
    running::read(world, &["status", "--print-id", &world.print_id()]);
    done(world, &["cancel", "--print-id", &world.print_id()]);
    world.settles_at(Reports::Operational, "cancel");
    // The read that finds the job over closes the print out.
    running::read(world, &["prints"]);

    // The spool's own events carry the start that loaded it and the charge;
    // the end names no spool, so it is read by its kind.
    let history = running::read(world, &["printer-history", "--spool-id", spool]);
    let ends = running::read(world, &["printer-history", "--kinds", "print_ended"]);
    let of_print = |read: &Value, kind: &str| {
        read["events"]
            .as_array()
            .cloned()
            .unwrap_or_default()
            .into_iter()
            .filter(|event| {
                event["kind"] == json!(kind) && event["print_id"] == json!(world.print_id())
            })
            .collect::<Vec<_>>()
    };
    assert_eq!(of_print(&history, "print_started").len(), 1, "{history}");
    let ended = of_print(&ends, "print_ended");
    assert_eq!(ended.len(), 1, "{ends}");
    assert_eq!(ended[0]["payload"]["outcome"], json!("cancelled"), "{ends}");
    let consumed = of_print(&history, "filament_consumed");
    assert_eq!(consumed.len(), 1, "{history}");
    assert_eq!(
        consumed[0]["payload"]["method"],
        json!("gcode_extrusion"),
        "the cancelled print was not charged what its own moves fed: {history}"
    );
    let grams = consumed[0]["payload"]["grams"]
        .as_f64()
        .expect("the charge carries its grams");
    let length = consumed[0]["payload"]["length_mm"]
        .as_f64()
        .unwrap_or_default();
    assert!(
        grams > 0.0 && length > 0.0 && length < FED_MM,
        "a print cancelled part-way was charged {length} mm of the {FED_MM} mm its file feeds"
    );
    let after =
        running::read(world, &["spool", "--spool-id", spool])["spool"]["remaining_g"].clone();
    assert!(
        close(&after, before - grams),
        "the spool holds {after} after a charge of {grams} g from {before} g"
    );
}
