//! What a command's output may carry: values its server's answer carried.
//!
//! # A closed rule rather than a hunt
//!
//! "Those bytes or any encoding of them" is a universal over an open set —
//! textual, compressed, segmented and transformed encodings are unbounded — and
//! no finite run over emitted values can settle it. What is settled here
//! instead is the equality: on every path where a command has a response to
//! answer from, every value its output carries is one that response carried,
//! carried unchanged. Because no response shape this system declares carries a
//! byte sequence or a string declared as image content, a value derived from an
//! image's content is not a value any response carried, and fails the equality
//! wherever it is put.
//!
//! The absolute path a materialization prints is no exception and needs none:
//! the server's own rule is that it answers a path on its own host, so that
//! path is a value the response carried.

use printobserver::render::fields;
use printobserver_types::serde_json::{self, Value};

use crate::traced::Ran;
use crate::walk::{self, Driven};
use crate::world::World;

use super::answers::labelled;
use super::{failures, running};

/// Every client command, on every path it answers from, carries only what it
/// was given.
pub fn every_output_carries_only_what_the_answer_carried(world: &World) {
    for one in walk::walk(world) {
        on_this_path(world, &one, &failures::succeeding(&one));
        if one.operation().action_kind().is_some() {
            on_this_path(world, &one, &failures::rejected(world, &one));
        }
    }
}

/// One path of one command, in both renderings.
///
/// Each rendering is compared against **its own** answer rather than against
/// the other one's: an answer carries the instant it was observed, so two runs
/// of one command differ in it by design, and comparing across them would be
/// comparing two facts rather than one.
fn on_this_path(world: &World, one: &Driven, arguments: &[String]) {
    let (machine, machine_answered) = run(world, one, arguments, true);
    let printed: Vec<(String, String)> =
        fields(&document(&machine.out, "this program's own output"));
    assert_eq!(
        printed,
        fields(&document(&machine_answered, "the supervisor's own answer")),
        "`{}` printed a value the supervisor's own answer did not carry",
        one.command.name
    );

    let (plain, plain_answered) = run(world, one, arguments, false);
    let carried = fields(&document(&plain_answered, "the supervisor's own answer"));
    let lines = labelled(&plain.out);
    for (at, value) in &carried {
        assert_eq!(
            lines.get(at),
            Some(value),
            "`{}` printed `{at}` as something other than what the answer carried",
            one.command.name
        );
    }
    assert_eq!(
        lines.len(),
        carried.len(),
        "`{}` printed a labelled line for something the answer did not carry: {lines:#?}",
        one.command.name
    );
}

/// One document, or a panic saying whose it was.
fn document(text: &str, whose: &str) -> Value {
    serde_json::from_str(text)
        .unwrap_or_else(|error| panic!("{whose} is not a document ({error}): {text}"))
}

/// One run, and the whole body the supervisor answered it with.
///
/// The machine is put back where the command needs it before **each** run
/// rather than before the pair: an action moves it, so the second run of a
/// resume would be one the policy refuses from the state the first left.
fn run(world: &World, one: &Driven, arguments: &[String], machine_readable: bool) -> (Ran, String) {
    world.wants(one.reports);
    let mut asked: Vec<&str> = arguments.iter().map(String::as_str).collect();
    if machine_readable {
        asked.push("--json");
    }
    world.proxy.forget();
    let ran = running::command(world, &asked);
    (ran, world.proxy.the_one_request().answered)
}
