//! A served answer is the output, whatever status it came under.
//!
//! What a command prints is the document its supervisor answered. That holds
//! on the paths this program will not act on too: an answer that is a document
//! is printed, in whichever rendering was asked for, and the exit and a
//! sentence on standard error say what the caller does next.

use printobserver::failure::Exit;
use printobserver::render::{Rendering, render};
use printobserver_types::serde_json::{self, Value};

use crate::machine::Reports;
use crate::traced::Ran;
use crate::world::{OTHER_CREDENTIAL, World};

use super::running;

/// Both renderings a caller can ask for, and the flag each is asked for by.
const RENDERINGS: [(Rendering, Option<&str>); 2] = [
    (Rendering::Machine, Some("--json")),
    (Rendering::Labelled, None),
];

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

    for (rendering, flag) in RENDERINGS {
        let mut arguments = vec![
            "pause",
            "--print-id",
            &print_id,
            "--actor",
            "operator",
            "--reason",
            "a pause the store cannot write down",
        ];
        arguments.extend(flag);
        world.proxy.forget();
        let ran = running::command(world, &arguments);
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
        printed_the_answer(&ran, &answered, rendering);
    }
    held.execute_batch("ROLLBACK")
        .expect("the store's write lock is let go");
}

/// A request the supervisor refused to authenticate still prints the document
/// the supervisor answered it with.
///
/// The supervisor serves under the world's own credential, so the same command
/// configured with the control credential is answered `401`. That answer is a
/// configuration to change, so the command exits `unconfigured` and standard
/// error says where the credential is read from — and standard output carries
/// exactly the document the proxy saw the supervisor send, so a caller reading
/// `--json` reads the refusal rather than nothing.
pub fn a_refused_credential_still_prints_the_supervisors_document(world: &World) {
    world.wants(Reports::Printing);
    let print_id = world.print_id();

    for (rendering, flag) in RENDERINGS {
        let arguments: Vec<String> = [
            "pause",
            "--print-id",
            &print_id,
            "--actor",
            "operator",
            "--reason",
            "a pause under a credential the supervisor refuses",
        ]
        .into_iter()
        .chain(flag)
        .map(str::to_owned)
        .collect();
        world.proxy.forget();
        let ran = running::configured(world, &arguments, OTHER_CREDENTIAL);
        let answered = world.proxy.the_one_request().answered;

        assert_eq!(
            ran.code,
            Some(i32::from(Exit::Unconfigured.status())),
            "a refused credential exited otherwise than `unconfigured`: {}",
            ran.said()
        );
        assert!(
            ran.err
                .contains("refused the credential this program presented")
                && ran.err.contains("`credential` in the `[client]` table")
                && ran.err.contains("PRINTOBSERVER_CREDENTIAL"),
            "a refused credential did not say on standard error where the credential is read \
             from: {}",
            ran.said()
        );
        printed_the_answer(&ran, &answered, rendering);
    }
}

/// Require standard output to be the supervisor's own answer, in `rendering`.
///
/// The machine rendering is held to the document the proxy saw the supervisor
/// send, parsed; the labelled one to that same document's labelled lines. A
/// run that printed nothing fails naming its exit and its standard error.
fn printed_the_answer(ran: &Ran, answered: &str, rendering: Rendering) {
    let sent: Value = serde_json::from_str(answered)
        .unwrap_or_else(|error| panic!("the supervisor's answer is no document ({error})"));
    assert!(
        !ran.out.is_empty(),
        "`{}` printed nothing for an answer it was sent; it exited {:?} and wrote to standard \
         error: {:?}",
        ran.arguments.join(" "),
        ran.code,
        ran.err
    );
    match rendering {
        Rendering::Machine => {
            let printed: Value = serde_json::from_str(&ran.out).unwrap_or_else(|error| {
                panic!(
                    "`{}` printed no document ({error}); it exited {:?} and wrote to standard \
                     error: {:?}",
                    ran.arguments.join(" "),
                    ran.code,
                    ran.err
                )
            });
            assert_eq!(
                printed,
                sent,
                "`{}` printed something other than the supervisor's document",
                ran.arguments.join(" ")
            );
        }
        Rendering::Labelled => assert_eq!(
            ran.out,
            render(&sent, Rendering::Labelled),
            "`{}` printed something other than the supervisor's document as labelled lines",
            ran.arguments.join(" ")
        ),
    }
}
