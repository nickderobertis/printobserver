//! A store whose one named write commits and is then reported as failed.
//!
//! The shape of a process dying between a write and the step after it: the
//! write is the real store's, carried out and committed, and the caller is
//! told it did not happen — so whatever the caller would have done next is
//! not done, and a restart over the same file is what has to finish it. Every
//! other call is the real store's, answered as it answers.
//!
//! It stands beside [`crate::failing_store`], which fails everything, and
//! [`crate::refusing_prints`], which refuses one aggregate: this one fails
//! nothing that did not happen.

use std::sync::{Arc, Mutex};

use printobserver_core::store::{
    ActionStore, AuditPage, BoxFuture, Charge, ChargeRecord, EventDraft, EventStore, FilamentStore,
    FileTally, HistoryQuery, ImageLookup, ImageStore, NewSpool, Pend, PrintStore, SessionStore,
    SettleOutcome, Settlement, SpoolMutation, SpoolWrite, StoreError, StoredQuery, StoredSignal,
    Stores, TimelineQuery, TimelineStore,
};
use printobserver_core::{
    ActionId, ActionRecord, ActionRequest, Actor, ExecutionOutcome, FilePosition, ImageRecord,
    Intervention, InterventionId, InterventionOutcome, JobManifest, ManifestNarrowing,
    PendingCharge, PolicyDecision, PrintOutcome, PrintRecord, SpoolId, SpoolRecord,
};
use printobserver_printer_api::{Adjustable, PrinterState};
use printobserver_store_sqlite::SqliteStore;
use printobserver_supervisor_api::SupervisionSession;
use printobserver_types::{
    EventId, EventRecord, EventSource, ImageId, PrintId, RawBytes, Timestamp,
};

/// What the store says of the write it carried out.
pub const DETAIL: &str = "the process died after this write committed";

/// The real store, with one named write reported as failed after it commits.
#[derive(Debug)]
pub struct CommitThenFail {
    /// The store every call is carried out by.
    inner: Arc<SqliteStore>,
    /// The write to report as failed, until it has been once.
    failing: Mutex<Option<&'static str>>,
}

impl CommitThenFail {
    /// The stores over `inner`, whose next `write` commits and is then
    /// reported as failed; every later call, that write's included, is
    /// answered as the store answers it.
    #[must_use]
    pub fn once(inner: SqliteStore, write: &'static str) -> Stores {
        Stores::of(Arc::new(Self {
            inner: Arc::new(inner),
            failing: Mutex::new(Some(write)),
        }))
    }

    /// Carry one call out, and report it failed when it is the named write's
    /// first.
    fn after<'a, T: Send + 'a>(
        &'a self,
        name: &'static str,
        done: BoxFuture<'a, Result<T, StoreError>>,
    ) -> BoxFuture<'a, Result<T, StoreError>> {
        Box::pin(async move {
            let answered = done.await?;
            let mut failing = self.failing.lock().expect("the wrapper is not poisoned");
            if *failing == Some(name) {
                *failing = None;
                return Err(StoreError::Database {
                    detail: DETAIL.to_owned(),
                });
            }
            Ok(answered)
        })
    }
}

impl PrintStore for CommitThenFail {
    fn open_print(
        &self,
        provider_print_id: Option<i64>,
        file_name: Option<String>,
    ) -> BoxFuture<'_, Result<PrintRecord, StoreError>> {
        self.after(
            "open_print",
            PrintStore::open_print(&*self.inner, provider_print_id, file_name),
        )
    }

    fn print(&self, print_id: PrintId) -> BoxFuture<'_, Result<Option<PrintRecord>, StoreError>> {
        self.after("print", PrintStore::print(&*self.inner, print_id))
    }

    fn open_prints(&self) -> BoxFuture<'_, Result<Vec<PrintRecord>, StoreError>> {
        self.after("open_prints", PrintStore::open_prints(&*self.inner))
    }

    fn prints(&self) -> BoxFuture<'_, Result<Vec<PrintRecord>, StoreError>> {
        self.after("prints", PrintStore::prints(&*self.inner))
    }

    fn attach_obico_print(
        &self,
        print_id: PrintId,
        obico_print_id: i64,
    ) -> BoxFuture<'_, Result<PrintRecord, StoreError>> {
        self.after(
            "attach_obico_print",
            PrintStore::attach_obico_print(&*self.inner, print_id, obico_print_id),
        )
    }

    fn record_job_sighting(
        &self,
        print_id: PrintId,
        job_started_at: Timestamp,
        job_print_time_s: Option<i64>,
    ) -> BoxFuture<'_, Result<PrintRecord, StoreError>> {
        self.after(
            "record_job_sighting",
            PrintStore::record_job_sighting(
                &*self.inner,
                print_id,
                job_started_at,
                job_print_time_s,
            ),
        )
    }

    fn print_by_provider_id(
        &self,
        provider_print_id: i64,
    ) -> BoxFuture<'_, Result<Option<PrintRecord>, StoreError>> {
        self.after(
            "print_by_provider_id",
            PrintStore::print_by_provider_id(&*self.inner, provider_print_id),
        )
    }

    fn end_print(
        &self,
        print_id: PrintId,
        state: PrinterState,
        ended_at: Timestamp,
        reason: String,
    ) -> BoxFuture<'_, Result<PrintRecord, StoreError>> {
        self.after(
            "end_print",
            PrintStore::end_print(&*self.inner, print_id, state, ended_at, reason),
        )
    }

    fn record_narrowing(
        &self,
        print_id: PrintId,
        narrowing: ManifestNarrowing,
    ) -> BoxFuture<'_, Result<PrintRecord, StoreError>> {
        self.after(
            "record_narrowing",
            PrintStore::record_narrowing(&*self.inner, print_id, narrowing),
        )
    }

    fn put_manifest(
        &self,
        print_id: PrintId,
        manifest: JobManifest,
    ) -> BoxFuture<'_, Result<(), StoreError>> {
        self.after(
            "put_manifest",
            PrintStore::put_manifest(&*self.inner, print_id, manifest),
        )
    }

    fn manifest(
        &self,
        print_id: PrintId,
    ) -> BoxFuture<'_, Result<Option<JobManifest>, StoreError>> {
        self.after("manifest", PrintStore::manifest(&*self.inner, print_id))
    }
}

impl EventStore for CommitThenFail {
    fn append_event(&self, draft: EventDraft) -> BoxFuture<'_, Result<EventRecord, StoreError>> {
        self.after(
            "append_event",
            EventStore::append_event(&*self.inner, draft),
        )
    }

    fn history(&self, query: HistoryQuery) -> BoxFuture<'_, Result<Vec<EventRecord>, StoreError>> {
        self.after("history", EventStore::history(&*self.inner, query))
    }

    fn audit_page(
        &self,
        print_id: PrintId,
        after: Option<EventId>,
        page_size: u32,
    ) -> BoxFuture<'_, Result<AuditPage, StoreError>> {
        self.after(
            "audit_page",
            EventStore::audit_page(&*self.inner, print_id, after, page_size),
        )
    }

    fn stored_after(
        &self,
        query: StoredQuery,
    ) -> BoxFuture<'_, Result<Vec<EventRecord>, StoreError>> {
        self.after(
            "stored_after",
            EventStore::stored_after(&*self.inner, query),
        )
    }

    fn newest_stored(&self) -> BoxFuture<'_, Result<Option<EventId>, StoreError>> {
        self.after("newest_stored", EventStore::newest_stored(&*self.inner))
    }

    fn on_stored(&self, signal: StoredSignal) {
        EventStore::on_stored(&*self.inner, signal);
    }
}

impl ImageStore for CommitThenFail {
    fn put_image(
        &self,
        print_id: PrintId,
        event_id: EventId,
        source_url: Option<String>,
        content_type: String,
        bytes: RawBytes,
    ) -> BoxFuture<'_, Result<ImageRecord, StoreError>> {
        self.after(
            "put_image",
            ImageStore::put_image(
                &*self.inner,
                print_id,
                event_id,
                source_url,
                content_type,
                bytes,
            ),
        )
    }

    fn image(&self, image_id: ImageId) -> BoxFuture<'_, Result<ImageLookup, StoreError>> {
        self.after("image", ImageStore::image(&*self.inner, image_id))
    }
}

impl ActionStore for CommitThenFail {
    fn record_action(
        &self,
        request: ActionRequest,
        decision: PolicyDecision,
    ) -> BoxFuture<'_, Result<ActionRecord, StoreError>> {
        self.after(
            "record_action",
            ActionStore::record_action(&*self.inner, request, decision),
        )
    }

    fn record_execution(
        &self,
        action_id: ActionId,
        outcome: ExecutionOutcome,
    ) -> BoxFuture<'_, Result<ActionRecord, StoreError>> {
        self.after(
            "record_execution",
            ActionStore::record_execution(&*self.inner, action_id, outcome),
        )
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
        self.after(
            "open_intervention",
            ActionStore::open_intervention(
                &*self.inner,
                action_id,
                adjustable,
                prior_value,
                applied_value,
                applied_at,
                expires_at,
            ),
        )
    }

    fn settle_intervention(
        &self,
        intervention_id: InterventionId,
        outcome: InterventionOutcome,
    ) -> BoxFuture<'_, Result<SettleOutcome, StoreError>> {
        self.after(
            "settle_intervention",
            ActionStore::settle_intervention(&*self.inner, intervention_id, outcome),
        )
    }

    fn due_interventions(
        &self,
        at: Timestamp,
    ) -> BoxFuture<'_, Result<Vec<Intervention>, StoreError>> {
        self.after(
            "due_interventions",
            ActionStore::due_interventions(&*self.inner, at),
        )
    }

    fn active_interventions(
        &self,
        print_id: PrintId,
    ) -> BoxFuture<'_, Result<Vec<Intervention>, StoreError>> {
        self.after(
            "active_interventions",
            ActionStore::active_interventions(&*self.inner, print_id),
        )
    }
}

impl SessionStore for CommitThenFail {
    fn put_session(&self, session: SupervisionSession) -> BoxFuture<'_, Result<(), StoreError>> {
        self.after(
            "put_session",
            SessionStore::put_session(&*self.inner, session),
        )
    }

    fn session(
        &self,
        print_id: PrintId,
    ) -> BoxFuture<'_, Result<Option<SupervisionSession>, StoreError>> {
        self.after("session", SessionStore::session(&*self.inner, print_id))
    }
}

impl FilamentStore for CommitThenFail {
    fn register_spool(
        &self,
        spool: NewSpool,
        source: EventSource,
        actor: Actor,
        reason: String,
    ) -> BoxFuture<'_, Result<(SpoolRecord, EventRecord), StoreError>> {
        self.after(
            "register_spool",
            FilamentStore::register_spool(&*self.inner, spool, source, actor, reason),
        )
    }

    fn apply_spool_changes(
        &self,
        mutations: Vec<SpoolMutation>,
        charges: Vec<Charge>,
        pends: Vec<Pend>,
    ) -> BoxFuture<'_, Result<SpoolWrite, StoreError>> {
        self.after(
            "apply_spool_changes",
            FilamentStore::apply_spool_changes(&*self.inner, mutations, charges, pends),
        )
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
        self.after(
            "settle_print",
            FilamentStore::settle_print(
                &*self.inner,
                print_id,
                outcome,
                ended,
                charges,
                pends,
                marks,
            ),
        )
    }

    fn print_settlement(
        &self,
        print_id: PrintId,
    ) -> BoxFuture<'_, Result<Option<(Timestamp, PrintOutcome)>, StoreError>> {
        self.after(
            "print_settlement",
            FilamentStore::print_settlement(&*self.inner, print_id),
        )
    }

    fn charges(&self, print_id: PrintId) -> BoxFuture<'_, Result<Vec<ChargeRecord>, StoreError>> {
        self.after("charges", FilamentStore::charges(&*self.inner, print_id))
    }

    fn pending_charges(
        &self,
        tool: Option<i64>,
    ) -> BoxFuture<'_, Result<Vec<PendingCharge>, StoreError>> {
        self.after(
            "pending_charges",
            FilamentStore::pending_charges(&*self.inner, tool),
        )
    }

    fn spool(&self, id: SpoolId) -> BoxFuture<'_, Result<Option<SpoolRecord>, StoreError>> {
        self.after("spool", FilamentStore::spool(&*self.inner, id))
    }

    fn spools(&self, include_retired: bool) -> BoxFuture<'_, Result<Vec<SpoolRecord>, StoreError>> {
        self.after(
            "spools",
            FilamentStore::spools(&*self.inner, include_retired),
        )
    }

    fn loaded_spools(&self) -> BoxFuture<'_, Result<Vec<SpoolRecord>, StoreError>> {
        self.after("loaded_spools", FilamentStore::loaded_spools(&*self.inner))
    }

    fn record_file_position(
        &self,
        print_id: PrintId,
        position: FilePosition,
    ) -> BoxFuture<'_, Result<(), StoreError>> {
        self.after(
            "record_file_position",
            FilamentStore::record_file_position(&*self.inner, print_id, position),
        )
    }

    fn file_position(
        &self,
        print_id: PrintId,
    ) -> BoxFuture<'_, Result<Option<FilePosition>, StoreError>> {
        self.after(
            "file_position",
            FilamentStore::file_position(&*self.inner, print_id),
        )
    }

    fn file_tallies(&self) -> BoxFuture<'_, Result<Vec<FileTally>, StoreError>> {
        self.after("file_tallies", FilamentStore::file_tallies(&*self.inner))
    }

    fn account_tallies(
        &self,
        tallies: Vec<FileTally>,
        charges: Vec<Charge>,
        marks: Vec<SpoolMutation>,
        events: Vec<EventDraft>,
    ) -> BoxFuture<'_, Result<Vec<EventRecord>, StoreError>> {
        self.after(
            "account_tallies",
            FilamentStore::account_tallies(&*self.inner, tallies, charges, marks, events),
        )
    }
}

impl TimelineStore for CommitThenFail {
    fn timeline(
        &self,
        query: TimelineQuery,
    ) -> BoxFuture<'_, Result<Vec<EventRecord>, StoreError>> {
        self.after("timeline", TimelineStore::timeline(&*self.inner, query))
    }
}
