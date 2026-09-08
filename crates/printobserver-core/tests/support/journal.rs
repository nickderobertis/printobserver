//! The ordered record of every port call the fakes received.
//!
//! Ordering is part of what this crate owes — the event append precedes every
//! other port call for that event, an accepted adjustment reads the snapshot
//! before it acts — so the fakes record the calls they receive in one shared
//! log rather than each keeping its own.
//!
//! The log is also where the **execution** half of the chokepoint's proof
//! lives. Every accepted decision the store records licenses exactly one action
//! call at the printer; an action call arriving with no licence is a violation,
//! recorded here and reported by [`Journal::assert_no_violations`]. That
//! assertion covers every path any test drives, whatever a source check can
//! recognize — an alias, a re-export or a dispatch it cannot resolve included.

use std::sync::Mutex;

use printobserver_types::{
    ActionId, Adjustable, EventKind, ExecutionOutcome, InterventionOutcome, PolicyDecision, PrintId,
};

/// One call a fake port received.
#[derive(Debug, Clone, PartialEq)]
pub enum Call {
    /// A print was opened.
    OpenPrint,
    /// A print was read.
    ReadPrint,
    /// A print was read by Obico's own identifier.
    ReadPrintByObicoId,
    /// A print was ended, in a state.
    EndPrint(String),
    /// A manifest narrowing was recorded.
    RecordNarrowing(Adjustable),
    /// An event was appended, of a kind.
    AppendEvent(EventKind),
    /// An image's bytes were stored.
    PutImage,
    /// An image was looked up.
    ReadImage,
    /// A request and the decision on it were recorded.
    RecordAction(PolicyDecision),
    /// What the printer made of an accepted action was recorded.
    RecordExecution(ExecutionOutcome),
    /// A bounded intervention was opened.
    OpenIntervention(Adjustable),
    /// A bounded intervention was settled.
    SettleIntervention(InterventionOutcome),
    /// A print's active interventions were read.
    ReadActiveInterventions,
    /// A manifest was stored.
    PutManifest,
    /// A manifest was read.
    ReadManifest,
    /// A print's history was read.
    ReadHistory,
    /// A page of a print's audit history was read.
    ReadAuditPage,
    /// A session was stored.
    PutSession,
    /// A session was read.
    ReadSession,
    /// The printer's state was read.
    Snapshot,
    /// The job the printer reports was read.
    Job,
    /// A print of a named file was started.
    Start(String),
    /// The print was paused.
    Pause,
    /// The print was resumed.
    Resume,
    /// The print was cancelled.
    Cancel,
    /// The feedrate factor was set.
    SetFeedrateFactor(f64),
    /// The flowrate factor was set.
    SetFlowrateFactor(f64),
    /// A tool's target temperature was set.
    SetToolTargetC(i64, f64),
    /// The bed's target temperature was set.
    SetBedTargetC(f64),
    /// The fan percentage was set.
    SetFanPercent(f64),
    /// A received body was normalized.
    Normalize,
    /// An image was retrieved.
    FetchImage,
    /// One supervision turn was run for a print.
    RunTurn(PrintId),
    /// A print's supervision session was closed.
    CloseSession(PrintId, String),
}

/// Which port a call was made at.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Port {
    /// The store.
    Store,
    /// The printer.
    Printer,
    /// The vision port.
    Vision,
    /// The supervising agent's harness.
    Supervisor,
}

impl Call {
    /// Which port this call was made at.
    #[must_use]
    pub const fn port(&self) -> Port {
        match self {
            Self::OpenPrint
            | Self::ReadPrint
            | Self::ReadPrintByObicoId
            | Self::EndPrint(_)
            | Self::RecordNarrowing(_)
            | Self::AppendEvent(_)
            | Self::PutImage
            | Self::ReadImage
            | Self::RecordAction(_)
            | Self::RecordExecution(_)
            | Self::OpenIntervention(_)
            | Self::SettleIntervention(_)
            | Self::ReadActiveInterventions
            | Self::PutManifest
            | Self::ReadManifest
            | Self::ReadHistory
            | Self::ReadAuditPage
            | Self::PutSession
            | Self::ReadSession => Port::Store,
            Self::Snapshot
            | Self::Job
            | Self::Start(_)
            | Self::Pause
            | Self::Resume
            | Self::Cancel
            | Self::SetFeedrateFactor(_)
            | Self::SetFlowrateFactor(_)
            | Self::SetToolTargetC(_, _)
            | Self::SetBedTargetC(_)
            | Self::SetFanPercent(_) => Port::Printer,
            Self::Normalize | Self::FetchImage => Port::Vision,
            Self::RunTurn(_) | Self::CloseSession(_, _) => Port::Supervisor,
        }
    }

    /// Whether this is a call to an **action** method of the printer port.
    ///
    /// The port's two reads, `snapshot` and `job`, are deliberately not
    /// actions: the loop reads the printer without taking a decision, so an
    /// absence that covered them would be one no conforming implementation
    /// could satisfy.
    #[must_use]
    pub const fn is_printer_action(&self) -> bool {
        matches!(self.port(), Port::Printer) && !matches!(self, Self::Snapshot | Self::Job)
    }

    /// Whether this call writes to the store.
    #[must_use]
    pub const fn is_store_write(&self) -> bool {
        matches!(
            self,
            Self::OpenPrint
                | Self::EndPrint(_)
                | Self::RecordNarrowing(_)
                | Self::AppendEvent(_)
                | Self::PutImage
                | Self::RecordAction(_)
                | Self::RecordExecution(_)
                | Self::OpenIntervention(_)
                | Self::SettleIntervention(_)
                | Self::PutManifest
                | Self::PutSession
        )
    }
}

/// The ordered record of every port call, and the licences that gate actions.
#[derive(Debug, Default)]
pub struct Journal {
    /// Every call, in the order the fakes received them.
    calls: Mutex<Vec<Call>>,
    /// Accepted decisions recorded and not yet spent by an action call.
    licences: Mutex<Vec<ActionId>>,
    /// Every action call that arrived with no decision recorded before it.
    violations: Mutex<Vec<String>>,
}

impl Journal {
    /// Record one call.
    pub fn record(&self, call: Call) {
        self.calls.lock().expect("the journal holds").push(call);
    }

    /// Record that a decision was accepted, licensing one action call.
    pub fn licence(&self, action_id: ActionId) {
        self.licences
            .lock()
            .expect("the journal holds")
            .push(action_id);
    }

    /// Spend a licence for one action call, or record a violation.
    pub fn spend_licence(&self, what: &str) {
        if self
            .licences
            .lock()
            .expect("the journal holds")
            .pop()
            .is_none()
        {
            self.violations
                .lock()
                .expect("the journal holds")
                .push(format!(
                    "{what} reached the printer with no policy decision recorded for it"
                ));
        }
    }

    /// Every call, in order.
    #[must_use]
    pub fn calls(&self) -> Vec<Call> {
        self.calls.lock().expect("the journal holds").clone()
    }

    /// Every call at one port, in order.
    #[must_use]
    pub fn at(&self, port: Port) -> Vec<Call> {
        self.calls()
            .into_iter()
            .filter(|call| call.port() == port)
            .collect()
    }

    /// Every action call the printer received, in order.
    #[must_use]
    pub fn printer_actions(&self) -> Vec<Call> {
        self.calls()
            .into_iter()
            .filter(Call::is_printer_action)
            .collect()
    }

    /// Every write the store received, in order.
    #[must_use]
    pub fn store_writes(&self) -> Vec<Call> {
        self.calls().into_iter().filter(Call::is_store_write).collect()
    }

    /// Where one call first appears, if it does.
    #[must_use]
    pub fn position(&self, call: &Call) -> Option<usize> {
        self.calls().iter().position(|seen| seen == call)
    }

    /// Forget every call recorded so far, keeping the licences.
    pub fn clear(&self) {
        self.calls.lock().expect("the journal holds").clear();
    }

    /// Every violation recorded so far.
    #[must_use]
    pub fn violations(&self) -> Vec<String> {
        self.violations.lock().expect("the journal holds").clone()
    }

    /// Fail the test when any action call arrived with no decision before it.
    ///
    /// # Panics
    ///
    /// Panics naming every such call, which is the property this assertion
    /// exists to make load-bearing.
    pub fn assert_no_violations(&self) {
        let violations = self.violations();
        assert!(
            violations.is_empty(),
            "the printer was acted on without a recorded decision: {violations:?}"
        );
    }
}
