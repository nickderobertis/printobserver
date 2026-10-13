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

use std::collections::{BTreeMap, BTreeSet};

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
        let arguments = failures::succeeding(&one);
        both_ways(world, &arguments, &one.operation(), Answer::Success, None);
        if one.operation().granted_as().is_some() {
            let refused = failures::rejected(world, &one);
            both_ways(
                world,
                &refused,
                &one.operation(),
                Answer::Rejected,
                Some(failures::rejected_from(&one)),
            );
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
        if operation.granted_as().is_some() {
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
    from: Option<crate::machine::Reports>,
) {
    let (machine, plain) =
        over_one_history(|machine_readable| run(world, arguments, machine_readable, from));
    the_two_renderings_agree(&machine, &plain, operation, answer);
}

/// How many pairs a moved history is given before the last one is ruled on.
const ATTEMPTS: usize = 3;

/// Both renderings of one invocation, of one history.
///
/// The supervisor observes a printing print on a schedule of its own, and every
/// observation lands in that print's history, so a run of an answer carrying
/// that history can be of a different history than the run beside it — which
/// two renderings of one answer then disagree over without either being wrong.
/// So when the pair disagrees, the document is asked for once more straight
/// after the labelled lines: one that then carries exactly what those lines
/// carry is of the history they were of, and is the document they are held to.
/// Otherwise the pair is taken again, and the last pair taken is what is ruled
/// on, so a disagreement that is the rendering's own still fails naming it.
pub fn over_one_history(take: impl Fn(bool) -> Ran) -> (Ran, Ran) {
    let mut machine = take(true);
    let mut plain = take(false);
    for _ in 1..ATTEMPTS {
        if comparable(&answered(&machine, true)) == comparable(&labelled(&plain.out)) {
            break;
        }
        let after = take(true);
        if comparable(&answered(&after, true)) == comparable(&labelled(&plain.out)) {
            return (after, plain);
        }
        machine = take(true);
        plain = take(false);
    }
    (machine, plain)
}

/// One answer's fields, with the values two runs differ in by design left out.
fn comparable(answer: &BTreeMap<String, String>) -> BTreeMap<&String, Option<&String>> {
    answer
        .iter()
        .map(|(at, value)| (at, (!differs_between_runs(at)).then_some(value)))
        .collect()
}

/// Whether a field carries an instant or an identifier, which differ between
/// two runs by design.
fn differs_between_runs(at: &str) -> bool {
    at.ends_with("_at") || at.ends_with("id")
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
///
/// The machine is put back where the command needs it before **each** run
/// rather than before the pair: an action moves it, so the second run of a
/// resume would be one the policy refuses from the state the first left — and
/// the two renderings would then be of two different answers.
fn run(
    world: &World,
    arguments: &[String],
    machine_readable: bool,
    from: Option<crate::machine::Reports>,
) -> Ran {
    if let Some(state) = from {
        world.wants(state);
    } else if let Some(one) = walk::walk(world)
        .into_iter()
        .find(|found| Some(&found.command.name) == arguments.first())
    {
        world.wants(one.reports);
    }
    if let Some(command) = arguments.first() {
        world.prepare_ledger_for(command);
    }
    let arguments = walk::naming_the_print_and_spool_now(arguments, world);
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
        if differs_between_runs(at) {
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
