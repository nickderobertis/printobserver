//! The four fake ports every journey in this crate drives against.
//!
//! There is **exactly one** implementation of the printer port here, shared by
//! every test, and its own assertion is what makes the chokepoint's proof cover
//! paths a source check cannot resolve. A second printer double would be a
//! second set of rules about what may reach a machine, so a check refuses one.
//!
//! None of these is a real printer, a real detector or a real model: this crate
//! depends on three ports and declares its own store interfaces, with nothing
//! that implements any of them, and its tests hold to
//! that.

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::{Arc, Condvar, Mutex, Weak};
use std::time::{Duration, Instant};

use printobserver_core::store::{
    ActionStore, AuditPage, Charge, ChargeRecord, EventDraft, EventStore, FilamentStore, FileTally,
    HistoryQuery, ImageLookup, ImageStore, NewSpool, Pend, PrintStore, SessionStore, SettleOutcome,
    Settlement, SpoolMutation, SpoolWrite, StoreError, StoredQuery, StoredSignal, TimelineQuery,
    TimelineStore, resolve_history_limit,
};
use printobserver_core::{
    ActionId, ActionRecord, ActionRequest, Actor, ExecutionOutcome, FilePosition, ImageRecord,
    Intervention, InterventionId, InterventionOutcome, JobManifest, ManifestNarrowing,
    PendingCharge, PolicyDecision, PrintAction, PrintOutcome, PrintRecord, SpoolId, SpoolRecord,
};
use printobserver_core::{Clock, PrintContext, Supervisor};
use printobserver_printer_api::{Adjustable, PrinterState};
use printobserver_printer_api::{
    BoxFuture, FileFacts, JobSnapshot, PrinterError, PrinterPort, PrinterSnapshot, ToolFilament,
};
use printobserver_supervisor_api::{
    AgentAssessment, Confidence, SupervisorError, SupervisorPort, TurnOutcome, TurnRequest,
};
use printobserver_supervisor_api::{SessionPhase, SupervisionSession};
use printobserver_types::{
    EventId, EventKind, EventRecord, EventSource, FileName, ImageId, PrintId, RawBytes, Timestamp,
};
use printobserver_vision_api::{FetchedImage, NormalizedAlert, VisionError, VisionPort};

use crate::journal::{Call, Journal};
use crate::ledger::{FakeLedger, LedgerWrite, timeline};

/// A clock a test moves by hand, and the only time core reads.
#[derive(Debug)]
pub struct FakeClock {
    /// Whole seconds after the Unix epoch.
    seconds: Mutex<i64>,
    /// Whole seconds that have passed since the clock started, which a step
    /// of the wall clock does not move.
    passed: Mutex<i64>,
}

impl FakeClock {
    /// A clock starting at a fixed instant.
    #[must_use]
    pub const fn new(seconds: i64) -> Self {
        Self {
            seconds: Mutex::new(seconds),
            passed: Mutex::new(0),
        }
    }

    /// Let time pass, which is the only thing that drives expiry.
    pub fn advance(&self, seconds: i64) {
        *self.seconds.lock().expect("the clock holds") += seconds;
        *self.passed.lock().expect("the clock holds") += seconds;
    }

    /// Step the wall clock alone, as a host's time synchronisation does, with
    /// no time passing.
    pub fn step_wall(&self, seconds: i64) {
        *self.seconds.lock().expect("the clock holds") += seconds;
    }
}

impl Clock for FakeClock {
    fn now(&self) -> Timestamp {
        Timestamp::from_unix_seconds(*self.seconds.lock().expect("the clock holds"))
            .expect("a representable instant")
    }

    fn elapsed(&self) -> Duration {
        let passed = *self.passed.lock().expect("the clock holds");
        Duration::from_secs(u64::try_from(passed).expect("time passed forwards"))
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
    /// `file_facts`.
    FileFacts,
    /// `printed_files`.
    PrintedFiles,
    /// `extruded_to`.
    ExtrudedTo,
    /// `push_events`.
    PushEvents,
    /// `send_command`.
    SendCommand,
}

impl PrinterMethod {
    /// Every method the printer port declares.
    pub const ALL: [Self; 16] = [
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
        Self::FileFacts,
        Self::PrintedFiles,
        Self::ExtrudedTo,
        Self::PushEvents,
        Self::SendCommand,
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
            Self::FileFacts => "file_facts",
            Self::PrintedFiles => "printed_files",
            Self::ExtrudedTo => "extruded_to",
            Self::PushEvents => "push_events",
            Self::SendCommand => "send_command",
        }
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
    /// The files it holds, by name.
    files: Mutex<BTreeMap<String, FileFacts>>,
    /// What each file feeds up to each position a test declared.
    extrusion: Mutex<BTreeMap<(String, i64), Vec<ToolFilament>>>,
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
            files: Mutex::new(BTreeMap::new()),
            extrusion: Mutex::new(BTreeMap::new()),
        }
    }

    /// Hold this file, replacing whatever was held under its name.
    pub fn holds_file(&self, facts: FileFacts) {
        self.files
            .lock()
            .expect("the printer holds")
            .insert(facts.file_name.clone(), facts);
    }

    /// One file as it is held now.
    ///
    /// # Panics
    ///
    /// Panics when no file of that name is held.
    #[must_use]
    pub fn file(&self, file_name: &str) -> FileFacts {
        self.files
            .lock()
            .expect("the printer holds")
            .get(file_name)
            .cloned()
            .unwrap_or_else(|| panic!("the printer holds {file_name}"))
    }

    /// Answer that one file feeds each tool this much up to one position.
    pub fn extrudes(&self, file_name: &str, position_bytes: i64, filament: Vec<ToolFilament>) {
        self.extrusion
            .lock()
            .expect("the printer holds")
            .insert((file_name.to_owned(), position_bytes), filament);
    }

    /// Report the job as this far into its file, and its file as this long.
    pub fn reports_position(&self, position_bytes: Option<i64>, size_bytes: Option<i64>) {
        let mut job = self.job.lock().expect("the printer holds");
        job.file_position_bytes = position_bytes;
        job.size_bytes = size_bytes;
    }

    /// Report the job's completion, as a fraction, or none.
    pub fn reports_completion(&self, completion: Option<f64>) {
        self.job.lock().expect("the printer holds").completion = completion.map(|value| {
            printobserver_types::Reported::new(value, printobserver_printer_api::COMPLETION_RANGE)
        });
    }

    /// Report the job's error text and whose its adapter says it is, or
    /// none.
    pub fn reports_error(&self, error: Option<(&str, printobserver_printer_api::ErrorSource)>) {
        let mut job = self.job.lock().expect("the printer holds");
        job.error = error.map(|(text, source)| printobserver_printer_api::JobError {
            text: text.to_owned(),
            source,
        });
    }

    /// Report this snapshot from now on.
    pub fn reports(&self, snapshot: PrinterSnapshot) {
        *self.snapshot.lock().expect("the printer holds") = snapshot;
    }

    /// Report this state from now on, leaving everything else as it was.
    pub fn reports_state(&self, state: PrinterState) {
        self.snapshot.lock().expect("the printer holds").connection = state;
    }

    /// Kept apart from `reports_state` because adoption reads the job alone, so
    /// what a listing adopts is set here and nowhere else.
    pub fn reports_job(&self, file_name: Option<&str>, state: PrinterState) {
        let mut job = self.job.lock().expect("the printer holds");
        job.file_name = file_name.map(str::to_owned);
        job.state = state;
    }

    /// Report the job as having run this long, in whole seconds, or report no
    /// running time at all; which job it is, is told apart by nothing else.
    pub fn reports_running_time(&self, seconds: Option<i64>) {
        self.job.lock().expect("the printer holds").print_time_s = seconds;
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

    fn file_facts(&self, file_name: FileName) -> BoxFuture<'_, Result<FileFacts, PrinterError>> {
        self.journal
            .record(Call::FileFacts(file_name.as_str().to_owned()));
        let answer = self.induced(PrinterMethod::FileFacts).map_or_else(
            || {
                self.files
                    .lock()
                    .expect("the printer holds")
                    .get(file_name.as_str())
                    .cloned()
                    .ok_or_else(|| PrinterError::Refused {
                        status: 404,
                        detail: format!("there is no file {file_name}"),
                    })
            },
            Err,
        );
        Box::pin(async move { answer })
    }

    fn printed_files(&self) -> BoxFuture<'_, Result<Vec<FileFacts>, PrinterError>> {
        self.journal.record(Call::PrintedFiles);
        let answer = self.induced(PrinterMethod::PrintedFiles).map_or_else(
            || {
                Ok(self
                    .files
                    .lock()
                    .expect("the printer holds")
                    .values()
                    .filter(|facts| facts.successes.saturating_add(facts.failures) > 0)
                    .cloned()
                    .collect())
            },
            Err,
        );
        Box::pin(async move { answer })
    }

    fn extruded_to(
        &self,
        file_name: FileName,
        position_bytes: i64,
    ) -> BoxFuture<'_, Result<Vec<ToolFilament>, PrinterError>> {
        self.journal.record(Call::ExtrudedTo(
            file_name.as_str().to_owned(),
            position_bytes,
        ));
        let answer = self.induced(PrinterMethod::ExtrudedTo).map_or_else(
            || {
                self.extrusion
                    .lock()
                    .expect("the printer holds")
                    .get(&(file_name.as_str().to_owned(), position_bytes))
                    .cloned()
                    .ok_or_else(|| PrinterError::Refused {
                        status: 404,
                        detail: format!("{file_name} was not read up to byte {position_bytes}"),
                    })
            },
            Err,
        );
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
    /// `open_prints`, which settling and observing read.
    OpenPrints,
    /// `end_print`, which a close-out calls after it settles.
    EndPrint,
    /// `settle_print`.
    SettlePrint,
    /// `apply_spool_changes`.
    ApplySpoolChanges,
    /// `account_tallies`.
    AccountTallies,
    /// `record_file_position`.
    RecordFilePosition,
    /// `history`, which the agent's interval is read back from.
    History,
    /// `due_interventions`, which the interventions left active are read by.
    DueInterventions,
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
    /// The filament ledger.
    ledger: FakeLedger,
}

/// How long a caller at a [`Meeting`] waits for the others before going on alone.
const PATIENCE: Duration = Duration::from_millis(400);

/// A place callers wait for one another at, for a journey about two of them
/// interleaving.
///
/// Unarmed, it holds nobody. Armed for a number of callers, each one that
/// arrives waits until that many have, or until [`PATIENCE`] runs out — so two
/// callers nothing keeps apart are made to overlap, and two that something does
/// keep apart each wait a moment alone and go on. A read arrives **after** it
/// has taken what it answers, so two reads that overlap here both answer what
/// was there before either caller wrote anything.
#[derive(Debug, Default)]
pub struct Meeting {
    /// How many callers are wanted, and how many have come.
    counts: Mutex<(usize, usize)>,
    /// Signalled whenever one arrives.
    arrived: Condvar,
}

impl Meeting {
    /// Arm it for this many callers.
    pub fn expect(&self, callers: usize) {
        *self.counts.lock().expect("the meeting holds") = (callers, 0);
    }

    /// Arrive, and wait for the others when it is armed.
    fn arrive(&self) {
        let mut counts = self.counts.lock().expect("the meeting holds");
        if counts.0 == 0 {
            return;
        }
        counts.1 += 1;
        self.arrived.notify_all();
        let _ = self
            .arrived
            .wait_timeout_while(counts, PATIENCE, |(wanted, came)| *came < *wanted)
            .expect("the meeting holds");
    }
}

/// Where appends of one kind wait until they are let through, for a journey
/// about what arrives while one is in flight.
///
/// Unarmed, it holds nothing. A held append waits at most as long as a journey
/// waits for anything, so a journey that failed before letting it through does
/// not hold its thread for ever.
#[derive(Debug, Default)]
pub struct AppendGate {
    /// The kind held, and how many appends of it are waiting.
    state: Mutex<(Option<EventKind>, usize)>,
    /// Signalled whenever the gate opens or an append arrives at it.
    changed: Condvar,
}

impl AppendGate {
    /// Wait here when appends of this kind are held.
    fn pass(&self, kind: &EventKind) {
        let mut state = self.state.lock().expect("the gate holds");
        if state.0.as_ref() != Some(kind) {
            return;
        }
        state.1 += 1;
        self.changed.notify_all();
        let (mut state, _) = self
            .changed
            .wait_timeout_while(state, Duration::from_secs(10), |(held, _)| {
                held.as_ref() == Some(kind)
            })
            .expect("the gate holds");
        state.1 -= 1;
    }
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
    /// Where the reads of the prints wait for one another, when armed.
    reads_of_the_prints: Meeting,
    /// Where appends of one kind wait to be let through, when armed.
    appends: AppendGate,
    /// The one print whose appends are refused, and how, when one is.
    refused_print: Mutex<Option<(PrintId, StoreError)>>,
    /// The methods induced to write everything and then answer a failure, as
    /// a store whose commit landed and whose answer was lost does — once each.
    lost_answers: Mutex<BTreeMap<StoreMethod, StoreError>>,
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
            reads_of_the_prints: Meeting::default(),
            appends: AppendGate::default(),
            refused_print: Mutex::new(None),
            lost_answers: Mutex::new(BTreeMap::new()),
        }
    }

    /// Make the next call of one method write everything it was given and then
    /// answer this failure, as a commit whose answer was lost does.
    pub fn loses_the_answer_to(&self, method: StoreMethod, error: StoreError) {
        self.lost_answers
            .lock()
            .expect("the store holds")
            .insert(method, error);
    }

    /// The answer to one write that committed: the failure induced to replace
    /// it, once, or the write's own.
    fn answer_after_commit<T>(
        &self,
        method: StoreMethod,
        written: Result<T, StoreError>,
    ) -> Result<T, StoreError> {
        let lost = self
            .lost_answers
            .lock()
            .expect("the store holds")
            .remove(&method);
        match (written, lost) {
            (Ok(_), Some(error)) => Err(error),
            (written, _) => written,
        }
    }

    /// A write of the ledger, refused outright when a failure is induced at
    /// its method, and answered with a lost answer when one is.
    fn ledger_call<T>(
        &self,
        method: StoreMethod,
        work: impl FnOnce(&mut LedgerWrite<'_>) -> Result<T, StoreError>,
    ) -> Result<T, StoreError> {
        if let Some(error) = self.induced(method) {
            return Err(error);
        }
        let written = self.write_ledger(work);
        self.answer_after_commit(method, written)
    }

    /// Make this many reads of the prints — every print, or the open ones —
    /// wait for one another before any of them answers.
    pub fn reads_of_the_prints_meet(&self, callers: usize) {
        self.reads_of_the_prints.expect(callers);
    }

    /// Hold every append of this kind until [`FakeStore::lets_appends_through`].
    pub fn holds_appends_of(&self, kind: EventKind) {
        self.appends.state.lock().expect("the gate holds").0 = Some(kind);
    }

    /// Wait until an append is held at the gate, or give up.
    ///
    /// # Panics
    ///
    /// Panics when none has arrived inside the deadline.
    pub fn await_a_held_append(&self) {
        let state = self.appends.state.lock().expect("the gate holds");
        let (state, _) = self
            .appends
            .changed
            .wait_timeout_while(state, Duration::from_secs(10), |(_, waiting)| *waiting == 0)
            .expect("the gate holds");
        assert!(state.1 > 0, "no append arrived at the gate");
    }

    /// Let every held append through, and hold none after it.
    pub fn lets_appends_through(&self) {
        self.appends.state.lock().expect("the gate holds").0 = None;
        self.appends.changed.notify_all();
    }

    /// Fail one method with one error from now on.
    pub fn fails(&self, method: StoreMethod, error: StoreError) {
        self.failures
            .lock()
            .expect("the store holds")
            .insert(method, error);
    }

    /// Refuse every append for one print with one error from now on, and
    /// write every other print's.
    pub fn refuses_appends_for(&self, print_id: PrintId, error: StoreError) {
        *self.refused_print.lock().expect("the store holds") = Some((print_id, error));
    }

    /// Stop failing every method.
    pub fn heals(&self) {
        self.failures.lock().expect("the store holds").clear();
        *self.refused_print.lock().expect("the store holds") = None;
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

    /// Every print it holds.
    #[must_use]
    pub fn prints(&self) -> Vec<PrintRecord> {
        self.held
            .lock()
            .expect("the store holds")
            .prints
            .values()
            .cloned()
            .collect()
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

    /// Every print it holds, most recently opened first, as the port declares.
    ///
    /// The fake clock does not move between two opens, so the order they were
    /// opened in is what says which is the more recent.
    fn newest_first(&self) -> Vec<PrintRecord> {
        let held = self.held.lock().expect("the store holds");
        held.opened
            .iter()
            .rev()
            .filter_map(|id| held.prints.get(id).cloned())
            .collect()
    }
}

impl FakeStore {
    /// Run one ledger write over a copy of the ledger, keeping the copy and
    /// the events it appended only when the whole write succeeded.
    fn write_ledger<T>(
        &self,
        work: impl FnOnce(&mut LedgerWrite<'_>) -> Result<T, StoreError>,
    ) -> Result<T, StoreError> {
        let mut held = self.held.lock().expect("the store holds");
        let (answer, ledger, events) = {
            let mut write = LedgerWrite::new(&held.ledger, &held.prints);
            let answer = work(&mut write)?;
            (answer, write.ledger, write.events)
        };
        held.ledger = ledger;
        held.events.extend(events);
        Ok(answer)
    }

    /// Read the ledger.
    fn read_ledger<T>(&self, read: impl FnOnce(&FakeLedger) -> T) -> T {
        read(&self.held.lock().expect("the store holds").ledger)
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

impl PrintStore for FakeStore {
    fn open_print(
        &self,
        provider_print_id: Option<i64>,
        file_name: Option<String>,
    ) -> printobserver_core::store::BoxFuture<'_, Result<PrintRecord, StoreError>> {
        self.journal.record(Call::OpenPrint);
        let record = PrintRecord {
            id: PrintId::new(),
            provider_print_id,
            file_name,
            state: PrinterState::Printing,
            opened_at: self.clock.now(),
            job_started_at: None,
            job_print_time_s: None,
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
    ) -> printobserver_core::store::BoxFuture<'_, Result<Option<PrintRecord>, StoreError>> {
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

    fn print_by_provider_id(
        &self,
        provider_print_id: i64,
    ) -> printobserver_core::store::BoxFuture<'_, Result<Option<PrintRecord>, StoreError>> {
        self.journal.record(Call::ReadPrintByProviderId);
        let found = self
            .held
            .lock()
            .expect("the store holds")
            .prints
            .values()
            .find(|print| print.provider_print_id == Some(provider_print_id))
            .cloned();
        Box::pin(async move { Ok(found) })
    }

    fn open_prints(
        &self,
    ) -> printobserver_core::store::BoxFuture<'_, Result<Vec<PrintRecord>, StoreError>> {
        self.journal.record(Call::ReadOpenPrints);
        if let Some(error) = self.induced(StoreMethod::OpenPrints) {
            return Box::pin(async move { Err(error) });
        }
        let found: Vec<PrintRecord> = self
            .newest_first()
            .into_iter()
            .filter(|print| print.ended_at.is_none())
            .collect();
        self.reads_of_the_prints.arrive();
        Box::pin(async move { Ok(found) })
    }

    fn prints(
        &self,
    ) -> printobserver_core::store::BoxFuture<'_, Result<Vec<PrintRecord>, StoreError>> {
        self.journal.record(Call::ReadPrints);
        let found = self.newest_first();
        self.reads_of_the_prints.arrive();
        Box::pin(async move { Ok(found) })
    }

    fn attach_obico_print(
        &self,
        print_id: PrintId,
        obico_print_id: i64,
    ) -> printobserver_core::store::BoxFuture<'_, Result<PrintRecord, StoreError>> {
        self.journal.record(Call::AttachObicoPrint(obico_print_id));
        let mut held = self.held.lock().expect("the store holds");
        let answer = match held.prints.get_mut(&print_id) {
            None => Err(StoreError::NotFound {
                what: format!("print {print_id}"),
            }),
            Some(print)
                if print
                    .provider_print_id
                    .is_some_and(|id| id != obico_print_id) =>
            {
                Err(StoreError::ConstraintRefused {
                    constraint: "prints.provider_print_id".to_owned(),
                })
            }
            Some(print) => {
                print.provider_print_id = Some(obico_print_id);
                Ok(print.clone())
            }
        };
        drop(held);
        Box::pin(async move { answer })
    }

    fn record_job_sighting(
        &self,
        print_id: PrintId,
        job_started_at: Timestamp,
        job_print_time_s: Option<i64>,
    ) -> printobserver_core::store::BoxFuture<'_, Result<PrintRecord, StoreError>> {
        self.journal.record(Call::RecordJobSighting);
        let mut held = self.held.lock().expect("the store holds");
        let answer = match held.prints.get_mut(&print_id) {
            None => Err(StoreError::NotFound {
                what: format!("print {print_id}"),
            }),
            Some(print) => {
                print.job_started_at = Some(job_started_at);
                print.job_print_time_s = print.job_print_time_s.max(job_print_time_s);
                Ok(print.clone())
            }
        };
        drop(held);
        Box::pin(async move { answer })
    }

    fn end_print(
        &self,
        print_id: PrintId,
        state: PrinterState,
        ended_at: Timestamp,
        reason: String,
    ) -> printobserver_core::store::BoxFuture<'_, Result<PrintRecord, StoreError>> {
        self.journal.record(Call::EndPrint(format!("{state:?}")));
        if let Some(error) = self.induced(StoreMethod::EndPrint) {
            return ready(Err(error));
        }
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
    ) -> printobserver_core::store::BoxFuture<'_, Result<PrintRecord, StoreError>> {
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

    fn put_manifest(
        &self,
        print_id: PrintId,
        manifest: JobManifest,
    ) -> printobserver_core::store::BoxFuture<'_, Result<(), StoreError>> {
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
    ) -> printobserver_core::store::BoxFuture<'_, Result<Option<JobManifest>, StoreError>> {
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
}

impl EventStore for FakeStore {
    fn append_event(
        &self,
        draft: EventDraft,
    ) -> printobserver_core::store::BoxFuture<'_, Result<EventRecord, StoreError>> {
        self.journal.record(Call::AppendEvent(draft.kind().clone()));
        self.appends.pass(draft.kind());
        if let Some(error) = self.induced(StoreMethod::AppendEvent) {
            return ready(Err(error));
        }
        if let Some((_, error)) = self
            .refused_print
            .lock()
            .expect("the store holds")
            .as_ref()
            .filter(|(refused, _)| draft.print_id == Some(*refused))
        {
            return ready(Err(error.clone()));
        }
        let record = EventRecord {
            id: EventId::new(),
            print_id: draft.print_id,
            source: draft.source,
            received_at: draft.received_at,
            image: None,
            body: draft.body,
            raw: draft.raw,
        };
        self.held
            .lock()
            .expect("the store holds")
            .events
            .push(record.clone());
        ready(Ok(record))
    }

    fn history(
        &self,
        query: HistoryQuery,
    ) -> printobserver_core::store::BoxFuture<'_, Result<Vec<EventRecord>, StoreError>> {
        self.journal.record(Call::ReadHistory);
        if let Some(error) = self.induced(StoreMethod::History) {
            return ready(Err(error));
        }
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
            .filter(|event| query.kinds.is_empty() || query.kinds.contains(event.kind()))
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
    ) -> printobserver_core::store::BoxFuture<'_, Result<AuditPage, StoreError>> {
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

    // The supervisor never reads the log in stored order, so neither read is
    // journaled: nothing the supervision tier asserts reaches them.
    fn stored_after(
        &self,
        query: StoredQuery,
    ) -> printobserver_core::store::BoxFuture<'_, Result<Vec<EventRecord>, StoreError>> {
        let limit = match query.resolved_limit() {
            Ok(limit) => limit as usize,
            Err(error) => return ready(Err(error)),
        };
        let held = self.held.lock().expect("the store holds");
        let from = query
            .after
            .and_then(|cursor| held.events.iter().position(|event| event.id == cursor))
            .map_or(0, |index| index + 1);
        let answered = held
            .events
            .iter()
            .skip(from)
            .filter(|event| {
                query
                    .print_id
                    .is_none_or(|print_id| event.print_id.is_none_or(|own| own == print_id))
            })
            .take(limit)
            .cloned()
            .collect();
        ready(Ok(answered))
    }

    fn newest_stored(
        &self,
    ) -> printobserver_core::store::BoxFuture<'_, Result<Option<EventId>, StoreError>> {
        let held = self.held.lock().expect("the store holds");
        ready(Ok(held.events.last().map(|event| event.id)))
    }

    // Nothing in the supervision tier waits on the log.
    fn on_stored(&self, _signal: StoredSignal) {}
}

impl ImageStore for FakeStore {
    fn put_image(
        &self,
        print_id: PrintId,
        event_id: EventId,
        source_url: Option<String>,
        content_type: String,
        bytes: RawBytes,
    ) -> printobserver_core::store::BoxFuture<'_, Result<ImageRecord, StoreError>> {
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
    ) -> printobserver_core::store::BoxFuture<'_, Result<ImageLookup, StoreError>> {
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
}

impl FakeStore {
    /// Record one request and its decision, against the print named or, with
    /// none named, the print most recently opened.
    fn record_bound(
        &self,
        bound: Option<PrintId>,
        request: ActionRequest,
        decision: PolicyDecision,
    ) -> printobserver_core::store::BoxFuture<'_, Result<ActionRecord, StoreError>> {
        self.journal.record(Call::RecordAction(decision.clone()));
        let mut held = self.held.lock().expect("the store holds");
        // With no print named, the record is attributed to the print most
        // recently opened — which is the one active print in every journey
        // that drives an action other than a close.
        let found = match bound {
            Some(named) => held.opened.contains(&named).then_some(named),
            None => held.opened.last().copied(),
        };
        let Some(print_id) = found else {
            drop(held);
            return ready(Err(StoreError::NotFound {
                what: "an open print to record the action against".to_owned(),
            }));
        };
        let record = ActionRecord {
            id: ActionId::new(),
            print_id,
            request,
            decision,
            executed_at: None,
            outcome: None,
        };
        held.actions.insert(record.id, record.clone());
        drop(held);
        if record.decision == PolicyDecision::Accepted {
            self.journal.licence(record.id);
        }
        ready(Ok(record))
    }
}

impl ActionStore for FakeStore {
    fn record_action(
        &self,
        request: ActionRequest,
        decision: PolicyDecision,
    ) -> printobserver_core::store::BoxFuture<'_, Result<ActionRecord, StoreError>> {
        self.record_bound(None, request, decision)
    }

    fn record_action_against(
        &self,
        print_id: PrintId,
        request: ActionRequest,
        decision: PolicyDecision,
    ) -> printobserver_core::store::BoxFuture<'_, Result<ActionRecord, StoreError>> {
        self.record_bound(Some(print_id), request, decision)
    }

    fn record_execution(
        &self,
        action_id: ActionId,
        outcome: ExecutionOutcome,
    ) -> printobserver_core::store::BoxFuture<'_, Result<ActionRecord, StoreError>> {
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
    ) -> printobserver_core::store::BoxFuture<'_, Result<Intervention, StoreError>> {
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
        held.interventions
            .insert(intervention.id, intervention.clone());
        drop(held);
        ready(Ok(intervention))
    }

    fn settle_intervention(
        &self,
        intervention_id: InterventionId,
        outcome: InterventionOutcome,
    ) -> printobserver_core::store::BoxFuture<'_, Result<SettleOutcome, StoreError>> {
        self.journal
            .record(Call::SettleIntervention(outcome.clone()));
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
    ) -> printobserver_core::store::BoxFuture<'_, Result<Vec<Intervention>, StoreError>> {
        if let Some(error) = self.induced(StoreMethod::DueInterventions) {
            return ready(Err(error));
        }
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
    ) -> printobserver_core::store::BoxFuture<'_, Result<Vec<Intervention>, StoreError>> {
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
}

impl SessionStore for FakeStore {
    fn put_session(
        &self,
        session: SupervisionSession,
    ) -> printobserver_core::store::BoxFuture<'_, Result<(), StoreError>> {
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
    ) -> printobserver_core::store::BoxFuture<'_, Result<Option<SupervisionSession>, StoreError>>
    {
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

/// The vision port, which core reaches to retrieve an image and to tell the
/// detector a detection was handled.
pub struct FakeVision {
    /// The shared ordered record of port calls.
    journal: Arc<Journal>,
    /// What it serves.
    image: Mutex<FetchedImage>,
    /// The failure induced at `fetch_image`, if one is.
    failure: Mutex<Option<VisionError>>,
    /// The failure induced at `clear_detection`, if one is.
    clearing_failure: Mutex<Option<VisionError>>,
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
            clearing_failure: Mutex::new(None),
        }
    }

    /// Fail every retrieval from now on.
    pub fn fails(&self, error: VisionError) {
        *self.failure.lock().expect("the vision port holds") = Some(error);
    }

    /// Stop failing every retrieval.
    pub fn heals(&self) {
        *self.failure.lock().expect("the vision port holds") = None;
    }

    /// Refuse every acknowledgement of a detection from now on.
    pub fn refuses_clearing(&self, error: VisionError) {
        *self.clearing_failure.lock().expect("the vision port holds") = Some(error);
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

    fn clear_detection(
        &self,
        detection: printobserver_vision_api::Detection,
    ) -> printobserver_vision_api::BoxFuture<'_, Result<(), VisionError>> {
        self.journal
            .record(Call::ClearDetection(detection.provider_printer_id));
        let answer = self
            .clearing_failure
            .lock()
            .expect("the vision port holds")
            .clone()
            .map_or(Ok(()), Err);
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
    /// Every credential a turn was issued, and who the authority admitted it
    /// as while that turn was still running.
    issued: Mutex<Vec<(String, Option<TurnBinding>)>>,
    /// The authority core opens each turn's issuer from.
    authority: Mutex<Option<Arc<FakeTurnAuthority>>>,
    /// Why each turn that asked for a credential was issued none.
    unissued: Mutex<Vec<String>>,
    /// Whether a turn, once issued its credential, never returns.
    stalls: Mutex<bool>,
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
            issued: Mutex::new(Vec::new()),
            authority: Mutex::new(None),
            unissued: Mutex::new(Vec::new()),
            stalls: Mutex::new(false),
        }
    }

    /// Never return from a turn once it has been issued its credential, so
    /// the only way it ends is by whoever is awaiting it giving up.
    pub fn stalls(&self) {
        *self.stalls.lock().expect("the harness holds") = true;
    }

    /// Why each turn that asked for a credential was issued none, in order.
    #[must_use]
    pub fn unissued(&self) -> Vec<String> {
        self.unissued.lock().expect("the harness holds").clone()
    }

    /// Every credential a turn was issued, in order, and who the authority
    /// admitted it as while that turn was still running.
    #[must_use]
    pub fn issued(&self) -> Vec<(String, Option<TurnBinding>)> {
        self.issued.lock().expect("the harness holds").clone()
    }

    /// Read the authority core opens each turn's issuer from.
    pub fn reads_authority(&self, authority: &Arc<FakeTurnAuthority>) {
        *self.authority.lock().expect("the harness holds") = Some(Arc::clone(authority));
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
        if let Some(open) = sessions.get_mut(&print_id) {
            open.last_turn_at = now;
            (open.clone(), SessionPhase::Continued)
        } else {
            {
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
        access: Arc<dyn printobserver_supervisor_api::TurnAccess>,
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
            // What a run is handed to reach the server with, and who core
            // admits it as while this turn runs.
            let issued = printobserver_supervisor_api::TurnSession::new(format!(
                "print-{}",
                request.print_id
            ))
            .and_then(|session| access.issue(&session));
            if let Err(error) = &issued {
                self.unissued
                    .lock()
                    .expect("the harness holds")
                    .push(error.to_string());
            }
            if let Ok(pass) = issued {
                let credential = pass
                    .environment()
                    .into_iter()
                    .find(|(name, _)| *name == printobserver_supervisor_api::CREDENTIAL_ENV)
                    .map(|(_, value)| value)
                    .unwrap_or_default();
                let admitted = self
                    .authority
                    .lock()
                    .expect("the harness holds")
                    .as_ref()
                    .and_then(|authority| authority.admit(&credential));
                self.issued
                    .lock()
                    .expect("the harness holds")
                    .push((credential, admitted));
            }
            let stalls = *self.stalls.lock().expect("the harness holds");
            if stalls {
                std::future::pending::<()>().await;
            }
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
            let induced = self.failure.lock().expect("the harness holds").clone();
            let answer = if let Some(error) = induced {
                Err(error)
            } else {
                {
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
    use printobserver_printer_api::{
        FAN_PERCENT_RANGE, FEEDRATE_FACTOR_RANGE, FLOWRATE_FACTOR_RANGE, HEATER_ACTUAL_C_RANGE,
        HEATER_OFFSET_C_RANGE, HEATER_TARGET_C_RANGE, HeaterSnapshot,
    };
    use printobserver_types::Reported;
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
    use printobserver_printer_api::COMPLETION_RANGE;
    use printobserver_types::Reported;
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
        file_position_bytes: None,
        filament: Vec::new(),
    }
}

/// Who one turn credential authenticates as.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TurnBinding {
    /// The session the run that holds it is in.
    pub session_name: String,
    /// The print the turn is about.
    pub print_id: PrintId,
}

/// An authority that mints numbered credentials and remembers which are live.
///
/// What core is responsible for is when an issuer is opened and when it is
/// revoked; how a credential is drawn is the composition root's, and its own
/// tier holds that. So these are numbered rather than random.
#[derive(Debug, Default)]
pub struct FakeTurnAuthority {
    /// Every live credential, and which turn issued it.
    live: Mutex<BTreeMap<String, (u64, TurnBinding)>>,
    /// The number the next credential or turn takes.
    next: Mutex<u64>,
}

impl FakeTurnAuthority {
    /// Who one credential authenticates as, while it is live.
    #[must_use]
    pub fn admit(&self, credential: &str) -> Option<TurnBinding> {
        self.live
            .lock()
            .expect("the authority holds")
            .get(credential)
            .map(|(_, binding)| binding.clone())
    }

    /// How many credentials are live.
    #[must_use]
    pub fn live(&self) -> usize {
        self.live.lock().expect("the authority holds").len()
    }

    /// The next number.
    fn number(&self) -> u64 {
        let mut next = self.next.lock().expect("the authority holds");
        *next += 1;
        *next
    }
}

/// One turn's issuer, over the fake authority.
struct FakeTurnScope {
    /// The authority it issues into.
    authority: Arc<FakeTurnAuthority>,
    /// Which turn this is.
    turn: u64,
    /// The print the turn is about.
    print_id: PrintId,
}

impl printobserver_supervisor_api::TurnAccess for FakeTurnScope {
    fn issue(
        &self,
        session: &printobserver_supervisor_api::TurnSession,
    ) -> Result<printobserver_supervisor_api::TurnPass, SupervisorError> {
        let credential = format!("turn-credential-{}", self.authority.number());
        let mut live = self.authority.live.lock().expect("the authority holds");
        live.retain(|_, (turn, _)| *turn != self.turn);
        live.insert(
            credential.clone(),
            (
                self.turn,
                TurnBinding {
                    session_name: session.to_string(),
                    print_id: self.print_id,
                },
            ),
        );
        printobserver_supervisor_api::TurnPass::new(None, credential)
    }
}

/// Opens one issuer per turn, over the one authority.
#[derive(Debug)]
pub struct OpensTurns(pub Arc<FakeTurnAuthority>);

impl printobserver_core::TurnAuthority for OpensTurns {
    fn open(&self, print_id: PrintId) -> printobserver_core::OpenedTurn {
        let turn = self.0.number();
        let authority = Arc::clone(&self.0);
        printobserver_core::OpenedTurn::new(
            Arc::new(FakeTurnScope {
                authority: Arc::clone(&self.0),
                turn,
                print_id,
            }),
            Box::new(move || {
                authority
                    .live
                    .lock()
                    .expect("the authority holds")
                    .retain(|_, (issued_by, _)| *issued_by != turn);
            }),
        )
    }
}

impl FilamentStore for FakeStore {
    fn register_spool(
        &self,
        spool: NewSpool,
        source: EventSource,
        actor: Actor,
        reason: String,
    ) -> printobserver_core::store::BoxFuture<'_, Result<(SpoolRecord, EventRecord), StoreError>>
    {
        ready(self.write_ledger(|write| write.register(spool, source, actor, reason)))
    }

    fn apply_spool_changes(
        &self,
        mutations: Vec<SpoolMutation>,
        charges: Vec<Charge>,
        pends: Vec<Pend>,
    ) -> printobserver_core::store::BoxFuture<'_, Result<SpoolWrite, StoreError>> {
        ready(self.ledger_call(StoreMethod::ApplySpoolChanges, |write| {
            write.apply(mutations, charges, pends)
        }))
    }

    fn settle_print(
        &self,
        print_id: PrintId,
        outcome: PrintOutcome,
        ended: EventDraft,
        charges: Vec<Charge>,
        pends: Vec<Pend>,
        marks: Vec<SpoolMutation>,
    ) -> printobserver_core::store::BoxFuture<'_, Result<Settlement, StoreError>> {
        ready(self.ledger_call(StoreMethod::SettlePrint, |write| {
            write.settle(print_id, outcome, ended, charges, pends, marks)
        }))
    }

    fn print_settlement(
        &self,
        print_id: PrintId,
    ) -> printobserver_core::store::BoxFuture<
        '_,
        Result<Option<(Timestamp, PrintOutcome)>, StoreError>,
    > {
        ready(Ok(self.read_ledger(|ledger| ledger.settlement(print_id))))
    }

    fn charges(
        &self,
        print_id: PrintId,
    ) -> printobserver_core::store::BoxFuture<'_, Result<Vec<ChargeRecord>, StoreError>> {
        ready(Ok(self.read_ledger(|ledger| ledger.charges(print_id))))
    }

    fn pending_charges(
        &self,
        tool: Option<i64>,
    ) -> printobserver_core::store::BoxFuture<'_, Result<Vec<PendingCharge>, StoreError>> {
        ready(Ok(self.read_ledger(|ledger| ledger.pending(tool))))
    }

    fn spool(
        &self,
        id: SpoolId,
    ) -> printobserver_core::store::BoxFuture<'_, Result<Option<SpoolRecord>, StoreError>> {
        ready(Ok(self.read_ledger(|ledger| ledger.spool(id))))
    }

    fn spools(
        &self,
        include_retired: bool,
    ) -> printobserver_core::store::BoxFuture<'_, Result<Vec<SpoolRecord>, StoreError>> {
        ready(Ok(self.read_ledger(|ledger| ledger.spools(include_retired))))
    }

    fn loaded_spools(
        &self,
    ) -> printobserver_core::store::BoxFuture<'_, Result<Vec<SpoolRecord>, StoreError>> {
        ready(Ok(self.read_ledger(FakeLedger::loaded_spools)))
    }

    fn record_file_position(
        &self,
        print_id: PrintId,
        position: FilePosition,
    ) -> printobserver_core::store::BoxFuture<'_, Result<(), StoreError>> {
        if let Some(error) = self.induced(StoreMethod::RecordFilePosition) {
            return ready(Err(error));
        }
        let mut held = self.held.lock().expect("the store holds");
        if !held.prints.contains_key(&print_id) {
            return ready(Err(StoreError::NotFound {
                what: format!("print {print_id}"),
            }));
        }
        held.ledger.record_position(print_id, position);
        ready(Ok(()))
    }

    fn file_position(
        &self,
        print_id: PrintId,
    ) -> printobserver_core::store::BoxFuture<'_, Result<Option<FilePosition>, StoreError>> {
        ready(Ok(self.read_ledger(|ledger| ledger.position(print_id))))
    }

    fn file_tallies(
        &self,
    ) -> printobserver_core::store::BoxFuture<'_, Result<Vec<FileTally>, StoreError>> {
        ready(Ok(self.read_ledger(FakeLedger::tallies)))
    }

    fn account_tallies(
        &self,
        tallies: Vec<FileTally>,
        charges: Vec<Charge>,
        marks: Vec<SpoolMutation>,
        events: Vec<EventDraft>,
    ) -> printobserver_core::store::BoxFuture<'_, Result<Vec<EventRecord>, StoreError>> {
        ready(self.ledger_call(StoreMethod::AccountTallies, |write| {
            write.account(&tallies, charges, marks, events)
        }))
    }
}

impl TimelineStore for FakeStore {
    fn timeline(
        &self,
        query: TimelineQuery,
    ) -> printobserver_core::store::BoxFuture<'_, Result<Vec<EventRecord>, StoreError>> {
        let held = self.held.lock().expect("the store holds");
        ready(timeline(&held.events, &query))
    }
}
