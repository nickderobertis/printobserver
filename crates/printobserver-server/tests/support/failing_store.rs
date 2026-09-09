//! A store that has failed, which is a thing that happens to a disk.
//!
//! Every read and write this server makes goes through the store port, and the
//! answer to "the disk is full" is not the same as the answer to "you asked for
//! a print that does not exist" — one is this server's own failure and the
//! other is the caller's mistake. That distinction is what this stands in for a
//! store to drive: it fails everything it is asked, in the store's own words,
//! so a journey can read what this server answers for each.
//!
//! `startup` is what a start needs before there is anything to serve, so it can
//! be made to succeed: a store that failed at start refuses the start, and a
//! store that fails afterwards is a running server answering failures.

use std::sync::atomic::{AtomicBool, Ordering};

use printobserver_store_api::{
    AuditPage, BoxFuture, EventDraft, HistoryQuery, ImageLookup, SettleOutcome, StoreError,
    StorePort,
};
use printobserver_types::{
    ActionId, ActionRecord, ActionRequest, Adjustable, EventId, EventRecord, ExecutionOutcome,
    ImageId, ImageRecord, Intervention, InterventionId, InterventionOutcome, JobManifest,
    ManifestNarrowing, PolicyDecision, PrintId, PrintRecord, PrinterState, RawBytes,
    SupervisionSession, Timestamp,
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
    pub fn after_starting() -> std::sync::Arc<Self> {
        std::sync::Arc::new(Self {
            starts: AtomicBool::new(true),
        })
    }

    /// A store that has already failed when the server tries to start.
    #[must_use]
    pub fn from_the_start() -> std::sync::Arc<Self> {
        std::sync::Arc::new(Self {
            starts: AtomicBool::new(false),
        })
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

impl StorePort for FailingStore {
    fn open_print(
        &self,
        _obico_print_id: Option<i64>,
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

    fn print_by_obico_id(
        &self,
        _obico_print_id: i64,
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

    fn append_event(&self, _draft: EventDraft) -> BoxFuture<'_, Result<EventRecord, StoreError>> {
        failed()
    }

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

    fn record_action(
        &self,
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
