//! A start is checked against the spools loaded before the printer is asked.
//!
//! These drive a start through the real action path and read back the
//! decision, the `preflight_checked` it wrote and whether the printer was ever
//! asked to start. The wait for `OctoPrint`'s analysis is driven by the
//! injected clock alone: a journey moves it, and nothing here sleeps for it.

use std::sync::Arc;

use printobserver_core::kinds::PreflightCheckedPayload;
use printobserver_core::{
    ActionOutcome, Actor, PolicyDecision, PreflightMode, PreflightVerdict, RejectionReason,
    SpoolCommand, SpoolRecord, UnverifiedReasonKind, block_on, buffer_g,
};
use printobserver_printer_api::FileFacts;
use printobserver_types::FileName;

use crate::journal::Call;
use crate::shelf::{
    FILE, TOTAL_MM, after_an_uncharged_print, assert_close, command, every, facts, grams, load,
    loaded_spool, on_tool, request_start, spool_now, spool_world, spool_world_with, start_action,
    timeline,
};
use crate::world::{World, agent_actor};

/// Register a PLA spool holding this much now, and load it onto tool 0.
fn loaded_holding(world: &World, name: &str, remaining_g: f64) -> SpoolRecord {
    let spool = command(
        world,
        SpoolCommand::RegisterSpool {
            name: name.to_owned(),
            material: "PLA".to_owned(),
            color: None,
            diameter_mm: None,
            density_g_cm3: None,
            initial_g: 1_000.0,
            remaining_g: Some(remaining_g),
            tare_g: None,
            reason: "a part-used spool".to_owned(),
            actor: Actor::Operator,
        },
    )
    .spool
    .expect("registered");
    assert_eq!(load(world, spool.id, 0).decision, PolicyDecision::Accepted);
    spool_now(world, spool.id)
}

/// The check a start was held to.
fn checked_for(world: &World, outcome: &ActionOutcome) -> PreflightCheckedPayload {
    let checked: Vec<PreflightCheckedPayload> = every::<PreflightCheckedPayload>(world)
        .into_iter()
        .filter(|checked| checked.action_id == Some(outcome.record.id))
        .collect();
    assert_eq!(checked.len(), 1, "every start writes one check naming it");
    checked.into_iter().next().expect("one")
}

/// Whether the printer was ever asked to start the file.
fn printer_asked_to_start(world: &World) -> bool {
    world.journal.count(&Call::Start(FILE.to_owned())) > 0
}

/// A start whose spool holds less than the print needs and its buffer is
/// refused naming the shortfall — the buffer a tenth of the requirement, or
/// five grams when that is more — and the printer is never asked.
#[test]
fn a_short_spool_refuses_the_start_naming_what_it_needs_and_holds() {
    for (length_mm, remaining_g) in [(TOTAL_MM, 18.0), (50_000.0, 100.0)] {
        let world = spool_world();
        world.printer.holds_file(facts(Some(length_mm)));
        let spool = loaded_holding(&world, "Short", remaining_g);
        let outcome = request_start(&world, Actor::Operator, false);

        let required = grams(&spool, length_mm);
        let buffer = (required * 0.1).max(5.0);
        assert_close(buffer, buffer_g(required, &world.config.spools));
        let PolicyDecision::Rejected(RejectionReason::InsufficientFilament {
            tool,
            spool_id,
            spool_name,
            required_g,
            buffer_g,
            remaining_g: held,
            unverified,
        }) = &outcome.record.decision
        else {
            panic!("expected a shortfall, found {:?}", outcome.record.decision);
        };
        assert_eq!((*tool, *spool_id), (0, spool.id));
        assert_eq!(spool_name, "Short");
        assert_close(*required_g, required);
        assert_close(*buffer_g, buffer);
        assert_close(*held, remaining_g);
        assert_eq!(unverified, &None);
        assert!(!printer_asked_to_start(&world));
        let checked = checked_for(&world, &outcome);
        assert_eq!(checked.preflight.verdict, PreflightVerdict::Insufficient);
        assert_eq!(
            checked.preflight.tools[0].verdict,
            PreflightVerdict::Insufficient
        );
        world.journal.assert_no_violations();
    }
}

/// A start onto a spool whose figure is in doubt is refused naming the spool
/// and its oldest mark, and the printer is never asked.
#[test]
fn an_unverified_spool_refuses_the_start_naming_it_and_its_mark() {
    let world = spool_world();
    let spool = loaded_spool(&world, "In Doubt", 1_000.0);
    let earlier = after_an_uncharged_print(&world);
    let outcome = request_start(&world, Actor::Operator, false);
    assert_eq!(
        outcome.record.decision,
        PolicyDecision::Rejected(RejectionReason::UnverifiedSpool {
            tool: 0,
            spool_id: spool.id,
            spool_name: "In Doubt".to_owned(),
            reason: printobserver_core::UnverifiedReason::UnchargedPrint { print_id: earlier },
        })
    );
    assert!(!printer_asked_to_start(&world));
    assert_eq!(
        checked_for(&world, &outcome).preflight.verdict,
        PreflightVerdict::UnverifiedSpool
    );
}

/// An operator overriding the check starts the print anyway, recorded as
/// overridden; an agent — or any actor that is not an operator — carrying the
/// override is still refused.
#[test]
fn only_an_operator_may_override_the_check() {
    for unverified in [false, true] {
        let world = spool_world();
        if unverified {
            let _ = loaded_spool(&world, "In Doubt", 1_000.0);
            after_an_uncharged_print(&world);
        } else {
            loaded_holding(&world, "Short", 10.0);
        }
        for actor in [
            agent_actor(printobserver_types::PrintId::new()),
            Actor::System,
        ] {
            let refused = request_start(&world, actor, true);
            let expected_refusal = if unverified {
                matches!(
                    refused.record.decision,
                    PolicyDecision::Rejected(RejectionReason::UnverifiedSpool { .. })
                )
            } else {
                matches!(
                    refused.record.decision,
                    PolicyDecision::Rejected(RejectionReason::InsufficientFilament { .. })
                )
            };
            assert!(expected_refusal, "{:?}", refused.record.decision);
            assert!(!printer_asked_to_start(&world));
        }
        let overridden = request_start(&world, Actor::Operator, true);
        assert_eq!(overridden.record.decision, PolicyDecision::Accepted);
        assert_eq!(overridden.executed, Some(Ok(())));
        assert!(printer_asked_to_start(&world));
        let checked = checked_for(&world, &overridden);
        assert_eq!(checked.preflight.verdict, PreflightVerdict::Overridden);
        assert_eq!(
            checked.preflight.tools[0].verdict,
            PreflightVerdict::Overridden
        );
    }
}

/// In mode `warn` a short spool's start goes ahead, recorded as warned.
#[test]
fn mode_warn_lets_the_start_go_ahead_recorded_as_warned() {
    let world = spool_world_with(|config| config.spools.preflight = PreflightMode::Warn);
    loaded_holding(&world, "Short", 10.0);
    let outcome = request_start(&world, Actor::Operator, false);
    assert_eq!(outcome.record.decision, PolicyDecision::Accepted);
    assert!(printer_asked_to_start(&world));
    assert_eq!(
        checked_for(&world, &outcome).preflight.verdict,
        PreflightVerdict::Warned
    );
}

/// Run a start on a thread of its own, and answer it once it has finished —
/// failing when it waits for anything the clock has not been moved for.
fn start_in_the_background(
    world: &World,
    actor: Actor,
    override_filament_check: bool,
) -> std::thread::JoinHandle<ActionOutcome> {
    use printobserver_printer_api::PrinterState;
    world.printer.reports_state(PrinterState::Operational);
    world.printer.reports_job(None, PrinterState::Operational);
    let core = Arc::clone(&world.core);
    std::thread::spawn(move || {
        block_on(core.request_action(
            printobserver_types::PrintId::new(),
            start_action(actor, override_filament_check),
        ))
        .expect("the start is recorded")
    })
}

/// Mode `off` checks nothing and waits for nothing, recording `unchecked`; and
/// with no tracked spool on any tool there is nothing to check, so a start of
/// a file with no analysis goes ahead at once in mode `refuse` too.
#[test]
fn mode_off_and_no_tracked_spool_each_start_without_waiting() {
    for mode_off in [true, false] {
        let world = spool_world_with(|config| {
            if mode_off {
                config.spools.preflight = PreflightMode::Off;
            }
        });
        world.printer.holds_file(facts(None));
        if mode_off {
            loaded_holding(&world, "Short", 1.0);
        }
        let started = start_in_the_background(&world, Actor::Operator, false);
        world.wait_until("the start finished without the clock moving", || {
            started.is_finished()
        });
        let outcome = started.join().expect("the start thread finishes");
        assert_eq!(outcome.record.decision, PolicyDecision::Accepted);
        assert!(printer_asked_to_start(&world));
        let checked = checked_for(&world, &outcome);
        assert_eq!(checked.preflight.verdict, PreflightVerdict::Unchecked);
        if mode_off {
            assert!(checked.preflight.tools.is_empty());
            assert_eq!(world.journal.count(&Call::FileFacts(FILE.to_owned())), 0);
        }
    }
}

/// A start of a file whose analysis appears while it waits takes the analysis
/// and is judged by the ordinary rule on its figures.
#[test]
fn a_start_waits_for_an_analysis_that_appears_and_judges_it() {
    let world = spool_world();
    world.printer.holds_file(facts(None));
    let spool = loaded_holding(&world, "Waited For", 18.0);
    let started = start_in_the_background(&world, Actor::Operator, false);
    world.wait_until("the start read the file once", || {
        world.journal.count(&Call::FileFacts(FILE.to_owned())) >= 1
    });
    world.clock.advance(45);
    world.printer.holds_file(facts(Some(TOTAL_MM)));
    let outcome = started.join().expect("the start thread finishes");

    let PolicyDecision::Rejected(RejectionReason::InsufficientFilament { required_g, .. }) =
        outcome.record.decision
    else {
        panic!("the analysis was not judged: {:?}", outcome.record.decision);
    };
    assert_close(required_g, grams(&spool, TOTAL_MM));
    assert!(!printer_asked_to_start(&world));
    let checked = checked_for(&world, &outcome);
    assert_eq!(checked.preflight.verdict, PreflightVerdict::Insufficient);
}

/// A start whose analysis has still not appeared when the wait elapses is
/// refused naming the file and the wait, the printer never asked; the same
/// start with an operator's override goes ahead as overridden.
#[test]
fn a_start_whose_analysis_never_appears_is_refused_unless_an_operator_overrides() {
    let world = spool_world();
    world.printer.holds_file(facts(None));
    loaded_holding(&world, "Never Analysed", 900.0);
    let waited = world.config.spools.analysis_wait_s;
    let reads = || world.journal.count(&Call::FileFacts(FILE.to_owned()));

    let started = start_in_the_background(&world, Actor::Operator, false);
    world.wait_until("the start read the file once", || reads() >= 1);
    world
        .clock
        .advance(i64::try_from(waited).expect("a wait in range"));
    let outcome = started.join().expect("the start thread finishes");
    assert_eq!(
        outcome.record.decision,
        PolicyDecision::Rejected(RejectionReason::AnalysisUnavailable {
            file_name: FILE.to_owned(),
            waited_s: waited,
        })
    );
    assert!(!printer_asked_to_start(&world));
    assert_eq!(
        checked_for(&world, &outcome).preflight.verdict,
        PreflightVerdict::NoAnalysis
    );

    let before = reads();
    let started = start_in_the_background(&world, Actor::Operator, true);
    world.wait_until("the second start read the file", || reads() > before);
    world
        .clock
        .advance(i64::try_from(waited).expect("a wait in range"));
    let overridden = started.join().expect("the start thread finishes");
    assert_eq!(overridden.record.decision, PolicyDecision::Accepted);
    assert!(printer_asked_to_start(&world));
    assert_eq!(
        checked_for(&world, &overridden).preflight.verdict,
        PreflightVerdict::Overridden
    );
}

/// A spool both short and in doubt refuses the start as short, carrying its
/// name and its oldest mark, so one refusal states both; and the check lists
/// every mark the spool holds.
#[test]
fn a_short_spool_in_doubt_says_both_in_one_refusal() {
    let world = spool_world();
    let spool = loaded_holding(&world, "Short And In Doubt", 10.0);
    let earlier = after_an_uncharged_print(&world);
    let outcome = request_start(&world, Actor::Operator, false);
    let PolicyDecision::Rejected(RejectionReason::InsufficientFilament {
        spool_name,
        unverified,
        ..
    }) = &outcome.record.decision
    else {
        panic!("expected a shortfall, found {:?}", outcome.record.decision);
    };
    assert_eq!(spool_name, "Short And In Doubt");
    assert_eq!(
        unverified,
        &Some(printobserver_core::UnverifiedReason::UnchargedPrint { print_id: earlier })
    );
    let checked = checked_for(&world, &outcome);
    let tool = &checked.preflight.tools[0];
    assert_eq!(tool.spool_id, Some(spool.id));
    assert_eq!(tool.marks.len(), 1);
    assert_eq!(
        tool.marks[0].reason.kind(),
        UnverifiedReasonKind::UnchargedPrint
    );
    assert!(!printer_asked_to_start(&world));
}

/// The read of the check reports each tool's spool, its name and its marks,
/// and records nothing at all.
#[test]
fn the_check_read_reports_each_tool_and_records_nothing() {
    let world = spool_world();
    world.printer.holds_file(FileFacts {
        filament: Some(vec![on_tool(0, 3_000.0), on_tool(1, 2_000.0)]),
        ..facts(None)
    });
    let spool = loaded_spool(&world, "Tool Zero", 1_000.0);
    let earlier = after_an_uncharged_print(&world);
    world.printer.holds_file(FileFacts {
        filament: Some(vec![on_tool(0, 3_000.0), on_tool(1, 2_000.0)]),
        ..facts(None)
    });
    let events_before = timeline(&world).len();
    let writes_before = world.journal.store_writes().len();

    let preflight = block_on(world.core.preflight(&FileName::new(FILE).expect("a name")))
        .expect("the check is read");

    assert_eq!(preflight.tools.len(), 2);
    let zero = &preflight.tools[0];
    assert_eq!(zero.spool_id, Some(spool.id));
    assert_eq!(zero.spool_name.as_deref(), Some("Tool Zero"));
    assert_eq!(
        zero.marks
            .iter()
            .map(|mark| mark.reason.clone())
            .collect::<Vec<_>>(),
        vec![printobserver_core::UnverifiedReason::UnchargedPrint { print_id: earlier }]
    );
    assert_eq!(zero.verdict, PreflightVerdict::UnverifiedSpool);
    assert_close(zero.required_g.expect("figured"), grams(&spool, 3_000.0));
    let one = &preflight.tools[1];
    assert_eq!(one.spool_id, None);
    assert_eq!(one.verdict, PreflightVerdict::Unchecked);
    assert_eq!(preflight.verdict, PreflightVerdict::UnverifiedSpool);
    assert_eq!(timeline(&world).len(), events_before);
    assert_eq!(world.journal.store_writes().len(), writes_before);
}

/// A file whose facts cannot be read is a file with no analysis: the check
/// reads it so, naming the spool on each tool loaded.
#[test]
fn a_file_that_cannot_be_read_is_checked_as_having_no_analysis() {
    let world = spool_world();
    let spool = loaded_spool(&world, "Unread", 1_000.0);
    world.printer.fails(
        crate::fakes::PrinterMethod::FileFacts,
        printobserver_printer_api::PrinterError::Unreachable {
            detail: "the printer is unreachable".to_owned(),
        },
    );
    let preflight = block_on(world.core.preflight(&FileName::new(FILE).expect("a name")))
        .expect("the check is read");
    assert_eq!(preflight.verdict, PreflightVerdict::NoAnalysis);
    assert_eq!(preflight.tools.len(), 1);
    assert_eq!(preflight.tools[0].spool_id, Some(spool.id));
    assert_eq!(preflight.tools[0].required_g, None);
}

/// An analysis naming a tool below zero, or a length that is negative or not a
/// number, is read as no filament for it: the check names only the tools a
/// print can feed.
#[test]
fn an_analysis_s_impossible_entries_are_left_out_of_the_check() {
    let world = spool_world();
    world.printer.holds_file(FileFacts {
        filament: Some(vec![
            on_tool(0, 3_000.0),
            on_tool(-1, 2_000.0),
            on_tool(1, -50.0),
            on_tool(2, f64::NAN),
        ]),
        ..facts(None)
    });
    let spool = loaded_spool(&world, "Only Tool Zero", 1_000.0);
    let preflight = block_on(world.core.preflight(&FileName::new(FILE).expect("a name")))
        .expect("the check is read");
    assert_eq!(preflight.tools.len(), 1);
    assert_eq!(preflight.tools[0].tool, 0);
    assert_close(
        preflight.tools[0].required_g.expect("figured"),
        grams(&spool, 3_000.0),
    );
}
