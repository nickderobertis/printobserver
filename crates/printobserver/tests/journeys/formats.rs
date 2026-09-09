//! Both renderings, on every path a command can answer from.
//!
//! Comparing the two field sets alone would be satisfied by two identical
//! unparseable answers, so each is also held to being the kind of output it is
//! meant to be: the machine-readable one parses under a real parser and
//! validates against the schema the server declares for the answer that path
//! produced, and the default one does not parse under that parser at all and
//! carries those same fields as labelled lines.
//!
//! The walk is over every path, not over the success path: a command whose two
//! renderings agree when it succeeds and diverge when the policy refuses it
//! satisfies a one-path reading and misses the point.

use std::collections::BTreeSet;

use printobserver_server::{Answer, OPERATIONS};
use printobserver_types::serde_json::{self, Value};

use crate::traced::Ran;
use crate::walk;
use crate::world::World;

use super::answers::{answered, labelled};
use super::{failures, materializing, running};

/// Every command, on every path it can answer from, in both renderings.
pub fn both_renderings_carry_the_same_fields(world: &World) {
    let covered = paths_this_walk_runs();
    for operation in OPERATIONS {
        for answer in operation.answers_with() {
            assert!(
                covered.contains(&(operation.name, *answer)),
                "`{}` declares a {answer} answer this walk does not run",
                operation.name
            );
        }
    }

    for one in walk::walk(world) {
        world.wants(one.reports);
        let arguments = failures::succeeding(&one);
        both_ways(world, &arguments, &one.operation(), Answer::Success);
        if one.operation().action_kind().is_some() {
            let refused = failures::rejected(world, &one);
            both_ways(world, &refused, &one.operation(), Answer::Rejected);
        }
    }
    materializing::both_renderings_of_a_path_that_names_no_file(world);
}

/// Every pair of an operation and an answer this walk runs.
///
/// Written out from what the walk below does rather than read off what the
/// server declares, which is what makes the comparison above a comparison: a
/// server that declared a third answer would fail it rather than be covered by
/// a set derived from itself.
fn paths_this_walk_runs() -> BTreeSet<(&'static str, Answer)> {
    let mut found = BTreeSet::new();
    for operation in OPERATIONS {
        found.insert((operation.name, Answer::Success));
        if operation.action_kind().is_some() {
            found.insert((operation.name, Answer::Rejected));
        }
    }
    found
}

/// One invocation in both renderings, held to everything both are held to.
pub fn both_ways(
    world: &World,
    arguments: &[String],
    operation: &printobserver_server::Operation,
    answer: Answer,
) {
    let machine = run(world, arguments, true);
    let plain = run(world, arguments, false);
    the_two_renderings_agree(&machine, &plain, operation, answer);
}

/// Two runs of one invocation, held to everything both renderings are held to.
pub fn the_two_renderings_agree(
    machine: &Ran,
    plain: &Ran,
    operation: &printobserver_server::Operation,
    answer: Answer,
) {
    the_machine_output_is_a_document_the_schema_admits(machine, operation, answer);
    the_default_output_is_not_that_document(plain);
    the_two_carry_the_same_fields(machine, plain);
}

/// One invocation, in one rendering.
fn run(world: &World, arguments: &[String], machine_readable: bool) -> Ran {
    let mut asked: Vec<&str> = arguments.iter().map(String::as_str).collect();
    if machine_readable {
        asked.push("--json");
    }
    running::command(world, &asked)
}

/// The machine-readable output parses, and the declared schema admits it.
fn the_machine_output_is_a_document_the_schema_admits(
    ran: &Ran,
    operation: &printobserver_server::Operation,
    answer: Answer,
) {
    let document: Value = serde_json::from_str(&ran.out).unwrap_or_else(|error| {
        panic!(
            "{:?} asked for machine-readable output and answered something that is not a \
             document ({error}): {}",
            ran.arguments, ran.out
        )
    });
    let schema = operation.response_schema(answer);
    let validator = jsonschema::validator_for(&schema).unwrap_or_else(|error| {
        panic!(
            "`{}`'s declared schema is not one ({error})",
            operation.name
        )
    });
    let refused: Vec<String> = validator
        .iter_errors(&document)
        .map(|error| error.to_string())
        .collect();
    assert!(
        refused.is_empty(),
        "`{}`'s {answer} answer is not what its own declared schema admits: {refused:#?}",
        operation.name
    );
}

/// The default output is labelled text rather than that document.
fn the_default_output_is_not_that_document(ran: &Ran) {
    assert!(
        serde_json::from_str::<Value>(&ran.out).is_err(),
        "{:?} answered the machine-readable document without being asked for it: {}",
        ran.arguments,
        ran.out
    );
    assert!(
        !ran.out.trim().is_empty(),
        "{:?} answered nothing at all",
        ran.arguments
    );
}

/// The two renderings carry the same fields, and the same values in them.
fn the_two_carry_the_same_fields(machine: &Ran, plain: &Ran) {
    let document = answered(machine, true);
    let lines = labelled(&plain.out);
    let in_document: BTreeSet<&String> = document.keys().collect();
    let in_lines: BTreeSet<&String> = lines.keys().collect();
    assert_eq!(
        in_document, in_lines,
        "{:?} carries different fields in its two renderings",
        machine.arguments
    );
    for (at, value) in &document {
        // The instants and the identifiers an answer carries differ between two
        // runs by design, so what is compared here is the field set and the
        // values that do not: a value that moved is one of those, and one that
        // did not is a field carrying something different in the two renderings.
        if at.ends_with("_at") || at.ends_with("id") {
            continue;
        }
        assert_eq!(
            lines.get(at),
            Some(value),
            "{:?} carries a different `{at}` in its two renderings",
            machine.arguments
        );
    }
}
