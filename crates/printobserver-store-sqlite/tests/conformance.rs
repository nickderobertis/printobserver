//! One suite, both implementations.
//!
//! Every journey here runs against the durable store and against the in-memory
//! one, so the second cannot drift into answering differently from the first —
//! which is the only thing that makes a tier running against the fake evidence
//! about the tier that will run against the real one. A check in
//! `port_coverage.rs` refuses a port method this suite does not exercise.

#[path = "support/block_on.rs"]
mod block_on;
#[path = "support/fixture.rs"]
mod fixture;

use std::sync::Arc;
use std::thread;

use block_on::block_on;
use fixture::{Fixture, draft, instant, manifest, request, session};
use printobserver_store_api::{
    DEFAULT_HISTORY_WINDOW, HistoryQuery, ImageLookup, MAX_HISTORY_LIMIT, SettleOutcome,
    StoreError, StorePort,
};
use printobserver_store_sqlite::settle_label;
use printobserver_types::{
    Adjustable, EventKind, EventRecord, ExecutionOutcome, ImageId, InterventionId,
    InterventionOutcome, PolicyDecision, PrintId, PrintRecord, PrinterState, RawBytes,
    RejectionReason, Timestamp,
};

/// A history read of one print, with no filter and no limit.
fn whole_window(print_id: PrintId) -> HistoryQuery {
    HistoryQuery {
        print_id,
        kinds: Vec::new(),
        since: None,
        until: None,
        limit: None,
    }
}

/// A print with one event on it, which most journeys need before anything else.
fn print_and_event(port: &Arc<dyn StorePort>) -> (PrintRecord, EventRecord) {
    let print = block_on(port.open_print(Some(7), Some("bracket.gcode".to_owned())))
        .expect("a print opens");
    let event = block_on(port.append_event(draft(
        Some(print.id),
        "obico_failure_alert",
        instant("2026-03-01T12:00:00Z"),
    )))
    .expect("an event is appended");
    (print, event)
}

/// Every record kind the contracts declare is written and read back.
#[test]
fn every_record_kind_is_written_and_read_back() {
    for store in Fixture::both() {
        let name = store.name();
        let port = store.port();
        let (print, event) = print_and_event(&port);

        assert_eq!(
            block_on(port.print(print.id)),
            Ok(Some(print.clone())),
            "{name}: the print did not read back"
        );
        assert_eq!(
            block_on(port.history(whole_window(print.id))),
            Ok(vec![event.clone()]),
            "{name}: the event did not read back"
        );

        let image = block_on(port.put_image(
            print.id,
            event.id,
            Some("https://obico.example/snapshot.jpg".to_owned()),
            "image/jpeg".to_owned(),
            RawBytes::new(b"the first snapshot".to_vec()),
        ))
        .expect("an image is stored");
        assert_eq!(
            block_on(port.image(image.id)).map(|lookup| match lookup {
                ImageLookup::Found { record, .. } | ImageLookup::FileMissing { record } => record,
            }),
            Ok(image.clone()),
            "{name}: the image did not read back"
        );

        let action = block_on(port.record_action(
            request(instant("2026-03-01T12:01:00Z")),
            PolicyDecision::Accepted,
        ))
        .expect("an action is recorded");
        assert_eq!(action.print_id, print.id, "{name}");
        let executed = block_on(port.record_execution(action.id, ExecutionOutcome::Succeeded))
            .expect("an execution is recorded");
        assert_eq!(
            executed.outcome,
            Some(ExecutionOutcome::Succeeded),
            "{name}: the execution outcome did not read back"
        );

        let intervention = block_on(port.open_intervention(
            action.id,
            Adjustable::Feedrate,
            Some(1.0),
            0.8,
            instant("2026-03-01T12:02:00Z"),
            instant("2026-03-01T12:32:00Z"),
        ))
        .expect("an intervention opens");
        assert_eq!(
            block_on(port.active_interventions(print.id)),
            Ok(vec![intervention.clone()]),
            "{name}: the intervention did not read back"
        );

        block_on(port.put_manifest(print.id, manifest())).expect("a manifest is stored");
        assert_eq!(
            block_on(port.manifest(print.id)),
            Ok(Some(manifest())),
            "{name}: the manifest did not read back"
        );

        let watch = session(print.id, instant("2026-03-01T11:59:00Z"));
        block_on(port.put_session(watch.clone())).expect("a session is stored");
        assert_eq!(
            block_on(port.session(print.id)),
            Ok(Some(watch)),
            "{name}: the session did not read back"
        );
    }
}

/// A print is read by its own identifier and by Obico's, and ends once.
#[test]
fn a_print_is_read_by_either_identifier_and_ends_with_its_reason() {
    for store in Fixture::both() {
        let name = store.name();
        let port = store.port();
        let print = block_on(port.open_print(Some(41), None)).expect("a print opens");

        assert_eq!(
            block_on(port.print_by_obico_id(41)),
            Ok(Some(print.clone())),
            "{name}: the print did not read back by Obico's identifier"
        );
        assert_eq!(
            block_on(port.print_by_obico_id(42)),
            Ok(None),
            "{name}: an unknown Obico identifier answered a print"
        );

        let narrowed = block_on(port.record_narrowing(
            print.id,
            printobserver_types::ManifestNarrowing {
                adjustable: Adjustable::Fan,
                requested: printobserver_types::Range {
                    min: 0.0,
                    max: 100.0,
                },
                applied: printobserver_types::Range {
                    min: 0.0,
                    max: 60.0,
                },
            },
        ))
        .expect("a narrowing is recorded");
        assert_eq!(narrowed.narrowings.len(), 1, "{name}");

        let ended_at = instant("2026-03-01T13:00:00Z");
        let ended = block_on(port.end_print(
            print.id,
            PrinterState::Cancelling,
            ended_at,
            "the operator cancelled it".to_owned(),
        ))
        .expect("the print ends");
        assert_eq!(ended.ended_at, Some(ended_at), "{name}");
        assert_eq!(ended.state, PrinterState::Cancelling, "{name}");
        assert_eq!(
            ended.end_reason.as_deref(),
            Some("the operator cancelled it"),
            "{name}"
        );
        assert_eq!(
            ended.narrowings, narrowed.narrowings,
            "{name}: ending the print lost its narrowings"
        );
    }
}

/// Every event this journey writes, with the kind and instant it was written at.
struct Written {
    /// The events, oldest first.
    events: Vec<EventRecord>,
}

impl Written {
    /// Write more events than the default window, over several kinds and spans.
    fn write(port: &Arc<dyn StorePort>, print_id: PrintId) -> Self {
        let kinds = [
            "obico_failure_alert",
            "malformed_external_event",
            "supervision_session_opened",
        ];
        let events = (0..DEFAULT_HISTORY_WINDOW + 10)
            .map(|index| {
                let minute = index % 60;
                let hour = 10 + index / 60;
                let at = instant(&format!("2026-03-01T{hour:02}:{minute:02}:00Z"));
                block_on(port.append_event(draft(
                    Some(print_id),
                    kinds[index as usize % kinds.len()],
                    at,
                )))
                .expect("an event is appended")
            })
            .collect();
        Self { events }
    }

    /// The events matching a kind and a span, oldest first.
    fn matching(
        &self,
        kind: Option<EventKind>,
        span: Option<(Timestamp, Timestamp)>,
    ) -> Vec<EventRecord> {
        self.events
            .iter()
            .filter(|event| kind.is_none_or(|wanted| event.kind() == wanted))
            .filter(|event| {
                span.is_none_or(|(since, until)| {
                    event.received_at >= since && event.received_at <= until
                })
            })
            .cloned()
            .collect()
    }
}

/// The history read answers newest first, filters, and refuses an over-limit.
#[test]
fn the_history_read_orders_filters_and_refuses() {
    for store in Fixture::both() {
        let name = store.name();
        let port = store.port();
        let print = block_on(port.open_print(None, None)).expect("a print opens");
        let written = Written::write(&port, print.id);

        let default = block_on(port.history(whole_window(print.id))).expect("a history reads");
        let window = usize::try_from(DEFAULT_HISTORY_WINDOW).expect("a window fits");
        assert_eq!(
            default.len(),
            window,
            "{name}: the default read did not take the declared window"
        );
        let newest: Vec<EventRecord> = written.events.iter().rev().take(window).cloned().collect();
        assert_eq!(
            default, newest,
            "{name}: the default read is not the newest window, newest first"
        );

        let kind = EventKind::MalformedExternalEvent;
        let by_kind = block_on(port.history(HistoryQuery {
            kinds: vec![kind],
            limit: Some(MAX_HISTORY_LIMIT),
            ..whole_window(print.id)
        }))
        .expect("a filtered history reads");
        let expected_kind = reversed(written.matching(Some(kind), None));
        assert!(
            !expected_kind.is_empty(),
            "{name}: the corpus has no {kind:?}"
        );
        assert_eq!(
            by_kind, expected_kind,
            "{name}: the kind filter did not hold"
        );

        let span = (
            instant("2026-03-01T10:20:00Z"),
            instant("2026-03-01T10:40:00Z"),
        );
        let by_span = block_on(port.history(HistoryQuery {
            since: Some(span.0),
            until: Some(span.1),
            limit: Some(MAX_HISTORY_LIMIT),
            ..whole_window(print.id)
        }))
        .expect("a filtered history reads");
        let expected_span = reversed(written.matching(None, Some(span)));
        assert!(!expected_span.is_empty(), "{name}: the corpus has no span");
        assert_eq!(
            by_span, expected_span,
            "{name}: the time filter did not hold"
        );

        let both = block_on(port.history(HistoryQuery {
            kinds: vec![kind],
            since: Some(span.0),
            until: Some(span.1),
            limit: Some(MAX_HISTORY_LIMIT),
            ..whole_window(print.id)
        }))
        .expect("a filtered history reads");
        let expected_both = reversed(written.matching(Some(kind), Some(span)));
        assert_distinguishing(name, &written, kind, span);
        assert_eq!(
            both, expected_both,
            "{name}: the two filters together did not hold"
        );

        let asked_for = MAX_HISTORY_LIMIT + 1;
        let refused = block_on(port.history(HistoryQuery {
            limit: Some(asked_for),
            ..whole_window(print.id)
        }));
        assert_eq!(
            refused,
            Err(StoreError::LimitRefused {
                limit: MAX_HISTORY_LIMIT,
                asked_for
            }),
            "{name}: a limit above the maximum was not refused"
        );
        assert!(
            refused
                .unwrap_err()
                .to_string()
                .contains(&MAX_HISTORY_LIMIT.to_string()),
            "{name}: the refusal does not name the maximum"
        );
    }
}

/// The same events, newest first.
fn reversed(mut events: Vec<EventRecord>) -> Vec<EventRecord> {
    events.reverse();
    events
}

/// The corpus tells a store that ignores one filter from one that applies both.
fn assert_distinguishing(
    name: &str,
    written: &Written,
    kind: EventKind,
    span: (Timestamp, Timestamp),
) {
    let both = written.matching(Some(kind), Some(span));
    let only_kind: Vec<&EventRecord> = written
        .events
        .iter()
        .filter(|event| {
            event.kind() == kind && !(event.received_at >= span.0 && event.received_at <= span.1)
        })
        .collect();
    let only_span: Vec<&EventRecord> = written
        .events
        .iter()
        .filter(|event| {
            event.kind() != kind && event.received_at >= span.0 && event.received_at <= span.1
        })
        .collect();
    let neither: Vec<&EventRecord> = written
        .events
        .iter()
        .filter(|event| {
            event.kind() != kind && !(event.received_at >= span.0 && event.received_at <= span.1)
        })
        .collect();
    assert!(
        !both.is_empty() && !only_kind.is_empty() && !only_span.is_empty() && !neither.is_empty(),
        "{name}: the corpus cannot tell the two filters apart: \
         {} match both, {} only the kind, {} only the span, {} neither",
        both.len(),
        only_kind.len(),
        only_span.len(),
        neither.len()
    );
}

/// The paged audit read walks a whole history, once each, and terminates.
#[test]
fn the_audit_read_walks_the_whole_history_to_exhaustion() {
    for store in Fixture::both() {
        let name = store.name();
        let port = store.port();
        let print = block_on(port.open_print(None, None)).expect("a print opens");
        let written = Written::write(&port, print.id);

        let mut walked: Vec<EventRecord> = Vec::new();
        let mut cursor = None;
        let mut pages = 0;
        loop {
            let page = block_on(port.audit_page(print.id, cursor, 7)).expect("a page reads");
            pages += 1;
            assert!(pages < 100, "{name}: the audit walk did not terminate");
            walked.extend(page.events);
            match page.next {
                Some(next) => cursor = Some(next),
                None => break,
            }
        }
        assert_eq!(
            walked, written.events,
            "{name}: the audit walk did not answer every event once, oldest first"
        );
    }
}

/// A row whose image file is gone is a missing image, not a failed read.
#[test]
fn an_image_whose_file_is_gone_is_answered_as_missing() {
    for store in Fixture::both() {
        let name = store.name();
        let port = store.port();
        let (print, event) = print_and_event(&port);
        let image = block_on(port.put_image(
            print.id,
            event.id,
            None,
            "image/jpeg".to_owned(),
            RawBytes::new(b"the only snapshot".to_vec()),
        ))
        .expect("an image is stored");

        let path = match block_on(port.image(image.id)) {
            Ok(ImageLookup::Found { path, .. }) => path,
            other => panic!("{name}: a stored image did not read back as found: {other:?}"),
        };
        std::fs::remove_file(&path).expect("the image file is removable");

        assert_eq!(
            block_on(port.image(image.id)),
            Ok(ImageLookup::FileMissing { record: image }),
            "{name}: a row whose file is gone was not answered as a missing image"
        );
        let absent = ImageId::new();
        assert_eq!(
            block_on(port.image(absent)),
            Err(StoreError::NotFound {
                what: format!("image {absent}")
            }),
            "{name}: an image no row names was not reported absent"
        );
    }
}

/// Every reference the port can name is refused when nothing has that value.
#[test]
fn every_reference_the_port_can_name_is_refused() {
    for store in Fixture::both() {
        let name = store.name();
        let port = store.port();
        let (print, event) = print_and_event(&port);
        let absent_print = PrintId::new();
        let absent_event = printobserver_types::EventId::new();
        let absent_action = printobserver_types::ActionId::new();

        assert_eq!(
            block_on(port.append_event(draft(
                Some(absent_print),
                "obico_failure_alert",
                instant("2026-03-01T12:00:00Z"),
            ))),
            Err(StoreError::ConstraintRefused {
                constraint: "events.print_id".to_owned()
            }),
            "{name}: an event naming no print was not refused"
        );
        assert_eq!(
            block_on(port.put_image(
                absent_print,
                event.id,
                None,
                "image/jpeg".to_owned(),
                RawBytes::new(b"orphan".to_vec())
            )),
            Err(StoreError::ConstraintRefused {
                constraint: "images.print_id".to_owned()
            }),
            "{name}: an image naming no print was not refused"
        );
        assert_eq!(
            block_on(port.put_image(
                print.id,
                absent_event,
                None,
                "image/jpeg".to_owned(),
                RawBytes::new(b"orphan".to_vec())
            )),
            Err(StoreError::ConstraintRefused {
                constraint: "images.event_id".to_owned()
            }),
            "{name}: an image naming no event was not refused"
        );
        assert_eq!(
            block_on(port.open_intervention(
                absent_action,
                Adjustable::Feedrate,
                None,
                0.8,
                instant("2026-03-01T12:00:00Z"),
                instant("2026-03-01T12:30:00Z")
            )),
            Err(StoreError::ConstraintRefused {
                constraint: "interventions.action_id".to_owned()
            }),
            "{name}: an intervention naming no action was not refused"
        );
        assert_eq!(
            block_on(port.put_manifest(absent_print, manifest())),
            Err(StoreError::ConstraintRefused {
                constraint: "manifests.print_id".to_owned()
            }),
            "{name}: a manifest naming no print was not refused"
        );
        assert_eq!(
            block_on(port.put_session(session(absent_print, instant("2026-03-01T12:00:00Z")))),
            Err(StoreError::ConstraintRefused {
                constraint: "sessions.print_id".to_owned()
            }),
            "{name}: a session naming no print was not refused"
        );
    }
}

/// An execution outcome against no action is refused for the constraint.
#[test]
fn an_execution_against_no_action_is_refused_for_the_constraint() {
    for store in Fixture::both() {
        let name = store.name();
        let port = store.port();
        let absent_action = printobserver_types::ActionId::new();

        assert_eq!(
            block_on(port.record_execution(absent_action, ExecutionOutcome::Succeeded)),
            Err(StoreError::ConstraintRefused {
                constraint: "executions.action_id".to_owned()
            }),
            "{name}: an outcome against no action was not refused for the constraint"
        );

        let absent_print = PrintId::new();
        assert_eq!(
            block_on(port.end_print(
                absent_print,
                PrinterState::Operational,
                instant("2026-03-01T12:00:00Z"),
                "nothing".to_owned()
            )),
            Err(StoreError::NotFound {
                what: format!("print {absent_print}")
            }),
            "{name}: a record that is absent is reported as absent, which is the \
             answer an execution against no action must not be"
        );
    }
}

/// An action binds to the open print, and is refused when there is none.
#[test]
fn an_action_binds_to_the_open_print_and_is_refused_without_one() {
    for store in Fixture::both() {
        let name = store.name();
        let port = store.port();

        assert_eq!(
            block_on(port.record_action(
                request(instant("2026-03-01T12:00:00Z")),
                PolicyDecision::Accepted
            )),
            Err(StoreError::NotFound {
                what: "open print to record this action against".to_owned()
            }),
            "{name}: an action with no open print was not refused"
        );

        let print = block_on(port.open_print(None, None)).expect("a print opens");
        let rejected = block_on(port.record_action(
            request(instant("2026-03-01T12:00:00Z")),
            PolicyDecision::Rejected(RejectionReason::NoActivePrint),
        ))
        .expect("an action is recorded");
        assert_eq!(rejected.print_id, print.id, "{name}");
        assert_eq!(
            rejected.decision,
            PolicyDecision::Rejected(RejectionReason::NoActivePrint),
            "{name}: the decision did not read back"
        );
        assert_eq!(rejected.outcome, None, "{name}");
    }
}

/// One intervention, and the interventions due to expire at an instant.
#[test]
fn an_intervention_settles_once_and_says_which_outcome_won() {
    for store in Fixture::both() {
        let name = store.name();
        let port = store.port();
        let (print, _) = print_and_event(&port);
        let action = block_on(port.record_action(
            request(instant("2026-03-01T12:00:00Z")),
            PolicyDecision::Accepted,
        ))
        .expect("an action is recorded");
        let intervention = block_on(port.open_intervention(
            action.id,
            Adjustable::BedTarget,
            None,
            60.0,
            instant("2026-03-01T12:00:00Z"),
            instant("2026-03-01T12:30:00Z"),
        ))
        .expect("an intervention opens");

        assert_eq!(
            block_on(port.due_interventions(instant("2026-03-01T12:29:00Z"))),
            Ok(Vec::new()),
            "{name}: an intervention was due before it expired"
        );
        assert_eq!(
            block_on(port.due_interventions(instant("2026-03-01T12:30:00Z"))),
            Ok(vec![intervention.clone()]),
            "{name}: an expired intervention was not due"
        );

        let settled =
            block_on(port.settle_intervention(intervention.id, InterventionOutcome::Restored))
                .expect("the intervention settles");
        match settled {
            SettleOutcome::Settled { intervention } => {
                assert_eq!(
                    intervention.outcome,
                    InterventionOutcome::Restored,
                    "{name}"
                );
                assert!(
                    intervention.restored_at.is_some(),
                    "{name}: a restored intervention recorded no instant"
                );
            }
            other @ SettleOutcome::AlreadySettled { .. } => {
                panic!("{name}: the first settle answered {other:?}")
            }
        }

        assert_eq!(
            block_on(
                port.settle_intervention(intervention.id, InterventionOutcome::RestoreUnavailable)
            ),
            Ok(SettleOutcome::AlreadySettled {
                outcome: InterventionOutcome::Restored
            }),
            "{name}: settling twice did not report the outcome that won"
        );
        assert_eq!(
            block_on(port.active_interventions(print.id)),
            Ok(Vec::new()),
            "{name}: a settled intervention is still active"
        );
        let absent = InterventionId::new();
        assert_eq!(
            block_on(port.settle_intervention(absent, InterventionOutcome::Restored)),
            Err(StoreError::NotFound {
                what: format!("intervention {absent}")
            }),
            "{name}: settling an intervention that does not exist answered a settle"
        );
    }
}

/// An expiry and a supersession contending cannot both take effect.
#[test]
fn an_expiry_and_a_supersession_cannot_both_take_effect() {
    let expiry = InterventionOutcome::Restored;
    let supersession = InterventionOutcome::Superseded {
        by: InterventionId::new(),
    };
    for (first, second) in [(&expiry, &supersession), (&supersession, &expiry)] {
        for store in Fixture::both() {
            let name = store.name();
            let port = store.port();
            let (_, _) = print_and_event(&port);
            let action = block_on(port.record_action(
                request(instant("2026-03-01T12:00:00Z")),
                PolicyDecision::Accepted,
            ))
            .expect("an action is recorded");
            let intervention = block_on(port.open_intervention(
                action.id,
                Adjustable::Flowrate,
                Some(1.0),
                0.9,
                instant("2026-03-01T12:00:00Z"),
                instant("2026-03-01T12:30:00Z"),
            ))
            .expect("an intervention opens");

            store.hold_points().settle().arm();
            let contenders: Vec<_> = [expiry.clone(), supersession.clone()]
                .into_iter()
                .map(|outcome| {
                    let port = Arc::clone(&port);
                    let id = intervention.id;
                    thread::spawn(move || block_on(port.settle_intervention(id, outcome)))
                })
                .collect();

            let point = store.hold_points().settle();
            point.await_holding(&[settle_label(&expiry), settle_label(&supersession)]);
            point.release(settle_label(first));
            point.release(settle_label(second));

            let answers: Vec<SettleOutcome> = contenders
                .into_iter()
                .map(|handle| handle.join().expect("the thread completes"))
                .map(|answer| answer.expect("a settle answers"))
                .collect();
            point.disarm();

            let won: Vec<&SettleOutcome> = answers
                .iter()
                .filter(|answer| matches!(answer, SettleOutcome::Settled { .. }))
                .collect();
            assert_eq!(
                won.len(),
                1,
                "{name}: releasing {} first, {} of the two settles took effect",
                settle_label(first),
                won.len()
            );
            let SettleOutcome::Settled { intervention: kept } = won[0] else {
                unreachable!("filtered to the settled answer")
            };
            let lost: Vec<&SettleOutcome> = answers
                .iter()
                .filter(|answer| matches!(answer, SettleOutcome::AlreadySettled { .. }))
                .collect();
            assert_eq!(
                lost,
                vec![&SettleOutcome::AlreadySettled {
                    outcome: kept.outcome.clone()
                }],
                "{name}: the settle that lost did not name the outcome that won"
            );
            assert_eq!(
                block_on(
                    port.settle_intervention(
                        intervention.id,
                        InterventionOutcome::RestoreUnavailable
                    )
                ),
                Ok(SettleOutcome::AlreadySettled {
                    outcome: kept.outcome.clone()
                }),
                "{name}: the store did not keep exactly the outcome that won"
            );
        }
    }
}
