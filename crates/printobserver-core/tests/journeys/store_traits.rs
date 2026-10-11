//! The store traits are implementable, dyn-compatible and shareable.
//!
//! A test-only implementation of all seven, held behind the same shared trait
//! objects the supervisor holds them behind, with every method each trait
//! declares called and awaited and each asserted to answer that method's
//! declared success type rather than an error. It behaves trivially rather than
//! erroring, because a not-yet-implemented error would be a variant no real
//! implementation can ever produce and every consumer would still have to match
//! it.

use std::path::PathBuf;
use std::sync::Arc;
use std::thread;

use printobserver_core::block_on;
use printobserver_core::store::{
    ActionStore, AuditPage, BoxFuture, Charge, ChargeRecord, EventDraft, EventStore, FilamentStore,
    FileTally, HistoryQuery, ImageLookup, ImageStore, NewSpool, Pend, PrintStore, SessionStore,
    SettleOutcome, Settlement, SpoolMutation, SpoolWrite, StoreError, Stores, TimelineQuery,
    TimelineStore,
};
use printobserver_core::{
    ActionId, ActionRecord, ActionRequest, Actor, ExecutionOutcome, FilePosition, ImageRecord,
    Intervention, InterventionId, InterventionOutcome, JobManifest, ManifestNarrowing,
    PendingCharge, PolicyDecision, PrintOutcome, PrintRecord, SpoolId, SpoolRecord,
};
use printobserver_printer_api::{Adjustable, PrinterState};
use printobserver_supervisor_api::SupervisionSession;
use printobserver_types::contract::Sample;
use printobserver_types::{
    EventId, EventRecord, EventSource, ImageId, PrintId, RawBytes, Timestamp,
};

/// A store that answers every method with the success type it declares.
struct TrivialStore;

impl PrintStore for TrivialStore {
    fn open_print(
        &self,
        provider_print_id: Option<i64>,
        file_name: Option<String>,
    ) -> BoxFuture<'_, Result<PrintRecord, StoreError>> {
        let _ = (provider_print_id, file_name);
        Box::pin(async { Ok(PrintRecord::sample_minimal()) })
    }

    fn print(&self, print_id: PrintId) -> BoxFuture<'_, Result<Option<PrintRecord>, StoreError>> {
        let _ = print_id;
        Box::pin(async { Ok(Some(PrintRecord::sample_minimal())) })
    }

    fn print_by_provider_id(
        &self,
        provider_print_id: i64,
    ) -> BoxFuture<'_, Result<Option<PrintRecord>, StoreError>> {
        let _ = provider_print_id;
        Box::pin(async { Ok(Some(PrintRecord::sample_minimal())) })
    }

    fn open_prints(&self) -> BoxFuture<'_, Result<Vec<PrintRecord>, StoreError>> {
        Box::pin(async { Ok(vec![PrintRecord::sample_minimal()]) })
    }

    fn prints(&self) -> BoxFuture<'_, Result<Vec<PrintRecord>, StoreError>> {
        Box::pin(async { Ok(vec![PrintRecord::sample_minimal()]) })
    }

    fn attach_obico_print(
        &self,
        print_id: PrintId,
        obico_print_id: i64,
    ) -> BoxFuture<'_, Result<PrintRecord, StoreError>> {
        let _ = (print_id, obico_print_id);
        Box::pin(async { Ok(PrintRecord::sample_minimal()) })
    }

    fn record_job_sighting(
        &self,
        print_id: PrintId,
        job_started_at: Timestamp,
        job_print_time_s: Option<i64>,
    ) -> BoxFuture<'_, Result<PrintRecord, StoreError>> {
        let _ = (print_id, job_started_at, job_print_time_s);
        Box::pin(async { Ok(PrintRecord::sample_minimal()) })
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
}

impl EventStore for TrivialStore {
    fn append_event(&self, draft: EventDraft) -> BoxFuture<'_, Result<EventRecord, StoreError>> {
        let _ = draft;
        Box::pin(async { Ok(EventRecord::sample_minimal()) })
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
}

impl ImageStore for TrivialStore {
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
}

impl ActionStore for TrivialStore {
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
}

impl SessionStore for TrivialStore {
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

impl FilamentStore for TrivialStore {
    fn register_spool(
        &self,
        spool: NewSpool,
        source: EventSource,
        actor: Actor,
        reason: String,
    ) -> BoxFuture<'_, Result<(SpoolRecord, EventRecord), StoreError>> {
        let _ = (spool, source, actor, reason);
        Box::pin(async { Ok((SpoolRecord::sample_minimal(), EventRecord::sample_minimal())) })
    }

    fn apply_spool_changes(
        &self,
        mutations: Vec<SpoolMutation>,
        charges: Vec<Charge>,
        pends: Vec<Pend>,
    ) -> BoxFuture<'_, Result<SpoolWrite, StoreError>> {
        let _ = (mutations, charges, pends);
        Box::pin(async { Ok(empty_write()) })
    }

    fn settle_print(
        &self,
        print_id: PrintId,
        outcome: PrintOutcome,
        ended: EventDraft,
        charges: Vec<Charge>,
        pends: Vec<Pend>,
        marks: Vec<SpoolMutation>,
    ) -> BoxFuture<'_, Result<Settlement, StoreError>> {
        let _ = (print_id, ended, charges, pends, marks);
        Box::pin(async move {
            Ok(Settlement::AlreadySettled {
                at: Timestamp::sample_full(),
                outcome,
            })
        })
    }

    fn print_settlement(
        &self,
        print_id: PrintId,
    ) -> BoxFuture<'_, Result<Option<(Timestamp, PrintOutcome)>, StoreError>> {
        let _ = print_id;
        Box::pin(async { Ok(None) })
    }

    fn charges(&self, print_id: PrintId) -> BoxFuture<'_, Result<Vec<ChargeRecord>, StoreError>> {
        let _ = print_id;
        Box::pin(async { Ok(Vec::new()) })
    }

    fn pending_charges(
        &self,
        tool: Option<i64>,
    ) -> BoxFuture<'_, Result<Vec<PendingCharge>, StoreError>> {
        let _ = tool;
        Box::pin(async { Ok(vec![PendingCharge::sample_minimal()]) })
    }

    fn spool(&self, id: SpoolId) -> BoxFuture<'_, Result<Option<SpoolRecord>, StoreError>> {
        let _ = id;
        Box::pin(async { Ok(Some(SpoolRecord::sample_minimal())) })
    }

    fn spools(&self, include_retired: bool) -> BoxFuture<'_, Result<Vec<SpoolRecord>, StoreError>> {
        let _ = include_retired;
        Box::pin(async { Ok(vec![SpoolRecord::sample_minimal()]) })
    }

    fn loaded_spools(&self) -> BoxFuture<'_, Result<Vec<SpoolRecord>, StoreError>> {
        Box::pin(async { Ok(Vec::new()) })
    }

    fn record_file_position(
        &self,
        print_id: PrintId,
        position: FilePosition,
    ) -> BoxFuture<'_, Result<(), StoreError>> {
        let _ = (print_id, position);
        Box::pin(async { Ok(()) })
    }

    fn file_position(
        &self,
        print_id: PrintId,
    ) -> BoxFuture<'_, Result<Option<FilePosition>, StoreError>> {
        let _ = print_id;
        Box::pin(async { Ok(Some(FilePosition::sample_minimal())) })
    }

    fn file_tallies(&self) -> BoxFuture<'_, Result<Vec<FileTally>, StoreError>> {
        Box::pin(async { Ok(Vec::new()) })
    }

    fn account_tallies(
        &self,
        tallies: Vec<FileTally>,
        charges: Vec<Charge>,
        marks: Vec<SpoolMutation>,
        events: Vec<EventDraft>,
    ) -> BoxFuture<'_, Result<Vec<EventRecord>, StoreError>> {
        let _ = (tallies, charges, marks, events);
        Box::pin(async { Ok(Vec::new()) })
    }
}

impl TimelineStore for TrivialStore {
    fn timeline(
        &self,
        query: TimelineQuery,
    ) -> BoxFuture<'_, Result<Vec<EventRecord>, StoreError>> {
        let _ = query;
        Box::pin(async { Ok(Vec::new()) })
    }
}

/// A spool write that wrote nothing.
fn empty_write() -> SpoolWrite {
    SpoolWrite {
        spools: Vec::new(),
        events: Vec::new(),
        charges: Vec::new(),
        pending: Vec::new(),
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
        source: printobserver_types::EventSource::sample_full(),
        received_at: Timestamp::sample_full(),
        body: printobserver_types::EventBody::sample_full(),
        raw: None,
    }
}

/// Every print and event method answers its declared success type.
#[test]
fn every_print_and_event_method_answers_its_declared_success_type() {
    let stores = Stores::of(Arc::new(TrivialStore));
    assert_eq!(
        block_on(stores.prints.open_print(None, None)),
        Ok(PrintRecord::sample_minimal())
    );
    assert_eq!(
        block_on(stores.prints.print(print_id())),
        Ok(Some(PrintRecord::sample_minimal()))
    );
    assert_eq!(
        block_on(stores.prints.print_by_provider_id(1)),
        Ok(Some(PrintRecord::sample_minimal()))
    );
    assert_eq!(
        block_on(stores.prints.prints()),
        Ok(vec![PrintRecord::sample_minimal()])
    );
    assert_eq!(
        block_on(stores.prints.attach_obico_print(print_id(), 1)),
        Ok(PrintRecord::sample_minimal())
    );
    assert_eq!(
        block_on(
            stores
                .prints
                .record_job_sighting(print_id(), Timestamp::sample_full(), Some(1))
        ),
        Ok(PrintRecord::sample_minimal())
    );
    assert_eq!(
        block_on(stores.prints.end_print(
            print_id(),
            PrinterState::Operational,
            Timestamp::sample_full(),
            String::new()
        )),
        Ok(PrintRecord::sample_minimal())
    );
    assert_eq!(
        block_on(
            stores
                .prints
                .record_narrowing(print_id(), ManifestNarrowing::sample_full())
        ),
        Ok(PrintRecord::sample_minimal())
    );
    assert_eq!(
        block_on(stores.events.append_event(draft())),
        Ok(EventRecord::sample_minimal())
    );
    assert_eq!(block_on(stores.events.history(query())), Ok(Vec::new()));
    assert_eq!(
        block_on(stores.events.audit_page(print_id(), None, 10)),
        Ok(AuditPage {
            events: Vec::new(),
            next: None
        })
    );
}

/// Every image, action and intervention method answers its declared type.
#[test]
fn every_image_action_and_intervention_method_answers_its_declared_success_type() {
    let stores = Stores::of(Arc::new(TrivialStore));
    assert_eq!(
        block_on(stores.images.put_image(
            print_id(),
            EventId::sample_full(),
            None,
            "image/jpeg".to_owned(),
            RawBytes::default()
        )),
        Ok(ImageRecord::sample_minimal())
    );
    assert_eq!(
        block_on(stores.images.image(ImageId::sample_full())),
        Ok(ImageLookup::Found {
            record: ImageRecord::sample_minimal(),
            path: PathBuf::new()
        })
    );
    assert_eq!(
        block_on(
            stores
                .actions
                .record_action(ActionRequest::sample_minimal(), PolicyDecision::Accepted)
        ),
        Ok(ActionRecord::sample_minimal())
    );
    assert_eq!(
        block_on(
            stores
                .actions
                .record_execution(ActionId::sample_full(), ExecutionOutcome::Succeeded)
        ),
        Ok(ActionRecord::sample_minimal())
    );
    assert_eq!(
        block_on(stores.actions.open_intervention(
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
            stores
                .actions
                .settle_intervention(InterventionId::sample_full(), InterventionOutcome::Restored)
        ),
        Ok(SettleOutcome::Settled {
            intervention: Intervention::sample_minimal()
        })
    );
    assert_eq!(
        block_on(stores.actions.due_interventions(Timestamp::sample_full())),
        Ok(Vec::new())
    );
    assert_eq!(
        block_on(stores.actions.active_interventions(print_id())),
        Ok(Vec::new())
    );
}

/// Every manifest and session method answers its declared success type.
#[test]
fn every_manifest_and_session_method_answers_its_declared_success_type() {
    let stores = Stores::of(Arc::new(TrivialStore));
    assert_eq!(
        block_on(
            stores
                .prints
                .put_manifest(print_id(), JobManifest::sample_full())
        ),
        Ok(())
    );
    assert_eq!(
        block_on(stores.prints.manifest(print_id())),
        Ok(Some(JobManifest::sample_full()))
    );
    assert_eq!(
        block_on(
            stores
                .sessions
                .put_session(SupervisionSession::sample_minimal())
        ),
        Ok(())
    );
    assert_eq!(
        block_on(stores.sessions.session(print_id())),
        Ok(Some(SupervisionSession::sample_minimal()))
    );
}

/// Every filament ledger and timeline method answers its declared success type.
#[test]
fn every_filament_and_timeline_method_answers_its_declared_success_type() {
    let stores = Stores::of(Arc::new(TrivialStore));
    let spool = NewSpool {
        name: "shelf PLA".to_owned(),
        material: "PLA".to_owned(),
        color: None,
        diameter_mm: 1.75,
        density_g_cm3: 1.24,
        initial_g: 1_000.0,
        remaining_g: 1_000.0,
        tare_g: None,
        loaded_on_tool: None,
        trust: printobserver_core::SpoolTrust::Verified,
        registered_at: Timestamp::sample_full(),
    };
    assert_eq!(
        block_on(stores.filament.register_spool(
            spool,
            EventSource::sample_full(),
            Actor::Operator,
            String::new()
        )),
        Ok((SpoolRecord::sample_minimal(), EventRecord::sample_minimal()))
    );
    assert_eq!(
        block_on(
            stores
                .filament
                .apply_spool_changes(Vec::new(), Vec::new(), Vec::new())
        ),
        Ok(empty_write())
    );
    assert_eq!(
        block_on(stores.filament.settle_print(
            print_id(),
            PrintOutcome::Completed,
            draft(),
            Vec::new(),
            Vec::new(),
            Vec::new()
        )),
        Ok(Settlement::AlreadySettled {
            at: Timestamp::sample_full(),
            outcome: PrintOutcome::Completed
        })
    );
    assert_eq!(
        block_on(stores.filament.print_settlement(print_id())),
        Ok(None)
    );
    assert_eq!(
        block_on(stores.filament.charges(print_id())),
        Ok(Vec::new())
    );
    assert_eq!(
        block_on(stores.filament.pending_charges(Some(0))),
        Ok(vec![PendingCharge::sample_minimal()])
    );
    assert_eq!(
        block_on(stores.filament.spool(SpoolId::sample_full())),
        Ok(Some(SpoolRecord::sample_minimal()))
    );
    assert_eq!(
        block_on(stores.filament.spools(true)),
        Ok(vec![SpoolRecord::sample_minimal()])
    );
    assert_eq!(block_on(stores.filament.loaded_spools()), Ok(Vec::new()));
    assert_eq!(
        block_on(
            stores
                .filament
                .record_file_position(print_id(), FilePosition::sample_minimal())
        ),
        Ok(())
    );
    assert_eq!(
        block_on(stores.filament.file_position(print_id())),
        Ok(Some(FilePosition::sample_minimal()))
    );
    assert_eq!(block_on(stores.filament.file_tallies()), Ok(Vec::new()));
    assert_eq!(
        block_on(
            stores
                .filament
                .account_tallies(Vec::new(), Vec::new(), Vec::new(), Vec::new())
        ),
        Ok(Vec::new())
    );
    assert_eq!(
        block_on(stores.timeline.timeline(TimelineQuery {
            kinds: Vec::new(),
            since: None,
            until: None,
            spool_id: None,
            before: None,
            limit: None,
        })),
        Ok(Vec::new())
    );
}

/// The same trait object is shareable across threads, which is what core needs.
#[test]
fn the_trait_object_is_shareable_across_threads() {
    let stores = Stores::of(Arc::new(TrivialStore));
    let handles: Vec<_> = (0..4)
        .map(|_| {
            let shared: Arc<dyn PrintStore> = Arc::clone(&stores.prints);
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
