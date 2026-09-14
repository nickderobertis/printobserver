//! Finding the print to act on: the listing, the job it adopts, and the alert
//! that joins the print adopted for it.
//!
//! A print started at the printer is known here before any provider reports on
//! it only because a listing adopted it, and an alert about that job afterwards
//! has to land on the same print rather than on a second one — or the operator
//! who found the first is watching a record nothing will ever write to again.

use printobserver_core::Clock as _;
use printobserver_core::block_on;
use printobserver_core::listing::{ACTIVE_STATES, PrintListing};
use printobserver_core::store::PrintStore as _;
use printobserver_printer_api::{PrinterError, PrinterState};
use printobserver_types::PrintId;

use crate::fakes::PrinterMethod;
use crate::journal::{Call, Port};
use crate::world::{World, failure_alert};

/// The file the printer reports it is running, unless a journey says otherwise.
const RUNNING: &str = "benchy.gcode";

/// One listing, read the way the operation reads it.
fn listing(world: &World) -> PrintListing {
    block_on(world.core.prints()).expect("the prints are listed")
}

/// Every identifier a listing carries, in the order it carries them.
fn ids(listing: &PrintListing) -> Vec<PrintId> {
    listing.prints.iter().map(|print| print.id).collect()
}

/// A job the printer is running, with no print open for it, is adopted exactly
/// once — and adopting it asks nothing of the machine but its job.
#[test]
fn a_running_job_nothing_holds_is_adopted_once_and_commands_nothing() {
    let world = World::new();
    world
        .printer
        .reports_job(Some("hold.gcode"), PrinterState::Printing);

    let first = listing(&world);

    let active = first.active.expect("the running job is named active");
    assert_eq!(
        ids(&first),
        vec![active],
        "one print was opened for the job"
    );
    let adopted = &first.prints[0];
    assert_eq!(adopted.provider_print_id, None);
    assert_eq!(adopted.file_name.as_deref(), Some("hold.gcode"));
    assert_eq!(adopted.ended_at, None);
    assert_eq!(
        adopted.state,
        PrinterState::Printing,
        "an adopted print records the state every opened print records"
    );

    world.journal.clear();
    assert_eq!(
        listing(&world),
        first,
        "a second read of the same job answered anything but the first"
    );
    assert!(
        !world.journal.calls().contains(&Call::OpenPrint),
        "a second read of the same job opened a print"
    );
    assert_eq!(
        world.journal.at(Port::Printer),
        vec![Call::Job],
        "listing asked the machine for something beside its job"
    );
    world.journal.assert_no_violations();
}

/// A paused job is adoptable too, and an adopted print still records
/// `printing`: the record says a print was opened, and the machine's own state
/// is the status read's answer.
#[test]
fn a_paused_job_is_adopted_and_records_the_state_an_opened_print_records() {
    let world = World::new();
    world
        .printer
        .reports_job(Some("paused.gcode"), PrinterState::Paused);

    let found = listing(&world);

    let active = found.active.expect("a paused job is active");
    assert_eq!(ids(&found), vec![active]);
    assert_eq!(found.prints[0].state, PrinterState::Printing);
    assert_eq!(
        ACTIVE_STATES,
        [PrinterState::Printing, PrinterState::Paused]
    );
}

/// With a print already open under the job's file, the most recently opened of
/// them is active and nothing is written.
#[test]
fn an_open_print_carrying_the_file_is_named_and_nothing_is_written() {
    let world = World::new();
    let older = world.open_print(7);
    let newer = world.open_print(8);
    let ended = world.open_print(9);
    block_on(world.store.end_print(
        ended.id,
        PrinterState::Operational,
        world.clock.now(),
        "it finished".to_owned(),
    ))
    .expect("the print ends");
    world
        .printer
        .reports_job(Some(RUNNING), PrinterState::Printing);
    world.journal.clear();

    let found = listing(&world);

    assert_eq!(
        found.active,
        Some(newer.id),
        "the active print is not the most recently opened open one carrying the file"
    );
    assert_eq!(
        ids(&found),
        vec![ended.id, newer.id, older.id],
        "the listing is not every print, most recently opened first"
    );
    assert!(
        !world.journal.calls().contains(&Call::OpenPrint),
        "a job with a print open for it opened another"
    );
    world.journal.assert_no_violations();
}

/// A machine with no job in progress names nothing active and opens nothing,
/// whatever file it last ran.
#[test]
fn a_job_in_no_active_state_names_nothing_and_opens_nothing() {
    for state in [
        PrinterState::Operational,
        PrinterState::Cancelling,
        PrinterState::Error,
        PrinterState::Offline,
        PrinterState::Unknown("Starting".to_owned()),
    ] {
        let world = World::new();
        world.printer.reports_job(Some(RUNNING), state.clone());

        let found = listing(&world);

        assert_eq!(found.active, None, "{state:?} was named active");
        assert!(found.prints.is_empty(), "{state:?} opened a print");
    }

    let world = World::new();
    world.printer.reports_job(None, PrinterState::Printing);
    let found = listing(&world);
    assert_eq!(
        found,
        PrintListing {
            prints: Vec::new(),
            active: None
        },
        "a job reporting no file was adopted under no name"
    );
}

/// A printer that cannot be read still lists what the store holds, with
/// nothing active.
#[test]
fn a_printer_that_cannot_be_read_still_lists_the_stored_prints() {
    let world = World::new();
    let held = world.open_print(7);
    world.printer.fails(
        PrinterMethod::Job,
        PrinterError::Unreachable {
            detail: "the machine is switched off".to_owned(),
        },
    );

    let found = listing(&world);

    assert_eq!(found.active, None);
    assert_eq!(ids(&found), vec![held.id]);
}

/// An alert about a job a listing adopted attaches its identifier to that
/// print and is handled against it, rather than opening a second.
#[test]
fn an_alert_about_an_adopted_job_attaches_to_the_adopted_print() {
    let world = World::new();
    world
        .printer
        .reports_job(Some(RUNNING), PrinterState::Printing);
    let adopted = listing(&world).active.expect("the job is adopted");

    let event = world
        .handle(failure_alert(4211))
        .expect("the alert is handled");

    assert_eq!(
        event.print_id,
        Some(adopted),
        "the alert was handled against another print"
    );
    let prints = world.store.prints();
    assert_eq!(
        prints.len(),
        1,
        "the alert opened a second print: {prints:?}"
    );
    assert_eq!(prints[0].provider_print_id, Some(4211));
    assert_eq!(world.agent.turns()[0].print_id, adopted);

    let again = world
        .handle(failure_alert(4211))
        .expect("a second alert is handled");
    assert_eq!(again.print_id, Some(adopted));
    assert_eq!(listing(&world).active, Some(adopted));
    assert_eq!(world.store.prints().len(), 1);
    world.journal.assert_no_violations();
}

/// A print already carrying the alert's identifier wins over one waiting under
/// its file name, and the waiting one is left as it was.
#[test]
fn a_print_carrying_the_identifier_wins_over_one_waiting_under_the_file() {
    let world = World::new();
    let carrying = world.open_print(4211);
    let waiting = block_on(world.store.open_print(None, Some(RUNNING.to_owned())))
        .expect("a print opens with no identifier");

    let event = world
        .handle(failure_alert(4211))
        .expect("the alert is handled");

    assert_eq!(event.print_id, Some(carrying.id));
    assert_eq!(
        world
            .store
            .print_now(waiting.id)
            .and_then(|print| print.provider_print_id),
        None,
        "the waiting print took an identifier another print already carries"
    );
}

/// An alert opens a print of its own when no open print waits for it: none
/// carries the file, or the one that does already carries another identifier,
/// or has ended.
#[test]
fn an_alert_opens_a_print_when_no_open_print_waits_for_it() {
    let world = World::new();
    let other_job = block_on(world.store.open_print(None, Some("other.gcode".to_owned())))
        .expect("a print of another file opens");
    let attached_elsewhere = world.open_print(9);
    let ended =
        block_on(world.store.open_print(None, Some(RUNNING.to_owned()))).expect("a print opens");
    block_on(world.store.end_print(
        ended.id,
        PrinterState::Operational,
        world.clock.now(),
        "it finished".to_owned(),
    ))
    .expect("the print ends");

    let event = world
        .handle(failure_alert(4211))
        .expect("the alert is handled");

    let opened = event.print_id.expect("the alert names a print");
    for existing in [other_job.id, attached_elsewhere.id, ended.id] {
        assert_ne!(
            opened, existing,
            "the alert attached to a print that was not waiting"
        );
    }
    let record = world.store.print_now(opened).expect("the print was opened");
    assert_eq!(record.provider_print_id, Some(4211));
    assert_eq!(record.file_name.as_deref(), Some(RUNNING));
    assert!(
        !world
            .journal
            .calls()
            .contains(&Call::AttachObicoPrint(4211)),
        "an identifier was attached with no print waiting for it"
    );
}
