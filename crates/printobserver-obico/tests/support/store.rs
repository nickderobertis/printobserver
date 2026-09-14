//! A store these journeys read back through.
//!
//! It is a real implementation of the three store traits the ingress names
//! rather than a stand-in for one: the ingress writes through the traits it
//! will hold in the server, and every assertion below reads what this store
//! *holds* afterwards through the traits' own reads. It keeps its records in
//! memory and its image bytes in a directory of its own, so that an image's
//! record and its file are two facts here as they are in the real one.
//!
//! It implements the print, event and image stores and no other, because the
//! ingress holds no other: what the supervision domain persists beyond those —
//! actions, interventions, sessions — is nothing an adapter can reach. The
//! methods of those three no journey here reaches answer
//! [`StoreError::NotFound`] naming themselves, rather than panicking: a store
//! double that aborts the process is one whose failure reads as a crash
//! instead of as a missing behaviour.

use std::path::PathBuf;
use std::sync::Mutex;

use printobserver_core::records::PrinterState;
use printobserver_core::store::{
    AuditPage, BoxFuture, EventDraft, EventStore, HistoryQuery, ImageLookup, ImageStore,
    PrintStore, StoreError,
};
use printobserver_core::{ImageRecord, JobManifest, ManifestNarrowing, PrintRecord};
use printobserver_types::{EventId, EventRecord, ImageId, PrintId, RawBytes, Timestamp};
use sha2::{Digest as _, Sha256};

/// Everything this store holds.
#[derive(Debug, Default)]
struct Held {
    /// Every print, in the order it was opened.
    prints: Vec<PrintRecord>,
    /// Every event, in the order it was appended.
    events: Vec<EventRecord>,
    /// Every image record.
    images: Vec<ImageRecord>,
}

/// A store that holds its records in memory and its images in a directory.
#[derive(Debug)]
pub struct MemoryStore {
    /// The records.
    held: Mutex<Held>,
    /// The state directory image paths are relative to.
    state_dir: tempfile::TempDir,
}

/// Answer a method these journeys do not reach.
fn unsupported<T: Send + 'static>(method: &str) -> BoxFuture<'static, Result<T, StoreError>> {
    let what = format!("store double that answers `{method}`");
    Box::pin(async move { Err(StoreError::NotFound { what }) })
}

impl MemoryStore {
    /// An empty store.
    pub fn new() -> Self {
        Self {
            held: Mutex::new(Held::default()),
            state_dir: tempfile::tempdir().expect("a temporary state directory"),
        }
    }

    /// Every print this store holds, in the order they were opened.
    pub fn prints(&self) -> Vec<PrintRecord> {
        self.held
            .lock()
            .expect("the store is not poisoned")
            .prints
            .clone()
    }

    /// Every event this store holds, in the order they were appended.
    ///
    /// The port's own history read is keyed by a print, and an event that names
    /// no print — which is what a body this system could not read becomes — is
    /// reachable only this way.
    pub fn events(&self) -> Vec<EventRecord> {
        self.held
            .lock()
            .expect("the store is not poisoned")
            .events
            .clone()
    }
}

impl PrintStore for MemoryStore {
    fn open_print(
        &self,
        provider_print_id: Option<i64>,
        file_name: Option<String>,
    ) -> BoxFuture<'_, Result<PrintRecord, StoreError>> {
        Box::pin(async move {
            let record = PrintRecord {
                id: PrintId::new(),
                provider_print_id,
                file_name,
                state: PrinterState::Printing,
                opened_at: Timestamp::now(),
                ended_at: None,
                end_reason: None,
                narrowings: Vec::new(),
            };
            let mut held = self.held.lock().expect("the store is not poisoned");
            held.prints.push(record.clone());
            Ok(record)
        })
    }

    fn print(&self, print_id: PrintId) -> BoxFuture<'_, Result<Option<PrintRecord>, StoreError>> {
        Box::pin(async move {
            let held = self.held.lock().expect("the store is not poisoned");
            Ok(held
                .prints
                .iter()
                .find(|record| record.id == print_id)
                .cloned())
        })
    }

    fn print_by_provider_id(
        &self,
        provider_print_id: i64,
    ) -> BoxFuture<'_, Result<Option<PrintRecord>, StoreError>> {
        Box::pin(async move {
            let held = self.held.lock().expect("the store is not poisoned");
            Ok(held
                .prints
                .iter()
                .find(|record| record.provider_print_id == Some(provider_print_id))
                .cloned())
        })
    }

    fn open_prints(&self) -> BoxFuture<'_, Result<Vec<PrintRecord>, StoreError>> {
        Box::pin(async move {
            let held = self.held.lock().expect("the store is not poisoned");
            Ok(held
                .prints
                .iter()
                .filter(|record| record.ended_at.is_none())
                .rev()
                .cloned()
                .collect())
        })
    }

    fn prints(&self) -> BoxFuture<'_, Result<Vec<PrintRecord>, StoreError>> {
        unsupported("prints")
    }

    fn attach_obico_print(
        &self,
        _print_id: PrintId,
        _obico_print_id: i64,
    ) -> BoxFuture<'_, Result<PrintRecord, StoreError>> {
        unsupported("attach_obico_print")
    }

    fn end_print(
        &self,
        print_id: PrintId,
        state: PrinterState,
        ended_at: Timestamp,
        reason: String,
    ) -> BoxFuture<'_, Result<PrintRecord, StoreError>> {
        Box::pin(async move {
            let mut held = self.held.lock().expect("the store is not poisoned");
            let record = held
                .prints
                .iter_mut()
                .find(|record| record.id == print_id)
                .ok_or_else(|| StoreError::NotFound {
                    what: format!("print {print_id}"),
                })?;
            record.state = state;
            record.ended_at = Some(ended_at);
            record.end_reason = Some(reason);
            Ok(record.clone())
        })
    }

    fn record_narrowing(
        &self,
        _print_id: PrintId,
        _narrowing: ManifestNarrowing,
    ) -> BoxFuture<'_, Result<PrintRecord, StoreError>> {
        unsupported("record_narrowing")
    }

    fn put_manifest(
        &self,
        _print_id: PrintId,
        _manifest: JobManifest,
    ) -> BoxFuture<'_, Result<(), StoreError>> {
        unsupported("put_manifest")
    }

    fn manifest(
        &self,
        _print_id: PrintId,
    ) -> BoxFuture<'_, Result<Option<JobManifest>, StoreError>> {
        unsupported("manifest")
    }
}

impl EventStore for MemoryStore {
    fn append_event(&self, draft: EventDraft) -> BoxFuture<'_, Result<EventRecord, StoreError>> {
        Box::pin(async move {
            let record = EventRecord {
                id: EventId::new(),
                print_id: draft.print_id,
                source: draft.source,
                received_at: draft.received_at,
                image: None,
                body: draft.body,
                raw: draft.raw,
            };
            let mut held = self.held.lock().expect("the store is not poisoned");
            held.events.push(record.clone());
            Ok(record)
        })
    }

    fn history(&self, query: HistoryQuery) -> BoxFuture<'_, Result<Vec<EventRecord>, StoreError>> {
        Box::pin(async move {
            let limit = usize::try_from(query.resolved_limit()?).unwrap_or(usize::MAX);
            let held = self.held.lock().expect("the store is not poisoned");
            Ok(held
                .events
                .iter()
                .rev()
                .filter(|record| record.print_id == Some(query.print_id))
                .filter(|record| query.kinds.is_empty() || query.kinds.contains(record.kind()))
                .take(limit)
                .cloned()
                .collect())
        })
    }

    fn audit_page(
        &self,
        _print_id: PrintId,
        _after: Option<EventId>,
        _page_size: u32,
    ) -> BoxFuture<'_, Result<AuditPage, StoreError>> {
        unsupported("audit_page")
    }
}

impl ImageStore for MemoryStore {
    fn put_image(
        &self,
        print_id: PrintId,
        event_id: EventId,
        source_url: Option<String>,
        content_type: String,
        bytes: RawBytes,
    ) -> BoxFuture<'_, Result<ImageRecord, StoreError>> {
        Box::pin(async move {
            let id = ImageId::new();
            let relative_path = format!("images/{id}.bin");
            let path = self.state_dir.path().join(&relative_path);
            std::fs::create_dir_all(path.parent().expect("the path has a parent")).map_err(
                |error| StoreError::Io {
                    detail: error.to_string(),
                },
            )?;
            std::fs::write(&path, bytes.as_slice()).map_err(|error| StoreError::Io {
                detail: error.to_string(),
            })?;
            let byte_len = i64::try_from(bytes.as_slice().len()).map_err(|error| {
                StoreError::ConstraintRefused {
                    constraint: error.to_string(),
                }
            })?;
            let record = ImageRecord {
                id,
                print_id,
                event_id,
                source_url,
                fetched_at: Timestamp::now(),
                content_type,
                byte_len,
                sha256: format!("{:x}", Sha256::digest(bytes.as_slice())),
                relative_path,
            };
            let mut held = self.held.lock().expect("the store is not poisoned");
            held.images.push(record.clone());
            Ok(record)
        })
    }

    fn image(&self, image_id: ImageId) -> BoxFuture<'_, Result<ImageLookup, StoreError>> {
        Box::pin(async move {
            let held = self.held.lock().expect("the store is not poisoned");
            let record = held
                .images
                .iter()
                .find(|record| record.id == image_id)
                .cloned()
                .ok_or_else(|| StoreError::NotFound {
                    what: format!("image {image_id}"),
                })?;
            let path: PathBuf = self.state_dir.path().join(&record.relative_path);
            if path.is_file() {
                Ok(ImageLookup::Found { record, path })
            } else {
                Ok(ImageLookup::FileMissing { record })
            }
        })
    }
}

/// A store that refuses every write, and answers no read.
///
/// It is here so a journey can drive the one path a working store never takes:
/// the ingress being unable to write anything down at all.
#[derive(Debug, Default)]
pub struct RefusingStore;

/// What this store answers to everything.
fn refused<T: Send + 'static>() -> BoxFuture<'static, Result<T, StoreError>> {
    Box::pin(async {
        Err(StoreError::Database {
            detail: "the database is not available".to_owned(),
        })
    })
}

impl PrintStore for RefusingStore {
    fn open_print(
        &self,
        _provider_print_id: Option<i64>,
        _file_name: Option<String>,
    ) -> BoxFuture<'_, Result<PrintRecord, StoreError>> {
        refused()
    }

    fn print(&self, _print_id: PrintId) -> BoxFuture<'_, Result<Option<PrintRecord>, StoreError>> {
        refused()
    }

    fn print_by_provider_id(
        &self,
        _provider_print_id: i64,
    ) -> BoxFuture<'_, Result<Option<PrintRecord>, StoreError>> {
        refused()
    }

    fn open_prints(&self) -> BoxFuture<'_, Result<Vec<PrintRecord>, StoreError>> {
        refused()
    }

    fn prints(&self) -> BoxFuture<'_, Result<Vec<PrintRecord>, StoreError>> {
        refused()
    }

    fn attach_obico_print(
        &self,
        _print_id: PrintId,
        _obico_print_id: i64,
    ) -> BoxFuture<'_, Result<PrintRecord, StoreError>> {
        refused()
    }

    fn end_print(
        &self,
        _print_id: PrintId,
        _state: PrinterState,
        _ended_at: Timestamp,
        _reason: String,
    ) -> BoxFuture<'_, Result<PrintRecord, StoreError>> {
        refused()
    }

    fn record_narrowing(
        &self,
        _print_id: PrintId,
        _narrowing: ManifestNarrowing,
    ) -> BoxFuture<'_, Result<PrintRecord, StoreError>> {
        refused()
    }

    fn put_manifest(
        &self,
        _print_id: PrintId,
        _manifest: JobManifest,
    ) -> BoxFuture<'_, Result<(), StoreError>> {
        refused()
    }

    fn manifest(
        &self,
        _print_id: PrintId,
    ) -> BoxFuture<'_, Result<Option<JobManifest>, StoreError>> {
        refused()
    }
}

impl EventStore for RefusingStore {
    fn append_event(&self, _draft: EventDraft) -> BoxFuture<'_, Result<EventRecord, StoreError>> {
        refused()
    }

    fn history(&self, _query: HistoryQuery) -> BoxFuture<'_, Result<Vec<EventRecord>, StoreError>> {
        refused()
    }

    fn audit_page(
        &self,
        _print_id: PrintId,
        _after: Option<EventId>,
        _page_size: u32,
    ) -> BoxFuture<'_, Result<AuditPage, StoreError>> {
        refused()
    }
}

impl ImageStore for RefusingStore {
    fn put_image(
        &self,
        _print_id: PrintId,
        _event_id: EventId,
        _source_url: Option<String>,
        _content_type: String,
        _bytes: RawBytes,
    ) -> BoxFuture<'_, Result<ImageRecord, StoreError>> {
        refused()
    }

    fn image(&self, _image_id: ImageId) -> BoxFuture<'_, Result<ImageLookup, StoreError>> {
        refused()
    }
}
