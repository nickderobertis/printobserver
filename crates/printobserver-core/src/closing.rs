//! Closing a print record printobserver left open, at an operator's word.
//!
//! A record a read never saw end stays open, and every later read is then
//! about it: a preflight warns of it and an action can be filed under it. A
//! close is the operator's judgement that it is over, so it is a print action
//! like any other — requested, decided, executed and recorded on the one path
//! [`crate::actions`] describes — that asks nothing of the machine.
//!
//! **It is refused while the record might still be live.** A record that has
//! ended is refused as one; a printer showing a job of the record's file
//! printing, paused or cancelling refuses it naming that state; and an
//! intervention for the print that has neither expired nor been restored
//! refuses it naming the intervention, because closing would strand a change
//! nothing would ever put back. A printer that cannot be read refuses nothing:
//! a stale record is most often left by a printer that is off.
//!
//! **It charges nothing.** The record's consumption was never seen to end, so
//! the close settles the print with no charge and no debit, and the history
//! keeps its `print_ended` with the outcome the operator gave.

use printobserver_printer_api::{JobSnapshot, PrinterState};
use printobserver_types::{EventBody, EventPayload as _, PrintId};

use printobserver_supervisor_api::{SupervisionSession, SupervisionSessionClosedPayload};

use crate::error::CoreError;
use crate::kinds::{PrintEndedPayload, system_source};
use crate::records::{PolicyDecision, PrintAction, PrintOutcome, PrintRecord, RejectionReason};
use crate::store::HistoryQuery;
use crate::supervisor::Supervisor;

/// What every operator close's recorded end reason begins with.
pub const CLOSED_BY_THE_OPERATOR: &str = "closed by the operator: ";

/// The end reason one operator close records, from the reason it was given.
#[must_use]
pub fn closing_reason(reason: &str) -> String {
    format!("{CLOSED_BY_THE_OPERATOR}{reason}")
}

/// Whether one job read shows the record's own print still under way.
///
/// A record that names no file cannot be told apart from whatever job the
/// printer holds, so any job printing, paused or cancelling holds it open.
fn still_active(print: &PrintRecord, job: &JobSnapshot) -> bool {
    let under_way = matches!(
        job.state,
        PrinterState::Printing | PrinterState::Paused | PrinterState::Cancelling
    );
    under_way
        && print
            .file_name
            .as_deref()
            .is_none_or(|file| job.file_name.as_deref() == Some(file))
}

impl Supervisor {
    /// Check a close the policy accepted, replacing the decision with the
    /// check's refusal when it refuses one, and answer the state the print
    /// ends in when it is still accepted. Nothing for any other request.
    pub(crate) async fn check_accepted_close(
        &self,
        action: &PrintAction,
        print: Option<&PrintRecord>,
        decision: &mut PolicyDecision,
    ) -> Result<Option<PrinterState>, CoreError> {
        let (PrintAction::ClosePrint { .. }, Some(print), PolicyDecision::Accepted) =
            (action, print, &decision)
        else {
            return Ok(None);
        };
        Ok(match self.check_close(print).await? {
            Ok(state) => Some(state),
            Err(refusal) => {
                *decision = PolicyDecision::Rejected(refusal);
                None
            }
        })
    }

    /// Check an accepted close against the printer and the print's
    /// interventions, and answer the state the print ends in when it is not
    /// refused.
    ///
    /// The state is the printer's from this read, or the record's last known
    /// one when the printer cannot be read.
    pub(crate) async fn check_close(
        &self,
        print: &PrintRecord,
    ) -> Result<Result<PrinterState, RejectionReason>, CoreError> {
        let state = match self.read_job().await {
            Ok(job) if still_active(print, &job) => {
                return Ok(Err(RejectionReason::PrintStillActive { state: job.state }));
            }
            Ok(job) => job.state,
            Err(_) => print.state.clone(),
        };
        let mut standing = self.stores().actions.active_interventions(print.id).await?;
        standing.sort_by_key(|held| held.applied_at);
        if let Some(held) = standing.first() {
            return Ok(Err(RejectionReason::InterventionStillActive {
                intervention_id: held.id,
            }));
        }
        Ok(Ok(state))
    }

    /// Close the record an accepted, executed close names, in the state the
    /// close's own check found; any other action closes nothing.
    pub(crate) async fn follow_the_close(
        &self,
        print_id: PrintId,
        action: &PrintAction,
        closing_state: Option<&PrinterState>,
    ) -> Result<(), CoreError> {
        if let (
            PrintAction::ClosePrint {
                outcome, reason, ..
            },
            Some(state),
        ) = (action, closing_state)
        {
            self.close_by_operator(print_id, *outcome, state, reason)
                .await?;
        }
        Ok(())
    }

    /// Close one print record at an operator's word, with no charge.
    ///
    /// The print is settled first — no charge, no pend, no mark, and its
    /// `print_ended` — then its supervision session is closed, and the record
    /// is ended last. A failure between any two leaves a settled, open print
    /// whose next close writes only what is missing and ends it.
    pub(crate) async fn close_by_operator(
        &self,
        print_id: PrintId,
        outcome: PrintOutcome,
        state: &PrinterState,
        reason: &str,
    ) -> Result<(), CoreError> {
        let closing = closing_reason(reason);
        self.forget_detector_pause(print_id);
        let last_position = self.stores().filament.file_position(print_id).await?;
        let ended = self.draft_of(
            Some(print_id),
            system_source(),
            &PrintEndedPayload {
                outcome,
                state: state.clone(),
                reason: closing.clone(),
                last_position,
            },
        )?;
        {
            let _writing = self.spool_writes().await;
            self.stores()
                .filament
                .settle_print(print_id, outcome, ended, Vec::new(), Vec::new(), Vec::new())
                .await?;
        }
        // The session closes before the record ends, so a close that fails
        // part-way leaves the record open for the next close to finish.
        self.close_session_of(print_id, &closing).await?;
        self.stores()
            .prints
            .end_print(print_id, state.clone(), self.clock().now(), closing)
            .await?;
        Ok(())
    }

    /// Close one print's supervision session — in the harness, in the record
    /// of it the store holds, and in the print's history — when the print had
    /// one.
    ///
    /// Each half is written once however often this is asked: the store's
    /// record only while it is open, and the history's
    /// `supervision_session_closed` only while the history carries none. So a
    /// close retried after one of them failed writes the other and nothing
    /// twice.
    pub(crate) async fn close_session_of(
        &self,
        print_id: PrintId,
        reason: &str,
    ) -> Result<(), CoreError> {
        let _ = self
            .agent()
            .close_session(print_id, reason.to_owned())
            .await;
        let Some(session) = self.stores().sessions.session(print_id).await? else {
            return Ok(());
        };
        let session_name = session.session_name.clone();
        if session.closed_at.is_none() {
            self.stores()
                .sessions
                .put_session(SupervisionSession {
                    closed_at: Some(self.clock().now()),
                    close_reason: Some(reason.to_owned()),
                    ..session
                })
                .await?;
        }
        let written = self
            .stores()
            .events
            .history(HistoryQuery {
                print_id,
                kinds: vec![SupervisionSessionClosedPayload::kind()],
                since: None,
                until: None,
                limit: Some(1),
            })
            .await?;
        if written.is_empty() {
            self.append_system_event(
                print_id,
                EventBody::of(&SupervisionSessionClosedPayload {
                    session_name,
                    close_reason: reason.to_owned(),
                })?,
            )
            .await?;
        }
        Ok(())
    }
}
