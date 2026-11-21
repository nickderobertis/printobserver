//! A first start names no print, and a start naming something that is no print
//! is refused saying what to pass, against a real supervisor the server command
//! started.
//!
//! A start with no print open opens a print of its own, so the operator — or
//! the operator's agent — starting a first print has no print to name. The
//! program mints the identifier the call is made under, and the answer names
//! the print the job runs under, which is the one `prints` lists and the one to
//! act on next.

use printobserver_types::PrintId;
use printobserver_types::serde_json::Value;

use crate::machine::Reports;
use crate::walk::manifest_of;
use crate::world::World;

use super::running;

/// Why the operator starts the print.
const REASON: &str = "the camera's frame shows a clear bed and the sheet the file is sliced for";

/// The values a start takes beside its print, each override given so the
/// start is refused for nothing but what the journey asks about.
fn start(world: &World, print: Option<&str>) -> crate::traced::Ran {
    let file = world.printable_file();
    let manifest = manifest_of("start-print", 1.3, &file).to_string();
    let mut asked = vec!["start-print"];
    if let Some(print) = print {
        asked.extend(["--print-id", print]);
    }
    asked.extend([
        "--actor",
        "operator",
        "--reason",
        REASON,
        "--file-name",
        &file,
        "--manifest",
        &manifest,
        "--override-filament-check",
        "true",
        "--override-material-check",
        "true",
        "--override-sheet-check",
        "true",
        "--json",
    ]);
    running::command(world, &asked)
}

/// A start naming no print id starts the print under one this program minted,
/// and its answer names the print `prints` then lists; a start naming
/// something that is no print id is refused, saying what to pass.
pub fn a_first_start_names_no_print(world: &World) {
    world.wants(Reports::Operational);
    for named in ["new", "00000000-0000-0000-0000-000000000000"] {
        let refused = start(world, Some(named));
        assert_ne!(refused.code, Some(0), "a start naming `{named}` went ahead");
        let said = refused.said();
        assert!(
            said.contains("printobserver prints") && said.contains("may name none"),
            "a start naming `{named}` was refused without saying what to pass: {said}"
        );
    }

    let started = start(world, None);
    assert_eq!(
        started.code,
        Some(0),
        "a start naming no print did not start: {}",
        started.said()
    );
    let answer: Value =
        printobserver_types::serde_json::from_str(&started.out).unwrap_or_else(|error| {
            panic!("the start answered no document ({error}): {}", started.out)
        });
    let print = answer["record"]["print_id"]
        .as_str()
        .unwrap_or_else(|| panic!("the start's answer names no print: {answer:#}"));
    print
        .parse::<PrintId>()
        .unwrap_or_else(|error| panic!("the start's answer names no print id: {error}"));
    let listed = running::read(world, &["prints"]);
    assert_eq!(
        listed["active"], print,
        "the print the start answered is not the one `prints` lists: {listed:#}"
    );
}
