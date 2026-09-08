//! The four fake ports every journey in this crate drives against.
//!
//! There is **exactly one** implementation of the printer port here, shared by
//! every test, and its own assertion is what makes the chokepoint's proof cover
//! paths a source check cannot resolve. A second printer double would be a
//! second set of rules about what may reach a machine, so a check refuses one.
//!
//! None of these is a real printer, a real Obico or a real model: this crate
//! declares four ports and nothing that implements them, and its tests hold to
//! that.

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::{Arc, Condvar, Mutex, Weak};
use std::time::Instant;

use printobserver_core::{Clock, Supervisor};
use printobserver_printer_api::{BoxFuture, PrinterError, PrinterPort};
use printobserver_store_api::{
    AuditPage, EventDraft, HistoryQuery, ImageLookup, SettleOutcome, StoreError, StorePort,
    resolve_history_limit,
};
use printobserver_supervisor_api::{
    SupervisorError, SupervisorPort, TurnOutcome, TurnRequest,
};
use printobserver_types::{
    ActionId, ActionRecord, ActionRequest, Adjustable, AgentAssessment, Confidence, EventId,
    EventPayload, EventRecord, EventSource, ExecutionOutcome, FileName, ImageId, ImageRecord,
    Intervention, InterventionId, InterventionOutcome, JobManifest, JobSnapshot, ManifestNarrowing,
    PolicyDecision, PrintAction, PrintContext, PrintId, PrintRecord, PrinterSnapshot, PrinterState,
    RawBytes, SessionPhase, SupervisionSession, Timestamp,
};
use printobserver_vision_api::{FetchedImage, NormalizedAlert, VisionError, VisionPort};

use crate::journal::{Call, Journal};

/// A clock a test moves by hand, and the only time core reads.
#[derive(Debug)]
pub struct FakeClock {
    /// Whole seconds after the Unix epoch.
    seconds: Mutex<i64>,
}

impl FakeClock {
    /// A clock starting at a fixed instant.
    #[must_use]
    pub const fn new(seconds: i64) -> Self {
        Self {
            seconds: Mutex::new(seconds),
        }
    }

    /// Move the clock forward, which is the only thing that drives expiry.
    pub fn advance(&self, seconds: i64) {
        *self.seconds.lock().expect("the clock holds") += seconds;
    }
}

impl Clock for FakeClock {
    fn now(&self) -> Timestamp {
        Timestamp::from_unix_seconds(*self.seconds.lock().expect("the clock holds"))
            .expect("a representable instant")
    }
}

/// One method the printer port declares.
///
/// The whole trait, so that a walk over its failure sites reads them off the
/// port rather than off a list kept beside it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum PrinterMethod {
    /// `snapshot`.
    Snapshot,
    /// `job`.
    Job,
    /// `start`.
    Start,
    /// `pause`.
    Pause,
    /// `resume`.
    Resume,
    /// `cancel`.
    Cancel,
    /// `set_feedrate_factor`.
    SetFeedrateFactor,
    /// `set_flowrate_factor`.
    SetFlowrateFactor,
    /// `set_tool_target_c`.
    SetToolTargetC,
    /// `set_bed_target_c`.
    SetBedTargetC,
    /// `set_fan_percent`.
    SetFanPercent,
}

impl PrinterMethod {
    /// Every method the printer port declares.
    pub const ALL: [Self; 11] = [
        Self::Snapshot,
        Self::Job,
        Self::Start,
        Self::Pause,
        Self::Resume,
        Self::Cancel,
        Self::SetFeedrateFactor,
        Self::SetFlowrateFactor,
        Self::SetToolTargetC,
        Self::SetBedTargetC,
        Self::SetFanPercent,
    ];

    /// The method's own name, as the trait spells it.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Snapshot => "snapshot",
            Self::Job => "job",
            Self::Start => "start",
            Self::Pause => "pause",
            Self::Resume => "resume",
            Self::Cancel => "cancel",
            Self::SetFeedrateFactor => "set_feedrate_factor",
            Self::SetFlowrateFactor => "set_flowrate_factor",
            Self::SetToolTargetC => "set_tool_target_c",
            Self::SetBedTargetC => "set_bed_target_c",
            Self::SetFanPercent => "set_fan_percent",
        }
    }

    /// Whether this is an action method rather than one of the two reads.
    #[must_use]
    pub const fn is_action(self) -> bool {
        !matches!(self, Self::Snapshot | Self::Job)
    }
}

/// The one printer-port double in this crate's tests.
///
/// Every action call it receives must have been preceded by a policy decision
/// recorded for that request. One that was not is a violation the journal
/// reports and [`Journal::assert_no_violations`] fails the test on.
pub struct FakePrinter {
    /// The shared ordered record of port calls.
    journal: Arc<Journal>,
    /// The state it reports.
    snapshot: Mutex<PrinterSnapshot>,
    /// The job it reports.
    job: Mutex<JobSnapshot>,
    /// The methods induced to fail, and how.
    failures: Mutex<BTreeMap<PrinterMethod, PrinterError>>,
}

impl FakePrinter {
    /// A printer reporting a snapshot and a job, failing nothing.
    #[must_use]
    pub fn new(journal: Arc<Journal>) -> Self {
        Self {
            journal,
            snapshot: Mutex::new(printer_snapshot(PrinterState::Printing)),
            job: Mutex::new(job_snapshot()),
            failures: Mutex::new(BTreeMap::new()),
        }
    }

    /// Report this snapshot from now on.
    pub fn reports(&self, snapshot: PrinterSnapshot) {
        *self.snapshot.lock().expect("the printer holds") = snapshot;
    }

    /// Report this state from now on, leaving everything else as it was.
    pub fn reports_state(&self, state: PrinterState) {
        self.snapshot
            .lock()
            .expect("the printer holds")
            .connection = state;
    }

    /// Fail one method with one error from now on.
    pub fn fails(&self, method: PrinterMethod, error: PrinterError) {
        self.failures
            .lock()
            .expect("the printer holds")
            .insert(method, error);
    }

    /// Stop failing every method.
    pub fn heals(&self) {
        self.failures.lock().expect("the printer holds").clear();
    }

    /// The failure induced at one method, if one is.
    fn induced(&self, method: PrinterMethod) -> Option<PrinterError> {
        self.failures
            .lock()
            .expect("the printer holds")
            .get(&method)
            .cloned()
    }

    /// Record one action call, spend its licence, and answer it.
    fn act(&self, method: PrinterMethod, call: Call) -> Result<(), PrinterError> {
        self.journal.record(call);
        self.journal.spend_licence(method.name());
        self.induced(method).map_or(Ok(()), Err)
    }
}

impl PrinterPort for FakePrinter {
    fn snapshot(&self) -> BoxFuture<'_, Result<PrinterSnapshot, PrinterError>> {
        self.journal.record(Call::Snapshot);
        let answer = self.induced(PrinterMethod::Snapshot).map_or_else(
            || Ok(self.snapshot.lock().expect("the printer holds").clone()),
            Err,
        );
        Box::pin(async move { answer })
    }

    fn job(&self) -> BoxFuture<'_, Result<JobSnapshot, PrinterError>> {
        self.journal.record(Call::Job);
        let answer = self.induced(PrinterMethod::Job).map_or_else(
            || Ok(self.job.lock().expect("the printer holds").clone()),
            Err,
        );
        Box::pin(async move { answer })
    }

    fn start(&self, file_name: FileName) -> BoxFuture<'_, Result<(), PrinterError>> {
        let answer = self.act(
            PrinterMethod::Start,
            Call::Start(file_name.as_str().to_owned()),
        );
        Box::pin(async move { answer })
    }

    fn pause(&self) -> BoxFuture<'_, Result<(), PrinterError>> {
        let answer = self.act(PrinterMethod::Pause, Call::Pause);
        Box::pin(async move { answer })
    }

    fn resume(&self) -> BoxFuture<'_, Result<(), PrinterError>> {
        let answer = self.act(PrinterMethod::Resume, Call::Resume);
        Box::pin(async move { answer })
    }

    fn cancel(&self) -> BoxFuture<'_, Result<(), PrinterError>> {
        let answer = self.act(PrinterMethod::Cancel, Call::Cancel);
        Box::pin(async move { answer })
    }

    fn set_feedrate_factor(&self, factor: f64) -> BoxFuture<'_, Result<(), PrinterError>> {
        let answer = self.act(
            PrinterMethod::SetFeedrateFactor,
            Call::SetFeedrateFactor(factor),
        );
        Box::pin(async move { answer })
    }

    fn set_flowrate_factor(&self, factor: f64) -> BoxFuture<'_, Result<(), PrinterError>> {
        let answer = self.act(
            PrinterMethod::SetFlowrateFactor,
            Call::SetFlowrateFactor(factor),
        );
        Box::pin(async move { answer })
    }

    fn set_tool_target_c(
        &self,
        tool: i64,
        target_c: f64,
    ) -> BoxFuture<'_, Result<(), PrinterError>> {
        let answer = self.act(
            PrinterMethod::SetToolTargetC,
            Call::SetToolTargetC(tool, target_c),
        );
        Box::pin(async move { answer })
    }

    fn set_bed_target_c(&self, target_c: f64) -> BoxFuture<'_, Result<(), PrinterError>> {
        let answer = self.act(PrinterMethod::SetBedTargetC, Call::SetBedTargetC(target_c));
        Box::pin(async move { answer })
    }

    fn set_fan_percent(&self, percent: f64) -> BoxFuture<'_, Result<(), PrinterError>> {
        let answer = self.act(PrinterMethod::SetFanPercent, Call::SetFanPercent(percent));
        Box::pin(async move { answer })
    }
}

/// One method of the store this crate's tests induce a failure at.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum StoreMethod {
    /// `append_event`, which is the loop's own first call.
    AppendEvent,
    /// `put_image`.
    PutImage,
}

/// Everything the fake store holds.
#[derive(Debug, Default)]
struct Held {
    /// Every print, by identifier.
    prints: BTreeMap<PrintId, PrintRecord>,
    /// The prints in the order they were opened.
    opened: Vec<PrintId>,
    /// Every event, oldest first.
    events: Vec<EventRecord>,
    /// Every image record, by identifier.
    images: BTreeMap<ImageId, ImageRecord>,
    /// Every action record, by identifier.
    actions: BTreeMap<ActionId, ActionRecord>,
    /// Every intervention, by identifier.
    interventions: BTreeMap<InterventionId, Intervention>,
    /// Every print's manifest.
    manifests: BTreeMap<PrintId, JobManifest>,
    /// Every print's session.
    sessions: BTreeMap<PrintId, SupervisionSession>,
}

/// An in-memory store, which is what a fake of this port is.
pub struct FakeStore {
    /// The shared ordered record of port calls.
    journal: Arc<Journal>,
    /// The one time source, shared with core.
    clock: Arc<FakeClock>,
    /// Everything it holds.
    held: Mutex<Held>,
    /// The methods induced to fail, and how.
    failures: Mutex<BTreeMap<StoreMethod, StoreError>>,
    /// Where image bytes are written.
    root: PathBuf,
}

impl FakeStore {
    /// A store holding nothing, failing nothing.
    ///
    /// # Panics
    ///
    /// Panics when its image directory cannot be created.
    #[must_use]
    pub fn new(journal: Arc<Journal>, clock: Arc<FakeClock>) -> Self {
        let root = std::env::temp_dir().join(format!("printobserver-core-{}", ImageId::new()));
        std::fs::create_dir_all(&root).expect("the image directory is created");
        Self {
            journal,
            clock,
            held: Mutex::new(Held::default()),
            failures: Mutex::new(BTreeMap::new()),
            root,
        }
    }

    /// Fail one method with one error from now on.
    pub fn fails(&self, method: StoreMethod, error: StoreError) {
        self.failures
            .lock()
            .expect("the store holds")
            .insert(method, error);
    }

    /// Stop failing every method.
    pub fn heals(&self) {
        self.failures.lock().expect("the store holds").clear();
    }

    /// The failure induced at one method, if one is.
    fn induced(&self, method: StoreMethod) -> Option<StoreError> {
        self.failures
            .lock()
            .expect("the store holds")
            .get(&method)
            .cloned()
    }

    /// Every event held for one print, oldest first.
    #[must_use]
    pub fn events_of(&self, print_id: PrintId) -> Vec<EventRecord> {
        self.held
            .lock()
            .expect("the store holds")
            .events
            .iter()
            .filter(|event| event.print_id == Some(print_id))
            .cloned()
            .collect()
    }

    /// Every action record held, in the order they were minted.
    #[must_use]
    pub fn action_records(&self) -> Vec<ActionRecord> {
        self.held
            .lock()
            .expect("the store holds")
            .actions
            .values()
            .cloned()
            .collect()
    }

    /// One intervention as it now stands.
    #[must_use]
    pub fn intervention(&self, id: InterventionId) -> Option<Intervention> {
        self.held
            .lock()
            .expect("the store holds")
            .interventions
            .get(&id)
            .cloned()
    }

    /// One print as it now stands.
    #[must_use]
    pub fn print_now(&self, id: PrintId) -> Option<PrintRecord> {
        self.held
            .lock()
            .expect("the store holds")
            .prints
            .get(&id)
            .cloned()
    }
}

impl Drop for FakeStore {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

/// A stand-in digest, so that an image record carries a stable hexadecimal one.
fn digest(bytes: &[u8]) -> String {
    let mut hash: u128 = 0x6c62_272e_07bb_0142_62b8_2175_6295_c58d;
    for byte in bytes {
        hash ^= u128::from(*byte);
        hash = hash.wrapping_mul(0x0000_0000_0100_0000_0000_0000_0000_013b);
    }
    format!("{hash:032x}{hash:032x}")
}

/// A future that is ready the moment it is polled.
fn ready<T: Send + 'static>(value: T) -> BoxFuture<'static, T> {
    Box::pin(async move { value })
}

impl StorePort for FakeStore {
    fn open_print(
        &self,
        obico_print_id: Option<i64>,
        file_name: Option<String>,
    ) -> printobserver_store_api::BoxFuture<'_, Result<PrintRecord, StoreError>> {
        self.journal.record(Call::OpenPrint);
        let record = PrintRecord {
            id: PrintId::new(),
            obico_print_id,
            file_name,
            state: PrinterState::Printing,
            opened_at: self.clock.now(),
            ended_at: None,
            end_reason: None,
            narrowings: Vec::new(),
        };
        let mut held = self.held.lock().expect("the store holds");
        held.opened.push(record.id);
        held.prints.insert(record.id, record.clone());
        drop(held);
        Box::pin(async move { Ok(record) })
    }

    fn print(
        &self,
        print_id: PrintId,
    ) -> printobserver_store_api::BoxFuture<'_, Result<Option<PrintRecord>, StoreError>> {
        self.journal.record(Call::ReadPrint);
        let found = self
            .held
            .lock()
            .expect("the store holds")
            .prints
            .get(&print_id)
            .cloned();
        Box::pin(async move { Ok(found) })
    }

    fn print_by_obico_id(
        &self,
        obico_print_id: i64,
    ) -> printobserver_store_api::BoxFuture<'_, Result<Option<PrintRecord>, StoreError>> {
        self.journal.record(Call::ReadPrintByObicoId);
        let found = self
            .held
            .lock()
            .expect("the store holds")
            .prints
            .values()
            .find(|print| print.obico_print_id == Some(obico_print_id))
            .cloned();
        Box::pin(async move { Ok(found) })
    }

    fn end_print(
        &self,
        print_id: PrintId,
        state: PrinterState,
        ended_at: Timestamp,
        reason: String,
    ) -> printobserver_store_api::BoxFuture<'_, Result<PrintRecord, StoreError>> {
        self.journal.record(Call::EndPrint(format!("{state:?}")));
        let mut held = self.held.lock().expect("the store holds");
        let answer = held.prints.get_mut(&print_id).map_or_else(
            || {
                Err(StoreError::NotFound {
                    what: format!("print {print_id}"),
                })
            },
            |print| {
                print.state = state;
                print.ended_at = Some(ended_at);
                print.end_reason = Some(reason);
                Ok(print.clone())
            },
        );
        drop(held);
        Box::pin(async move { answer })
    }

    fn record_narrowing(
        &self,
        print_id: PrintId,
        narrowing: ManifestNarrowing,
    ) -> printobserver_store_api::BoxFuture<'_, Result<PrintRecord, StoreError>> {
        self.journal
            .record(Call::RecordNarrowing(narrowing.adjustable));
        let mut held = self.held.lock().expect("the store holds");
        let answer = held.prints.get_mut(&print_id).map_or_else(
            || {
                Err(StoreError::NotFound {
                    what: format!("print {print_id}"),
                })
            },
            |print| {
                print.narrowings.push(narrowing);
                Ok(print.clone())
            },
        );
        drop(held);
        Box::pin(async move { answer })
    }

    fn append_event(
        &self,
        draft: EventDraft,
    ) -> printobserver_store_api::BoxFuture<'_, Result<EventRecord, StoreError>> {
        self.journal.record(Call::AppendEvent(draft.kind()));
        if let Some(error) = self.induced(StoreMethod::AppendEvent) {
            return ready(Err(error));
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
        self.held
            .lock()
            .expect("the store holds")
            .events
            .push(record.clone());
        ready(Ok(record))
    }

    fn put_image(
        &self,
        print_id: PrintId,
        event_id: EventId,
        source_url: Option<String>,
        content_type: String,
        bytes: RawBytes,
    ) -> printobserver_store_api::BoxFuture<'_, Result<ImageRecord, StoreError>> {
        self.journal.record(Call::PutImage);
        if let Some(error) = self.induced(StoreMethod::PutImage) {
            return ready(Err(error));
        }
        let id = ImageId::new();
        let relative_path = format!("images/{id}.bin");
        let path = self.root.join(&relative_path);
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        let raw = bytes.as_slice();
        let byte_len = i64::try_from(raw.len()).unwrap_or(i64::MAX);
        let sha256 = digest(raw);
        if let Err(error) = std::fs::write(&path, raw) {
            return ready(Err(StoreError::Io {
                detail: error.to_string(),
            }));
        }
        let record = ImageRecord {
            id,
            print_id,
            event_id,
            source_url,
            fetched_at: self.clock.now(),
            content_type,
            byte_len,
            sha256: sha256.clone(),
            relative_path,
        };
        let mut held = self.held.lock().expect("the store holds");
        held.images.insert(id, record.clone());
        if let Some(event) = held.events.iter_mut().find(|event| event.id == event_id) {
            event.image = Some(printobserver_types::ImageRef { id, sha256 });
        }
        drop(held);
        ready(Ok(record))
    }

    fn image(
        &self,
        image_id: ImageId,
    ) -> printobserver_store_api::BoxFuture<'_, Result<ImageLookup, StoreError>> {
        self.journal.record(Call::ReadImage);
        let held = self.held.lock().expect("the store holds");
        let answer = held.images.get(&image_id).cloned().map_or_else(
            || {
                Err(StoreError::NotFound {
                    what: format!("image {image_id}"),
                })
            },
            |record| {
                let path = self.root.join(&record.relative_path);
                Ok(if path.exists() {
                    ImageLookup::Found { record, path }
                } else {
                    ImageLookup::FileMissing { record }
                })
            },
        );
        drop(held);
        Box::pin(async move { answer })
    }

    fn record_action(
        &self,
        request: ActionRequest,
        decision: PolicyDecision,
    ) -> printobserver_store_api::BoxFuture<'_, Result<ActionRecord, StoreError>> {
        self.journal.record(Call::RecordAction(decision.clone()));
        let mut held = self.held.lock().expect("the store holds");
        // This port carries no print on a request, so the record is attributed
        // to the print most recently opened — which is the one active print in
        // every journey that drives an action.
        let Some(print_id) = held.opened.last().copied() else {
            drop(held);
            return ready(Err(StoreError::NotFound {
                what: "an open print to record the action against".to_owned(),
            }));
        };
        let record = ActionRecord {
            id: ActionId::new(),
            print_id,
            request,
            decision: decision.clone(),
            executed_at: None,
            outcome: None,
        };
        held.actions.insert(record.id, record.clone());
        drop(held);
        if decision == PolicyDecision::Accepted {
            self.journal.licence(record.id);
        }
        ready(Ok(record))
    }

    fn record_execution(
        &self,
        action_id: ActionId,
        outcome: ExecutionOutcome,
    ) -> printobserver_store_api::BoxFuture<'_, Result<ActionRecord, StoreError>> {
        self.journal.record(Call::RecordExecution(outcome.clone()));
        let executed_at = self.clock.now();
        let mut held = self.held.lock().expect("the store holds");
        let answer = held.actions.get_mut(&action_id).map_or_else(
            || {
                Err(StoreError::NotFound {
                    what: format!("action {action_id}"),
                })
            },
            |record| {
                record.executed_at = Some(executed_at);
                record.outcome = Some(outcome);
                Ok(record.clone())
            },
        );
        drop(held);
        Box::pin(async move { answer })
    }

    fn open_intervention(
        &self,
        action_id: ActionId,
        adjustable: Adjustable,
        prior_value: Option<f64>,
        applied_value: f64,
        applied_at: Timestamp,
        expires_at: Timestamp,
    ) -> printobserver_store_api::BoxFuture<'_, Result<Intervention, StoreError>> {
        self.journal.record(Call::OpenIntervention(adjustable));
        let mut held = self.held.lock().expect("the store holds");
        let Some(action) = held.actions.get(&action_id).cloned() else {
            drop(held);
            return ready(Err(StoreError::NotFound {
                what: format!("action {action_id}"),
            }));
        };
        let intervention = Intervention {
            id: InterventionId::new(),
            print_id: action.print_id,
            action_id,
            adjustable,
            prior_value,
            applied_value,
            applied_at,
            expires_at,
            restored_at: None,
            outcome: InterventionOutcome::StillActive,
        };
        held.interventions.insert(intervention.id, intervention.clone());
        drop(held);
        ready(Ok(intervention))
    }

    fn settle_intervention(
        &self,
        intervention_id: InterventionId,
        outcome: InterventionOutcome,
    ) -> printobserver_store_api::BoxFuture<'_, Result<SettleOutcome, StoreError>> {
        self.journal.record(Call::SettleIntervention(outcome.clone()));
        let restored_at = self.clock.now();
        let mut held = self.held.lock().expect("the store holds");
        let answer = held.interventions.get_mut(&intervention_id).map_or_else(
            || {
                Err(StoreError::NotFound {
                    what: format!("intervention {intervention_id}"),
                })
            },
            |held| {
                if held.outcome == InterventionOutcome::StillActive {
                    held.outcome = outcome;
                    if held.outcome == InterventionOutcome::Restored {
                        held.restored_at = Some(restored_at);
                    }
                    Ok(SettleOutcome::Settled {
                        intervention: held.clone(),
                    })
                } else {
                    Ok(SettleOutcome::AlreadySettled {
                        outcome: held.outcome.clone(),
                    })
                }
            },
        );
        drop(held);
        Box::pin(async move { answer })
    }

    fn due_interventions(
        &self,
        at: Timestamp,
    ) -> printobserver_store_api::BoxFuture<'_, Result<Vec<Intervention>, StoreError>> {
        let due: Vec<Intervention> = self
            .held
            .lock()
            .expect("the store holds")
            .interventions
            .values()
            .filter(|held| {
                held.outcome == InterventionOutcome::StillActive && held.expires_at <= at
            })
            .cloned()
            .collect();
        ready(Ok(due))
    }

    fn active_interventions(
        &self,
        print_id: PrintId,
    ) -> printobserver_store_api::BoxFuture<'_, Result<Vec<Intervention>, StoreError>> {
        self.journal.record(Call::ReadActiveInterventions);
        let active: Vec<Intervention> = self
            .held
            .lock()
            .expect("the store holds")
            .interventions
            .values()
            .filter(|held| {
                held.print_id == print_id && held.outcome == InterventionOutcome::StillActive
            })
            .cloned()
            .collect();
        ready(Ok(active))
    }

    fn put_manifest(
        &self,
        print_id: PrintId,
        manifest: JobManifest,
    ) -> printobserver_store_api::BoxFuture<'_, Result<(), StoreError>> {
        self.journal.record(Call::PutManifest);
        self.held
            .lock()
            .expect("the store holds")
            .manifests
            .insert(print_id, manifest);
        ready(Ok(()))
    }

    fn manifest(
        &self,
        print_id: PrintId,
    ) -> printobserver_store_api::BoxFuture<'_, Result<Option<JobManifest>, StoreError>> {
        self.journal.record(Call::ReadManifest);
        let found = self
            .held
            .lock()
            .expect("the store holds")
            .manifests
            .get(&print_id)
            .cloned();
        ready(Ok(found))
    }

    fn history(
        &self,
        query: HistoryQuery,
    ) -> printobserver_store_api::BoxFuture<'_, Result<Vec<EventRecord>, StoreError>> {
        self.journal.record(Call::ReadHistory);
        let limit = match resolve_history_limit(query.limit) {
            Ok(limit) => limit as usize,
            Err(error) => return ready(Err(error)),
        };
        let answered: Vec<EventRecord> = self
            .held
            .lock()
            .expect("the store holds")
            .events
            .iter()
            .rev()
            .filter(|event| event.print_id == Some(query.print_id))
            .filter(|event| query.kinds.is_empty() || query.kinds.contains(&event.kind()))
            .take(limit)
            .cloned()
            .collect();
        ready(Ok(answered))
    }

    fn audit_page(
        &self,
        print_id: PrintId,
        after: Option<EventId>,
        page_size: u32,
    ) -> printobserver_store_api::BoxFuture<'_, Result<AuditPage, StoreError>> {
        self.journal.record(Call::ReadAuditPage);
        let size = match resolve_history_limit(Some(page_size)) {
            Ok(size) => size as usize,
            Err(error) => return ready(Err(error)),
        };
        let held = self.held.lock().expect("the store holds");
        let all: Vec<EventRecord> = held
            .events
            .iter()
            .filter(|event| event.print_id == Some(print_id))
            .cloned()
            .collect();
        drop(held);
        let start = after.map_or(0, |cursor| {
            all.iter()
                .position(|event| event.id == cursor)
                .map_or(0, |index| index + 1)
        });
        let events: Vec<EventRecord> = all.iter().skip(start).take(size).cloned().collect();
        let next = if start + events.len() < all.len() {
            events.last().map(|event| event.id)
        } else {
            None
        };
        ready(Ok(AuditPage { events, next }))
    }

    fn put_session(
        &self,
        session: SupervisionSession,
    ) -> printobserver_store_api::BoxFuture<'_, Result<(), StoreError>> {
        self.journal.record(Call::PutSession);
        self.held
            .lock()
            .expect("the store holds")
            .sessions
            .insert(session.print_id, session);
        ready(Ok(()))
    }

    fn session(
        &self,
        print_id: PrintId,
    ) -> printobserver_store_api::BoxFuture<'_, Result<Option<SupervisionSession>, StoreError>> {
        self.journal.record(Call::ReadSession);
        let found = self
            .held
            .lock()
            .expect("the store holds")
            .sessions
            .get(&print_id)
            .cloned();
        ready(Ok(found))
    }
}

/// The vision port, which core reaches only to retrieve an image.
pub struct FakeVision {
    /// The shared ordered record of port calls.
    journal: Arc<Journal>,
    /// What it serves.
    image: Mutex<FetchedImage>,
    /// The failure induced at `fetch_image`, if one is.
    failure: Mutex<Option<VisionError>>,
}

impl FakeVision {
    /// A vision port serving one image and failing nothing.
    #[must_use]
    pub fn new(journal: Arc<Journal>) -> Self {
        Self {
            journal,
            image: Mutex::new(FetchedImage {
                bytes: RawBytes::new(b"\x89PNG\r\n\x1a\n".to_vec()),
                content_type: "image/png".to_owned(),
            }),
            failure: Mutex::new(None),
        }
    }

    /// Fail every retrieval from now on.
    pub fn fails(&self, error: VisionError) {
        *self.failure.lock().expect("the vision port holds") = Some(error);
    }

    /// The image it serves.
    #[must_use]
    pub fn served(&self) -> FetchedImage {
        self.image.lock().expect("the vision port holds").clone()
    }
}

impl VisionPort for FakeVision {
    fn normalize(
        &self,
        body: RawBytes,
        content_type: Option<String>,
    ) -> printobserver_vision_api::BoxFuture<'_, Result<NormalizedAlert, VisionError>> {
        self.journal.record(Call::Normalize);
        let _ = content_type;
        Box::pin(async move {
            Err(VisionError::Malformed {
                detail: "this fake normalizes nothing: core is given alerts already normalized"
                    .to_owned(),
                raw: body,
            })
        })
    }

    fn fetch_image(
        &self,
        source_url: String,
    ) -> printobserver_vision_api::BoxFuture<'_, Result<FetchedImage, VisionError>> {
        self.journal.record(Call::FetchImage);
        let _ = source_url;
        let answer = self
            .failure
            .lock()
            .expect("the vision port holds")
            .clone()
            .map_or_else(|| Ok(self.served()), Err);
        Box::pin(async move { answer })
    }
}

/// What the fake supervisor observed about the turns it ran.
#[derive(Debug, Default)]
pub struct Overlap {
    /// How many turns have been entered.
    pub entered: usize,
    /// How many turns have returned.
    pub returned: usize,
    /// The most turns ever entered and not yet returned.
    pub high_water: usize,
    /// When each turn was entered.
    pub entries: Vec<Instant>,
    /// When each turn returned.
    pub exits: Vec<Instant>,
}

/// A gate a test holds one call at.
#[derive(Debug, Default)]
struct Gate {
    /// Whether a call arriving now is held.
    held: Mutex<bool>,
    /// Signalled when the hold is released.
    released: Condvar,
}

/// The supervising agent's harness, as a fake.
///
/// It opens the session on the first turn of a print and continues it after,
/// which is this port's own doing rather than core's; it reads the print's
/// context back through core, which is what the turn's context command does;
/// and it acts, when a test asks it to, by calling back in through the ordinary
/// action path.
pub struct FakeSupervisor {
    /// The shared ordered record of port calls.
    journal: Arc<Journal>,
    /// The one time source.
    clock: Arc<FakeClock>,
    /// Core, so that a turn can read context and act the way the agent does.
    core: Mutex<Weak<Supervisor>>,
    /// One session per print, once a turn has opened it.
    sessions: Mutex<BTreeMap<PrintId, SupervisionSession>>,
    /// Every turn it was asked to run.
    turns: Mutex<Vec<TurnRequest>>,
    /// The context each turn read back through core.
    contexts: Mutex<Vec<Result<PrintContext, String>>>,
    /// What the agent does during its turn, when it acts.
    action: Mutex<Option<PrintAction>>,
    /// What each action the agent took answered.
    acted: Mutex<Vec<Result<printobserver_core::ActionOutcome, String>>>,
    /// The failure induced at `run_turn`, if one is.
    failure: Mutex<Option<SupervisorError>>,
    /// What it observed about overlap between turns.
    overlap: Mutex<Overlap>,
    /// Where a test holds the first call.
    gate: Gate,
}

impl FakeSupervisor {
    /// A harness that opens a session, runs a turn and answers an assessment.
    #[must_use]
    pub fn new(journal: Arc<Journal>, clock: Arc<FakeClock>) -> Self {
        Self {
            journal,
            clock,
            core: Mutex::new(Weak::new()),
            sessions: Mutex::new(BTreeMap::new()),
            turns: Mutex::new(Vec::new()),
            contexts: Mutex::new(Vec::new()),
            action: Mutex::new(None),
            acted: Mutex::new(Vec::new()),
            failure: Mutex::new(None),
            overlap: Mutex::new(Overlap::default()),
            gate: Gate::default(),
        }
    }

    /// Give the harness the core its turns read context from and act through.
    pub fn attach(&self, core: &Arc<Supervisor>) {
        *self.core.lock().expect("the harness holds") = Arc::downgrade(core);
    }

    /// Act with this request on every turn from now on.
    pub fn acts_with(&self, action: PrintAction) {
        *self.action.lock().expect("the harness holds") = Some(action);
    }

    /// Stop acting on every turn.
    pub fn acts_with_nothing(&self) {
        *self.action.lock().expect("the harness holds") = None;
    }

    /// Fail every turn from now on.
    pub fn fails(&self, error: SupervisorError) {
        *self.failure.lock().expect("the harness holds") = Some(error);
    }

    /// Stop failing every turn.
    pub fn heals(&self) {
        *self.failure.lock().expect("the harness holds") = None;
    }

    /// Hold the next call that arrives until [`FakeSupervisor::release`].
    pub fn hold(&self) {
        *self.gate.held.lock().expect("the gate holds") = true;
    }

    /// Let a held call carry on.
    pub fn release(&self) {
        *self.gate.held.lock().expect("the gate holds") = false;
        self.gate.released.notify_all();
    }

    /// Every turn it was asked to run, in order.
    #[must_use]
    pub fn turns(&self) -> Vec<TurnRequest> {
        self.turns.lock().expect("the harness holds").clone()
    }

    /// The context each turn read back through core, in order.
    #[must_use]
    pub fn contexts(&self) -> Vec<Result<PrintContext, String>> {
        self.contexts.lock().expect("the harness holds").clone()
    }

    /// What each action the agent took answered, in order.
    #[must_use]
    pub fn acted(&self) -> Vec<Result<printobserver_core::ActionOutcome, String>> {
        self.acted.lock().expect("the harness holds").clone()
    }

    /// How many turns have been entered and not yet returned.
    #[must_use]
    pub fn live(&self) -> usize {
        let overlap = self.overlap.lock().expect("the harness holds");
        overlap.entered - overlap.returned
    }

    /// How many turns have been entered at all.
    #[must_use]
    pub fn entered(&self) -> usize {
        self.overlap.lock().expect("the harness holds").entered
    }

    /// The most turns ever entered and not yet returned.
    #[must_use]
    pub fn high_water(&self) -> usize {
        self.overlap.lock().expect("the harness holds").high_water
    }

    /// When each turn was entered, and when each returned.
    #[must_use]
    pub fn instants(&self) -> (Vec<Instant>, Vec<Instant>) {
        let overlap = self.overlap.lock().expect("the harness holds");
        (overlap.entries.clone(), overlap.exits.clone())
    }

    /// Note that a turn has been entered, and how many are now live.
    fn on_enter(&self) {
        let mut overlap = self.overlap.lock().expect("the harness holds");
        overlap.entered += 1;
        overlap.entries.push(Instant::now());
        let live = overlap.entered - overlap.returned;
        overlap.high_water = overlap.high_water.max(live);
    }

    /// Note that a turn has returned.
    fn on_exit(&self) {
        let mut overlap = self.overlap.lock().expect("the harness holds");
        overlap.returned += 1;
        overlap.exits.push(Instant::now());
    }

    /// Block here while a test is holding this call.
    fn wait_while_held(&self) {
        let mut held = self.gate.held.lock().expect("the gate holds");
        while *held {
            held = self.gate.released.wait(held).expect("the gate holds");
        }
    }

    /// The session for one print, opening it if this is its first turn.
    fn session_for(&self, print_id: PrintId) -> (SupervisionSession, SessionPhase) {
        let now = self.clock.now();
        let mut sessions = self.sessions.lock().expect("the harness holds");
        match sessions.get_mut(&print_id) {
            Some(open) => {
                open.last_turn_at = now;
                (open.clone(), SessionPhase::Continued)
            }
            None => {
                let session = SupervisionSession {
                    print_id,
                    session_name: format!("print-{print_id}"),
                    harness_identity: "printobserver-supervisor".to_owned(),
                    created_at: now,
                    last_turn_at: now,
                    closed_at: None,
                    close_reason: None,
                };
                sessions.insert(print_id, session.clone());
                (session, SessionPhase::Created)
            }
        }
    }
}

impl SupervisorPort for FakeSupervisor {
    fn run_turn(
        &self,
        request: TurnRequest,
    ) -> printobserver_supervisor_api::BoxFuture<'_, Result<TurnOutcome, SupervisorError>> {
        self.journal.record(Call::RunTurn(request.print_id));
        self.turns
            .lock()
            .expect("the harness holds")
            .push(request.clone());
        Box::pin(async move {
            self.on_enter();
            self.wait_while_held();
            let core = self.core.lock().expect("the harness holds").upgrade();
            if let Some(core) = core {
                // What the turn's own context command does: read the print's
                // context back through core.
                let seen = core
                    .context(request.print_id)
                    .await
                    .map_err(|error| error.to_string());
                self.contexts.lock().expect("the harness holds").push(seen);
                // And, if it acts, act through the ordinary action path.
                let asked = self.action.lock().expect("the harness holds").clone();
                if let Some(action) = asked {
                    let outcome = core
                        .request_action(request.print_id, action)
                        .await
                        .map_err(|error| error.to_string());
                    self.acted.lock().expect("the harness holds").push(outcome);
                }
            }
            let answer = match self.failure.lock().expect("the harness holds").clone() {
                Some(error) => Err(error),
                None => {
                    let (session, phase) = self.session_for(request.print_id);
                    Ok(TurnOutcome {
                        session,
                        phase,
                        assessment: AgentAssessment {
                            summary: "the print is running".to_owned(),
                            confidence: Confidence::High,
                            should_continue: true,
                            did: "watched".to_owned(),
                            why: "nothing needed doing".to_owned(),
                            escalating: false,
                        },
                    })
                }
            };
            self.on_exit();
            answer
        })
    }

    fn close_session(
        &self,
        print_id: PrintId,
        close_reason: String,
    ) -> printobserver_supervisor_api::BoxFuture<'_, Result<(), SupervisorError>> {
        self.journal
            .record(Call::CloseSession(print_id, close_reason.clone()));
        let mut sessions = self.sessions.lock().expect("the harness holds");
        if let Some(session) = sessions.get_mut(&print_id) {
            session.closed_at = Some(self.clock.now());
            session.close_reason = Some(close_reason);
        }
        drop(sessions);
        Box::pin(async move { Ok(()) })
    }
}

/// A printer snapshot in one state, with a prior value for every adjustable.
#[must_use]
pub fn printer_snapshot(state: PrinterState) -> PrinterSnapshot {
    use printobserver_types::{
        FAN_PERCENT_RANGE, FEEDRATE_FACTOR_RANGE, FLOWRATE_FACTOR_RANGE, HEATER_ACTUAL_C_RANGE,
        HEATER_OFFSET_C_RANGE, HEATER_TARGET_C_RANGE, HeaterSnapshot, Reported,
    };
    let heater = |target: f64| HeaterSnapshot {
        actual_c: Some(Reported::new(target - 0.5, HEATER_ACTUAL_C_RANGE)),
        target_c: Some(Reported::new(target, HEATER_TARGET_C_RANGE)),
        offset_c: Some(Reported::new(0.0, HEATER_OFFSET_C_RANGE)),
    };
    PrinterSnapshot {
        connection: state,
        tools: vec![heater(215.0)],
        bed: Some(heater(60.0)),
        chamber: None,
        feedrate_factor: Some(Reported::new(1.0, FEEDRATE_FACTOR_RANGE)),
        flowrate_factor: Some(Reported::new(1.0, FLOWRATE_FACTOR_RANGE)),
        fan_percent: Some(Reported::new(40.0, FAN_PERCENT_RANGE)),
        observed_at: Timestamp::from_unix_seconds(1_700_000_000).expect("a fixed instant"),
    }
}

/// The job the fake printer reports.
#[must_use]
pub fn job_snapshot() -> JobSnapshot {
    use printobserver_types::{COMPLETION_RANGE, Reported};
    JobSnapshot {
        file_name: Some("benchy.gcode".to_owned()),
        file_origin: Some("local".to_owned()),
        size_bytes: Some(4_194_304),
        estimated_print_time_s: Some(7200),
        completion: Some(Reported::new(0.42, COMPLETION_RANGE)),
        print_time_s: Some(3024),
        print_time_left_s: Some(4176),
        state: PrinterState::Printing,
        error: None,
    }
}
