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
    over_one_snapshot(
        |machine_readable| run(world, arguments, machine_readable, from),
        operation,
        answer,
    );
}

/// Compare actual command reads, allowing the caller to advance the real
/// supervisor between them when exercising the observation boundary.
pub fn over_one_snapshot(
    mut take: impl FnMut(bool) -> Ran,
    operation: &printobserver_server::Operation,
    answer: Answer,
) {
    for attempt in 0..HISTORY_ATTEMPTS {
        let machine = take(true);
        let plain = take(false);
        // Validate each rendering even when background producers advanced the
        // state: an invalid document must never be hidden by another read.
        the_machine_output_is_a_document_the_schema_admits(&machine, operation, answer);
        the_default_output_is_not_that_document(&plain);
        if matches!(operation.name, "history" | "context")
            && snapshot_ids(&answered(&machine, true)) != snapshot_ids(&labelled(&plain.out))
            && attempt + 1 < HISTORY_ATTEMPTS
        {
            continue;
        }
        the_two_renderings_agree(&machine, &plain, operation, answer);
        return;
    }
}

/// The existing history-read bound also covers context's moving event window.
const HISTORY_ATTEMPTS: usize = 5;

/// Event and image identities distinguish actual snapshots before comparing
/// their complete field sets and values. No field is removed from that check.
fn snapshot_ids(fields: &BTreeMap<String, String>) -> BTreeMap<&String, &String> {
    fields
        .iter()
        .filter(|(at, _)| {
            at.as_str() == "context.latest_image.id"
                || ((at.starts_with("events.") || at.starts_with("context.recent_events."))
                    && at.rsplit('.').next() == Some("id"))
        })
        .collect()
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
        // The instants — `*_at`, or an `at` of a state that happened then — and
        // the identifiers an answer carries differ between two runs by design, so what is compared here is the field set and the
        // values that do not: a value that moved is one of those, and one that
        // did not is a field carrying something different in the two renderings.
        if at.ends_with("_at") || at.rsplit('.').next() == Some("at") || at.ends_with("id") {
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

/// A real alert/image arrives between two real CLI reads of one supervisor.
#[test]
fn parity_survives_an_image_and_event_arriving_between_reads() {
    let world = World::open(crate::world::STOOD_IN);
    let operation = printobserver_server::operation("context").expect("context operation");
    let mut reads = 0;
    over_one_snapshot(
        |machine_readable| {
            reads += 1;
            if reads == 2 {
                world.freshen_the_image();
            }
            let mut arguments = vec!["context", "--print-id"];
            let print = world.print_id();
            arguments.push(&print);
            if machine_readable {
                arguments.push("--json");
            }
            running::command(&world, &arguments)
        },
        operation,
        Answer::Success,
    );
    assert!(reads > 2, "the changed snapshot was not encountered");
}

/// A moving producer cannot make the comparison pass without observing parity.
#[test]
fn parity_refuses_snapshots_that_keep_changing_through_the_existing_bound() {
    let world = World::open(crate::world::STOOD_IN);
    let operation = printobserver_server::operation("context").expect("context operation");
    let mut reads = 0;
    // The closure only reads commands and appends fixture events. Its World is
    // discarded after refusal; no partially completed comparison is reused.
    let refused = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        over_one_snapshot(
            |machine_readable| {
                reads += 1;
                if !machine_readable {
                    world.freshen_the_image();
                }
                let print = world.print_id();
                let mut arguments = vec!["context", "--print-id", &print];
                if machine_readable {
                    arguments.push("--json");
                }
                running::command(&world, &arguments)
            },
            operation,
            Answer::Success,
        );
    }))
    .expect_err("continually different snapshots were accepted as parity");
    let message = refused
        .downcast_ref::<String>()
        .expect("parity failure text");
    assert!(message.contains("in its two renderings"), "{message}");
    assert_eq!(reads, HISTORY_ATTEMPTS * 2);
}
