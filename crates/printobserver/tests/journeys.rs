//! Every client command, against a real supervisor the server command started.
//!
//! # What each run is held to
//!
//! Every invocation of the cross-product carries all of it: the request the
//! server received is the one that command is supposed to make **carrying the
//! caller's own values**, the effect is confirmed by reading it back through
//! this same surface with those same values in the record, and the process
//! tree connected to no endpoint but the configured one. Three assertions on
//! the same run rather than one chosen invocation per command, because an
//! alternate input form that sent the wrong request would otherwise pass for
//! having been driven only against the destination.
//!
//! # Every assertion here is proven to bite
//!
//! `tests/journeys/tainting.rs` drives this program with one defect in it —
//! substituting a value, substituting a reason, connecting to the machine
//! directly, connecting only under one combination of options, and putting
//! image bytes in the output — through these same assertions, and each is
//! refused. A tier whose assertions cannot catch a violation is a tier nobody
//! has proven catches one.
//!
//! What is on the far side of the printer port here is a socket rather than a
//! printer, which is the one thing this tier does not have for real and is what
//! lets it run in every gate. `tests/integration.rs` runs the same walk over
//! the `OctoPrint` `just octoprint-up` provisioned.

#[path = "support/announced.rs"]
mod announced;
#[cfg(unix)]
#[path = "support/harness.rs"]
mod harness;
#[cfg(unix)]
#[path = "journeys/harness_turn.rs"]
mod harness_turn;
#[path = "support/host.rs"]
mod host;
#[path = "support/machine.rs"]
mod machine;
#[path = "support/proxy.rs"]
mod proxy;
#[path = "support/scripted.rs"]
mod scripted;
#[path = "support/standalone.rs"]
mod standalone;
#[path = "support/traced.rs"]
mod traced;
#[path = "support/walk.rs"]
mod walk;
#[cfg(unix)]
#[path = "journeys/watching.rs"]
mod watching;
#[path = "support/world.rs"]
mod world;

#[path = "journeys/announcing.rs"]
mod announcing;
#[path = "journeys/answers.rs"]
mod answers;
#[path = "journeys/carrying.rs"]
mod carrying;
#[path = "journeys/closing.rs"]
mod closing;
#[path = "journeys/confirming.rs"]
mod confirming;
#[path = "journeys/documented.rs"]
mod documented;
#[path = "journeys/documenting.rs"]
mod documenting;
#[path = "journeys/durations.rs"]
mod durations;
#[path = "journeys/failures.rs"]
mod failures;
#[path = "journeys/finding.rs"]
mod finding;
#[path = "journeys/formats.rs"]
mod formats;
#[path = "journeys/looking.rs"]
mod looking;
#[path = "journeys/materializing.rs"]
mod materializing;
#[path = "journeys/redaction.rs"]
mod redaction;
#[path = "journeys/running.rs"]
mod running;
#[path = "journeys/spooling.rs"]
mod spooling;
#[path = "journeys/starting.rs"]
mod starting;
#[path = "journeys/streaming.rs"]
mod streaming;
#[path = "journeys/tainting.rs"]
mod tainting;
#[path = "journeys/tier.rs"]
mod tier;
#[path = "support/unreachable.rs"]
mod unreachable;
#[path = "journeys/upgrading.rs"]
mod upgrading;

use world::World;

/// The whole tier, over one supervisor.
///
/// One world rather than one per journey: the supervisor is a process and the
/// store is a file, and starting sixteen of each to walk sixteen commands would
/// spend the tier's time on process startup rather than on what it proves.
#[test]
fn every_client_command_is_proven_against_a_real_supervisor() {
    let world = World::open(world::STOOD_IN);

    tier::run(&world, tier::WHOLE);
}

/// A print started at the printer is found with `printobserver prints`, read
/// by the identifier that names, and joined by the first alert about it.
///
/// A world of its own, because the journey ends the print every other journey
/// here acts on.
#[test]
fn a_print_started_at_the_printer_is_found_and_joined_by_its_alert() {
    let world = World::open(world::STOOD_IN);

    finding::a_print_started_at_the_printer_is_found_read_and_joined_by_its_alert(&world);
}

/// An operator keeps the spool ledger through this program, and reads all of
/// it back through the same program.
///
/// A world of its own, because the journey's spools are the only ones the
/// assertions about the listing and the history are about.
#[test]
fn an_operator_keeps_the_spool_ledger_through_this_program() {
    let world = World::open(world::STOOD_IN);

    spooling::an_operator_keeps_the_ledger_through_this_program(&world);
}

/// Every refusal of the spool ledger is answered through this program with
/// its tagged cause, which an automation acts on without reading the message.
///
/// A world of its own, because the journey's spools are the only ones the
/// automation's listing is about.
#[test]
fn every_spool_refusal_is_answered_with_its_cause_through_this_program() {
    let world = World::open(world::STOOD_IN);

    spooling::every_spool_refusal_is_answered_with_its_cause(&world);
}

/// A supervisor of this build started over the store a published release
/// wrote migrates it, says what it changed, and reads every event that release
/// wrote through this program.
#[test]
fn a_store_a_published_release_wrote_is_migrated_and_still_reads() {
    upgrading::a_store_0_4_3_wrote_is_migrated_and_every_event_still_reads(world::STOOD_IN);
}

/// The listing opens with one line per spool, and a start the filament check
/// refuses says what to do about the spool or the file it was refused for.
///
/// A world of its own, whose starts wait a second for an analysis rather than
/// the five minutes a supervisor waits by default.
#[test]
fn the_listing_and_a_refused_start_say_what_each_spool_holds() {
    let world = World::configured(
        world::STOOD_IN,
        &world::committed_skill(),
        None,
        |document| {
            document["spools"] = printobserver_types::serde_json::json!({
                "analysis_wait_seconds": spooling::ANALYSIS_WAIT_S,
            });
        },
    );

    spooling::the_listing_and_the_start_say_what_each_spool_holds(&world);
}

/// `printobserver server` refuses a configuration granting the agent
/// `close_print`, naming the action, and serves nothing: closing a print record
/// is the operator's judgement that the print is over.
#[test]
fn a_configuration_granting_the_agent_close_print_is_refused_at_start() {
    let said = starting::refused_start(|document| {
        let agent = &mut document["safety"]["actions"]["agent"];
        *agent = printobserver_types::serde_json::json!(["pause", "close_print"]);
    });

    assert!(
        said.contains("close_print") && said.contains("agent") && said.contains("safety"),
        "the refusal does not name the action, the agent and where it is granted: {said}"
    );
}

/// `printobserver server` refuses each of the printer watch's own settings
/// out of the range it admits, naming the field, and serves nothing: a poll
/// under five seconds or over five minutes, an unattended window under a
/// minute or over a day, a blank-frame bound under one or over 128 grey
/// levels, and a detector address that is no web address.
#[test]
fn the_watchs_settings_out_of_range_are_refused_at_start_by_name() {
    use printobserver_types::serde_json::{Value, json};
    /// One edit of the configuration document.
    type Edit = fn(&mut Value);
    let refusals: [(&str, &str, Edit); 7] = [
        ("supervisor", "printer_poll_s", |document| {
            document["supervisor"]["printer_poll_s"] = json!(4);
        }),
        ("supervisor", "printer_poll_s", |document| {
            document["supervisor"]["printer_poll_s"] = json!(301);
        }),
        ("supervisor", "unattended_window_s", |document| {
            document["supervisor"]["unattended_window_s"] = json!(59);
        }),
        ("supervisor", "unattended_window_s", |document| {
            document["supervisor"]["unattended_window_s"] = json!(86_401);
        }),
        ("vision", "blank_frame_max_stddev", |document| {
            document["vision"] = json!({ "blank_frame_max_stddev": 0.5 });
        }),
        ("vision", "blank_frame_max_stddev", |document| {
            document["vision"] = json!({ "blank_frame_max_stddev": 128.5 });
        }),
        ("obico", "ml_api_url", |document| {
            document["obico"] = json!({ "ml_api_url": "ftp://127.0.0.1:3333" });
        }),
    ];
    for (table, field, edit) in refusals {
        let said = starting::refused_start(edit);
        assert!(
            said.contains(&format!("{table}.{field}")),
            "the refusal does not name `{table}.{field}`: {said}"
        );
    }
}

/// `printobserver server` says when it started without accounting the prints
/// the printer counted while nothing watched, because the printer's file list
/// could not be read — and serves.
///
/// The machine this tier stands in answers that read with a document it cannot
/// be read from, so every world here starts this way.
#[test]
fn the_server_says_it_skipped_the_tallies_it_could_not_read_and_serves() {
    let world = World::open(world::STOOD_IN);

    let said = world.said_so_far();
    assert!(
        said.contains("printobserver: the printer could not be read")
            && said.contains("were not accounted on this start"),
        "the server did not say it skipped the accounting: {said}"
    );
    assert_eq!(
        running::read(&world, &["prints"])["active"],
        printobserver_types::serde_json::json!(world.print_id()),
        "the server that skipped the accounting does not serve"
    );
}

/// Every command example the reference documents show, run against a real
/// supervisor and compared with what the document shows beside it.
///
/// Every example of every declared document is run: an example the walk has no
/// way to run is a finding of its own rather than one it steps over, so a
/// document cannot pass by showing something nothing here executes.
#[test]
fn every_documented_example_prints_what_the_document_shows() {
    let world = World::open(world::STOOD_IN);

    documenting::accepts_the_committed_documentation(&world);
}

/// The documented operator workflow, carried out from the documentation alone,
/// against a real supervisor over a stood-in machine.
///
/// The printer tier runs the same walk against the `OctoPrint` `just
/// octoprint-up` provisioned; this is the same walk in every gate, so a change
/// that made the documentation unfollowable is refused before the printer tier
/// runs.
#[test]
fn the_documented_operator_workflow_is_carried_out_from_the_documentation_alone() {
    let world = World::open(world::STOOD_IN);

    documented::walk(&world);
}

/// The documented operator workflow from a skill directory of its own, and
/// nothing else.
///
/// This program carries no skill: an installed host has the directory `gh skill
/// install` put down, and a configuration naming its `SKILL.md`. This copies
/// the committed skill directory alone into a directory carrying nothing else,
/// starts the real server configured with it, opens every link the skill
/// carries from there, carries the whole turn out of that copy, and — where a
/// harness can be stood in on the server's path — establishes that a
/// supervision turn's harness ran in that directory.
#[test]
fn the_configured_skill_directory_carries_the_documented_operator_workflow() {
    documenting::the_configured_skill_directory_carries_the_turn();
}

/// A supervisor that never says where it is serving fails the wait within its
/// deadline, stopped, with everything it printed in the failure — rather than
/// holding the whole tier with no verdict.
#[test]
fn a_supervisor_that_never_announces_itself_fails_within_the_deadline() {
    announcing::a_supervisor_that_never_announces_is_stopped_and_reported();
}

#[test]
fn a_supervisor_that_exits_unannounced_fails_when_its_output_ends() {
    announcing::a_supervisor_that_exits_unannounced_is_answered_when_its_output_ends();
}

#[test]
fn a_supervisor_announcing_no_address_fails_naming_it() {
    announcing::a_supervisor_announcing_no_address_is_stopped_and_reported();
}

/// Reading what a stopped supervisor printed is bounded too, even where
/// another process still holds its output open.
#[test]
fn a_stopped_supervisors_output_is_read_no_longer_than_the_drain() {
    announcing::a_stream_held_open_is_read_no_longer_than_the_drain();
}

/// The deadline is a backstop and never the signal: a supervisor that says
/// where it is serving is answered the moment it does.
#[test]
fn a_supervisor_that_announces_itself_is_answered_at_once() {
    announcing::a_supervisor_that_announces_is_answered_at_once();
}
