//! The port composes nothing of its own into a turn.
//!
//! The prompt is the committed template with its three slots filled and nothing
//! else, and the system prompt is the skill file at the configured path rather
//! than text this crate carries.

use std::fs;
use std::sync::Arc;

use printobserver_oneharness::{CONTEXT_COMMAND_SLOT, EVENT_SLOT, IMAGE_SLOT, NO_IMAGE, SLOTS};
use printobserver_supervisor_api::SupervisorPort;
use printobserver_types::{
    EventPayload, MalformedExternalEventPayload, ObicoFailureAlertPayload, ObicoNotificationType,
    ObicoPrinterNotificationPayload, PrintId, serde_json,
};

use crate::support::{
    Fixture, HARNESS, Watch, always, assessment, block_on, config, event,
    generated_assessment_schema, port, schema_read_lock, skill_path, template_path, turn,
};

/// The literal text of the committed template between its slots, in the order
/// the template writes them, and the slots in that same order.
fn segments(template: &str) -> (Vec<String>, Vec<&'static str>) {
    let mut positions: Vec<(usize, &'static str)> = SLOTS
        .iter()
        .map(|slot| {
            (
                template.find(slot).expect("the template declares the slot"),
                *slot,
            )
        })
        .collect();
    positions.sort_unstable();
    let mut literals = Vec::new();
    let mut order = Vec::new();
    let mut cursor = 0;
    for (at, slot) in positions {
        literals.push(template[cursor..at].to_owned());
        order.push(slot);
        cursor = at + slot.len();
    }
    literals.push(template[cursor..].to_owned());
    (literals, order)
}

/// What one prompt put in each slot, or `None` when it is not this template
/// with only its slots filled.
///
/// The first literal must be the prompt's own prefix and the last its own
/// suffix, so a port that put a sentence of its own before or after the
/// template fails here rather than being read as a longer filling.
fn fillings(literals: &[String], prompt: &str) -> Option<Vec<String>> {
    let first = literals.first()?;
    let last = literals.last()?;
    if !prompt.starts_with(first.as_str()) || !prompt.ends_with(last.as_str()) {
        return None;
    }
    let mut cursor = first.len();
    let end = prompt.len() - last.len();
    if end < cursor {
        return None;
    }
    let mut found = Vec::new();
    for literal in &literals[1..literals.len() - 1] {
        let at = cursor + prompt.get(cursor..end)?.find(literal.as_str())?;
        found.push(prompt.get(cursor..at)?.to_owned());
        cursor = at + literal.len();
    }
    found.push(prompt.get(cursor..end)?.to_owned());
    Some(found)
}

/// Every prompt is the committed template with only its three slots differing.
#[test]
fn every_prompt_is_the_committed_template_with_only_its_slots_filled() {
    // Every answer this journey drives is judged by the checked-in assessment
    // schema, so it is held still while the journey reads it.
    let _schemas = schema_read_lock();
    let fixture = Fixture::new("prompting");
    let watch = Arc::new(Watch::default());
    let supervisor = port(
        config(
            &fixture,
            HARNESS,
            &generated_assessment_schema(),
            always("SID-PROMPT", &assessment("the print is fine", "high")),
        ),
        &watch,
    );

    let template = fs::read_to_string(template_path()).expect("the committed template is readable");
    let (literals, order) = segments(&template);

    let picture = fixture.path("frame.jpg");
    fs::write(&picture, b"not really a picture").expect("a scratch picture");
    let triggers = [
        (
            EventPayload::ObicoFailureAlert(ObicoFailureAlertPayload {
                is_warning: false,
                print_paused: true,
                obico_print_id: Some(7),
                file_name: Some("bracket.gcode".to_owned()),
                started_at: None,
                ended_at: None,
            }),
            Some(picture.clone()),
        ),
        (
            EventPayload::ObicoPrinterNotification(ObicoPrinterNotificationPayload {
                notification_type: ObicoNotificationType::FilamentChange,
                obico_print_id: Some(7),
                file_name: None,
                started_at: None,
                ended_at: None,
            }),
            None,
        ),
        (
            EventPayload::MalformedExternalEvent(MalformedExternalEventPayload {
                detail: "the body was not JSON".to_owned(),
            }),
            Some(picture),
        ),
    ];

    let mut seen = Vec::new();
    for (payload, image) in triggers {
        let print_id = PrintId::new();
        let request = turn(print_id, event(print_id, payload), image.clone());
        let expected_event =
            serde_json::to_string_pretty(&request.event).expect("the event is writable");
        let expected_image = image
            .as_ref()
            .map_or_else(|| NO_IMAGE.to_owned(), |path| path.display().to_string());
        let expected_command = request.context_command.clone();
        block_on(supervisor.run_turn(request)).expect("the turn runs");

        let prompt = watch
            .requests()
            .last()
            .expect("the port built a run request")
            .prompt
            .first()
            .expect("the run request carries a prompt")
            .clone();
        let found = fillings(&literals, &prompt).unwrap_or_else(|| {
            panic!("the prompt is not the committed template with only its slots filled:\n{prompt}")
        });
        for (slot, filling) in order.iter().zip(&found) {
            match *slot {
                EVENT_SLOT => assert_eq!(filling, &expected_event),
                IMAGE_SLOT => assert_eq!(filling, &expected_image),
                CONTEXT_COMMAND_SLOT => assert_eq!(filling, &expected_command),
                other => panic!("the template declares an unknown slot {other}"),
            }
        }
        seen.push(found);
    }

    // The literals never moved: only the slots differ between the turns.
    assert_eq!(seen.len(), 3);
    assert_ne!(
        seen[0], seen[1],
        "two different events filled the same slots"
    );
}

/// The system prompt is the skill file at the configured path, read from the
/// tree when the port is built.
#[test]
fn the_system_prompt_is_the_skill_file_at_the_configured_path() {
    // Every answer this journey drives is judged by the checked-in assessment
    // schema, so it is held still while the journey reads it.
    let _schemas = schema_read_lock();
    let fixture = Fixture::new("skill");
    let schema = generated_assessment_schema();
    let environment = always("SID-SKILL", &assessment("the print is fine", "high"));

    // The committed skill, exactly.
    let watch = Arc::new(Watch::default());
    let supervisor = port(
        config(&fixture, HARNESS, &schema, environment.clone()),
        &watch,
    );
    let print_id = PrintId::new();
    block_on(supervisor.run_turn(turn(
        print_id,
        event(
            print_id,
            EventPayload::MalformedExternalEvent(MalformedExternalEventPayload {
                detail: "the body was not JSON".to_owned(),
            }),
        ),
        None,
    )))
    .expect("the turn runs");
    let committed = fs::read_to_string(skill_path()).expect("the committed skill is readable");
    assert_eq!(
        watch
            .requests()
            .last()
            .expect("the port built a run request")
            .system
            .as_deref(),
        Some(committed.as_str()),
        "the system prompt is not the committed skill"
    );

    // And a skill file whose content changes on disk changes what the request
    // carries, which a crate that embedded the skill's text could not do.
    let scratch = fixture.path("skill.md");
    fs::write(&scratch, &committed).expect("a scratch skill");
    let mut configured = config(&fixture, HARNESS, &schema, environment);
    configured.skill_path = scratch.clone();

    for text in [committed.as_str(), "Watch the print. Say what you see."] {
        fs::write(&scratch, text).expect("the scratch skill is writable");
        let rebuilt_watch = Arc::new(Watch::default());
        let rebuilt = port(configured.clone(), &rebuilt_watch);
        let print_id = PrintId::new();
        block_on(rebuilt.run_turn(turn(
            print_id,
            event(
                print_id,
                EventPayload::MalformedExternalEvent(MalformedExternalEventPayload {
                    detail: "the body was not JSON".to_owned(),
                }),
            ),
            None,
        )))
        .expect("the turn runs");
        assert_eq!(
            rebuilt_watch
                .requests()
                .last()
                .expect("the port built a run request")
                .system
                .as_deref(),
            Some(text),
            "the system prompt did not follow the file at the configured path"
        );
    }
}
