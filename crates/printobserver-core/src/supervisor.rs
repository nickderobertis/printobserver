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
//! [`PrintAction`](printobserver_types::PrintAction) and the printer.
//!
//! The port's two reads, `snapshot` and `job`, are outside the rule for that
//! same reason, and are reached through [`Supervisor::read_snapshot`] and
//! [`Supervisor::read_job`].
//!
//! # Nothing here sends a command string to a printer
//!
//! There is no method on the printer port that admits one, and the only
//! caller-supplied text that reaches the machine's own file API is the
//! [`FileName`](printobserver_types::FileName) newtype, which refuses anything
//! but a name.

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex, Weak};

use printobserver_printer_api::{PrinterError, PrinterPort};
use printobserver_store_api::{StoreError, StorePort};
use printobserver_supervisor_api::SupervisorPort;
use printobserver_types::{
    ActionRecord, ActionRequest, ExecutionOutcome, JobSnapshot, PolicyDecision, PrintAction,
    PrintContext, PrintId, PrinterSnapshot, Timestamp,
};
use printobserver_vision_api::VisionPort;

use crate::block_on::block_on;
use crate::clock::Clock;
use crate::config::CoreConfig;
use crate::turn_lock::TurnLocks;

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
    /// Durable state.
    store: Arc<dyn StorePort>,
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
    /// When the agent last acted on each print.
    last_agent_action: Mutex<BTreeMap<PrintId, Timestamp>>,
    /// The context collected for the turn currently running on each print.
    pending_context: Mutex<BTreeMap<PrintId, PrintContext>>,
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
    /// Build a supervisor over the four ports, and start its expiry driver.
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
        store: Arc<dyn StorePort>,
        vision: Arc<dyn VisionPort>,
        agent: Arc<dyn SupervisorPort>,
        clock: Arc<dyn Clock>,
    ) -> Arc<Self> {
        let poll = config.expiry_poll;
        let supervisor = Arc::new(Self {
            printer,
            store,
            vision,
            agent,
            clock,
            config,
            turns: TurnLocks::default(),
            last_agent_action: Mutex::new(BTreeMap::new()),
            pending_context: Mutex::new(BTreeMap::new()),
        });
        let weak: Weak<Self> = Arc::downgrade(&supervisor);
        std::thread::Builder::new()
            .name("printobserver-expiry".to_owned())
            .spawn(move || {
                while let Some(supervisor) = weak.upgrade() {
                    let _ = block_on(supervisor.sweep_expired());
                    drop(supervisor);
                    std::thread::sleep(poll);
                }
            })
            .expect("the expiry driver thread starts");
        supervisor
    }

    /// Durable state.
    pub(crate) fn store(&self) -> &Arc<dyn StorePort> {
        &self.store
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

    /// Record that the agent acted on one print at an instant.
    pub(crate) fn note_agent_action(&self, print_id: PrintId, at: Timestamp) {
        self.last_agent_action
            .lock()
            .expect("the agent's own record is not poisoned")
            .insert(print_id, at);
    }

    /// When the agent last acted on one print, if it has.
    pub(crate) fn last_agent_action(&self, print_id: PrintId) -> Option<Timestamp> {
        self.last_agent_action
            .lock()
            .expect("the agent's own record is not poisoned")
            .get(&print_id)
            .copied()
    }

    /// Hold the context collected for the turn now running on one print.
    pub(crate) fn hold_context(&self, print_id: PrintId, context: PrintContext) {
        self.pending_context
            .lock()
            .expect("the held context is not poisoned")
            .insert(print_id, context);
    }

    /// Drop the context held for one print, once its turn has returned.
    pub(crate) fn drop_context(&self, print_id: PrintId) {
        self.pending_context
            .lock()
            .expect("the held context is not poisoned")
            .remove(&print_id);
    }

    /// The context held for the turn now running on one print, if one is.
    pub(crate) fn held_context(&self, print_id: PrintId) -> Option<PrintContext> {
        self.pending_context
            .lock()
            .expect("the held context is not poisoned")
            .get(&print_id)
            .cloned()
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

    /// Record the decision for one request, and — if it was accepted — issue it.
    ///
    /// **This is the one function of this crate that calls an action method of
    /// the printer port.** Every action, from every actor — the agent, an
    /// operator, the command line, a client — arrives here, and the decision is
    /// recorded before anything reaches the machine. A rejected request reaches
    /// no action method at all: the record of the rejection is the only thing
    /// that happens.
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
    ) -> Result<Issued, StoreError> {
        let record = self
            .store
            .record_action(request.clone(), decision.clone())
            .await?;
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
            // Acknowledging a failure is a decision about the print rather than
            // a movement of the machine. `Stop` is the one disposition that
            // asks for something at the printer, and cancelling is how a print
            // is stopped; the other two ask for nothing there.
            PrintAction::AcknowledgeFailure { disposition, .. } => {
                if matches!(
                    disposition,
                    printobserver_types::AcknowledgementDisposition::Stop
                ) {
                    self.printer.cancel().await
                } else {
                    Ok(())
                }
            }
        };
        let outcome = match &executed {
            Ok(()) => ExecutionOutcome::Succeeded,
            Err(error) => ExecutionOutcome::Failed {
                reason: error.to_string(),
            },
        };
        let record = self.store.record_execution(record.id, outcome).await?;
        Ok(Issued {
            record,
            executed: Some(executed),
        })
    }
}
