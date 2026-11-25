//! A served answer is the output, whatever status it came under.
//!
//! What a command prints is the document its supervisor answered. That holds
//! on the paths this program will not act on too: an answer that is a document
//! is printed, and the exit and a sentence on standard error say what the
//! caller does next.

use printobserver::failure::Exit;
use printobserver_types::serde_json::{self, Value};

use crate::machine::Reports;
use crate::world::World;

use super::running;

/// A request the supervisor served and this program will not act on still
/// prints the document the supervisor answered.
///
/// The condition is one a slow host meets on its own: a write to the store that
/// another connection holds the lock for outlasts the store's lock timeout, and
/// the supervisor answers the request it was making the write for under `500`,
/// with a document saying why. Holding the lock from a second connection here
/// is that writer, stalled for as long as the command takes; the store is
/// opened through its own `connect`, so it is the store's own lock being held.
///
/// The request was made and answered, so the answer is the output: the
/// caller's shell sees `refused`, standard error says which status came back,
/// and standard output carries exactly the document the proxy saw the
/// supervisor send — rather than nothing, which a caller reading `--json` can
/// only read as no answer at all.
pub fn an_answer_it_will_not_act_on_still_prints_its_document(world: &World) {
    world.wants(Reports::Printing);
    let print_id = world.print_id();
    let held = printobserver_store_sqlite::connect(
        &world
            .root
            .path()
            .join("state")
            .join(printobserver_store_sqlite::DATABASE_FILE_NAME),
    )
    .expect("the supervisor's store opens beside it");
    held.execute_batch("BEGIN IMMEDIATE")
        .expect("the store's write lock is taken");

    world.proxy.forget();
    let ran = running::command(
        world,
        &[
            "pause",
            "--print-id",
            &print_id,
            "--actor",
            "operator",
            "--reason",
            "a pause the store cannot write down",
            "--json",
        ],
    );
    held.execute_batch("ROLLBACK")
        .expect("the store's write lock is let go");
    let answered = world.proxy.the_one_request().answered;

    assert_eq!(
        ran.code,
        Some(i32::from(Exit::Refused.status())),
        "a served answer this program will not act on exited otherwise: {}",
        ran.said()
    );
    assert!(
        ran.err.contains("answered 500"),
        "the store's stalled write was not what the supervisor answered: {}",
        ran.said()
    );
    let printed: Value = serde_json::from_str(&ran.out).unwrap_or_else(|error| {
        panic!(
            "a served answer printed no document ({error}); it exited {:?} and wrote to \
                 standard error: {:?}",
            ran.code, ran.err
        )
    });
    let sent: Value = serde_json::from_str(&answered)
        .unwrap_or_else(|error| panic!("the supervisor's answer is no document ({error})"));
    assert_eq!(
        printed, sent,
        "a served answer printed something other than the supervisor's document"
    );
}
