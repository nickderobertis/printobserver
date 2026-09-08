//! The same port, held in process memory, for a tier that needs no durability.
//!
//! Not a mock: it answers the port's questions the way the durable store does,
//! and the one conformance suite in `tests/` drives every journey against both
//! of them so that it cannot drift into answering differently. Image bytes are
//! on the filesystem here too, because the port hands a caller the path its
//! bytes are at and an implementation holding them in memory would have to
//! answer a path nothing is at.

use std::path::{Path, PathBuf};
use std::sync::{Mutex, MutexGuard, PoisonError};

use printobserver_store_api::{
    AuditPage, BoxFuture, EventDraft, HistoryQuery, ImageLookup, SettleOutcome, StoreError,
    StorePort, resolve_history_limit,
};
use printobserver_types::{
    ActionId, ActionRecord, ActionRequest, Adjustable, EventId, EventRecord, ExecutionOutcome,
    ImageId, ImageRecord, ImageRef, Intervention, InterventionId, InterventionOutcome, JobManifest,
    ManifestNarrowing, PolicyDecision, PrintId, PrintRecord, PrinterState, RawBytes,
    SupervisionSession, Timestamp,
};

use crate::hold::{HoldPoints, settle_label};
use crate::images::{resolve, store_bytes};
use crate::values::{not_found, refused};

/// Every record this store holds, in the order each was written.
#[derive(Debug, Default)]
struct Records {
    /// The prints.
    prints: Vec<PrintRecord>,
    /// The events, without the image each may have arrived with.
    events: Vec<EventRecord>,
    /// The images.
    images: Vec<ImageRecord>,
    /// The actions, with whatever the printer made of each.
    actions: Vec<ActionRecord>,
    /// The bounded interventions.
    interventions: Vec<Intervention>,
    /// One manifest per print.
    manifests: Vec<(PrintId, JobManifest)>,
    /// One supervision session per print.
    sessions: Vec<SupervisionSession>,
}

/// The in-memory implementation of the store port.
#[derive(Debug)]
pub struct MemoryStore {
    /// Where image bytes are written, and what stored paths are relative to.
    state_dir: PathBuf,
    /// The records.
    records: Mutex<Records>,
    /// The two places this store's own tests hold a call at.
    points: HoldPoints,
}

/// The lock, taken whether or not a panicking test poisoned it.
fn lock(records: &Mutex<Records>) -> MutexGuard<'_, Records> {
    records.lock().unwrap_or_else(PoisonError::into_inner)
}

impl MemoryStore {
    /// A store holding nothing, writing image bytes under a state directory.
    ///
    /// # Errors
    ///
    /// Returns [`StoreError::Io`] when the state directory cannot be created.
    pub fn new(state_dir: impl AsRef<Path>) -> Result<Self, StoreError> {
        let state_dir = state_dir.as_ref().to_path_buf();
        std::fs::create_dir_all(&state_dir)
            .map_err(|error| crate::values::io_error(&state_dir, &error))?;
        Ok(Self {
            state_dir,
            records: Mutex::new(Records::default()),
            points: HoldPoints::new(),
        })
    }

    /// The state directory every stored path is relative to.
    #[must_use]
    pub fn state_dir(&self) -> &Path {
        &self.state_dir
    }

    /// The places this store holds a call at, for a test that arms one.
    #[must_use]
    pub const fn hold_points(&self) -> &HoldPoints {
        &self.points
    }

    /// One event, with the image it arrived with read off the images.
    fn with_image(records: &Records, event: &EventRecord) -> EventRecord {
        let mut found: Vec<&ImageRecord> = records
            .images
            .iter()
            .filter(|image| image.event_id == event.id)
            .collect();
        found.sort_by_key(|image| (image.fetched_at, image.id));
        EventRecord {
            image: found.first().map(|image| ImageRef {
                id: image.id,
                sha256: image.sha256.clone(),
            }),
            ..event.clone()
        }
    }

    /// The events of one print, oldest first.
    fn events_of(records: &Records, print_id: PrintId) -> Vec<EventRecord> {
        let mut found: Vec<EventRecord> = records
            .events
            .iter()
            .filter(|event| event.print_id == Some(print_id))
            .map(|event| Self::with_image(records, event))
            .collect();
        found.sort_by_key(|event| (event.received_at, event.id));
        found
    }

    /// Open a print, minting its identifier.
    fn insert_print(&self, obico_print_id: Option<i64>, file_name: Option<String>) -> PrintRecord {
        let record = PrintRecord {
            id: PrintId::new(),
            obico_print_id,
            file_name,
            state: PrinterState::Printing,
            opened_at: Timestamp::now(),
            ended_at: None,
            end_reason: None,
            narrowings: Vec::new(),
        };
        lock(&self.records).prints.push(record.clone());
        record
    }

    /// Change one print, or report that there is no such print.
    fn amend_print(
        &self,
        print_id: PrintId,
        amend: impl FnOnce(&mut PrintRecord),
    ) -> Result<PrintRecord, StoreError> {
        let mut records = lock(&self.records);
        let found = records
            .prints
            .iter_mut()
            .find(|print| print.id == print_id)
            .ok_or_else(|| not_found(&format!("print {print_id}")))?;
        amend(found);
        Ok(found.clone())
    }

    /// Record one request together with the decision taken on it.
    fn insert_action(
        &self,
        request: ActionRequest,
        decision: PolicyDecision,
    ) -> Result<ActionRecord, StoreError> {
        let mut records = lock(&self.records);
        let mut open: Vec<&PrintRecord> = records
            .prints
            .iter()
            .filter(|print| print.ended_at.is_none())
            .collect();
        open.sort_by_key(|print| (print.opened_at, print.id));
        let print_id = open
            .last()
            .map(|print| print.id)
            .ok_or_else(|| not_found("open print to record this action against"))?;
        let record = ActionRecord {
            id: ActionId::new(),
            print_id,
            request,
            decision,
            executed_at: None,
            outcome: None,
        };
        records.actions.push(record.clone());
        Ok(record)
    }

    /// Record what the printer made of an action that reached it.
    fn insert_execution(
        &self,
        action_id: ActionId,
        outcome: ExecutionOutcome,
    ) -> Result<ActionRecord, StoreError> {
        let mut records = lock(&self.records);
        let found = records
            .actions
            .iter_mut()
            .find(|action| action.id == action_id)
            .ok_or_else(|| refused("executions.action_id"))?;
        if found.outcome.is_some() {
            return Err(refused("executions.action_id"));
        }
        found.executed_at = Some(Timestamp::now());
        found.outcome = Some(outcome);
        Ok(found.clone())
    }

    /// Open a bounded intervention against the print its action belongs to.
    fn insert_intervention(
        &self,
        action_id: ActionId,
        adjustable: Adjustable,
        prior_value: Option<f64>,
        applied_value: f64,
        applied_at: Timestamp,
        expires_at: Timestamp,
    ) -> Result<Intervention, StoreError> {
        let mut records = lock(&self.records);
        let print_id = records
            .actions
            .iter()
            .find(|action| action.id == action_id)
            .map(|action| action.print_id)
            .ok_or_else(|| refused("interventions.action_id"))?;
        let intervention = Intervention {
            id: InterventionId::new(),
            print_id,
            action_id,
            adjustable,
            prior_value,
            applied_value,
            applied_at,
            expires_at,
            restored_at: None,
            outcome: InterventionOutcome::StillActive,
        };
        records.interventions.push(intervention.clone());
        Ok(intervention)
    }

    /// Settle one intervention, or report the outcome that already won.
    ///
    /// The outcome is read, and then written under one lock conditional on that
    /// same outcome, so an expiry and a supersession contending here cannot both
    /// take effect.
    fn settle(
        &self,
        intervention_id: InterventionId,
        outcome: InterventionOutcome,
    ) -> Result<SettleOutcome, StoreError> {
        let current = lock(&self.records)
            .interventions
            .iter()
            .find(|intervention| intervention.id == intervention_id)
            .map(|intervention| intervention.outcome.clone())
            .ok_or_else(|| not_found(&format!("intervention {intervention_id}")))?;
        self.points.settle().reach(settle_label(&outcome));
        if current != InterventionOutcome::StillActive {
            return Ok(SettleOutcome::AlreadySettled { outcome: current });
        }

        let mut records = lock(&self.records);
        let found = records
            .interventions
            .iter_mut()
            .find(|intervention| intervention.id == intervention_id)
            .ok_or_else(|| not_found(&format!("intervention {intervention_id}")))?;
        if found.outcome != InterventionOutcome::StillActive {
            return Ok(SettleOutcome::AlreadySettled {
                outcome: found.outcome.clone(),
            });
        }
        if outcome == InterventionOutcome::Restored {
            found.restored_at = Some(Timestamp::now());
        }
        found.outcome = outcome;
        Ok(SettleOutcome::Settled {
            intervention: found.clone(),
        })
    }
}

impl StorePort for MemoryStore {
    fn open_print(
        &self,
        obico_print_id: Option<i64>,
        file_name: Option<String>,
    ) -> BoxFuture<'_, Result<PrintRecord, StoreError>> {
        Box::pin(async move { Ok(self.insert_print(obico_print_id, file_name)) })
    }

    fn print(&self, print_id: PrintId) -> BoxFuture<'_, Result<Option<PrintRecord>, StoreError>> {
        Box::pin(async move {
            Ok(lock(&self.records)
                .prints
                .iter()
                .find(|print| print.id == print_id)
                .cloned())
        })
    }

    fn print_by_obico_id(
        &self,
        obico_print_id: i64,
    ) -> BoxFuture<'_, Result<Option<PrintRecord>, StoreError>> {
        Box::pin(async move {
            let records = lock(&self.records);
            let mut found: Vec<&PrintRecord> = records
                .prints
                .iter()
                .filter(|print| print.obico_print_id == Some(obico_print_id))
                .collect();
            found.sort_by_key(|print| (print.opened_at, print.id));
            Ok(found.last().map(|print| (*print).clone()))
        })
    }

    fn end_print(
        &self,
        print_id: PrintId,
        state: PrinterState,
        ended_at: Timestamp,
        reason: String,
    ) -> BoxFuture<'_, Result<PrintRecord, StoreError>> {
        Box::pin(async move {
            self.amend_print(print_id, |print| {
                print.state = state;
                print.ended_at = Some(ended_at);
                print.end_reason = Some(reason);
            })
        })
    }

    fn record_narrowing(
        &self,
        print_id: PrintId,
        narrowing: ManifestNarrowing,
    ) -> BoxFuture<'_, Result<PrintRecord, StoreError>> {
        Box::pin(
            async move { self.amend_print(print_id, |print| print.narrowings.push(narrowing)) },
        )
    }

    fn append_event(&self, draft: EventDraft) -> BoxFuture<'_, Result<EventRecord, StoreError>> {
        Box::pin(async move {
            let mut records = lock(&self.records);
            if let Some(print_id) = draft.print_id
                && !records.prints.iter().any(|print| print.id == print_id)
            {
                return Err(refused("events.print_id"));
            }
            let record = EventRecord {
                id: EventId::new(),
                print_id: draft.print_id,
                source: draft.source,
                received_at: draft.received_at,
                image: None,
                payload: draft.payload,
                raw: draft.raw,
            };
            records.events.push(record.clone());
            Ok(record)
        })
    }

    fn put_image(
        &self,
        print_id: PrintId,
        event_id: EventId,
        source_url: Option<String>,
        content_type: String,
        bytes: RawBytes,
    ) -> BoxFuture<'_, Result<ImageRecord, StoreError>> {
        Box::pin(async move {
            let stored = store_bytes(&self.state_dir, bytes.as_slice(), &self.points)?;
            let mut records = lock(&self.records);
            if !records.prints.iter().any(|print| print.id == print_id) {
                return Err(refused("images.print_id"));
            }
            if !records.events.iter().any(|event| event.id == event_id) {
                return Err(refused("images.event_id"));
            }
            let record = ImageRecord {
                id: ImageId::new(),
                print_id,
                event_id,
                source_url,
                fetched_at: Timestamp::now(),
                content_type,
                byte_len: stored.byte_len,
                sha256: stored.sha256,
                relative_path: stored.relative_path,
            };
            records.images.push(record.clone());
            Ok(record)
        })
    }

    fn image(&self, image_id: ImageId) -> BoxFuture<'_, Result<ImageLookup, StoreError>> {
        Box::pin(async move {
            let record = lock(&self.records)
                .images
                .iter()
                .find(|image| image.id == image_id)
                .cloned()
                .ok_or_else(|| not_found(&format!("image {image_id}")))?;
            let path = resolve(&self.state_dir, &record.relative_path);
            if path.is_file() {
                Ok(ImageLookup::Found { record, path })
            } else {
                Ok(ImageLookup::FileMissing { record })
            }
        })
    }

    fn record_action(
        &self,
        request: ActionRequest,
        decision: PolicyDecision,
    ) -> BoxFuture<'_, Result<ActionRecord, StoreError>> {
        Box::pin(async move { self.insert_action(request, decision) })
    }

    fn record_execution(
        &self,
        action_id: ActionId,
        outcome: ExecutionOutcome,
    ) -> BoxFuture<'_, Result<ActionRecord, StoreError>> {
        Box::pin(async move { self.insert_execution(action_id, outcome) })
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
        Box::pin(async move {
            self.insert_intervention(
                action_id,
                adjustable,
                prior_value,
                applied_value,
                applied_at,
                expires_at,
            )
        })
    }

    fn settle_intervention(
        &self,
        intervention_id: InterventionId,
        outcome: InterventionOutcome,
    ) -> BoxFuture<'_, Result<SettleOutcome, StoreError>> {
        Box::pin(async move { self.settle(intervention_id, outcome) })
    }

    fn due_interventions(
        &self,
        at: Timestamp,
    ) -> BoxFuture<'_, Result<Vec<Intervention>, StoreError>> {
        Box::pin(async move {
            let records = lock(&self.records);
            let mut found: Vec<Intervention> = records
                .interventions
                .iter()
                .filter(|intervention| {
                    intervention.outcome == InterventionOutcome::StillActive
                        && intervention.expires_at <= at
                })
                .cloned()
                .collect();
            found.sort_by_key(|intervention| (intervention.expires_at, intervention.id));
            Ok(found)
        })
    }

    fn active_interventions(
        &self,
        print_id: PrintId,
    ) -> BoxFuture<'_, Result<Vec<Intervention>, StoreError>> {
        Box::pin(async move {
            let records = lock(&self.records);
            let mut found: Vec<Intervention> = records
                .interventions
                .iter()
                .filter(|intervention| {
                    intervention.print_id == print_id
                        && intervention.outcome == InterventionOutcome::StillActive
                })
                .cloned()
                .collect();
            found.sort_by_key(|intervention| (intervention.applied_at, intervention.id));
            Ok(found)
        })
    }

    fn put_manifest(
        &self,
        print_id: PrintId,
        manifest: JobManifest,
    ) -> BoxFuture<'_, Result<(), StoreError>> {
        Box::pin(async move {
            let mut records = lock(&self.records);
            if !records.prints.iter().any(|print| print.id == print_id) {
                return Err(refused("manifests.print_id"));
            }
            records.manifests.retain(|(held, _)| *held != print_id);
            records.manifests.push((print_id, manifest));
            Ok(())
        })
    }

    fn manifest(
        &self,
        print_id: PrintId,
    ) -> BoxFuture<'_, Result<Option<JobManifest>, StoreError>> {
        Box::pin(async move {
            Ok(lock(&self.records)
                .manifests
                .iter()
                .find(|(held, _)| *held == print_id)
                .map(|(_, manifest)| manifest.clone()))
        })
    }

    fn history(&self, query: HistoryQuery) -> BoxFuture<'_, Result<Vec<EventRecord>, StoreError>> {
        Box::pin(async move {
            let limit = query.resolved_limit()? as usize;
            let records = lock(&self.records);
            let mut found: Vec<EventRecord> = Self::events_of(&records, query.print_id)
                .into_iter()
                .filter(|event| query.kinds.is_empty() || query.kinds.contains(&event.kind()))
                .filter(|event| query.since.is_none_or(|since| event.received_at >= since))
                .filter(|event| query.until.is_none_or(|until| event.received_at <= until))
                .collect();
            found.reverse();
            found.truncate(limit);
            Ok(found)
        })
    }

    fn audit_page(
        &self,
        print_id: PrintId,
        after: Option<EventId>,
        page_size: u32,
    ) -> BoxFuture<'_, Result<AuditPage, StoreError>> {
        Box::pin(async move {
            let asked_for = (page_size > 0).then_some(page_size);
            let size = resolve_history_limit(asked_for)? as usize;
            let records = lock(&self.records);
            let all = Self::events_of(&records, print_id);
            let start = match after {
                None => 0,
                Some(cursor) => {
                    let found = records
                        .events
                        .iter()
                        .find(|event| event.id == cursor)
                        .ok_or_else(|| not_found(&format!("event {cursor}")))?;
                    all.iter()
                        .position(|event| {
                            (event.received_at, event.id) > (found.received_at, found.id)
                        })
                        .unwrap_or(all.len())
                }
            };
            let mut events: Vec<EventRecord> = all.into_iter().skip(start).collect();
            let next = (events.len() > size).then(|| events[size - 1].id);
            events.truncate(size);
            Ok(AuditPage { events, next })
        })
    }

    fn put_session(&self, session: SupervisionSession) -> BoxFuture<'_, Result<(), StoreError>> {
        Box::pin(async move {
            let mut records = lock(&self.records);
            if !records
                .prints
                .iter()
                .any(|print| print.id == session.print_id)
            {
                return Err(refused("sessions.print_id"));
            }
            records
                .sessions
                .retain(|held| held.print_id != session.print_id);
            records.sessions.push(session);
            Ok(())
        })
    }

    fn session(
        &self,
        print_id: PrintId,
    ) -> BoxFuture<'_, Result<Option<SupervisionSession>, StoreError>> {
        Box::pin(async move {
            Ok(lock(&self.records)
                .sessions
                .iter()
                .find(|session| session.print_id == print_id)
                .cloned())
        })
    }
}
