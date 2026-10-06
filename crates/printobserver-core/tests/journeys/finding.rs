//! Finding the print to act on: the listing, the job it adopts, and the alert
//! that joins the print adopted for it.
//!
//! A print started at the printer is known here before any provider reports on
//! it only because a listing adopted it, and an alert about that job afterwards
//! has to land on the same print rather than on a second one — or the operator
//! who found the first is watching a record nothing will ever write to again.

use printobserver_core::Clock as _;
use printobserver_core::block_on;
use printobserver_core::listing::{
    ACTIVE_STATES, JOB_IDENTITY_TOLERANCE_S, PrintListing, REPLACED_REASON,
};
use printobserver_core::store::PrintStore as _;
use printobserver_printer_api::{PrinterError, PrinterState};
use printobserver_types::{PrintId, Timestamp};

use crate::fakes::PrinterMethod;
use crate::journal::{Call, Port};
use crate::world::{World, failure_alert};

/// The file the printer reports it is running, unless a journey says otherwise.
const RUNNING: &str = "benchy.gcode";

fn listing(world: &World) -> PrintListing {
    block_on(world.core.list_and_adopt_prints()).expect("the prints are listed")
}

fn ids(listing: &PrintListing) -> Vec<PrintId> {
    listing.prints.iter().map(|print| print.id).collect()
}

/// A listing and an alert about the same job, resolved at the same moment,
/// leave one print for that job.
///
/// Each reads the open prints and then may write one, so interleaved they could
/// each find none and each open one. The store here makes the two reads wait
/// for one another, which is exactly that interleaving when nothing keeps the
/// two resolutions apart — and a short wait alone when something does.
#[test]
fn a_listing_and_an_alert_resolved_at_once_leave_one_print_for_the_job() {
    let world = World::new();
    world
        .printer
        .reports_job(Some(RUNNING), PrinterState::Printing);
    world.store.reads_of_the_prints_meet(2);

    let (listed, handled) = std::thread::scope(|scope| {
        let listing_one = scope.spawn(|| listing(&world));
        let handling_one = scope.spawn(|| world.handle(failure_alert(4211)));
        (
            listing_one.join().expect("the listing ran"),
            handling_one.join().expect("the handling ran"),
        )
    });

    let prints = world.store.prints();
    assert_eq!(
        prints.len(),
        1,
        "a listing and an alert resolved at once opened a print each: {prints:?}"
    );
    let only = &prints[0];
    assert_eq!(only.provider_print_id, Some(4211), "{prints:?}");
    assert_eq!(listed.active, Some(only.id));
    assert_eq!(
        handled.expect("the alert is handled").print_id,
        Some(only.id)
    );
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

/// When the job the clock now reads began, `running` seconds ago.
fn started(world: &World, running: i64) -> Timestamp {
    world
        .clock
        .now()
        .plus_seconds(-running)
        .expect("a representable instant")
}

/// The printer reports `file` in `state`, `running` seconds into it.
fn reports(world: &World, file: &str, state: PrinterState, running: Option<i64>) {
    world.printer.reports_job(Some(file), state);
    world.printer.reports_running_time(running);
}

/// The printer prints `file`, `running` seconds into it.
fn runs(world: &World, file: &str, running: Option<i64>) {
    reports(world, file, PrinterState::Printing, running);
}

/// The one print the store holds.
fn only_print(world: &World) -> printobserver_core::PrintRecord {
    let prints = world.store.prints();
    assert_eq!(prints.len(), 1, "one job was split into prints: {prints:?}");
    prints[0].clone()
}

/// A print opened for a job records what the read saw of it: when the job
/// began, and — when it was printing — how long it had printed. A job read
/// paused records its start and no printing time, since its running time
/// counts the pause; one reporting no running time records neither.
#[test]
fn a_print_opened_for_a_running_job_records_what_the_read_saw() {
    let world = World::new();
    runs(&world, RUNNING, Some(3024));

    let found = listing(&world);

    let opened = &found.prints[0];
    assert_eq!(found.active, Some(opened.id));
    assert_eq!(
        (opened.job_started_at, opened.job_print_time_s),
        (Some(started(&world, 3024)), Some(3024)),
        "the print does not record the start and printing time the read saw"
    );
    assert_eq!(world.store.print_now(opened.id), Some(opened.clone()));

    let world = World::new();
    reports(&world, RUNNING, PrinterState::Paused, Some(500));
    listing(&world);
    let opened = only_print(&world);
    assert_eq!(
        (opened.job_started_at, opened.job_print_time_s),
        (Some(started(&world, 500)), None),
        "a paused job's running time, which counts the pause, was taken for printing time"
    );

    let world = World::new();
    runs(&world, RUNNING, None);
    listing(&world);
    let opened = only_print(&world);
    assert_eq!(
        (opened.job_started_at, opened.job_print_time_s),
        (None, None),
        "a job reporting no running time was given a sighting nobody made"
    );
}

/// One job read again later, its printing time grown, is the same print — and
/// so is one reporting exactly the tolerance less than the longest the print
/// recorded, which leaves that longest where it was.
#[test]
fn the_same_job_read_later_is_the_same_print() {
    let world = World::new();
    runs(&world, RUNNING, Some(600));
    let first = listing(&world).active.expect("the job is adopted");

    world.clock.advance(900);
    runs(&world, RUNNING, Some(1500));
    assert_eq!(listing(&world).active, Some(first));
    assert_eq!(only_print(&world).job_print_time_s, Some(1500));

    world.clock.advance(300);
    runs(&world, RUNNING, Some(1500 - JOB_IDENTITY_TOLERANCE_S));
    let found = listing(&world);
    assert_eq!(
        found.active,
        Some(first),
        "a job exactly the tolerance short was taken for another job"
    );
    let held = only_print(&world);
    assert_eq!(held.ended_at, None);
    assert_eq!(
        held.job_print_time_s,
        Some(1500),
        "a shorter running time lowered the longest the job was seen printing"
    );
}

/// A later job of the same file — one reporting more than the tolerance less
/// printing than the open print's job had already done — closes the open
/// print as replaced and opens its own.
#[test]
fn a_later_job_of_the_same_file_closes_the_stale_print_and_opens_its_own() {
    let world = World::new();
    runs(&world, RUNNING, Some(600));
    let stale = listing(&world).active.expect("the first job is adopted");

    // The first job ended and a second began between two reads, and nothing
    // read the printer while it was idle.
    world.clock.advance(300);
    let later = 600 - JOB_IDENTITY_TOLERANCE_S - 1;
    runs(&world, RUNNING, Some(later));
    let found = listing(&world);

    let fresh = found.active.expect("the later job is named active");
    assert_ne!(
        fresh, stale,
        "a later job of the same file was adopted into the earlier job's print"
    );
    let stale_now = world.store.print_now(stale).expect("the stale print reads");
    assert_eq!(stale_now.end_reason.as_deref(), Some(REPLACED_REASON));
    assert_eq!(stale_now.ended_at, Some(world.clock.now()));
    assert_eq!(stale_now.state, PrinterState::Operational);
    let fresh_now = world.store.print_now(fresh).expect("the fresh print reads");
    assert_eq!(fresh_now.ended_at, None);
    assert_eq!(fresh_now.file_name.as_deref(), Some(RUNNING));
    assert_eq!(
        (fresh_now.job_started_at, fresh_now.job_print_time_s),
        (Some(started(&world, later)), Some(later))
    );
    assert_eq!(ids(&found), vec![fresh, stale]);
    world.journal.assert_no_violations();
}

/// An open print recorded before sightings were — or a job reporting no
/// running time — is matched by its file name alone, and the print then
/// records what the read saw of the job adopted into it.
#[test]
fn an_unknown_running_time_on_either_side_is_matched_by_file_name_alone() {
    let world = World::new();
    let legacy = block_on(world.store.open_print(None, Some(RUNNING.to_owned())))
        .expect("a print recorded before sightings were");
    assert_eq!(
        (legacy.job_started_at, legacy.job_print_time_s),
        (None, None)
    );
    runs(&world, RUNNING, Some(42));

    assert_eq!(listing(&world).active, Some(legacy.id));
    assert_eq!(
        world
            .store
            .print_now(legacy.id)
            .map(|print| (print.job_started_at, print.job_print_time_s)),
        Some((Some(started(&world, 42)), Some(42))),
        "the adopted print does not record what the read saw of its job"
    );

    // Now the printer stops reporting a running time: nothing tells this job
    // from another, so the file name decides as it did before.
    world.clock.advance(7200);
    runs(&world, RUNNING, None);
    assert_eq!(listing(&world).active, Some(legacy.id));
    assert_eq!(only_print(&world).id, legacy.id);
}

/// A pause the reads see — longer than the tolerance — and the resume after
/// it keep one print, and the resume moves the start the print records later
/// by the pause, which is what `OctoPrint` does to the running time.
#[test]
fn a_long_pause_the_reads_see_keeps_one_print_and_resets_its_start() {
    let world = World::new();
    runs(&world, RUNNING, Some(600));
    let print = listing(&world).active.expect("the job is adopted");
    let began = started(&world, 600);

    // It prints a minute more and pauses; the pause counts while it lasts.
    world.clock.advance(60);
    reports(&world, RUNNING, PrinterState::Paused, Some(660));
    assert_eq!(listing(&world).active, Some(print));
    world.clock.advance(600);
    reports(&world, RUNNING, PrinterState::Paused, Some(1260));
    assert_eq!(listing(&world).active, Some(print));
    assert_eq!(only_print(&world).job_started_at, Some(began));

    // It resumes, and the pause comes back out of the running time.
    world.clock.advance(30);
    runs(&world, RUNNING, Some(690));
    assert_eq!(
        listing(&world).active,
        Some(print),
        "the resume split the print"
    );
    let held = only_print(&world);
    assert_eq!(held.ended_at, None);
    assert_eq!(
        (held.job_started_at, held.job_print_time_s),
        (Some(started(&world, 690)), Some(690)),
        "the resume did not reset the start the print records"
    );
    assert_eq!(
        held.job_started_at,
        began.plus_seconds(600).ok(),
        "the start moved by something other than the pause"
    );
}

/// A pause and a resume that both fall between two reads keep one print,
/// however long the pause and however the second read finds the job.
#[test]
fn a_pause_between_two_reads_keeps_one_print() {
    for pause in [60, JOB_IDENTITY_TOLERANCE_S + 1, 3600, 86_400] {
        let world = World::new();
        runs(&world, RUNNING, Some(600));
        let print = listing(&world).active.expect("the job is adopted");

        // Ten seconds more printing, the pause, and forty seconds after it.
        world.clock.advance(10 + pause + 40);
        runs(&world, RUNNING, Some(650));
        assert_eq!(
            listing(&world).active,
            Some(print),
            "a {pause} s pause split it"
        );

        // Paused again by the next read: the running time counts that pause.
        world.clock.advance(5 + pause);
        reports(&world, RUNNING, PrinterState::Paused, Some(650 + 5 + pause));
        assert_eq!(
            listing(&world).active,
            Some(print),
            "a {pause} s pause split it"
        );
        assert_eq!(only_print(&world).ended_at, None);
    }
}

/// A job read paused, then cancelled and replaced by a job of the same file
/// before the next read, is a later job wherever the earlier one was seen
/// printing for longer than the later one has, by more than the tolerance.
#[test]
fn a_job_seen_paused_then_replaced_before_the_next_read_is_a_later_job() {
    let world = World::new();
    runs(&world, RUNNING, Some(1800));
    let earlier = listing(&world).active.expect("the job is adopted");
    world.clock.advance(60);
    reports(&world, RUNNING, PrinterState::Paused, Some(1860));
    assert_eq!(listing(&world).active, Some(earlier));

    world.clock.advance(400);
    runs(&world, RUNNING, Some(200));
    let later = listing(&world)
        .active
        .expect("the later job is named active");

    assert_ne!(
        later, earlier,
        "the replacement was adopted into the paused job's print"
    );
    assert_eq!(
        world
            .store
            .print_now(earlier)
            .and_then(|print| print.end_reason),
        Some(REPLACED_REASON.to_owned())
    );
    assert_eq!(world.store.prints().len(), 2);
}

/// What the reads cannot tell from one job paused between them is kept as one
/// print: a later job that has printed for at least as long as the earlier
/// one was seen printing, less the tolerance, and a later job replacing one no
/// read ever found printing.
#[test]
fn what_the_reads_cannot_tell_from_a_pause_is_kept_as_one_print() {
    // The earlier job was seen printing for 100 s; the later one, read 400 s
    // after, has printed for 300 s. The earlier job paused for 100 s between
    // the reads and resumed would report exactly that.
    let world = World::new();
    runs(&world, RUNNING, Some(100));
    let print = listing(&world).active.expect("the job is adopted");
    world.clock.advance(400);
    runs(&world, RUNNING, Some(300));
    assert_eq!(listing(&world).active, Some(print));
    assert_eq!(only_print(&world).ended_at, None);

    // The earlier job was only ever seen paused, so nothing says how long it
    // had printed; a later job ten seconds in could be it, resumed.
    let world = World::new();
    reports(&world, RUNNING, PrinterState::Paused, Some(1800));
    let print = listing(&world).active.expect("the job is adopted");
    world.clock.advance(600);
    runs(&world, RUNNING, Some(10));
    assert_eq!(listing(&world).active, Some(print));
    assert_eq!(only_print(&world).ended_at, None);
}
