//! Every process a real turn creates is the responder this repository ships,
//! and none is the `OneHarness` command-line binary.
//!
//! This is the runtime half of the prohibition on this crate spawning anything.
//! The structural half — that no source of this crate uses a process-spawning
//! interface at all, whatever executable it would name — is a check over the
//! crate's own sources, driven against a fixture that spawns a computed name.
//!
//! What this half establishes is bounded, and deliberately stated as the bound
//! it is: under a turn driving every method the port declares, every process
//! created is the test-only responder. A spawn initiated from code this crate
//! calls but does not contain would look the same here, which is what the
//! structural half is for.

use std::sync::Arc;

use printobserver_supervisor_api::SupervisorPort;
use printobserver_types::{EventPayload, MalformedExternalEventPayload, PrintId};

use crate::support::{
    Fixture, HARNESS, Watch, always, assessment, block_on, config, event,
    generated_assessment_schema, port, responder, turn,
};

/// Driving every method the port declares creates only the responder.
#[test]
fn every_process_a_turn_creates_is_the_responder_this_repository_ships() {
    let fixture = Fixture::new("spawning");
    let watch = Arc::new(Watch::default());
    let supervisor = port(
        config(
            &fixture,
            HARNESS,
            &generated_assessment_schema(),
            always("SID-SPAWN", &assessment("the print is fine", "high")),
        ),
        &watch,
    );

    let print_id = PrintId::new();
    let payload = || {
        EventPayload::MalformedExternalEvent(MalformedExternalEventPayload {
            detail: "the body was not JSON".to_owned(),
        })
    };

    block_on(supervisor.run_turn(turn(print_id, event(print_id, payload()), None)))
        .expect("the turn runs");
    block_on(supervisor.close_session(print_id, "finished".to_owned())).expect("the close runs");
    block_on(supervisor.run_turn(turn(print_id, event(print_id, payload()), None)))
        .expect("the turn after the close runs");

    let programs = watch.programs();
    assert!(
        !programs.is_empty(),
        "no process was created at all, so this journey observed nothing"
    );
    for program in programs {
        assert_eq!(
            program,
            responder(),
            "a process other than the responder this repository ships was created"
        );
        let name = program
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or_default();
        assert!(
            !name.starts_with("oneharness"),
            "the OneHarness command-line binary was run: {name}"
        );
    }
}
