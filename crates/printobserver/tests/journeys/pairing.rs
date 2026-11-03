//! A start's build sheet and the material check, through this program.
//!
//! The compiled program as a subprocess, against a real supervisor whose
//! machine serves no file's settings — so with a spool loaded every start's
//! material check refuses its file as unread, which is what lets one journey
//! walk each check in turn. A start naming no sheet, then each refusal the
//! next override gets past, exits with the rejected class and says what to do;
//! the start asking every override is carried out.

use printobserver::failure::Exit;
use printobserver_types::serde_json::{Value, json};

use crate::machine::Reports;
use crate::traced::Ran;
use crate::world::World;

use super::running;

/// The reason every step of this journey gives.
const REASON: &str = "the operator is starting a print on the sheet that is down";

/// A start's manifest, declaring a sheet or none.
fn manifest(file: &str, sheet: Option<&str>) -> String {
    let mut manifest = json!({
        "file_name": file,
        "material": "PLA",
        "nozzle_diameter_mm": 0.4,
        "slicer_profile": "the pairing journey's profile",
        "allowed": {},
        "metadata": {},
    });
    if let Some(sheet) = sheet {
        manifest["sheet"] = json!(sheet);
    }
    manifest.to_string()
}

/// One start of the world's file as the operator, on a sheet or none, asking
/// these overrides — each set `true` — and `--json` when it is among them.
fn started(world: &World, sheet: Option<&str>, overrides: &[&str]) -> Ran {
    world.wants(Reports::Operational);
    let file = world.printable_file();
    let manifest = manifest(&file, sheet);
    let print = world.print_id();
    let mut asked = vec![
        "start-print",
        "--print-id",
        &print,
        "--file-name",
        &file,
        "--manifest",
        &manifest,
        "--actor",
        "operator",
        "--reason",
        REASON,
    ];
    for name in overrides {
        asked.push(name);
        if *name != "--json" {
            asked.push("true");
        }
    }
    running::command(world, &asked)
}

/// Assert one start exited with the rejected class, saying each of these.
fn assert_rejected(ran: &Ran, saying: &[&str]) {
    assert_eq!(
        ran.code,
        Some(i32::from(Exit::Rejected.status())),
        "{}",
        ran.said()
    );
    for said in saying {
        assert!(
            ran.err.contains(said),
            "the note does not say {said:?}: {}",
            ran.err
        );
    }
}

/// Register a full roll of one material and load it onto tool 0.
fn registered_and_loaded(world: &World, material: &str) {
    let name = format!("{material} on the sheet journey");
    let spool = running::read(
        world,
        &[
            "register-spool",
            "--name",
            &name,
            "--material",
            material,
            "--initial-g",
            "1000",
            "--actor",
            "operator",
            "--reason",
            REASON,
        ],
    )["spool"]["id"]
        .as_str()
        .expect("a registration answers its spool")
        .to_owned();
    let loaded = running::command(
        world,
        &[
            "load-spool",
            "--spool-id",
            &spool,
            "--tool",
            "0",
            "--actor",
            "operator",
            "--reason",
            REASON,
        ],
    );
    assert_eq!(loaded.code, Some(0), "{}", loaded.said());
}

/// A start naming no sheet is rejected whatever it overrides; then the
/// material refusal, and each sheet refusal past it — a sheet the table does
/// not allow the material on, a sheet it does not list, a material it has no
/// entry for — each name the override that gets past it; and the start asking
/// every override is carried out.
pub fn each_override_gets_a_start_past_its_own_check(world: &World) {
    registered_and_loaded(world, "PLA");

    let every = [
        "--override-filament-check",
        "--override-material-check",
        "--override-sheet-check",
    ];
    assert_rejected(
        &started(world, None, &every),
        &["sheet_not_declared", "`sheet`", "No override"],
    );
    assert_rejected(
        &started(world, Some("textured-pei"), &every[..1]),
        &[
            "material_refused (unread)",
            "--override-material-check true",
        ],
    );
    assert_rejected(
        &started(world, Some("textured-pei"), &every[..2]),
        &[
            "sheet_refused (not_allowed)",
            "allows `PLA` on `smooth-pei` and not on `textured-pei`",
            "--override-sheet-check true",
        ],
    );
    assert_rejected(
        &started(world, Some("glass"), &every[..2]),
        &[
            "sheet_refused (unknown_sheet)",
            "lists `glass`",
            "`smooth-pei`, `textured-pei`",
            "--override-sheet-check true",
        ],
    );
    registered_and_loaded(world, "ASA");
    assert_rejected(
        &started(world, Some("smooth-pei"), &every[..2]),
        &[
            "sheet_refused (unlisted_material)",
            "no entry for `ASA`",
            "[sheets.allowed]",
            "--override-sheet-check true",
        ],
    );
    assert_eq!(world.reported_state(), Some(Reports::Operational));

    let mut asking = every.to_vec();
    asking.push("--json");
    let carried = started(world, Some("textured-pei"), &asking);
    assert_eq!(carried.code, Some(0), "{}", carried.said());
    let answer: Value = printobserver_types::serde_json::from_str(&carried.out)
        .unwrap_or_else(|error| panic!("the start answered {error}: {}", carried.out));
    let action = &answer["record"]["request"]["action"];
    for field in ["override_material_check", "override_sheet_check"] {
        assert_eq!(action[field], json!(true), "{answer}");
    }
    assert_eq!(action["manifest"]["sheet"], "textured-pei", "{answer}");
    assert_eq!(answer["record"]["decision"], "accepted", "{answer}");
}
