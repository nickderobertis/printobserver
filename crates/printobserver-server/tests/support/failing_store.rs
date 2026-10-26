//! A store that has failed, which is a thing that happens to a disk.
//!
//! Every read and write this server makes goes through the store port, and the
//! answer to "the disk is full" is not the same as the answer to "you asked for
//! a print that does not exist" — one is this server's own failure and the
//! other is the caller's mistake. That distinction is what this stands in for a
//! store to drive: it fails everything it is asked, in the store's own words,
//! so a journey can read what this server answers for each.
//!
//! `startup` is what a start needs before there is anything to serve — the open
//! prints, and the printer's tallies the start accounts — so it can be made to
//! succeed: a store that failed at start refuses the start, and a
//! store that fails afterwards is a running server answering failures.

use std::sync::atomic::{AtomicBool, Ordering};

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
use printobserver_types::{
    EventId, EventRecord, EventSource, ImageId, PrintId, RawBytes, Timestamp,
};

/// What the store says when it has failed.
pub const DETAIL: &str = "the disk is full";

/// A store that fails everything it is asked.
#[derive(Debug, Default)]
pub struct FailingStore {
    /// Whether the two reads a start makes answer rather than fail.
    starts: AtomicBool,
}

impl FailingStore {
    /// A store that lets a server start, and fails everything afterwards.
    #[must_use]
    pub fn after_starting() -> Stores {
        Stores::of(std::sync::Arc::new(Self {
            starts: AtomicBool::new(true),
        }))
    }

    /// A store that has already failed when the server tries to start.
    #[must_use]
    pub fn from_the_start() -> Stores {
        Stores::of(std::sync::Arc::new(Self {
            starts: AtomicBool::new(false),
        }))
    }
}

/// The failure this store answers everything with.
fn failed<T>() -> BoxFuture<'static, Result<T, StoreError>>
where
    T: Send + 'static,
{
    Box::pin(async {
        Err(StoreError::Database {
            detail: DETAIL.to_owned(),
        })
    })
}

impl PrintStore for FailingStore {
    fn open_print(
        &self,
        _provider_print_id: Option<i64>,
        _file_name: Option<String>,
    ) -> BoxFuture<'_, Result<PrintRecord, StoreError>> {
        failed()
    }

    fn print(&self, _print_id: PrintId) -> BoxFuture<'_, Result<Option<PrintRecord>, StoreError>> {
        failed()
    }

    fn open_prints(&self) -> BoxFuture<'_, Result<Vec<PrintRecord>, StoreError>> {
        if self.starts.load(Ordering::SeqCst) {
            return Box::pin(async { Ok(Vec::new()) });
        }
        failed()
    }

    fn prints(&self) -> BoxFuture<'_, Result<Vec<PrintRecord>, StoreError>> {
        failed()
    }

    fn attach_obico_print(
        &self,
        _print_id: PrintId,
        _obico_print_id: i64,
    ) -> BoxFuture<'_, Result<PrintRecord, StoreError>> {
        failed()
    }

    fn record_job_sighting(
        &self,
        _print_id: PrintId,
        _job_started_at: Timestamp,
        _job_print_time_s: Option<i64>,
    ) -> BoxFuture<'_, Result<PrintRecord, StoreError>> {
        failed()
    }

    fn print_by_provider_id(
        &self,
        _provider_print_id: i64,
    ) -> BoxFuture<'_, Result<Option<PrintRecord>, StoreError>> {
        failed()
    }

    fn end_print(
        &self,
        _print_id: PrintId,
        _state: PrinterState,
        _ended_at: Timestamp,
        _reason: String,
    ) -> BoxFuture<'_, Result<PrintRecord, StoreError>> {
        failed()
    }

    fn record_narrowing(
        &self,
        _print_id: PrintId,
        _narrowing: ManifestNarrowing,
    ) -> BoxFuture<'_, Result<PrintRecord, StoreError>> {
        failed()
    }

    fn put_manifest(
        &self,
        _print_id: PrintId,
        _manifest: JobManifest,
    ) -> BoxFuture<'_, Result<(), StoreError>> {
        failed()
    }

    fn manifest(
        &self,
        _print_id: PrintId,
    ) -> BoxFuture<'_, Result<Option<JobManifest>, StoreError>> {
        failed()
    }
}

impl EventStore for FailingStore {
    fn append_event(&self, _draft: EventDraft) -> BoxFuture<'_, Result<EventRecord, StoreError>> {
        failed()
    }

    fn history(&self, _query: HistoryQuery) -> BoxFuture<'_, Result<Vec<EventRecord>, StoreError>> {
        failed()
    }

    fn audit_page(
        &self,
        _print_id: PrintId,
        _after: Option<EventId>,
        _page_size: u32,
    ) -> BoxFuture<'_, Result<AuditPage, StoreError>> {
        failed()
    }
}

impl ImageStore for FailingStore {
    fn put_image(
        &self,
        _print_id: PrintId,
        _event_id: EventId,
        _source_url: Option<String>,
        _content_type: String,
        _bytes: RawBytes,
    ) -> BoxFuture<'_, Result<ImageRecord, StoreError>> {
        failed()
    }

    fn image(&self, _image_id: ImageId) -> BoxFuture<'_, Result<ImageLookup, StoreError>> {
        failed()
    }
}

impl ActionStore for FailingStore {
    fn record_action(
        &self,
        _request: ActionRequest,
        _decision: PolicyDecision,
    ) -> BoxFuture<'_, Result<ActionRecord, StoreError>> {
        failed()
    }

    fn record_action_against(
        &self,
        _print_id: PrintId,
        _request: ActionRequest,
        _decision: PolicyDecision,
    ) -> BoxFuture<'_, Result<ActionRecord, StoreError>> {
        failed()
    }

    fn record_execution(
        &self,
        _action_id: ActionId,
        _outcome: ExecutionOutcome,
    ) -> BoxFuture<'_, Result<ActionRecord, StoreError>> {
        failed()
    }

    fn open_intervention(
        &self,
        _action_id: ActionId,
        _adjustable: Adjustable,
        _prior_value: Option<f64>,
        _applied_value: f64,
        _applied_at: Timestamp,
        _expires_at: Timestamp,
    ) -> BoxFuture<'_, Result<Intervention, StoreError>> {
        failed()
    }

    fn settle_intervention(
        &self,
        _intervention_id: InterventionId,
        _outcome: InterventionOutcome,
    ) -> BoxFuture<'_, Result<SettleOutcome, StoreError>> {
        failed()
    }

    fn due_interventions(
        &self,
        _at: Timestamp,
    ) -> BoxFuture<'_, Result<Vec<Intervention>, StoreError>> {
        if self.starts.load(Ordering::SeqCst) {
            return Box::pin(async { Ok(Vec::new()) });
        }
        failed()
    }

    fn active_interventions(
        &self,
        _print_id: PrintId,
    ) -> BoxFuture<'_, Result<Vec<Intervention>, StoreError>> {
        failed()
    }
}

impl SessionStore for FailingStore {
    fn put_session(&self, _session: SupervisionSession) -> BoxFuture<'_, Result<(), StoreError>> {
        failed()
    }

    fn session(
        &self,
        _print_id: PrintId,
    ) -> BoxFuture<'_, Result<Option<SupervisionSession>, StoreError>> {
        failed()
    }
}

impl FilamentStore for FailingStore {
    fn register_spool(
        &self,
        _spool: NewSpool,
        _source: EventSource,
        _actor: Actor,
        _reason: String,
    ) -> BoxFuture<'_, Result<(SpoolRecord, EventRecord), StoreError>> {
        failed()
    }

    fn apply_spool_changes(
        &self,
        _mutations: Vec<SpoolMutation>,
        _charges: Vec<Charge>,
        _pends: Vec<Pend>,
    ) -> BoxFuture<'_, Result<SpoolWrite, StoreError>> {
        failed()
    }

    fn settle_print(
        &self,
        _print_id: PrintId,
        _outcome: PrintOutcome,
        _ended: EventDraft,
        _charges: Vec<Charge>,
        _pends: Vec<Pend>,
        _marks: Vec<SpoolMutation>,
    ) -> BoxFuture<'_, Result<Settlement, StoreError>> {
        failed()
    }

    fn print_settlement(
        &self,
        _print_id: PrintId,
    ) -> BoxFuture<'_, Result<Option<(Timestamp, PrintOutcome)>, StoreError>> {
        failed()
    }

    fn charges(&self, _print_id: PrintId) -> BoxFuture<'_, Result<Vec<ChargeRecord>, StoreError>> {
        failed()
    }

    fn pending_charges(
        &self,
        _tool: Option<i64>,
    ) -> BoxFuture<'_, Result<Vec<PendingCharge>, StoreError>> {
        failed()
    }

    fn spool(&self, _id: SpoolId) -> BoxFuture<'_, Result<Option<SpoolRecord>, StoreError>> {
        failed()
    }

    fn spools(
        &self,
        _include_retired: bool,
    ) -> BoxFuture<'_, Result<Vec<SpoolRecord>, StoreError>> {
        failed()
    }

    fn loaded_spools(&self) -> BoxFuture<'_, Result<Vec<SpoolRecord>, StoreError>> {
        failed()
    }

    fn record_file_position(
        &self,
        _print_id: PrintId,
        _position: FilePosition,
    ) -> BoxFuture<'_, Result<(), StoreError>> {
        failed()
    }

    fn file_position(
        &self,
        _print_id: PrintId,
    ) -> BoxFuture<'_, Result<Option<FilePosition>, StoreError>> {
        failed()
    }

    fn file_tallies(&self) -> BoxFuture<'_, Result<Vec<FileTally>, StoreError>> {
        if self.starts.load(Ordering::SeqCst) {
            return Box::pin(async { Ok(Vec::new()) });
        }
        failed()
    }

    fn account_tallies(
        &self,
        tallies: Vec<FileTally>,
        charges: Vec<Charge>,
        marks: Vec<SpoolMutation>,
        events: Vec<EventDraft>,
    ) -> BoxFuture<'_, Result<Vec<EventRecord>, StoreError>> {
        // A start over a printer holding no tally accounts nothing, and that
        // is all a start that is let through asks.
        let nothing = tallies.is_empty() && charges.is_empty() && marks.is_empty();
        if self.starts.load(Ordering::SeqCst) && nothing && events.is_empty() {
            return Box::pin(async { Ok(Vec::new()) });
        }
        failed()
    }
}

impl TimelineStore for FailingStore {
    fn timeline(
        &self,
        _query: TimelineQuery,
    ) -> BoxFuture<'_, Result<Vec<EventRecord>, StoreError>> {
        failed()
    }
}
