//! The operator's word that the bed is clear, given with `printobserver
//! bed-clear` and honoured by the `recover-*` commands, against a real
//! supervisor the server command started.
//!
//! The word is read back the way an operator reads it — `printer-history` —
//! and each recovery it stands in for is asked with no assertion of its own
//! and answers the word it was carried out on. A print started afterwards
//! ends it, and a word claimed for the agent is refused. The tall-print scene
//! the word exists for, the restart and a policy that does not count it are
//! the server tier's own journey (`printobserver-server`'s
//! `journeys/bed_clear.rs`), over a machine that serves the print's G-code.

use printobserver_types::serde_json::Value;

use crate::machine::Reports;
use crate::walk::manifest_of;
use crate::world::World;

use super::running;

/// Why the operator says the bed is clear.
const REASON: &str = "the cancelled print is off the bed and nothing else is on it";

/// Ask for one command as the operator, with a reason, answering whether it
/// succeeded and the document it printed.
fn as_the_operator(world: &World, command: &str, arguments: &[&str]) -> (Option<i32>, Value) {
    let mut asked = vec![command, "--actor", "operator", "--reason", REASON];
    asked.extend_from_slice(arguments);
    asked.push("--json");
    let ran = running::command(world, &asked);
    let document = printobserver_types::serde_json::from_str(&ran.out).unwrap_or(Value::Null);
    (ran.code, document)
}

/// The recoveries the word stands in for, each with no assertion of its own.
const UNASSERTED: [(&str, &[&str]); 4] = [
    ("recover-home-z", &[]),
    ("recover-park", &[]),
    ("recover-heat", &["--tool", "0", "--target-c", "150"]),
    ("recover-home-xy", &[]),
];

/// `printobserver bed-clear` records the operator's word with its actor and
/// reason; each recovery that needs the bed or the nozzle clear is then
/// carried out with no assertion of its own, naming the word in its audit;
/// a print started through printobserver ends it, and a word claimed for the
/// agent is refused.
pub fn the_operators_word_is_honoured_until_a_print_starts(world: &World) {
    world.wants(Reports::Operational);
    let (code, refused) = as_the_operator(world, "recover-home-z", &[]);
    assert_ne!(
        code,
        Some(0),
        "a Z home went ahead on nobody's word: {refused:#}"
    );

    let (code, given) = as_the_operator(world, "bed-clear", &[]);
    assert_eq!(code, Some(0), "`bed-clear` was not recorded: {given:#}");
    let recorded = given["event"].clone();
    assert_eq!(recorded["kind"], "bed_clear_asserted", "{given:#}");
    assert_eq!(recorded["payload"]["actor"], "operator", "{given:#}");
    assert_eq!(recorded["payload"]["reason"], REASON, "{given:#}");
    let read = running::read(
        world,
        &[
            "printer-history",
            "--kinds",
            "bed_clear_asserted",
            "--limit",
            "1",
        ],
    );
    assert_eq!(read["events"][0]["id"], recorded["id"], "{read:#}");
    assert_eq!(
        read["events"][0]["payload"], recorded["payload"],
        "{read:#}"
    );
    assert_eq!(read["events"][0]["print_id"], Value::Null, "{read:#}");

    for (command, arguments) in UNASSERTED {
        let (code, answer) = as_the_operator(world, command, arguments);
        assert_eq!(
            code,
            Some(0),
            "`{command}` was refused on the word: {answer:#}"
        );
        assert_eq!(answer["decision"]["executed"], "succeeded", "{command}");
        for event in answer["events"].as_array().expect("the audit") {
            assert_eq!(
                event["payload"]["bed_clear"], recorded["id"],
                "`{command}` does not name the word it was carried out on: {event:#}"
            );
        }
    }

    let agent = r#"{"agent":{"session_name":"print-a-session-of-its-own"}}"#;
    let claimed = running::command(
        world,
        &["bed-clear", "--actor", agent, "--reason", REASON, "--json"],
    );
    assert_ne!(
        claimed.code,
        Some(0),
        "a word claimed for the agent was taken"
    );
    let after = running::read(
        world,
        &[
            "printer-history",
            "--kinds",
            "bed_clear_asserted",
            "--limit",
            "5",
        ],
    );
    assert_eq!(
        after["events"].as_array().map(Vec::len),
        Some(1),
        "a refused word was recorded: {after:#}"
    );

    a_print_started_ends_it(world);
}

/// A print started through printobserver ends the word: a Z home asserting
/// nothing is refused again, naming no word.
fn a_print_started_ends_it(world: &World) {
    let file = world.printable_file();
    let manifest = manifest_of("start-print", 1.3, &file).to_string();
    let print_id = world.print_id();
    let (code, started) = as_the_operator(
        world,
        "start-print",
        &[
            "--print-id",
            &print_id,
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
        ],
    );
    assert_eq!(code, Some(0), "the next print did not start: {started:#}");
    world.wants(Reports::Operational);
    let (code, answer) = as_the_operator(world, "recover-home-z", &[]);
    assert_ne!(
        code,
        Some(0),
        "a Z home went ahead on a word a print ended: {answer:#}"
    );
    for event in answer["events"].as_array().into_iter().flatten() {
        assert!(
            event["payload"].get("bed_clear").is_none(),
            "a Z home named a word a print ended: {event:#}"
        );
    }
}
