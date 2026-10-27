//! The supervisor, and the one call site every action passes through.
//!
//! # This module is the chokepoint
//!
//! Every action this crate takes at the printer port is issued from
//! [`Supervisor::issue_decided_action`] and from nowhere else, and that
//! function records the policy decision for the request **before** it issues
//! the call. That is structural rather than a convention, and two properties of
//! this module are what make it so:
//!
//! 1. **No other item of this crate names an action method.** A check over this
//!    crate's own sources enumerates every syntactic reference to an action
//!    method of the printer port — a call, a path taken as a value, a function
//!    pointer, an alias — and refuses one outside that function.
//! 2. **No item outside this module can obtain the handle to call one on.** The
//!    printer handle is declared once, as a private field here, and the
//!    identifier `PrinterPort` appears nowhere else in this crate. Nothing
//!    hands it out: no function anywhere returns it.
//!
//! **The decision gates actions, not port traffic.** Appending an event,
//! writing its image, reading a snapshot, running a supervision turn and
//! persisting its assessment are all port calls, none of them is an action
//! anyone requested, and none of them takes a decision — the event append in
//! particular is the *first* thing the loop does, before any decision could
//! exist. What the policy stands between is a requested
//! [`PrintAction`](crate::records::PrintAction) and the printer.
//!
//! The port's reads — `snapshot`, `job` and the three reads of the files the
//! printer holds — are outside the rule for that same reason, and are reached
//! through [`Supervisor::read_snapshot`], [`Supervisor::read_job`],
//! [`Supervisor::read_file_facts`], [`Supervisor::read_printed_files`] and
//! [`Supervisor::read_extruded_to`].
//!
//! # Nothing here sends a command string to a printer
//!
//! There is no method on the printer port that admits one, and the only
//! caller-supplied text that reaches the machine's own file API is the
//! [`FileName`](printobserver_types::FileName) newtype, which refuses anything
//! but a name.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::{Arc, Mutex, MutexGuard, Weak};
use std::time::Duration;

use crate::records::{
    ActionId, ActionRecord, ActionRequest, ActorClass, ExecutionOutcome, PolicyDecision,
    PrintAction,
};
use printobserver_printer_api::{
    FileFacts, JobSnapshot, PrinterError, PrinterPort, PrinterSnapshot, ToolFilament,
};
use printobserver_supervisor_api::SupervisorPort;
use printobserver_types::{EventPayload as _, EventRecord, FileName, PrintId, Timestamp};
use printobserver_vision_api::VisionPort;

use crate::store::{HistoryQuery, MAX_HISTORY_LIMIT, StoreError, Stores};

use crate::block_on::block_on;
use crate::clock::{Clock, ClockReading};
use crate::config::CoreConfig;
use crate::decision::changes_the_machine;
use crate::detector::DetectorPauses;
use crate::error::CoreError;
use crate::expiry::Deadlines;
use crate::inbox::Inboxes;
use crate::kinds::{ActionExecutedPayload, ActionRequestedPayload};
use crate::turn_access::{NoAccess, OpenedTurn, TurnAuthority};
use crate::turn_lock::TurnLocks;
use crate::watch::WatchState;

/// What became of one request that reached [`Supervisor::issue_decided_action`].
#[derive(Debug, Clone, PartialEq)]
pub struct Issued {
    /// The record of the request and the decision taken on it.
    pub record: ActionRecord,
    /// What the printer made of it, absent when the request was rejected.
    pub executed: Option<Result<(), PrinterError>>,
}

impl Issued {
    /// Whether the request was accepted and the printer took it.
    #[must_use]
    pub fn succeeded(&self) -> bool {
        matches!(self.executed, Some(Ok(())))
    }

    /// Why the printer refused it, when it reached the printer and was refused.
    #[must_use]
    pub fn printer_error(&self) -> Option<&PrinterError> {
        match &self.executed {
            Some(Err(error)) => Some(error),
            _ => None,
        }
    }
}

/// The supervision core: one printer, one store, one agent, one clock.
///
/// Built with [`Supervisor::new`], which answers a shared handle because the
/// expiry driver holds a weak reference to it: a bounded intervention expires
/// with nothing asking it to, and the driver is the thread that makes that so.
pub struct Supervisor {
    /// The printer. Private to this module, and named nowhere else.
    printer: Arc<dyn PrinterPort>,
    /// Durable state, one handle per aggregate.
    stores: Stores,
    /// External observations.
    vision: Arc<dyn VisionPort>,
    /// The supervising agent's harness.
    agent: Arc<dyn SupervisorPort>,
    /// The one time source.
    clock: Arc<dyn Clock>,
    /// The server configuration.
    config: CoreConfig,
    /// One supervision turn per print at a time.
    turns: TurnLocks,
    /// One resolution of which print a job or an alert is at a time.
    ///
    /// Finding the print the printer's job belongs to and finding the print an
    /// alert belongs to each read the open prints and may then write one, so
    /// two of them interleaved could each find none and each open one. They
    /// take this lock, under [`Supervisor::resolving_key`], for as long as that
    /// read and write take.
    resolving: TurnLocks,
    /// The one key [`Supervisor::resolving`] is held under.
    resolving_key: PrintId,
    /// One expiry of a print's interventions at a time.
    ///
    /// The expiry driver's sweep, a print's close-out and a start's
    /// reconciliation can each reach one intervention, and its restoration is
    /// issued before its outcome is settled, so two expiries interleaved would
    /// each put the value back.
    expiring: TurnLocks,
    /// One write to the spool ledger at a time.
    ///
    /// Every spool command, every settlement, every filament-change signal and
    /// every accounting of the printer's tallies reads the spools it charges
    /// and then writes them; two of them interleaved would each compute from
    /// the same figure and one of the two would be lost. They take this lock,
    /// under [`Supervisor::resolving_key`], for as long as that read and write
    /// take, and never take another lock while they hold it.
    spool_writes: TurnLocks,
    /// The prints this supervisor has itself seen printing or paused, or
    /// started: a print it closes out that is not among them ended while
    /// nothing was watching it.
    seen_running: Mutex<BTreeSet<PrintId>>,
    /// When, on the clock nothing steps, the agent last changed the machine
    /// on each print: absent until it is known, and `None` once it is known
    /// the agent never has.
    last_agent_action: Mutex<BTreeMap<PrintId, Option<AgentAction>>>,
    /// Both of the clock's readings as this supervisor started, which every
    /// instant a previous supervisor recorded is translated against once.
    started: ClockReading,
    /// The active interventions being counted down.
    deadlines: Deadlines,
    /// What reaches each print's running turn, and which prints have one.
    inboxes: Inboxes,
    /// The prints the detector paused and nobody has resumed yet.
    detector_pauses: Mutex<DetectorPauses>,
    /// Where each supervision turn's runs take their credentials from, once
    /// the composition root has installed it.
    turn_authority: std::sync::OnceLock<Arc<dyn TurnAuthority>>,
    /// What the printer watch holds between its reads.
    pub(crate) watch: Mutex<WatchState>,
}

/// When the agent last changed the machine on one print, as this supervisor
/// measures it.
#[derive(Debug, Clone, Copy)]
struct AgentAction {
    /// The reading of the clock nothing steps this supervisor counts on from.
    seen_at: Duration,
    /// How long before `seen_at` the change was made: nothing for one this
    /// supervisor made, and for one a previous supervisor recorded, how long
    /// it had been waited already as this one started.
    already: Duration,
}

impl core::fmt::Debug for Supervisor {
    fn fmt(&self, formatter: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        formatter
            .debug_struct("Supervisor")
            .field("config", &self.config)
            .finish_non_exhaustive()
    }
}

impl Supervisor {
    /// Build a supervisor over the three ports and the stores, and start its
    /// expiry driver.
    ///
    /// The driver is a thread of core's own holding a weak reference, so it
    /// stops of its own accord when the last handle to the supervisor is
    /// dropped. It reads the injected clock and nothing else, which is what
    /// makes a bounded intervention expire with nothing asking it to.
    ///
    /// # Panics
    ///
    /// Panics when the operating system refuses to start that thread, which is
    /// a supervisor whose bounded interventions would never expire — the one
    /// state this crate must not come up in quietly.
    #[must_use]
    pub fn new(
        config: CoreConfig,
        printer: Arc<dyn PrinterPort>,
        stores: Stores,
        vision: Arc<dyn VisionPort>,
        agent: Arc<dyn SupervisorPort>,
        clock: Arc<dyn Clock>,
    ) -> Arc<Self> {
        let poll = config.expiry_poll;
        let started = ClockReading::of(clock.as_ref());
        let supervisor = Arc::new(Self {
            printer,
            stores,
            vision,
            agent,
            clock,
            config,
            turns: TurnLocks::default(),
            resolving: TurnLocks::default(),
            resolving_key: PrintId::new(),
            expiring: TurnLocks::default(),
            spool_writes: TurnLocks::default(),
            seen_running: Mutex::new(BTreeSet::new()),
            last_agent_action: Mutex::new(BTreeMap::new()),
            started,
            deadlines: Deadlines::default(),
            inboxes: Inboxes::default(),
            detector_pauses: Mutex::new(BTreeMap::new()),
            turn_authority: std::sync::OnceLock::new(),
            watch: Mutex::new(WatchState::default()),
        });
        let weak: Weak<Self> = Arc::downgrade(&supervisor);
        std::thread::Builder::new()
            .name("printobserver-expiry".to_owned())
            .spawn(move || {
                while let Some(supervisor) = weak.upgrade() {
                    let _ = block_on(supervisor.sweep_expired());
                    block_on(supervisor.resume_due_detector_pauses());
                    drop(supervisor);
                    std::thread::sleep(poll);
                }
            })
            .expect("the expiry driver thread starts");
        supervisor
    }

    /// Install where every supervision turn's runs take their credentials
    /// from. The first installation stands; answers whether this one did.
    pub fn install_turn_authority(&self, authority: Arc<dyn TurnAuthority>) -> bool {
        self.turn_authority.set(authority).is_ok()
    }

    /// The issuer for one turn about one print, opened now.
    pub(crate) fn open_turn(&self, print_id: PrintId) -> OpenedTurn {
        self.turn_authority
            .get()
            .map_or_else(NoAccess::opened, |authority| authority.open(print_id))
    }

    /// Durable state, one handle per aggregate.
    pub(crate) const fn stores(&self) -> &Stores {
        &self.stores
    }

    /// External observations.
    pub(crate) fn vision(&self) -> &Arc<dyn VisionPort> {
        &self.vision
    }

    /// The supervising agent's harness.
    pub(crate) fn agent(&self) -> &Arc<dyn SupervisorPort> {
        &self.agent
    }

    /// The one time source.
    pub(crate) fn clock(&self) -> &Arc<dyn Clock> {
        &self.clock
    }

    /// The server configuration this supervisor runs under.
    #[must_use]
    pub const fn config(&self) -> &CoreConfig {
        &self.config
    }

    /// One supervision turn per print at a time.
    pub(crate) const fn turns(&self) -> &TurnLocks {
        &self.turns
    }

    /// One expiry of a print's interventions at a time.
    pub(crate) const fn expiring(&self) -> &TurnLocks {
        &self.expiring
    }

    /// Both of the clock's readings as this supervisor started.
    pub(crate) const fn started(&self) -> ClockReading {
        self.started
    }

    /// The active interventions being counted down.
    pub(crate) const fn deadlines(&self) -> &Deadlines {
        &self.deadlines
    }

    /// What reaches each print's running turn.
    pub(crate) const fn inboxes(&self) -> &Inboxes {
        &self.inboxes
    }

    /// The prints the detector paused and nobody has resumed yet.
    pub(crate) fn detector_pauses(&self) -> MutexGuard<'_, DetectorPauses> {
        self.detector_pauses
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    /// Wait until no other resolution of a print is running, then hold it.
    pub(crate) async fn resolving(&self) -> crate::turn_lock::TurnGuard<'_> {
        self.resolving.acquire(self.resolving_key).await
    }

    /// Wait until no other write to the spool ledger is running, then hold it.
    pub(crate) async fn spool_writes(&self) -> crate::turn_lock::TurnGuard<'_> {
        self.spool_writes.acquire(self.resolving_key).await
    }

    /// Record that this supervisor saw one print's job printing or paused.
    pub(crate) fn note_seen_running(&self, print_id: PrintId) {
        self.seen_running
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .insert(print_id);
    }

    /// Whether this supervisor saw one print's job printing or paused.
    pub(crate) fn was_seen_running(&self, print_id: PrintId) -> bool {
        self.seen_running
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .contains(&print_id)
    }

    /// Record that the agent changed the machine on one print at a reading of
    /// the clock nothing steps.
    pub(crate) fn note_agent_action(&self, print_id: PrintId, at: Duration) {
        self.last_agents_actions().insert(
            print_id,
            Some(AgentAction {
                seen_at: at,
                already: Duration::ZERO,
            }),
        );
    }

    /// How long ago, on the clock nothing steps, the agent last changed the
    /// machine on one print, if it has.
    ///
    /// A print this supervisor has not seen the agent act on is read from its
    /// history once: the newest change the agent made that reached the machine,
    /// translated against the reading this supervisor took as it started, so
    /// that what is left of the interval when a supervisor restarts is waited
    /// out on the clock nothing steps. The time it had already been waited at
    /// that reading is kept as a span of its own rather than as a reading of
    /// that clock, whose origin may be later than the change: a host's
    /// monotonic clock starts near zero with the process. A change recorded
    /// after that reading — a wall clock stepped back across the restart — is
    /// taken as made the moment this supervisor started, which waits the whole
    /// interval rather than longer.
    ///
    /// # Errors
    ///
    /// Returns the store's own error when the print's history could not be
    /// read, which the next request reads again.
    pub(crate) async fn since_last_agent_action(
        &self,
        print_id: PrintId,
    ) -> Result<Option<Duration>, CoreError> {
        let known = self.last_agents_actions().get(&print_id).copied();
        let last = if let Some(last) = known {
            last
        } else {
            let started = self.started;
            let recorded = self
                .recorded_agent_action(print_id)
                .await?
                .map(|at| AgentAction {
                    seen_at: started.elapsed,
                    already: started
                        .wall
                        .as_utc()
                        .signed_duration_since(*at.as_utc())
                        .to_std()
                        .unwrap_or_default(),
                });
            *self
                .last_agents_actions()
                .entry(print_id)
                .or_insert(recorded)
        };
        Ok(last.map(|action| {
            self.clock()
                .elapsed()
                .saturating_sub(action.seen_at)
                .saturating_add(action.already)
        }))
    }

    /// When the newest change the agent made to the machine on one print was
    /// recorded, if its history holds one.
    async fn recorded_agent_action(
        &self,
        print_id: PrintId,
    ) -> Result<Option<Timestamp>, CoreError> {
        let history = self
            .stores()
            .events
            .history(HistoryQuery {
                print_id,
                kinds: vec![
                    ActionRequestedPayload::kind(),
                    ActionExecutedPayload::kind(),
                ],
                since: None,
                until: None,
                limit: Some(MAX_HISTORY_LIMIT),
            })
            .await?;
        let executed: BTreeSet<ActionId> = history
            .iter()
            .filter_map(EventRecord::payload_as::<ActionExecutedPayload>)
            .filter_map(Result::ok)
            .map(|payload| payload.action_id)
            .collect();
        // Newest first, so the first change found is the newest.
        Ok(history.iter().find_map(|event| {
            let requested = event.payload_as::<ActionRequestedPayload>()?.ok()?;
            (executed.contains(&requested.action_id)
                && requested.actor.class() == ActorClass::Agent
                && changes_the_machine(requested.action.kind()))
            .then_some(event.received_at)
        }))
    }

    /// The agent's last change on each print, held for one read or write.
    fn last_agents_actions(&self) -> MutexGuard<'_, BTreeMap<PrintId, Option<AgentAction>>> {
        self.last_agent_action
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    /// Read the printer's current state.
    ///
    /// A read rather than an action: it changes nothing at the machine, so it
    /// takes no policy decision and is outside the chokepoint's rule.
    pub(crate) async fn read_snapshot(&self) -> Result<PrinterSnapshot, PrinterError> {
        self.printer.snapshot().await
    }

    /// Read the job the printer reports it is running.
    ///
    /// A read, for the same reason [`Supervisor::read_snapshot`] is.
    pub(crate) async fn read_job(&self) -> Result<JobSnapshot, PrinterError> {
        self.printer.job().await
    }

    /// Read what the printer knows of one file it holds.
    ///
    /// A read, for the same reason [`Supervisor::read_snapshot`] is.
    pub(crate) async fn read_file_facts(
        &self,
        file_name: FileName,
    ) -> Result<FileFacts, PrinterError> {
        self.printer.file_facts(file_name).await
    }

    /// Read every file the printer holds that carries a print tally.
    ///
    /// A read, for the same reason [`Supervisor::read_snapshot`] is.
    pub(crate) async fn read_printed_files(&self) -> Result<Vec<FileFacts>, PrinterError> {
        self.printer.printed_files().await
    }

    /// Read the filament each tool is fed by one file's own moves, up to a
    /// byte position in it.
    ///
    /// A read, for the same reason [`Supervisor::read_snapshot`] is.
    pub(crate) async fn read_extruded_to(
        &self,
        file_name: FileName,
        position_bytes: i64,
    ) -> Result<Vec<ToolFilament>, PrinterError> {
        self.printer.extruded_to(file_name, position_bytes).await
    }

    /// Record the decision for one request, and — if it was accepted — issue it.
    ///
    /// **This is the one function of this crate that calls an action method of
    /// the printer port.** Every action, from every actor — the agent, an
    /// operator, the command line, a client — arrives here, and the decision is
    /// recorded before anything reaches the machine. A rejected request reaches
    /// no action method at all: the record of the rejection is the only thing
    /// that happens.
    ///
    /// `against` is the print the request was asked of, when it was asked of
    /// one.
    ///
    /// # Errors
    ///
    /// Returns the store's own error when the decision or the execution could
    /// not be recorded. A failure at the printer is not an error here: it is
    /// recorded as [`ExecutionOutcome::Failed`] and answered in [`Issued`], so
    /// that a caller sees a request that was made and refused rather than a
    /// request that never happened.
    pub(crate) async fn issue_decided_action(
        &self,
        request: ActionRequest,
        decision: PolicyDecision,
        against: Option<PrintId>,
    ) -> Result<Issued, StoreError> {
        // A close is asked of the record it names, which may have ended or may
        // not be the newest open one, so it is recorded against that record;
        // every other request binds where the store binds it.
        let actions = &self.stores.actions;
        let record = match (&request.action, against) {
            (PrintAction::ClosePrint { .. }, Some(print_id)) => {
                actions
                    .record_action_against(print_id, request.clone(), decision.clone())
                    .await?
            }
            _ => {
                actions
                    .record_action(request.clone(), decision.clone())
                    .await?
            }
        };
        if decision != PolicyDecision::Accepted {
            return Ok(Issued {
                record,
                executed: None,
            });
        }
        let executed = match &request.action {
            PrintAction::Pause { .. } => self.printer.pause().await,
            PrintAction::Resume { .. } => self.printer.resume().await,
            PrintAction::Cancel { .. } => self.printer.cancel().await,
            PrintAction::StartPrint { file_name, .. } => {
                self.printer.start(file_name.clone()).await
            }
            PrintAction::SetFeedrateFactor { factor, .. } => {
                self.printer.set_feedrate_factor(*factor).await
            }
            PrintAction::SetFlowrateFactor { factor, .. } => {
                self.printer.set_flowrate_factor(*factor).await
            }
            PrintAction::SetToolTargetC { tool, target_c, .. } => {
                self.printer.set_tool_target_c(*tool, *target_c).await
            }
            PrintAction::SetBedTargetC { target_c, .. } => {
                self.printer.set_bed_target_c(*target_c).await
            }
            PrintAction::SetFanPercent { percent, .. } => {
                self.printer.set_fan_percent(*percent).await
            }
            // Acknowledging a failure is a decision about the print written into
            // its record, and asks nothing of the machine whatever the
            // disposition. Stopping a print is cancelling it, which the policy
            // grants each actor on its own: an acknowledgement that cancelled
            // would let an actor granted acknowledgements and not cancellation
            // stop a print all the same. Closing a record writes that record
            // and asks nothing of the machine either: what it does is done once
            // its execution is recorded ([`crate::closing`]).
            PrintAction::AcknowledgeFailure { .. } | PrintAction::ClosePrint { .. } => Ok(()),
        };
        let outcome = match &executed {
            Ok(()) => ExecutionOutcome::Succeeded,
            Err(error) => ExecutionOutcome::Failed {
                reason: error.to_string(),
            },
        };
        let record = self
            .stores
            .actions
            .record_execution(record.id, outcome)
            .await?;
        Ok(Issued {
            record,
            executed: Some(executed),
        })
    }
}
