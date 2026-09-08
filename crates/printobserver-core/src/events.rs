//! The loop: one normalized event becomes one supervised turn.
//!
//! The order is the contract. **The store's event append is the first thing the
//! loop does**, before the image is written, before the printer is read and
//! before a supervision turn is run, so that a failure of any of those is
//! recorded against an event that is already in the history. The one failure
//! that cannot be covered that way is the append itself: the store is where the
//! record lives, so when the append fails there is nowhere to record it. The
//! loop then calls no other port for that event, answers the store's own error
//! to its caller rather than swallowing it, and goes on to handle the next
//! event. Losing the alert that way is an availability failure; acting on the
//! printer with no record behind it would be a safety one, and this crate takes
//! the first over the second.
//!
//! # The context a turn reads is the one the loop collected
//!
//! A turn reads its print's context by running the command
//! [`CoreConfig::context_command_for`](crate::CoreConfig::context_command_for)
//! names, and that command reads it back through [`Supervisor::context`]. While
//! a turn is running, that call answers the context the loop collected for the
//! event that prompted it rather than a fresh read: what the agent reasons
//! about is the state the event was handled at, not whatever the machine has
//! drifted to since.

use printobserver_store_api::HistoryQuery;
use printobserver_supervisor_api::TurnRequest;
use printobserver_types::{
    EventPayload, EventRecord, EventSource, ImageRef, PortFailurePayload, PortFailureSite,
    PrintContext, PrintId, PrintRecord, PrinterState, SessionPhase,
    SupervisionSessionClosedPayload, SupervisionSessionOpenedPayload,
};
use printobserver_vision_api::NormalizedAlert;

use crate::error::CoreError;
use crate::supervisor::Supervisor;

/// The states a print does not carry on from.
///
/// Read off the printer's own reported state rather than inferred from a
/// notification's wording: the machine is the authority on whether it is still
/// printing.
pub const TERMINAL_STATES: [PrinterState; 3] = [
    PrinterState::Operational,
    PrinterState::Error,
    PrinterState::Offline,
];

/// Obico's own identifier for the print an alert is about, when it names one.
fn obico_print_id(payload: &EventPayload) -> Option<i64> {
    match payload {
        EventPayload::ObicoFailureAlert(alert) => alert.obico_print_id,
        EventPayload::ObicoPrinterNotification(notification) => notification.obico_print_id,
        _ => None,
    }
}

/// The file an alert names, when it names one.
fn alert_file_name(payload: &EventPayload) -> Option<String> {
    match payload {
        EventPayload::ObicoFailureAlert(alert) => alert.file_name.clone(),
        EventPayload::ObicoPrinterNotification(notification) => notification.file_name.clone(),
        _ => None,
    }
}

impl Supervisor {
    /// Handle one normalized event, from the store's own append to the turn.
    ///
    /// # Errors
    ///
    /// Returns the store's own error when the **initial event append** fails,
    /// which is the one failure this loop cannot record anywhere; no other port
    /// is called for that event, and a later event whose append succeeds is
    /// handled to completion. Every other port failure this loop reaches is
    /// recorded against the event rather than answered here.
    pub async fn handle_event(&self, alert: NormalizedAlert) -> Result<EventRecord, CoreError> {
        let print = self.resolve_print(&alert).await?;
        let draft = printobserver_store_api::EventDraft {
            print_id: print.as_ref().map(|record| record.id),
            source: alert.source,
            received_at: alert.received_at,
            payload: alert.payload.clone(),
            raw: Some(alert.raw.clone()),
        };
        let event = self
            .store()
            .append_event(draft)
            .await
            .map_err(CoreError::Store)?;
        let Some(print) = print else {
            return Ok(event);
        };
        let image = self
            .write_image(&print, &event, alert.image_url.clone())
            .await;
        let terminal = self.supervise(&print, &event, image).await?;
        if let Some(state) = terminal {
            self.close_out_print(&print, &state).await?;
        }
        Ok(event)
    }

    /// The print an alert belongs to, opened if this system has not seen it.
    async fn resolve_print(
        &self,
        alert: &NormalizedAlert,
    ) -> Result<Option<PrintRecord>, CoreError> {
        let Some(obico_print_id) = obico_print_id(&alert.payload) else {
            return Ok(None);
        };
        if let Some(found) = self.store().print_by_obico_id(obico_print_id).await? {
            return Ok(Some(found));
        }
        let opened = self
            .store()
            .open_print(Some(obico_print_id), alert_file_name(&alert.payload))
            .await?;
        Ok(Some(opened))
    }

    /// Fetch and store the image an alert names, recording either failure.
    async fn write_image(
        &self,
        print: &PrintRecord,
        event: &EventRecord,
        image_url: Option<String>,
    ) -> Option<ImageRef> {
        let source_url = image_url?;
        let fetched = match self.vision().fetch_image(source_url.clone()).await {
            Ok(fetched) => fetched,
            Err(error) => {
                self.record_port_failure(
                    print.id,
                    event.id,
                    PortFailureSite::ImageWrite,
                    error.to_string(),
                )
                .await;
                return None;
            }
        };
        match self
            .store()
            .put_image(
                print.id,
                event.id,
                Some(source_url),
                fetched.content_type,
                fetched.bytes,
            )
            .await
        {
            Ok(record) => Some(ImageRef {
                id: record.id,
                sha256: record.sha256,
            }),
            Err(error) => {
                self.record_port_failure(
                    print.id,
                    event.id,
                    PortFailureSite::ImageWrite,
                    error.to_string(),
                )
                .await;
                None
            }
        }
    }

    /// Collect the context, run one turn on it, and persist what it answered.
    ///
    /// Answers the terminal state the printer reported, when it reported one.
    async fn supervise(
        &self,
        print: &PrintRecord,
        event: &EventRecord,
        image: Option<ImageRef>,
    ) -> Result<Option<PrinterState>, CoreError> {
        let guard = self.turns().acquire(print.id).await;
        let context = self.gather_context(print, Some(event.id)).await?;
        let terminal = context
            .printer
            .as_ref()
            .map(|snapshot| snapshot.connection.clone())
            .filter(|state| TERMINAL_STATES.contains(state));
        let image_path = match &image {
            Some(reference) => self.image_path(reference).await,
            None => None,
        };
        let mut carried = event.clone();
        carried.image = image.or(carried.image);
        self.hold_context(print.id, context);
        let turn = self
            .agent()
            .run_turn(TurnRequest {
                print_id: print.id,
                event: carried,
                image_path,
                context_command: self.config().context_command_for(print.id),
            })
            .await;
        self.drop_context(print.id);
        drop(guard);
        match turn {
            Ok(outcome) => {
                if outcome.phase == SessionPhase::Created {
                    self.append_system_event(
                        print.id,
                        EventPayload::SupervisionSessionOpened(SupervisionSessionOpenedPayload {
                            session_name: outcome.session.session_name.clone(),
                            harness_identity: outcome.session.harness_identity.clone(),
                        }),
                    )
                    .await?;
                }
                let session_name = outcome.session.session_name.clone();
                self.store().put_session(outcome.session).await?;
                self.append_system_event(
                    print.id,
                    EventPayload::AgentAssessment(printobserver_types::AgentAssessmentPayload {
                        session_name,
                        assessment: outcome.assessment,
                    }),
                )
                .await?;
            }
            Err(error) => {
                self.record_port_failure(
                    print.id,
                    event.id,
                    PortFailureSite::SupervisionTurn,
                    error.to_string(),
                )
                .await;
            }
        }
        Ok(terminal)
    }

    /// Everything a supervision turn is given about one print.
    ///
    /// While a turn is running for the print, this answers the context the loop
    /// collected for the event that prompted it.
    ///
    /// # Errors
    ///
    /// Returns [`CoreError::NoSuchPrint`] when nothing is held under that
    /// identifier, and the store's own error when the context could not be
    /// read.
    pub async fn context(&self, print_id: PrintId) -> Result<PrintContext, CoreError> {
        if let Some(held) = self.held_context(print_id) {
            return Ok(held);
        }
        let print = self
            .store()
            .print(print_id)
            .await?
            .ok_or(CoreError::NoSuchPrint { print_id })?;
        self.gather_context(&print, None).await
    }

    /// Read one print's whole context, recording a printer read that fails.
    ///
    /// A read failure is recorded against the event being handled when there is
    /// one; a context read that no event prompted has nowhere to record it and
    /// carries the absence in the context itself, which is what an absent
    /// `printer` or `job` means.
    async fn gather_context(
        &self,
        print: &PrintRecord,
        event_id: Option<printobserver_types::EventId>,
    ) -> Result<PrintContext, CoreError> {
        let printer = match self.read_snapshot().await {
            Ok(snapshot) => Some(snapshot),
            Err(error) => {
                self.record_read_failure(
                    print.id,
                    event_id,
                    PortFailureSite::PrinterSnapshot,
                    error.to_string(),
                )
                .await;
                None
            }
        };
        let job = match self.read_job().await {
            Ok(job) => Some(job),
            Err(error) => {
                self.record_read_failure(
                    print.id,
                    event_id,
                    PortFailureSite::PrinterJob,
                    error.to_string(),
                )
                .await;
                None
            }
        };
        let manifest = self.store().manifest(print.id).await?;
        let bounds = self.bounds_for(Some(print), print.id).await?;
        let interventions = self.store().active_interventions(print.id).await?;
        let recent_events = self
            .store()
            .history(HistoryQuery {
                print_id: print.id,
                kinds: Vec::new(),
                since: None,
                until: None,
                limit: Some(self.config().recent_events),
            })
            .await?;
        let latest_image = recent_events.iter().find_map(|record| record.image.clone());
        Ok(PrintContext {
            print: print.clone(),
            printer,
            job,
            manifest,
            bounds: bounds.effective,
            interventions,
            recent_events,
            latest_image,
        })
    }

    /// Where one image's bytes are, when the store still has them.
    async fn image_path(&self, image: &ImageRef) -> Option<std::path::PathBuf> {
        match self.store().image(image.id).await {
            Ok(printobserver_store_api::ImageLookup::Found { path, .. }) => Some(path),
            _ => None,
        }
    }

    /// End the print, expire what it had running, and close its session.
    ///
    /// The interventions are expired **before** the print is ended, because a
    /// restoration is an action against the print and a print that has already
    /// ended is one no action may be taken against.
    async fn close_out_print(
        &self,
        print: &PrintRecord,
        state: &PrinterState,
    ) -> Result<(), CoreError> {
        let reason = format!("the print reached {state:?}");
        self.expire_all_active(print.id).await?;
        self.store()
            .end_print(print.id, state.clone(), self.clock().now(), reason.clone())
            .await?;
        let _ = self.agent().close_session(print.id, reason.clone()).await;
        if let Some(session) = self.store().session(print.id).await? {
            self.append_system_event(
                print.id,
                EventPayload::SupervisionSessionClosed(SupervisionSessionClosedPayload {
                    session_name: session.session_name,
                    close_reason: reason,
                }),
            )
            .await?;
        }
        Ok(())
    }

    /// Append one event this system raised itself.
    async fn append_system_event(
        &self,
        print_id: PrintId,
        payload: EventPayload,
    ) -> Result<EventRecord, CoreError> {
        Ok(self
            .store()
            .append_event(printobserver_store_api::EventDraft {
                print_id: Some(print_id),
                source: EventSource::System,
                received_at: self.clock().now(),
                payload,
                raw: None,
            })
            .await?)
    }

    /// Record a printer read that failed, when an event prompted the read.
    async fn record_read_failure(
        &self,
        print_id: PrintId,
        event_id: Option<printobserver_types::EventId>,
        site: PortFailureSite,
        detail: String,
    ) {
        if let Some(event_id) = event_id {
            self.record_port_failure(print_id, event_id, site, detail)
                .await;
        }
    }

    /// Record that a port failed while one event was being handled.
    ///
    /// The event is already in the history by the time any of these sites is
    /// reached, which is what gives the failure somewhere to be recorded.
    /// Nothing is answered to the caller: the loop survives this and goes on.
    async fn record_port_failure(
        &self,
        print_id: PrintId,
        event_id: printobserver_types::EventId,
        site: PortFailureSite,
        detail: String,
    ) {
        let _ = self
            .append_system_event(
                print_id,
                EventPayload::PortFailure(PortFailurePayload {
                    event_id,
                    site,
                    detail,
                }),
            )
            .await;
    }
}
