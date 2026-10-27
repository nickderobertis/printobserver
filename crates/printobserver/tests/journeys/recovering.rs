//! Recovery, driven through the real `printobserver recover-*` commands
//! against a real supervisor over `OctoPrint`'s own virtual printer.
//!
//! What the machine did is read from the machine's own reports: its heater
//! targets and its state as the recovery read passes them through, its
//! position as the recovery read answers it while Z is trusted, and — while Z
//! is not, which is when printobserver reads no position at all — from the
//! position replies the machine's own serial log records for the `M114`s the
//! supervisor's watch sends after a print ends unfinished. What a journey
//! types at the printer's terminal, and the reconnect a drop needs, go
//! through the scripted environment's own `terminal` and `connect`: this
//! program names no `OctoPrint` of its own, in its tests or anywhere else.
//! Every refusal is asserted to leave the targets and the position as they
//! were.
//!
//! Each journey puts the machine back as `just octoprint-up` left it — cold
//! heaters and the hold print running — because the integration tier's other
//! suites act on the same machine.

use std::path::PathBuf;
use std::process::Command;
use std::time::{Duration, Instant};

use printobserver_types::serde_json::{Value, json};

use crate::world::{SCRIPTED, World, committed_skill};

/// The part a print is interrupted part-way through.
const PART: &str = "recovery-part.gcode";

/// The short print run to its end.
const FINISH: &str = "recovery-finish.gcode";

/// The file the scripted environment holds printing.
const HOLD: &str = "hold.gcode";

/// The tallest part [`PART`] lays down before its long dwell.
const PART_TALLEST_MM: f64 = 2.0;

/// The tallest part [`FINISH`] lays down.
const FINISH_TALLEST_MM: f64 = 1.0;

/// The footprint both lay down, as `[min_x, min_y, max_x, max_y]`.
const FOOTPRINT: [f64; 4] = [90.0, 90.0, 110.0, 110.0];

/// Where the journeys' configuration parks the head.
const PARK_XY: [f64; 2] = [10.0, 200.0];

/// How long one step waits for the machine.
const PATIENCE: Duration = Duration::from_secs(150);

/// The reason every request of these journeys carries.
const REASON: &str = "free the nozzle from the part";

/// The recovery facts of the virtual printer, with one direction away from
/// the bed.
fn facts(away_from_bed: &str) -> Value {
    json!({
        "away_from_bed": away_from_bed,
        "max_z_mm": 200.0,
        "max_lift_mm": 50.0,
        "clearance_margin_mm": 5.0,
        "park_xy": PARK_XY,
        "z_home_xy": [100.0, 100.0],
        "z_home_sweeps_bed": false,
    })
}

/// A world over the scripted environment, its supervisor stating the virtual
/// printer's recovery facts and reading the machine every five seconds.
fn recovering_world(away_from_bed: &str) -> World {
    World::configured(SCRIPTED, &committed_skill(), None, |document| {
        document["recovery"] = facts(away_from_bed);
        document["supervisor"]["printer_poll_s"] = json!(5);
    })
}

/// Run one client command, answering whether it succeeded and the document
/// it printed.
fn run(world: &World, command: &str, arguments: &[&str]) -> (bool, Value) {
    let ran = Command::new(env!("CARGO_BIN_EXE_printobserver"))
        .arg(command)
        .args(arguments)
        .args(["--json", "--config"])
        .arg(world.client_config())
        .output()
        .expect("the command runs");
    let document = printobserver_types::serde_json::from_slice(&ran.stdout).unwrap_or_else(|_| {
        panic!(
            "`{command}` printed no document: {}{}",
            String::from_utf8_lossy(&ran.stdout),
            String::from_utf8_lossy(&ran.stderr)
        )
    });
    (ran.status.success(), document)
}

/// Ask for one request as the operator, with a reason.
fn ask(world: &World, command: &str, arguments: &[&str]) -> (bool, Value) {
    let mut asked = vec!["--actor", "operator", "--reason", REASON];
    asked.extend_from_slice(arguments);
    run(world, command, &asked)
}

/// Ask for one recovery the printer is to carry out, and assert it was.
fn carried_out(world: &World, command: &str, arguments: &[&str]) -> Value {
    let (ok, answer) = ask(world, command, arguments);
    assert!(ok, "`{command}` was not carried out: {answer:#}");
    assert_eq!(answer["outcome"], "succeeded", "{command}: {answer:#}");
    answer
}

/// The name of the one refusal an answer carries.
fn refusal_of(refusal: &Value) -> Option<String> {
    match refusal {
        Value::String(named) => Some(named.clone()),
        Value::Object(named) => named.keys().next().cloned(),
        _ => None,
    }
}

/// Ask for one recovery that is to be refused, and assert it was refused as
/// `expected`, the machine's targets and position as they were.
fn refused(
    world: &World,
    command: &str,
    arguments: &[&str],
    expected: &str,
    position: &dyn Fn() -> [f64; 3],
) -> Value {
    let before = (targets(world), position());
    let (ok, answer) = ask(world, command, arguments);
    assert!(!ok, "`{command}` was carried out: {answer:#}");
    assert_eq!(
        refusal_of(&answer["rejection"]).as_deref(),
        Some(expected),
        "{command}: {answer:#}"
    );
    let after = (targets(world), position());
    assert_eq!(
        after.0, before.0,
        "`{command}`'s refusal heated or cooled the machine"
    );
    assert!(
        near(after.1, before.1),
        "`{command}`'s refusal moved the head from {:?} to {:?}",
        before.1,
        after.1
    );
    answer
}

/// The recovery read, as the operator.
fn status(world: &World) -> Value {
    let (ok, answer) = run(world, "recover-status", &[]);
    assert!(ok, "the recovery read was refused: {answer:#}");
    answer
}

/// What one action's entry of the recovery read says: nothing when it is
/// allowed, and its refusal's name when it is not.
fn allowance(status: &Value, action: &str) -> Option<String> {
    let entry = status["actions"]
        .as_array()
        .expect("every action")
        .iter()
        .find(|entry| entry["action"] == action)
        .unwrap_or_else(|| panic!("the recovery read carries no `{action}`"));
    refusal_of(&entry["refusal"])
}

/// The machine's tool and bed targets, as it reports them.
fn targets(world: &World) -> (Option<f64>, Option<f64>) {
    let read = status(world);
    (
        read["temperatures"]["tools"][0]["target_c"].as_f64(),
        read["temperatures"]["bed"]["target_c"].as_f64(),
    )
}

/// The machine's state, as it reports it.
fn state(world: &World) -> String {
    status(world)["state"]
        .as_str()
        .unwrap_or_default()
        .to_owned()
}

/// Where the machine reports the head while Z is trusted.
fn trusted_position(world: &World) -> [f64; 3] {
    let read = status(world);
    ["x", "y", "z"].map(|axis| {
        read["position"][axis]
            .as_f64()
            .unwrap_or_else(|| panic!("no trusted position was read: {read:#}"))
    })
}

/// Whether two positions are the same to within a micron.
fn near(a: [f64; 3], b: [f64; 3]) -> bool {
    a.iter()
        .zip(b)
        .all(|(one, other)| (one - other).abs() < 1e-3)
}

/// Wait until something is reached.
fn until(describing: &str, mut reached: impl FnMut() -> Result<(), String>) {
    let deadline = Instant::now() + PATIENCE;
    loop {
        match reached() {
            Ok(()) => return,
            Err(last) => assert!(
                Instant::now() < deadline,
                "{describing} never happened: {last}"
            ),
        }
        std::thread::sleep(Duration::from_millis(500));
    }
}

/// Wait until the machine reports one state.
fn reads(world: &World, wanted: &str) {
    until(&format!("the machine reading {wanted}"), || {
        let now = state(world);
        if now == wanted { Ok(()) } else { Err(now) }
    });
}

/// Wait until the machine reports its targets at these values.
fn heating_to(world: &World, describing: &str, wanted: (Option<f64>, Option<f64>)) {
    until(describing, || {
        let now = targets(world);
        if now == wanted {
            Ok(())
        } else {
            Err(format!("the targets are {now:?}, not {wanted:?}"))
        }
    });
}

/// Wait until the recovery read reports the head at one place.
fn trusted_at(world: &World, describing: &str, wanted: [f64; 3]) {
    until(describing, || {
        let now = trusted_position(world);
        if near(now, wanted) {
            Ok(())
        } else {
            Err(format!("the head is at {now:?}, not {wanted:?}"))
        }
    });
}

/// The root of this workspace.
fn workspace_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

/// Run one command of the scripted environment's own script.
fn environment(arguments: &[&str]) {
    let ran = Command::new("uv")
        .args([
            "run",
            "-q",
            "python",
            "tools/octoprint-env/octoprint_env.py",
        ])
        .args(arguments)
        .current_dir(workspace_root())
        .output()
        .expect("the environment's script runs");
    assert!(
        ran.status.success(),
        "`octoprint_env.py {arguments:?}` failed: {}",
        String::from_utf8_lossy(&ran.stderr)
    );
}

/// Upload one of this crate's fixtures to the instance under its own name.
fn upload(fixture: &str) {
    let file = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(fixture);
    environment(&["upload", "--file", &file.display().to_string()]);
}

/// The instance's own serial log, from `from` bytes on.
fn serial_log_after(from: usize) -> String {
    std::fs::read_to_string(workspace_root().join(".octoprint-env/instance/logs/serial.log"))
        .unwrap_or_default()
        .get(from..)
        .unwrap_or_default()
        .to_owned()
}

/// How long the serial log is now.
fn serial_log_length() -> usize {
    serial_log_after(0).len()
}

/// The position one position reply in the serial log states.
fn replied(line: &str) -> Option<[f64; 3]> {
    let reply = line.split_once("Recv: ")?.1;
    let axis = |name: &str| {
        reply
            .split_whitespace()
            .find_map(|word| word.strip_prefix(name))
            .and_then(|value| value.parse::<f64>().ok())
    };
    Some([axis("X:")?, axis("Y:")?, axis("Z:")?])
}

/// The first position the machine replied with after `from` bytes of its
/// serial log — to the `M114` the supervisor's watch sends on its own each
/// poll after a print ends unfinished — waiting for one.
fn logged_after(from: usize) -> [f64; 3] {
    let mut found = None;
    until("a position reply in the serial log", || {
        found = serial_log_after(from).lines().find_map(replied);
        found.map(|_| ()).ok_or_else(|| "none yet".to_owned())
    });
    found.expect("a position reply")
}

/// Where the head is, from the next position reply the machine logs.
fn logged_position() -> [f64; 3] {
    logged_after(serial_log_length())
}

/// Set the machine's job down: whatever print it is running, cancelled.
fn idle(world: &World) {
    until("the machine to be idle", || {
        let now = state(world);
        if now == "operational" {
            return Ok(());
        }
        if matches!(now.as_str(), "printing" | "paused") {
            let (_, listed) = run(world, "prints", &[]);
            if let Some(active) = listed["active"].as_str() {
                let _ = ask(world, "cancel", &["--print-id", active]);
            }
        }
        Err(now)
    });
}

/// Start one file through printobserver on an idle machine, answering the
/// print it opened, waiting out the moment after a cancel in which the
/// machine still refuses a start.
fn started(world: &World, file: &str) -> String {
    let manifest = json!({
        "file_name": file,
        "material": "PLA",
        "nozzle_diameter_mm": 0.4,
        "slicer_profile": "the recovery journeys' own profile",
        "allowed": {},
        "metadata": {},
    })
    .to_string();
    let mut print_id = String::new();
    until(&format!("{file} to start"), || {
        // A listing settles the prints against the idle machine first, so a
        // start opens a print of its own rather than one a read is ending.
        let (_, listed) = run(world, "prints", &[]);
        let asked = listed["prints"][0]["id"]
            .as_str()
            .map_or_else(|| world.print_id(), str::to_owned);
        let (ok, answer) = ask(
            world,
            "start-print",
            &[
                "--print-id",
                &asked,
                "--file-name",
                file,
                "--manifest",
                &manifest,
                "--override-filament-check",
                "true",
            ],
        );
        if ok {
            answer["record"]["print_id"]
                .as_str()
                .unwrap_or_default()
                .clone_into(&mut print_id);
            Ok(())
        } else {
            Err(answer.to_string())
        }
    });
    reads(world, "printing");
    print_id
}

/// Wait until one print has ended, by the printer's history.
fn ended(world: &World, print_id: &str) {
    until(&format!("print {print_id} to end"), || {
        let (_, history) = run(
            world,
            "printer-history",
            &["--kinds", "print_ended", "--limit", "50"],
        );
        let events = history["events"].as_array().cloned().unwrap_or_default();
        if events.iter().any(|event| event["print_id"] == print_id) {
            Ok(())
        } else {
            Err(format!("{events:?}"))
        }
    });
}

/// Put the machine back as the bring-up left it: cold, and the hold print
/// running.
fn put_back(world: &World) {
    idle(world);
    if targets(world) != (Some(0.0), Some(0.0)) {
        carried_out(world, "recover-cool", &[]);
    }
    started(world, HOLD);
}

/// The footprint one recovery read answers for the last print, as its corners.
fn footprint_of(status: &Value) -> [f64; 4] {
    let footprint = &status["last_print"]["footprint"];
    ["min_x", "min_y", "max_x", "max_y"].map(|corner| {
        footprint[corner]
            .as_f64()
            .unwrap_or_else(|| panic!("the footprint carries no {corner}: {status:#}"))
    })
}

/// After a print interrupted part-way: the recovery read says Z is untrusted,
/// what the part is and what each action would meet; a lift and a fan are
/// carried out and the machine follows; the three moves over the part are
/// refused for untrusted Z; a Z home with the bed asserted clear homes Z and
/// trusts it; below the margin over the part the three are refused, and the
/// operator's word lets each go ahead; above it, they go ahead on the
/// clearance measured; and a cool turns both heaters off.
pub fn after_an_interrupted_print() {
    let world = recovering_world("z_plus");
    interrupted_part_way(&world);
    while_z_is_untrusted(&world);
    once_z_is_homed(&world);
    cooled(&world);
    put_back(&world);
}

/// Print the part and interrupt it in its dwell, the supervisor having read
/// how far into the file it got; then the recovery read's account of it.
fn interrupted_part_way(world: &World) {
    upload(PART);
    idle(world);
    started(world, PART);
    // The supervisor reads how far into the file the print got: once that is
    // in the long dwell, the part is as tall as it will be when interrupted.
    until("the part's height to be read up to its dwell", || {
        let read = status(world);
        if read["last_print"]["tallest_mm"].as_f64() == Some(PART_TALLEST_MM) {
            Ok(())
        } else {
            Err(read["last_print"].to_string())
        }
    });
    environment(&["terminal", "--line", "!!DEBUG:drop_connection"]);
    reads(world, "offline");
    until("the drop to be recorded", || {
        let (_, history) = run(
            world,
            "printer-history",
            &["--kinds", "printer_connection_changed", "--limit", "50"],
        );
        let events = history["events"].as_array().cloned().unwrap_or_default();
        if events
            .iter()
            .any(|event| event["payload"]["to"] == "offline")
        {
            Ok(())
        } else {
            Err(format!("{events:?}"))
        }
    });
    environment(&["connect"]);
    reads(world, "operational");

    let read = status(world);
    assert!(read["z_trust"].get("untrusted").is_some(), "{read:#}");
    assert_eq!(read["position"], Value::Null);
    assert_eq!(read["last_print"]["tallest_mm"], PART_TALLEST_MM);
    let footprint = footprint_of(&read);
    assert!(
        footprint
            .iter()
            .zip(FOOTPRINT)
            .all(|(found, laid)| (found - laid).abs() < 1e-9),
        "the footprint is {footprint:?}, not {FOOTPRINT:?}"
    );
    for action in ["recover_lift", "recover_cool", "recover_fan"] {
        assert_eq!(allowance(&read, action), None, "{action}: {read:#}");
    }
    for action in ["recover_heat", "recover_park", "recover_home_xy"] {
        assert_eq!(
            allowance(&read, action).as_deref(),
            Some("z_untrusted"),
            "{action}"
        );
    }
    assert_eq!(
        allowance(&read, "recover_home_z").as_deref(),
        Some("assertion_required")
    );
}

/// With Z untrusted: a lift moves the head up by what was asked, and a fan is
/// sent as the duty it is; the three moves over the part are refused for it.
fn while_z_is_untrusted(world: &World) {
    let before = logged_position();
    let mark = serial_log_length();
    carried_out(world, "recover-lift", &["--mm", "5"]);
    let lifted = [before[0], before[1], before[2] + 5.0];
    until("the lift, by the machine's own reply", || {
        let now = logged_after(mark);
        if near(now, lifted) {
            Ok(())
        } else {
            Err(format!("{now:?}"))
        }
    });

    let fan = serial_log_length();
    carried_out(world, "recover-fan", &["--percent", "50"]);
    until("the fan line to be sent", || {
        let sent = serial_log_after(fan);
        if sent.contains("M106 S128") {
            Ok(())
        } else {
            Err(sent)
        }
    });

    for (command, arguments) in [
        ("recover-heat", &["--tool", "0", "--target-c", "150"][..]),
        ("recover-park", &[][..]),
        ("recover-home-xy", &[][..]),
    ] {
        refused(world, command, arguments, "z_untrusted", &logged_position);
    }
}

/// Once Z is homed with the bed asserted clear: below the margin over the
/// part the three are refused, and the operator's word lets each go ahead;
/// lifted clear, they go ahead on the clearance measured.
fn once_z_is_homed(world: &World) {
    let homing = logged_position();
    carried_out(world, "recover-home-z", &["--bed-clear-asserted", "true"]);
    let read = status(world);
    assert!(
        read["z_trust"].get("trusted_since").is_some(),
        "a Z home left Z untrusted: {read:#}"
    );
    trusted_at(world, "the Z home", [homing[0], homing[1], 0.0]);

    // At Z 0, under a 2 mm part and a 5 mm margin, each is refused.
    let trusted = || trusted_position(world);
    for (command, arguments) in [
        ("recover-heat", &["--tool", "0", "--target-c", "150"][..]),
        ("recover-park", &[][..]),
        ("recover-home-xy", &[][..]),
    ] {
        let answer = refused(
            world,
            command,
            arguments,
            "insufficient_clearance",
            &trusted,
        );
        assert_eq!(
            answer["rejection"]["insufficient_clearance"],
            json!({ "nozzle_z_mm": 0.0, "tallest_mm": PART_TALLEST_MM, "margin_mm": 5.0 })
        );
    }

    // The operator's word lets each go ahead below the margin.
    let asserted = ["--clearance-asserted", "true"];
    carried_out(
        world,
        "recover-heat",
        &["--tool", "0", "--target-c", "120", asserted[0], asserted[1]],
    );
    heating_to(world, "the asserted heat", (Some(120.0), Some(0.0)));
    carried_out(world, "recover-park", &asserted);
    trusted_at(world, "the asserted park", [PARK_XY[0], PARK_XY[1], 0.0]);
    carried_out(world, "recover-home-xy", &asserted);
    trusted_at(world, "the asserted XY home", [0.0, 0.0, 0.0]);

    // Lifted clear of the part, each goes ahead on the clearance measured.
    carried_out(world, "recover-lift", &["--mm", "10"]);
    trusted_at(world, "the lift clear of the part", [0.0, 0.0, 10.0]);
    for action in ["recover_heat", "recover_park", "recover_home_xy"] {
        assert_eq!(allowance(&status(world), action), None, "{action}");
    }
    carried_out(world, "recover-heat", &["--tool", "0", "--target-c", "140"]);
    heating_to(world, "the measured heat", (Some(140.0), Some(0.0)));
    carried_out(world, "recover-park", &[]);
    trusted_at(world, "the measured park", [PARK_XY[0], PARK_XY[1], 10.0]);
    carried_out(world, "recover-home-xy", &[]);
    trusted_at(world, "the measured XY home", [0.0, 0.0, 10.0]);
}

/// A cool sets both targets to zero, each sent to the machine as zero; and
/// every request of the walk is in the printer's history, printer-scoped.
fn cooled(world: &World) {
    let log = serial_log_length();
    carried_out(world, "recover-cool", &[]);
    heating_to(world, "the cool", (Some(0.0), Some(0.0)));
    until("both targets sent as zero", || {
        let sent = serial_log_after(log);
        let tool = sent
            .lines()
            .any(|line| line.contains("Send:") && line.contains("M104") && line.contains("S0"));
        let bed = sent
            .lines()
            .any(|line| line.contains("Send:") && line.contains("M140 S0"));
        if tool && bed { Ok(()) } else { Err(sent) }
    });

    let (_, history) = run(
        world,
        "printer-history",
        &["--kinds", "recovery_executed", "--limit", "100"],
    );
    let executed = history["events"].as_array().expect("the history's events");
    assert_eq!(executed.len(), 11, "{history:#}");
    assert!(executed.iter().all(|event| event["print_id"].is_null()));
}

/// A print printobserver started, run to its end, leaves Z trusted, and each
/// move over the part allowed by the clearance measured above it.
pub fn after_a_print_printobserver_started_and_finished() {
    let world = recovering_world("z_plus");
    upload(FINISH);
    idle(&world);
    let print_id = started(&world, FINISH);
    ended(&world, &print_id);

    let read = status(&world);
    assert!(
        read["z_trust"].get("trusted_since").is_some(),
        "a print printobserver started left Z untrusted: {read:#}"
    );
    assert_eq!(read["last_print"]["print_id"], print_id.as_str());
    assert_eq!(
        read["last_print"]["tallest_mm"], FINISH_TALLEST_MM,
        "{read:#}"
    );
    assert_eq!(read["position"]["z"], 20.0, "{read:#}");
    for action in ["recover_heat", "recover_park", "recover_home_xy"] {
        assert_eq!(allowance(&read, action), None, "{action}: {read:#}");
    }
    put_back(&world);
}

/// A recovery executed while a print printobserver started is paused makes it
/// not resumable: the recovery read says so, and its resume is refused naming
/// the recovery that did it, the machine still paused where the recovery left
/// it. A paused print given no recovery resumes.
pub fn a_recovery_under_a_paused_print_makes_it_not_resumable() {
    let world = recovering_world("z_plus");
    upload(PART);
    idle(&world);
    let print_id = started(&world, PART);
    let (ok, paused) = ask(&world, "pause", &["--print-id", &print_id]);
    assert!(ok, "the pause was refused: {paused:#}");
    reads(&world, "paused");

    let before = trusted_position(&world);
    let lifted = carried_out(&world, "recover-lift", &["--mm", "5"]);
    let lifted_to = [before[0], before[1], before[2] + 5.0];
    trusted_at(&world, "the lift under the paused print", lifted_to);
    let recovery_event_id = lifted["events"]
        .as_array()
        .and_then(|events| {
            events
                .iter()
                .find(|event| event["kind"] == "recovery_executed")
        })
        .and_then(|event| event["id"].as_str())
        .expect("the lift recorded its execution")
        .to_owned();

    assert_eq!(
        status(&world)["paused_print"],
        json!({ "print_id": print_id, "resumable": false })
    );
    let (ok, resumed) = ask(&world, "resume", &["--print-id", &print_id]);
    assert!(!ok, "a print a recovery moved was resumed: {resumed:#}");
    assert_eq!(
        resumed["record"]["decision"]["rejected"],
        json!({ "print_not_resumable": { "recovery_event_id": recovery_event_id } })
    );
    std::thread::sleep(Duration::from_secs(2));
    assert_eq!(state(&world), "paused");
    assert!(
        near(trusted_position(&world), lifted_to),
        "the refused resume moved the head"
    );

    // A paused print given no recovery resumes.
    idle(&world);
    let print_id = started(&world, PART);
    let (ok, paused) = ask(&world, "pause", &["--print-id", &print_id]);
    assert!(ok, "the pause was refused: {paused:#}");
    reads(&world, "paused");
    assert_eq!(
        status(&world)["paused_print"],
        json!({ "print_id": print_id, "resumable": true })
    );
    let (ok, resumed) = ask(&world, "resume", &["--print-id", &print_id]);
    assert!(
        ok,
        "a paused print no recovery moved was refused: {resumed:#}"
    );
    reads(&world, "printing");
    put_back(&world);
}

/// Where away from the bed is down the axis, a lift moves Z down by what was
/// asked, never towards the bed.
pub fn a_lift_moves_away_from_the_bed_whichever_way_that_is() {
    let world = recovering_world("z_minus");
    upload(FINISH);
    idle(&world);
    let print_id = started(&world, FINISH);
    ended(&world, &print_id);
    let before = trusted_position(&world);
    carried_out(&world, "recover-lift", &["--mm", "5"]);
    trusted_at(
        &world,
        "the lift down the axis",
        [before[0], before[1], before[2] - 5.0],
    );
    put_back(&world);
}
