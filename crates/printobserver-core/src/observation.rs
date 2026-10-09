//! A periodic observation of every active print, which no detector asked for.
//!
//! A failure detector alerts on what it was trained to see, and a print can go
//! wrong in ways that stay below every one of its thresholds: firmware pausing
//! a print and cutting its heater while the host goes on reporting it printing,
//! a fan cut at a bridge, a wall going thin. Nothing in the machine's state or
//! the camera's frame is hidden from an agent that looks — so every interval
//! ([`CoreConfig::observation_interval`](crate::CoreConfig::observation_interval))
//! each active print is written a [`PeriodicObservationPayload`] event carrying
//! the printer's telemetry, with the camera's frame as its image, and the turn
//! that event claims is that print's agent looking at it.
//!
//! # Which prints are observed
//!
//! The printer's job is read and settled exactly as the listing's is
//! ([`crate::listing`]): a job found over closes its prints, and a running job
//! no open print records is adopted. When that read succeeds the print the job
//! is — when it is one — is the one active print; a printer whose job cannot be
//! read leaves every open print a candidate, because nothing then says which
//! of them stopped. A printer reporting one of
//! [`TERMINAL_STATES`](crate::TERMINAL_STATES) is not printing anything, and an
//! ended print has nothing left to act on, so neither is observed.
//!
//! # No pile-up
//!
//! An observation claims a print's turn only when nothing holds it
//! ([`Inboxes::claim_if_idle`](crate::Inboxes::claim_if_idle)). A print whose
//! turn is running — on an alert, or on the last observation — is being
//! watched already, and is given no observation to queue behind it; so at most
//! one observation per print is ever outstanding, and every turn the agent is
//! given still runs under the same one-agent-per-print rule and the same action
//! policy, `safety.agent_min_interval_s` included.
//!
//! # The reads precede the append
//!
//! Everywhere else the event append is the first thing the loop does
//! ([`crate::events`]), because an alert arrives from outside and must be in
//! the history before anything can fail on its behalf. An observation is
//! raised here, and what it records *is* the reads, so they are taken first and
//! the event carries them; a read that failed is then recorded against the
//! event as a port failure, as the loop records one, and the camera is asked
//! for its frame only once the event is there to record a refusal against.

use printobserver_printer_api::{JobSnapshot, PrinterError, PrinterSnapshot};
use printobserver_types::EventBody;

use crate::error::CoreError;
use crate::events::{PendingTurn, TERMINAL_STATES};
use crate::inbox::Arrival;
use crate::kinds::{PeriodicObservationPayload, PortFailureSite};
use crate::records::PrintRecord;
use crate::supervisor::Supervisor;

impl Supervisor {
    /// Observe every active print nobody is watching, and answer the turns
    /// those observations claimed, to be run by
    /// [`Supervisor::run_supervision`].
    ///
    /// # Errors
    ///
    /// Returns the store's own error when the open prints could not be read or
    /// settled, or an observation could not be written down. An observation
    /// that could not be written down claims no turn.
    pub async fn observe_active_prints(&self) -> Result<Vec<PendingTurn>, CoreError> {
        let mut turns = Vec::new();
        for print in self.prints_to_observe().await? {
            if let Some(turn) = self.observe(print).await? {
                turns.push(turn);
            }
        }
        Ok(turns)
    }

    /// The open prints an observation is owed to, once the printer's job has
    /// settled them.
    async fn prints_to_observe(&self) -> Result<Vec<PrintRecord>, CoreError> {
        let resolving = self.resolving().await;
        let active = match self.observe_job().await {
            Some(observed) => Some(self.settle_and_adopt(&observed).await?),
            None => None,
        };
        drop(resolving);
        let open = self.stores().prints.open_prints().await?;
        Ok(match active {
            Some(running) => open
                .into_iter()
                .filter(|print| Some(print.id) == running)
                .collect(),
            None => open,
        })
    }

    /// Observe one print, when it is printing and nobody is watching it.
    async fn observe(&self, print: PrintRecord) -> Result<Option<PendingTurn>, CoreError> {
        let printer = self.read_snapshot().await;
        if printer
            .as_ref()
            .is_ok_and(|snapshot| TERMINAL_STATES.contains(&snapshot.connection))
        {
            return Ok(None);
        }
        let job = self.read_job().await;
        if !self.inboxes().claim_if_idle(print.id) {
            return Ok(None);
        }
        match self.write_observation_down(&print, printer, job).await {
            Ok(arrival) => Ok(Some(PendingTurn::new(print, arrival, Vec::new()))),
            Err(error) => {
                // Anything handed to the print while it was claimed is owed a
                // turn of its own, and is answered as one rather than dropped.
                let Some(mut left) = self.inboxes().next_or_release(print.id) else {
                    return Err(error);
                };
                let latest = left.pop().ok_or(error)?;
                Ok(Some(PendingTurn::new(print, latest, left)))
            }
        }
    }

    /// Write one observation down, recording a read that failed against it,
    /// and take the camera's frame as its image.
    async fn write_observation_down(
        &self,
        print: &PrintRecord,
        printer: Result<PrinterSnapshot, PrinterError>,
        job: Result<JobSnapshot, PrinterError>,
    ) -> Result<Arrival, CoreError> {
        let payload = PeriodicObservationPayload {
            interval_s: self.config().observation_interval.as_secs(),
            printer: printer.as_ref().ok().cloned(),
            job: job.as_ref().ok().cloned(),
        };
        let mut event = self
            .append_system_event(print.id, EventBody::of(&payload)?)
            .await?;
        if let Err(error) = printer {
            self.record_port_failure(
                print.id,
                event.id,
                PortFailureSite::PrinterSnapshot,
                error.to_string(),
            )
            .await;
        }
        if let Err(error) = job {
            self.record_port_failure(
                print.id,
                event.id,
                PortFailureSite::PrinterJob,
                error.to_string(),
            )
            .await;
        }
        if let Some(url) = &self.config().camera_snapshot_url {
            event.image = self
                .take_frame(print.id, &event, url.as_str().to_owned())
                .await;
        }
        Ok(Arrival {
            event,
            detection: None,
        })
    }
}
