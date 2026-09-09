//! The store port is implementable, dyn-compatible and shareable.
//!
//! A test-only implementation, held behind the same shared trait object the
//! supervision core will hold it behind, with every method the trait declares
//! called and awaited and each asserted to answer that method's declared
//! success type rather than an error. It behaves trivially rather than
//! erroring, because a not-yet-implemented error would be a variant no real
//! implementation can ever produce and every consumer would still have to match
//! it.

#[path = "support/block_on.rs"]
mod block_on;

use std::path::PathBuf;
use std::sync::Arc;
use std::thread;

use block_on::block_on;
use printobserver_store_api::{
    AuditPage, BoxFuture, EventDraft, HistoryQuery, ImageLookup, SettleOutcome, StoreError,
    StorePort,
};
use printobserver_types::contract::Sample;
use printobserver_types::{
    ActionId, ActionRecord, ActionRequest, Adjustable, EventId, EventRecord, ExecutionOutcome,
    ImageId, ImageRecord, Intervention, InterventionId, InterventionOutcome, JobManifest,
    ManifestNarrowing, PolicyDecision, PrintId, PrintRecord, PrinterState, RawBytes,
    SupervisionSession, Timestamp,
};

/// A store that answers every method with the success type it declares.
struct TrivialStore;

impl StorePort for TrivialStore {
    fn open_print(
        &self,
        obico_print_id: Option<i64>,
        file_name: Option<String>,
    ) -> BoxFuture<'_, Result<PrintRecord, StoreError>> {
        let _ = (obico_print_id, file_name);
        Box::pin(async { Ok(PrintRecord::sample_minimal()) })
    }

    fn print(&self, print_id: PrintId) -> BoxFuture<'_, Result<Option<PrintRecord>, StoreError>> {
        let _ = print_id;
        Box::pin(async { Ok(Some(PrintRecord::sample_minimal())) })
    }

    fn print_by_obico_id(
        &self,
        obico_print_id: i64,
    ) -> BoxFuture<'_, Result<Option<PrintRecord>, StoreError>> {
        let _ = obico_print_id;
        Box::pin(async { Ok(Some(PrintRecord::sample_minimal())) })
    }

    fn open_prints(&self) -> BoxFuture<'_, Result<Vec<PrintRecord>, StoreError>> {
        Box::pin(async { Ok(vec![PrintRecord::sample_minimal()]) })
    }

    fn end_print(
        &self,
        print_id: PrintId,
        state: PrinterState,
        ended_at: Timestamp,
        reason: String,
    ) -> BoxFuture<'_, Result<PrintRecord, StoreError>> {
        let _ = (print_id, state, ended_at, reason);
        Box::pin(async { Ok(PrintRecord::sample_minimal()) })
    }

    fn record_narrowing(
        &self,
        print_id: PrintId,
        narrowing: ManifestNarrowing,
    ) -> BoxFuture<'_, Result<PrintRecord, StoreError>> {
        let _ = (print_id, narrowing);
        Box::pin(async { Ok(PrintRecord::sample_minimal()) })
    }

    fn append_event(&self, draft: EventDraft) -> BoxFuture<'_, Result<EventRecord, StoreError>> {
        let _ = draft;
        Box::pin(async { Ok(EventRecord::sample_minimal()) })
    }

    fn put_image(
        &self,
        print_id: PrintId,
        event_id: EventId,
        source_url: Option<String>,
        content_type: String,
        bytes: RawBytes,
    ) -> BoxFuture<'_, Result<ImageRecord, StoreError>> {
        let _ = (print_id, event_id, source_url, content_type, bytes);
        Box::pin(async { Ok(ImageRecord::sample_minimal()) })
    }

    fn image(&self, image_id: ImageId) -> BoxFuture<'_, Result<ImageLookup, StoreError>> {
        let _ = image_id;
        Box::pin(async {
            Ok(ImageLookup::Found {
                record: ImageRecord::sample_minimal(),
                path: PathBuf::new(),
            })
        })
    }

    fn record_action(
        &self,
        request: ActionRequest,
        decision: PolicyDecision,
    ) -> BoxFuture<'_, Result<ActionRecord, StoreError>> {
        let _ = (request, decision);
        Box::pin(async { Ok(ActionRecord::sample_minimal()) })
    }

    fn record_execution(
        &self,
        action_id: ActionId,
        outcome: ExecutionOutcome,
    ) -> BoxFuture<'_, Result<ActionRecord, StoreError>> {
        let _ = (action_id, outcome);
        Box::pin(async { Ok(ActionRecord::sample_minimal()) })
    }

    fn open_intervention(
        &self,
        action_id: ActionId,
        adjustable: Adjustable,
        prior_value: Option<f64>,
        applied_value: f64,
        applied_at: Timestamp,
        expires_at: Timestamp,
    ) -> BoxFuture<'_, Result<Intervention, StoreError>> {
        let _ = (
            action_id,
            adjustable,
            prior_value,
            applied_value,
            applied_at,
            expires_at,
        );
        Box::pin(async { Ok(Intervention::sample_minimal()) })
    }

    fn settle_intervention(
        &self,
        intervention_id: InterventionId,
        outcome: InterventionOutcome,
    ) -> BoxFuture<'_, Result<SettleOutcome, StoreError>> {
        let _ = (intervention_id, outcome);
        Box::pin(async {
            Ok(SettleOutcome::Settled {
                intervention: Intervention::sample_minimal(),
            })
        })
    }

    fn due_interventions(
        &self,
        at: Timestamp,
    ) -> BoxFuture<'_, Result<Vec<Intervention>, StoreError>> {
        let _ = at;
        Box::pin(async { Ok(Vec::new()) })
    }

    fn active_interventions(
        &self,
        print_id: PrintId,
    ) -> BoxFuture<'_, Result<Vec<Intervention>, StoreError>> {
        let _ = print_id;
        Box::pin(async { Ok(Vec::new()) })
    }

    fn put_manifest(
        &self,
        print_id: PrintId,
        manifest: JobManifest,
    ) -> BoxFuture<'_, Result<(), StoreError>> {
        let _ = (print_id, manifest);
        Box::pin(async { Ok(()) })
    }

    fn manifest(
        &self,
        print_id: PrintId,
    ) -> BoxFuture<'_, Result<Option<JobManifest>, StoreError>> {
        let _ = print_id;
        Box::pin(async { Ok(Some(JobManifest::sample_full())) })
    }

    fn history(&self, query: HistoryQuery) -> BoxFuture<'_, Result<Vec<EventRecord>, StoreError>> {
        let _ = query;
        Box::pin(async { Ok(Vec::new()) })
    }

    fn audit_page(
        &self,
        print_id: PrintId,
        after: Option<EventId>,
        page_size: u32,
    ) -> BoxFuture<'_, Result<AuditPage, StoreError>> {
        let _ = (print_id, after, page_size);
        Box::pin(async {
            Ok(AuditPage {
                events: Vec::new(),
                next: None,
            })
        })
    }

    fn put_session(&self, session: SupervisionSession) -> BoxFuture<'_, Result<(), StoreError>> {
        let _ = session;
        Box::pin(async { Ok(()) })
    }

    fn session(
        &self,
        print_id: PrintId,
    ) -> BoxFuture<'_, Result<Option<SupervisionSession>, StoreError>> {
        let _ = print_id;
        Box::pin(async { Ok(Some(SupervisionSession::sample_minimal())) })
    }
}

/// A print identifier to drive the methods that take one.
fn print_id() -> PrintId {
    PrintId::sample_full()
}

/// The query to drive the history read with.
fn query() -> HistoryQuery {
    HistoryQuery {
        print_id: print_id(),
        kinds: Vec::new(),
        since: None,
        until: None,
        limit: None,
    }
}

/// The draft to drive the event append with.
fn draft() -> EventDraft {
    EventDraft {
        print_id: Some(print_id()),
        source: printobserver_types::EventSource::Obico,
        received_at: Timestamp::sample_full(),
        payload: printobserver_types::EventPayload::sample_minimal(),
        raw: None,
    }
}

/// Every print and event method answers its declared success type.
#[test]
fn every_print_and_event_method_answers_its_declared_success_type() {
    let port: Arc<dyn StorePort> = Arc::new(TrivialStore);
    assert_eq!(
        block_on(port.open_print(None, None)),
        Ok(PrintRecord::sample_minimal())
    );
    assert_eq!(
        block_on(port.print(print_id())),
        Ok(Some(PrintRecord::sample_minimal()))
    );
    assert_eq!(
        block_on(port.print_by_obico_id(1)),
        Ok(Some(PrintRecord::sample_minimal()))
    );
    assert_eq!(
        block_on(port.end_print(
            print_id(),
            PrinterState::Operational,
            Timestamp::sample_full(),
            String::new()
        )),
        Ok(PrintRecord::sample_minimal())
    );
    assert_eq!(
        block_on(port.record_narrowing(print_id(), ManifestNarrowing::sample_full())),
        Ok(PrintRecord::sample_minimal())
    );
    assert_eq!(
        block_on(port.append_event(draft())),
        Ok(EventRecord::sample_minimal())
    );
    assert_eq!(block_on(port.history(query())), Ok(Vec::new()));
    assert_eq!(
        block_on(port.audit_page(print_id(), None, 10)),
        Ok(AuditPage {
            events: Vec::new(),
            next: None
        })
    );
}

/// Every image, action and intervention method answers its declared type.
#[test]
fn every_image_action_and_intervention_method_answers_its_declared_success_type() {
    let port: Arc<dyn StorePort> = Arc::new(TrivialStore);
    assert_eq!(
        block_on(port.put_image(
            print_id(),
            EventId::sample_full(),
            None,
            "image/jpeg".to_owned(),
            RawBytes::default()
        )),
        Ok(ImageRecord::sample_minimal())
    );
    assert_eq!(
        block_on(port.image(ImageId::sample_full())),
        Ok(ImageLookup::Found {
            record: ImageRecord::sample_minimal(),
            path: PathBuf::new()
        })
    );
    assert_eq!(
        block_on(port.record_action(ActionRequest::sample_minimal(), PolicyDecision::Accepted)),
        Ok(ActionRecord::sample_minimal())
    );
    assert_eq!(
        block_on(port.record_execution(ActionId::sample_full(), ExecutionOutcome::Succeeded)),
        Ok(ActionRecord::sample_minimal())
    );
    assert_eq!(
        block_on(port.open_intervention(
            ActionId::sample_full(),
            Adjustable::Feedrate,
            None,
            0.8,
            Timestamp::sample_full(),
            Timestamp::sample_full()
        )),
        Ok(Intervention::sample_minimal())
    );
    assert_eq!(
        block_on(
            port.settle_intervention(InterventionId::sample_full(), InterventionOutcome::Restored)
        ),
        Ok(SettleOutcome::Settled {
            intervention: Intervention::sample_minimal()
        })
    );
    assert_eq!(
        block_on(port.due_interventions(Timestamp::sample_full())),
        Ok(Vec::new())
    );
    assert_eq!(
        block_on(port.active_interventions(print_id())),
        Ok(Vec::new())
    );
}

/// Every manifest and session method answers its declared success type.
#[test]
fn every_manifest_and_session_method_answers_its_declared_success_type() {
    let port: Arc<dyn StorePort> = Arc::new(TrivialStore);
    assert_eq!(
        block_on(port.put_manifest(print_id(), JobManifest::sample_full())),
        Ok(())
    );
    assert_eq!(
        block_on(port.manifest(print_id())),
        Ok(Some(JobManifest::sample_full()))
    );
    assert_eq!(
        block_on(port.put_session(SupervisionSession::sample_minimal())),
        Ok(())
    );
    assert_eq!(
        block_on(port.session(print_id())),
        Ok(Some(SupervisionSession::sample_minimal()))
    );
}

/// The same trait object is shareable across threads, which is what core needs.
#[test]
fn the_trait_object_is_shareable_across_threads() {
    let port: Arc<dyn StorePort> = Arc::new(TrivialStore);
    let handles: Vec<_> = (0..4)
        .map(|_| {
            let shared = Arc::clone(&port);
            thread::spawn(move || block_on(shared.print(print_id())))
        })
        .collect();
    for handle in handles {
        assert_eq!(
            handle.join().expect("the thread completes"),
            Ok(Some(PrintRecord::sample_minimal()))
        );
    }
}

/// An image lookup distinguishes a missing file from a missing record.
#[test]
fn an_image_lookup_distinguishes_a_missing_file() {
    let missing = ImageLookup::FileMissing {
        record: ImageRecord::sample_minimal(),
    };
    assert_ne!(
        missing,
        ImageLookup::Found {
            record: ImageRecord::sample_minimal(),
            path: PathBuf::new()
        }
    );
}

/// Settling an already-settled intervention answers the outcome that won.
#[test]
fn settling_twice_answers_the_outcome_that_won() {
    let already = SettleOutcome::AlreadySettled {
        outcome: InterventionOutcome::Restored,
    };
    assert_ne!(
        already,
        SettleOutcome::Settled {
            intervention: Intervention::sample_minimal()
        }
    );
}

/// Every variant of this port's error vocabulary says what it is.
#[test]
fn every_error_variant_says_what_it_is() {
    let variants = [
        StoreError::ConstraintRefused {
            constraint: "print_id".to_owned(),
        },
        StoreError::NotFound {
            what: "print".to_owned(),
        },
        StoreError::LimitRefused {
            limit: 1_000,
            asked_for: 5_000,
        },
        StoreError::Database {
            detail: "locked".to_owned(),
        },
        StoreError::Io {
            detail: "no space".to_owned(),
        },
    ];
    for variant in variants {
        assert!(!variant.to_string().is_empty(), "{variant:?} says nothing");
    }
}
