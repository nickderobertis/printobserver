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
//! # Context reads gather the print's current state
//!
//! A turn reads its print's context by running the command
//! [`CoreConfig::context_command_for`](crate::CoreConfig::context_command_for)
//! names, and that command reads it back through [`Supervisor::context`]. Each
//! read gathers the current printer, job and interventions, even while a turn
//! is running, so an operator's completed action is visible immediately. The
//! turn's event and situation still describe what prompted it; a look
//! ([`crate::look`]) also takes a fresh camera frame.
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
use printobserver_printer_api::{JobSnapshot, PrinterState};
use printobserver_supervisor_api::SessionPhase;
use printobserver_supervisor_api::{SupervisionSessionOpenedPayload, TurnRequest, TurnSituation};
use printobserver_types::{EventBody, EventRecord, ImageRef, PrintId};
use printobserver_vision_api::NormalizedAlert;

use crate::context::PrintContext;
use crate::error::CoreError;
use crate::inbox::Arrival;
use crate::kinds::{AgentAssessmentPayload, PortFailurePayload, PortFailureSite, system_source};
use crate::listing::{self, ObservedJob, reached_reason};
use crate::supervisor::Supervisor;
use crate::watch::{concerns_its_end, noticed_of};

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
    /// What was handed to the print before that event and never taken,
    /// oldest first.
    earlier: Vec<Arrival>,
}

impl PendingTurn {
    /// The turn one arrival claimed, after what was handed over before it.
    pub(crate) const fn new(print: PrintRecord, arrival: Arrival, earlier: Vec<Arrival>) -> Self {
        Self {
            print,
            arrival,
            earlier,
        }
    }
}

/// The print an alert belongs to, and whether resolving it opened one.
enum Resolution {
    /// The alert is about no print.
    NoPrint,
    /// A print this system already held.
    Existing(PrintRecord),
    /// A print opened for it now, which is owed its `print_started`.
    Opened(PrintRecord),
}

impl Resolution {
    /// The print, however it was found.
    const fn print(&self) -> Option<&PrintRecord> {
        match self {
            Self::NoPrint => None,
            Self::Existing(print) | Self::Opened(print) => Some(print),
        }
    }
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
    /// recorded against the event rather than answered here — except the
    /// writes that come after the event is already in the history and are
    /// owed the spool ledger: a new print's `print_started`, and what a
    /// filament-change signal the alert relays charges and marks. Those are
    /// answered as the store's own error, the alert itself already written.
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
        let resolution = self.resolve_print(&alert).await?;
        let draft = EventDraft {
            print_id: resolution.print().map(|record| record.id),
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
        let print = match resolution {
            Resolution::NoPrint => return Ok(Received { event, turn: None }),
            Resolution::Existing(print) => print,
            // llmlint: ignore[changed_behavior_has_e2e] suppressions.toml has the reason.
            Resolution::Opened(print) => {
                self.record_print_started(print.id, print.file_name.clone(), None, None)
                    .await?;
                print
            }
        };
        let image = self
            .write_image(&print, &event, alert.image_url.clone())
            .await;
        if print.ended_at.is_some() {
            return Ok(Received { event, turn: None });
        }
        if let Some(signal) = alert.signal {
            self.handle_signal(&print, signal, alert.kind()).await?;
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
            .then(|| PendingTurn::new(print, arrival, Vec::new()));
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
        let PendingTurn {
            print,
            arrival,
            earlier,
        } = turn;
        let mut next = Some((arrival, earlier));
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
                        .close_out_print(print.id, &state, &reached_reason(&state), None)
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
    /// recently opened print with no end recorded, no provider identifier, the
    /// alert's own file name and a job the alert's start may be
    /// ([`listing::may_have_started_it`]), which is a job somebody found before
    /// the provider reported on it and which takes the identifier here;
    /// otherwise a print opened for it. The file name is the one key the two
    /// sides share — the provider names the job by the file the printer
    /// reported, and the printer names it by that same file — and the start is
    /// what tells that job from a later one of the same file.
    ///
    /// Nothing here reads the printer: an alert is written down before anything
    /// is asked of a port, and a print a later job replaced is closed by the
    /// next read that finds that job, not by an alert about it.
    ///
    /// Answers whether this opened it, so that the print's `print_started` is
    /// written once the alert itself has been.
    async fn resolve_print(&self, alert: &NormalizedAlert) -> Result<Resolution, CoreError> {
        let Some(provider_print) = &alert.print else {
            return Ok(Resolution::NoPrint);
        };
        let _resolving = self.resolving().await;
        if let Some(found) = self
            .stores()
            .prints
            .print_by_provider_id(provider_print.id)
            .await?
        {
            return Ok(Resolution::Existing(found));
        }
        if let Some(file_name) = &provider_print.file_name {
            let unattached = self
                .stores()
                .prints
                .open_prints()
                .await?
                .into_iter()
                .find(|open| {
                    open.provider_print_id.is_none()
                        && open.file_name.as_ref() == Some(file_name)
                        && listing::may_have_started_it(open, provider_print.started_at)
                });
            if let Some(found) = unattached {
                let attached = self
                    .stores()
                    .prints
                    .attach_obico_print(found.id, provider_print.id)
                    .await?;
                return Ok(Resolution::Existing(attached));
            }
        }
        let opened = self
            .stores()
            .prints
            .open_print(Some(provider_print.id), provider_print.file_name.clone())
            .await?;
        Ok(Resolution::Opened(opened))
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
            .is_none_or(|held| held.ended_at.is_some() && !concerns_its_end(event.kind()))
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
            printer_state,
            detector_warned: arrival.detection.map(|detection| detection.warning),
            detector_paused_the_print: arrival
                .detection
                .map(|detection| detection.paused_the_print),
            arrived_while_busy: earlier.into_iter().map(|left| left.event).collect(),
            noticed: noticed_of(event),
        };
        // The turn's credentials are bound to it from here, and revoked the
        // moment it returns — whatever it returned — and when this future is
        // abandoned, because dropping what was opened revokes too.
        let opened = self.open_turn(print.id);
        let turn = self
            .agent()
            .run_turn(
                TurnRequest {
                    print_id: print.id,
                    event: event.clone(),
                    image_path,
                    context_command: self.config().context_command_for(print.id),
                    situation,
                },
                opened.access(),
            )
            .await;
        opened.revoke();
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

    /// Gather the current context of one print, including while its
    /// supervision turn is running.
    ///
    /// # Errors
    ///
    /// Returns [`CoreError::NoSuchPrint`] when nothing is held under that
    /// identifier, and the store's own error when the context could not be
    /// read.
    pub async fn context(&self, print_id: PrintId) -> Result<PrintContext, CoreError> {
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
    /// The job read settles the open prints exactly as the listing's does
    /// ([`crate::listing`]) before the rest of the context is read: a job found
    /// over closes the print, a later job of its file closes it as replaced,
    /// and a job found to be its own is recorded on it — so the context
    /// answered is the print as it now stands. Within a turn the turn holds the
    /// print, and that turn closes it.
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
        // The job is read and settled under one hold on resolution, as the
        // listing's is. A start holds it from the read that chooses its print
        // until the printer has been asked to start, so a job read before that
        // hold and settled after it would be the idle machine the start has
        // since moved, and would close the print the start had just opened.
        let resolving = self.resolving().await;
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
        let print = match &job {
            Some(job) if print.ended_at.is_none() => self.settle_for_context(print, job).await?,
            _ => print.clone(),
        };
        drop(resolving);
        let print = &print;
        let manifest = self.stores().prints.manifest(print.id).await?;
        let bounds = self.bounds_for(Some(print), print.id).await?;
        let interventions = self.stores().actions.active_interventions(print.id).await?;
        let recent_events = self.recent_events_of(print).await?;
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
            detector: self.detector_health(),
        })
    }

    /// One print's most recent events, newest first: its own, and what the
    /// printer watch recorded of the machine since it opened, which is the
    /// printer's and carries no print.
    async fn recent_events_of(&self, print: &PrintRecord) -> Result<Vec<EventRecord>, CoreError> {
        let limit = self.config().recent_events;
        let mut events = self
            .stores()
            .events
            .history(HistoryQuery {
                print_id: print.id,
                kinds: Vec::new(),
                since: None,
                until: None,
                limit: Some(limit),
            })
            .await?;
        let kinds = crate::watch::PRINTER_WATCH_KINDS
            .iter()
            .filter_map(|kind| printobserver_types::EventKind::new(*kind).ok())
            .collect();
        let watched = self
            .stores()
            .timeline
            .timeline(crate::store::TimelineQuery {
                kinds,
                since: Some(print.opened_at),
                until: None,
                spool_id: None,
                before: None,
                limit: Some(limit),
            })
            .await?;
        events.extend(watched.into_iter().filter(|event| event.print_id.is_none()));
        events.sort_by_key(|event| core::cmp::Reverse((event.received_at, event.id)));
        events.truncate(usize::try_from(limit).unwrap_or(usize::MAX));
        Ok(events)
    }

    /// Where one image's bytes are, when the store still has them.
    pub(crate) async fn image_path(&self, image: &ImageRef) -> Option<std::path::PathBuf> {
        match self.stores().images.image(image.id).await {
            Ok(ImageLookup::Found { path, .. }) => Some(path),
            _ => None,
        }
    }

    /// Settle the open prints against the job a context read found, and
    /// answer the print the context is about as that left it.
    ///
    /// The caller holds resolution from before it read that job.
    async fn settle_for_context(
        &self,
        print: &PrintRecord,
        job: &JobSnapshot,
    ) -> Result<PrintRecord, CoreError> {
        let observed = ObservedJob {
            job: job.clone(),
            observed_at: self.clock().now(),
        };
        let settled = self.settle_open_prints(&observed).await?;
        match settled.running {
            Some(running) if running.id == print.id => Ok(running),
            _ if settled.closed.contains(&print.id) => Ok(self
                .stores()
                .prints
                .print(print.id)
                .await?
                .unwrap_or_else(|| print.clone())),
            _ => Ok(print.clone()),
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
    ///
    /// The print's filament is settled after its interventions are expired
    /// and before it is ended ([`crate::accounting`]): a crash between the two
    /// leaves a settled, open print, which the next close-out only ends.
    /// `closing_job` is the job read that found the print over, when there
    /// was one; without it the settlement reads the job itself.
    pub(crate) async fn close_out_print(
        &self,
        print_id: PrintId,
        state: &PrinterState,
        reason: &str,
        closing_job: Option<&JobSnapshot>,
    ) -> Result<bool, CoreError> {
        let print = match self.stores().prints.print(print_id).await? {
            Some(print) if print.ended_at.is_none() => print,
            _ => return Ok(false),
        };
        self.forget_detector_pause(print_id);
        self.expire_all_active(print_id).await?;
        self.settle_filament(&print, state, reason, closing_job)
            .await?;
        self.stores()
            .prints
            .end_print(
                print_id,
                state.clone(),
                self.clock().now(),
                reason.to_owned(),
            )
            .await?;
        self.close_session_of(print_id, reason).await?;
        self.after_close_out(print_id).await?;
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
