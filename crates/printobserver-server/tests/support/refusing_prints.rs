//! A print store whose read of the open prints can be made to fail, and then
//! to recover, under a running server.
//!
//! Everything else is the durable store's own answer: this stands between the
//! server and that store, passing every call through, so that a journey can
//! fail the one read a periodic observation round begins with and watch the
//! running server carry on once the store answers again.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use printobserver_core::store::{BoxFuture, PrintStore, StoreError};
use printobserver_core::{JobManifest, ManifestNarrowing, PrintRecord};
use printobserver_printer_api::PrinterState;
use printobserver_types::{PrintId, Timestamp};

/// What the store says while its read of the open prints is failing.
pub const DETAIL: &str = "the disk is full";

/// A print store passing every call to another, but for the open prints'
/// read while it is refusing.
pub struct RefusingPrints {
    /// The store every call is passed to.
    inner: Arc<dyn PrintStore>,
    /// Whether the read of the open prints fails.
    refusing: AtomicBool,
}

impl RefusingPrints {
    /// Pass every call to `inner`, refusing nothing yet.
    #[must_use]
    pub fn over(inner: Arc<dyn PrintStore>) -> Arc<Self> {
        Arc::new(Self {
            inner,
            refusing: AtomicBool::new(false),
        })
    }

    /// Fail the read of the open prints from now on, or stop failing it.
    pub fn refusing(&self, refusing: bool) {
        self.refusing.store(refusing, Ordering::SeqCst);
    }
}

impl PrintStore for RefusingPrints {
    fn open_print(
        &self,
        provider_print_id: Option<i64>,
        file_name: Option<String>,
    ) -> BoxFuture<'_, Result<PrintRecord, StoreError>> {
        self.inner.open_print(provider_print_id, file_name)
    }

    fn print(&self, print_id: PrintId) -> BoxFuture<'_, Result<Option<PrintRecord>, StoreError>> {
        self.inner.print(print_id)
    }

    fn open_prints(&self) -> BoxFuture<'_, Result<Vec<PrintRecord>, StoreError>> {
        if self.refusing.load(Ordering::SeqCst) {
            return Box::pin(async {
                Err(StoreError::Io {
                    detail: DETAIL.to_owned(),
                })
            });
        }
        self.inner.open_prints()
    }

    fn prints(&self) -> BoxFuture<'_, Result<Vec<PrintRecord>, StoreError>> {
        self.inner.prints()
    }

    fn attach_obico_print(
        &self,
        print_id: PrintId,
        obico_print_id: i64,
    ) -> BoxFuture<'_, Result<PrintRecord, StoreError>> {
        self.inner.attach_obico_print(print_id, obico_print_id)
    }

    fn record_job_sighting(
        &self,
        print_id: PrintId,
        job_started_at: Timestamp,
        job_print_time_s: Option<i64>,
    ) -> BoxFuture<'_, Result<PrintRecord, StoreError>> {
        self.inner
            .record_job_sighting(print_id, job_started_at, job_print_time_s)
    }

    fn print_by_provider_id(
        &self,
        provider_print_id: i64,
    ) -> BoxFuture<'_, Result<Option<PrintRecord>, StoreError>> {
        self.inner.print_by_provider_id(provider_print_id)
    }

    fn end_print(
        &self,
        print_id: PrintId,
        state: PrinterState,
        ended_at: Timestamp,
        reason: String,
    ) -> BoxFuture<'_, Result<PrintRecord, StoreError>> {
        self.inner.end_print(print_id, state, ended_at, reason)
    }

    fn record_narrowing(
        &self,
        print_id: PrintId,
        narrowing: ManifestNarrowing,
    ) -> BoxFuture<'_, Result<PrintRecord, StoreError>> {
        self.inner.record_narrowing(print_id, narrowing)
    }

    fn put_manifest(
        &self,
        print_id: PrintId,
        manifest: JobManifest,
    ) -> BoxFuture<'_, Result<(), StoreError>> {
        self.inner.put_manifest(print_id, manifest)
    }

    fn manifest(
        &self,
        print_id: PrintId,
    ) -> BoxFuture<'_, Result<Option<JobManifest>, StoreError>> {
        self.inner.manifest(print_id)
    }
}
