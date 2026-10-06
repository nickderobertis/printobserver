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
//! drifted to since. A look ([`crate::look`]) is how it sees the machine as it
//! is now.
//!
//! # One agent per print
//!
//! [`Supervisor::receive_event`] writes an event down and claims its print's
//! turn, or — when a turn is already running for that print — hands the event
//! to that turn through the print's inbox ([`crate::inbox`]), where its next
//! look takes it. [`Supervisor::run_supervision`] runs a claimed turn and then
//! one more, in the same session, for whatever arrived during it that it never
//! took. A print that has ended starts no turn at all, and its inbox is
//! released with whatever was still waiting in it.

use crate::records::PrintRecord;
use crate::store::{EventDraft, HistoryQuery, ImageLookup};
use printobserver_printer_api::PrinterState;
use printobserver_supervisor_api::SessionPhase;
use printobserver_supervisor_api::{
    SupervisionSessionClosedPayload, SupervisionSessionOpenedPayload, TurnRequest, TurnSituation,
};
use printobserver_types::{EventBody, EventRecord, ImageRef, PrintId};
use printobserver_vision_api::NormalizedAlert;

use crate::context::PrintContext;
use crate::error::CoreError;
use crate::inbox::Arrival;
use crate::kinds::{AgentAssessmentPayload, PortFailurePayload, PortFailureSite, system_source};
use crate::listing::{is_over, reached_reason};
use crate::supervisor::Supervisor;

/// One event written down, and the turn it claimed, when it claimed one.
#[derive(Debug, Clone, PartialEq)]
pub struct Received {
    /// The event, as the store holds it.
    pub event: EventRecord,
    /// The turn it claimed: absent when it belongs to no print, to a print that
    /// has ended, or was handed to the turn already running for its print.
    pub turn: Option<PendingTurn>,
}

/// What became of one turn.
enum Supervised {
    /// It ran, and the printer was not in a terminal state.
    Ran,
    /// It ran, and the printer reported this terminal state.
    Terminal(PrinterState),
    /// The print had ended by the time the turn held it, so nothing ran.
    Ended,
}

/// A turn one event claimed, to be run by [`Supervisor::run_supervision`].
#[derive(Debug, Clone, PartialEq)]
pub struct PendingTurn {
    /// The print the turn is about.
    print: PrintRecord,
    /// The event that claimed it.
    arrival: Arrival,
}

/// One printer state in the printer contract's own spelling.
fn state_name(state: &PrinterState) -> Option<String> {
    printobserver_types::serde_json::to_value(state)
        .ok()
        .and_then(|value| value.as_str().map(str::to_owned))
}

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
        let received = self.receive_event(alert).await?;
        if let Some(turn) = received.turn {
            self.run_supervision(turn).await?;
        }
        Ok(received.event)
    }

    /// Write one normalized event down, and say whether it needs a turn of
    /// its own.
    ///
    /// The first half of [`Supervisor::handle_event`], for a caller that runs
    /// the turn somewhere else so that the next event can be written down
    /// while this one's turn is still running. An event for a print whose turn
    /// is already running is handed to that turn and answers no turn of its
    /// own: one agent per print, and what arrives reaches the agent already
    /// watching it.
    ///
    /// # Errors
    ///
    /// Exactly as [`Supervisor::handle_event`]'s.
    pub async fn receive_event(&self, alert: NormalizedAlert) -> Result<Received, CoreError> {
        let print = self.resolve_print(&alert).await?;
        let draft = EventDraft {
            print_id: print.as_ref().map(|record| record.id),
            source: alert.source.clone(),
            received_at: alert.received_at,
            body: alert.body.clone(),
            raw: Some(alert.raw.clone()),
        };
        let event = self
            .stores()
            .events
            .append_event(draft)
            .await
            .map_err(CoreError::Store)?;
        let Some(print) = print else {
            return Ok(Received { event, turn: None });
        };
        let image = self
            .write_image(&print, &event, alert.image_url.clone())
            .await;
        if print.ended_at.is_some() {
            return Ok(Received { event, turn: None });
        }
        let mut carried = event.clone();
        carried.image = image.or(carried.image);
        if let Some(detection) = alert.detection
            && detection.paused_the_print
        {
            self.note_detector_pause(print.id, event.id, detection);
        }
        let arrival = Arrival {
            event: carried,
            detection: alert.detection,
        };
        let turn = self
            .inboxes()
            .claim_or_hand_over(print.id, arrival.clone())
            .then_some(PendingTurn { print, arrival });
        Ok(Received { event, turn })
    }

    /// Run the turn one received event claimed, and one more for whatever
    /// arrived during it that it never took, until nothing is left.
    ///
    /// The detector's pause the agent adjusted the print under is resumed as
    /// each turn ends, when its grace has not already resumed it.
    ///
    /// # Errors
    ///
    /// Returns the store's own error when a turn's outcome could not be
    /// recorded; a turn that fails is recorded rather than answered.
    pub async fn run_supervision(&self, turn: PendingTurn) -> Result<(), CoreError> {
        let PendingTurn { print, arrival } = turn;
        let mut next = Some((arrival, Vec::new()));
        while let Some((arrival, earlier)) = next {
            let supervised = self.supervise(&print, &arrival, earlier).await;
            if !matches!(supervised, Ok(Supervised::Ended)) {
                self.resume_adjusted_detector_pause(print.id).await;
            }
            match supervised {
                Ok(Supervised::Ran) => {}
                Ok(Supervised::Ended) => {
                    self.inboxes().release(print.id);
                    self.forget_detector_pause(print.id);
                    return Ok(());
                }
                Ok(Supervised::Terminal(state)) => {
                    self.inboxes().release(print.id);
                    let _turn = self.turns().acquire(print.id).await;
                    return self
                        .close_out_print(print.id, &state, &reached_reason(&state))
                        .await
                        .map(drop);
                }
                Err(error) => {
                    self.inboxes().release(print.id);
                    return Err(error);
                }
            }
            next = self
                .inboxes()
                .next_or_release(print.id)
                .and_then(|mut left| left.pop().map(|latest| (latest, left)));
        }
        Ok(())
    }

    /// The print an alert belongs to, opened if this system has not seen it.
    ///
    /// Correlated on the provider's own identifier the alert carries beside its
    /// body, and never on the body by kind: which provider raised the alert is
    /// the adapter's business, and this loop reads nothing an adapter declares.
    ///
    /// In order: the print already carrying that identifier; otherwise the most
    /// recently opened print with no end recorded, no provider identifier and
    /// the alert's own file name, which is a job somebody found before the
    /// provider reported on it and which takes the identifier here; otherwise a
    /// print opened for it. The file name is the one key the two sides share —
    /// the provider names the job by the file the printer reported, and the
    /// printer names it by that same file.
    async fn resolve_print(
        &self,
        alert: &NormalizedAlert,
    ) -> Result<Option<PrintRecord>, CoreError> {
        let Some(provider_print) = &alert.print else {
            return Ok(None);
        };
        let _resolving = self.resolving().await;
        if let Some(found) = self
            .stores()
            .prints
            .print_by_provider_id(provider_print.id)
            .await?
        {
            return Ok(Some(found));
        }
        if let Some(file_name) = &provider_print.file_name {
            let unattached = self
                .stores()
                .prints
                .open_prints()
                .await?
                .into_iter()
                .find(|open| {
                    open.provider_print_id.is_none() && open.file_name.as_ref() == Some(file_name)
                });
            if let Some(found) = unattached {
                let attached = self
                    .stores()
                    .prints
                    .attach_obico_print(found.id, provider_print.id)
                    .await?;
                return Ok(Some(attached));
            }
        }
        let opened = self
            .stores()
            .prints
            .open_print(Some(provider_print.id), provider_print.file_name.clone())
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
            .stores()
            .images
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
    /// `earlier` is what arrived for the print before `arrival` while an
    /// earlier turn was running and that turn never took, oldest first.
    ///
    /// Answers the terminal state the printer reported, when it reported one,
    /// and runs nothing for a print that ended — at a read that found its job
    /// over — before this turn came to hold it.
    async fn supervise(
        &self,
        print: &PrintRecord,
        arrival: &Arrival,
        earlier: Vec<Arrival>,
    ) -> Result<Supervised, CoreError> {
        let event = &arrival.event;
        let guard = self.turns().acquire(print.id).await;
        if self
            .stores()
            .prints
            .print(print.id)
            .await?
            .is_none_or(|held| held.ended_at.is_some())
        {
            return Ok(Supervised::Ended);
        }
        let context = self.gather_context(print, Some(event.id)).await?;
        let printer_state = context
            .printer
            .as_ref()
            .map(|snapshot| snapshot.connection.clone());
        let terminal = printer_state
            .clone()
            .filter(|state| TERMINAL_STATES.contains(state));
        let image_path = match &event.image {
            Some(reference) => self.image_path(reference).await,
            None => None,
        };
        let situation = TurnSituation {
            printer_state: printer_state.as_ref().and_then(state_name),
            detector_warned: arrival.detection.map(|detection| detection.warning),
            detector_paused_the_print: arrival
                .detection
                .map(|detection| detection.paused_the_print),
            arrived_while_busy: earlier.into_iter().map(|left| left.event).collect(),
        };
        self.hold_context(print.id, context);
        let turn = self
            .agent()
            .run_turn(TurnRequest {
                print_id: print.id,
                event: event.clone(),
                image_path,
                context_command: self.config().context_command_for(print.id),
                situation,
            })
            .await;
        self.drop_context(print.id);
        drop(guard);
        match turn {
            Ok(outcome) => {
                if outcome.phase == SessionPhase::Created {
                    self.append_system_event(
                        print.id,
                        EventBody::of(&SupervisionSessionOpenedPayload {
                            session_name: outcome.session.session_name.clone(),
                            harness_identity: outcome.session.harness_identity.clone(),
                        })?,
                    )
                    .await?;
                }
                let session_name = outcome.session.session_name.clone();
                self.stores().sessions.put_session(outcome.session).await?;
                self.append_system_event(
                    print.id,
                    EventBody::of(&AgentAssessmentPayload {
                        session_name,
                        assessment: outcome.assessment,
                    })?,
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
        Ok(terminal.map_or(Supervised::Ran, Supervised::Terminal))
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
            .stores()
            .prints
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
    ///
    /// A job read that finds the job over closes the print, as every read that
    /// finds it so does ([`crate::listing`]), before the rest of the context is
    /// read — so the context answered is the print as it now stands, ended.
    /// Within a turn the turn holds the print, and that turn closes it.
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
        let ended = job
            .as_ref()
            .filter(|job| print.ended_at.is_none() && is_over(&job.state));
        let print = match ended {
            Some(job)
                if self
                    .close_out_from_read(print.id, &job.state, &reached_reason(&job.state))
                    .await? =>
            {
                self.stores()
                    .prints
                    .print(print.id)
                    .await?
                    .unwrap_or_else(|| print.clone())
            }
            _ => print.clone(),
        };
        let print = &print;
        let manifest = self.stores().prints.manifest(print.id).await?;
        let bounds = self.bounds_for(Some(print), print.id).await?;
        let interventions = self.stores().actions.active_interventions(print.id).await?;
        let recent_events = self
            .stores()
            .events
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
    pub(crate) async fn image_path(&self, image: &ImageRef) -> Option<std::path::PathBuf> {
        match self.stores().images.image(image.id).await {
            Ok(ImageLookup::Found { path, .. }) => Some(path),
            _ => None,
        }
    }

    /// End the print, expire what it had running, and close its session.
    ///
    /// The interventions are expired **before** the print is ended, because a
    /// restoration is an action against the print and a print that has already
    /// ended is one no action may be taken against.
    ///
    /// The caller holds the print's turn, so no turn is acting on it while it
    /// closes. Answers whether this closed it: a print already ended — by
    /// another read, or by the turn that held it — is left as it ended.
    pub(crate) async fn close_out_print(
        &self,
        print_id: PrintId,
        state: &PrinterState,
        reason: &str,
    ) -> Result<bool, CoreError> {
        match self.stores().prints.print(print_id).await? {
            Some(print) if print.ended_at.is_none() => {}
            _ => return Ok(false),
        }
        self.forget_detector_pause(print_id);
        self.expire_all_active(print_id).await?;
        self.stores()
            .prints
            .end_print(
                print_id,
                state.clone(),
                self.clock().now(),
                reason.to_owned(),
            )
            .await?;
        let _ = self.agent().close_session(print_id, reason.to_owned()).await;
        if let Some(session) = self.stores().sessions.session(print_id).await? {
            self.append_system_event(
                print_id,
                EventBody::of(&SupervisionSessionClosedPayload {
                    session_name: session.session_name,
                    close_reason: reason.to_owned(),
                })?,
            )
            .await?;
        }
        Ok(true)
    }

    /// Append one event this system raised itself.
    pub(crate) async fn append_system_event(
        &self,
        print_id: PrintId,
        body: EventBody,
    ) -> Result<EventRecord, CoreError> {
        Ok(self
            .stores()
            .events
            .append_event(EventDraft {
                print_id: Some(print_id),
                source: system_source(),
                received_at: self.clock().now(),
                body,
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
    pub(crate) async fn record_port_failure(
        &self,
        print_id: PrintId,
        event_id: printobserver_types::EventId,
        site: PortFailureSite,
        detail: String,
    ) {
        // A record that will not render is dropped exactly as one the store
        // refuses is: the failure it was about has already been answered, and
        // the loop survives not having written it down.
        let Ok(body) = EventBody::of(&PortFailurePayload {
            event_id,
            site,
            detail,
        }) else {
            return;
        };
        let _ = self.append_system_event(print_id, body).await;
    }
}
