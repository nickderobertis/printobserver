//! The spool ledger's commands and reads, through the one granted path.
//!
//! A spool command is an action of its own kind: the safety envelope grants it
//! per actor class exactly as it grants a print action, an ungranted one is
//! refused [`ActorMayNotRequest`](RejectionReason::ActorMayNotRequest), and
//! one the ledger cannot carry out is refused
//! [`SpoolRefused`](RejectionReason::SpoolRefused) with a tagged
//! [`SpoolRefusalCause`] a client acts on, beside a sentence for a person. Every refusal
//! writes a `spool_command_rejected`, and every accepted command writes its
//! spool events through one store write carrying who asked and why — so a
//! remaining figure can be traced to every print and correction behind it.
//!
//! Every command is carried out under the supervisor's one spool lock, so two
//! commands — or a command and a settlement — never compute from the same
//! stale figure.
//!
//! # A load mid-print, and a load after an unconfirmed change
//!
//! A load or unload during a print first charges what the tool fed up to the
//! position read now to the spool that was feeding it, and opens the next
//! segment for the spool loaded; the close-out charges that spool only the
//! remainder. After an unconfirmed filament change the open segment is not the
//! outgoing spool's: a load of another spool takes every pending charge on the
//! tool and the open post-signal segment to that point, and a load naming the
//! spool already there confirms it was never swapped — taking the pending
//! charges, clearing its filament-change marks and charging nothing for the
//! open segment, which the close-out charges to it as the remainder.

use printobserver_types::{EventRecord, EventSource, PrintId};

use crate::accounting::{
    AFTER_CHANGE, Figures, feed_to, holds_any_change_mark, holds_change_mark, spool_on_tool,
};
use crate::actions::source_of;
use crate::error::CoreError;
use crate::filament::{DEFAULT_DIAMETER_MM, MarkClearing, density_for, marks_cleared_by};
use crate::kinds::{
    SpoolAdjustedPayload, SpoolCommandRejectedPayload, SpoolLoadedPayload, SpoolRetiredPayload,
    SpoolUnloadedPayload,
};
use crate::records::{
    ClearedMark, PolicyDecision, RejectionReason, SpoolCommand, SpoolField, SpoolId, SpoolRecord,
    SpoolRefusalCause, SpoolTrust,
};
use crate::store::{
    Charge, EventDraft, NewSpool, SpoolChange, SpoolMutation, StoreError, TimelineQuery,
};
use crate::supervisor::Supervisor;

/// What became of one spool command.
#[derive(Debug, Clone, PartialEq)]
pub struct SpoolCommandOutcome {
    /// The decision taken on it.
    pub decision: PolicyDecision,
    /// The spool it was about, as the command left it, when there is one.
    pub spool: Option<SpoolRecord>,
    /// Every event it wrote, in the order they were written.
    pub events: Vec<EventRecord>,
}

/// A refusal of a spool command: its cause, and a sentence saying why and what
/// to do instead.
fn refused(cause: SpoolRefusalCause, message: impl Into<String>) -> RejectionReason {
    RejectionReason::SpoolRefused {
        cause,
        message: message.into(),
    }
}

/// The refusal of a figure a field cannot hold.
fn invalid_figure(field: SpoolField, value: f64, message: impl Into<String>) -> RejectionReason {
    refused(
        SpoolRefusalCause::InvalidAdjustmentFigure { field, value },
        message,
    )
}

/// A registration's fields, as the command gave them.
struct Registration<'a> {
    /// The operator's own label for the spool.
    name: &'a str,
    /// The material, such as `PLA`.
    material: &'a str,
    /// The filament's colour.
    color: Option<&'a String>,
    /// The filament's diameter, in millimetres, when given.
    diameter_mm: Option<f64>,
    /// The material's density, when given.
    density_g_cm3: Option<f64>,
    /// The net filament on the spool when full, in grams.
    initial_g: f64,
    /// The filament on it now, in grams, when given.
    remaining_g: Option<f64>,
    /// The empty spool's own weight, in grams, when given.
    tare_g: Option<f64>,
}

/// Whether a figure is a finite number at or above zero.
fn non_negative(value: f64) -> bool {
    value.is_finite() && value >= 0.0
}

/// Whether a figure is a finite number above zero.
fn positive(value: f64) -> bool {
    value.is_finite() && value > 0.0
}

/// The marks one spool loses to a clearing, named against it.
fn cleared_on(spool: &SpoolRecord, clearing: MarkClearing) -> Vec<ClearedMark> {
    let (_, removed) = spool.trust.clone().cleared(marks_cleared_by(clearing));
    removed
        .into_iter()
        .map(|reason| ClearedMark {
            spool_id: spool.id,
            reason,
        })
        .collect()
}

/// The mutation clearing one spool's marks of one clearing's kinds.
///
/// It writes no event of its own — the command's own event, written at the
/// same instant, names the marks it cleared.
fn clearing(spool_id: SpoolId, clearing: MarkClearing, command: &EventDraft) -> SpoolMutation {
    SpoolMutation {
        spool_id,
        change: SpoolChange::ClearMarks {
            kinds: marks_cleared_by(clearing).to_vec(),
        },
        at: command.received_at,
    }
}

/// What one accepted command asks the store to write.
#[derive(Debug, Default)]
struct Write {
    /// The spool mutations, in order.
    mutations: Vec<SpoolMutation>,
    /// The charges, in order.
    charges: Vec<Charge>,
}

impl Supervisor {
    /// Carry out one spool command, through the same grant every action passes.
    ///
    /// # Errors
    ///
    /// Returns the store's own error when the command or its refusal could not
    /// be written. A refusal is not an error: it is written down and answered
    /// in [`SpoolCommandOutcome::decision`].
    pub async fn request_spool_command(
        &self,
        command: SpoolCommand,
    ) -> Result<SpoolCommandOutcome, CoreError> {
        let actor = command.actor().clone();
        let class = actor.class();
        let kind = command.kind();
        let granted = self
            .config()
            .envelope
            .actions
            .get(&class)
            .is_some_and(|kinds| kinds.contains(&kind));
        if !granted {
            let reason = RejectionReason::ActorMayNotRequest {
                actor_class: class,
                action: kind,
            };
            return self.refuse(command, reason).await;
        }
        let _writing = self.spool_writes().await;
        let during = self.print_during().await?.map(|print| print.id);
        let carried = match &command {
            SpoolCommand::RegisterSpool {
                name,
                material,
                color,
                diameter_mm,
                density_g_cm3,
                initial_g,
                remaining_g,
                tare_g,
                reason,
                actor,
            } => {
                let registration = Registration {
                    name,
                    material,
                    color: color.as_ref(),
                    diameter_mm: *diameter_mm,
                    density_g_cm3: *density_g_cm3,
                    initial_g: *initial_g,
                    remaining_g: *remaining_g,
                    tare_g: *tare_g,
                };
                self.register(&registration, actor, reason).await?
            }
            SpoolCommand::LoadSpool { spool_id, tool, .. } => {
                self.load(&command, *spool_id, *tool, during).await?
            }
            SpoolCommand::UnloadSpool { spool_id, .. } => {
                self.unload(&command, *spool_id, during).await?
            }
            SpoolCommand::AdjustSpool {
                spool_id,
                remaining_g,
                gross_g,
                ..
            } => {
                self.adjust(&command, *spool_id, *remaining_g, *gross_g, during)
                    .await?
            }
            SpoolCommand::RetireSpool { spool_id, .. } => {
                self.retire(&command, *spool_id, during).await?
            }
        };
        match carried {
            Ok(outcome) => Ok(outcome),
            Err(reason) => self.refuse(command, reason).await,
        }
    }

    /// Every spool, retired ones only when asked for, most recently registered
    /// first.
    ///
    /// # Errors
    ///
    /// Returns the store's own error when the spools could not be read.
    // llmlint: ignore[invalid_states_unrepresentable] suppressions.toml has the reason.
    pub async fn spools(&self, include_retired: bool) -> Result<Vec<SpoolRecord>, CoreError> {
        Ok(self.stores().filament.spools(include_retired).await?)
    }

    /// One spool, retired or not.
    ///
    /// # Errors
    ///
    /// Returns the store's own error when the spool could not be read.
    pub async fn spool(&self, spool_id: SpoolId) -> Result<Option<SpoolRecord>, CoreError> {
        Ok(self.stores().filament.spool(spool_id).await?)
    }

    /// The printer timeline, newest first.
    ///
    /// # Errors
    ///
    /// Returns the store's own error when the timeline could not be read, a
    /// kind outside the timeline or a limit above the maximum included.
    pub async fn timeline(&self, query: TimelineQuery) -> Result<Vec<EventRecord>, CoreError> {
        Ok(self.stores().timeline.timeline(query).await?)
    }

    /// Every charge held after an unconfirmed filament change — on one tool,
    /// when one is named — oldest first.
    ///
    /// # Errors
    ///
    /// Returns the store's own error when the charges could not be read.
    pub async fn pending_charges(
        &self,
        tool: Option<i64>,
    ) -> Result<Vec<crate::records::PendingCharge>, CoreError> {
        Ok(self.stores().filament.pending_charges(tool).await?)
    }

    /// Write a refused command down and answer the refusal.
    async fn refuse(
        &self,
        command: SpoolCommand,
        reason: RejectionReason,
    ) -> Result<SpoolCommandOutcome, CoreError> {
        let decision = PolicyDecision::Rejected(reason);
        let source = source_of(command.actor());
        let during = self.print_during().await?.map(|print| print.id);
        let spool = match &command {
            SpoolCommand::RegisterSpool { .. } => None,
            SpoolCommand::LoadSpool { spool_id, .. }
            | SpoolCommand::UnloadSpool { spool_id, .. }
            | SpoolCommand::AdjustSpool { spool_id, .. }
            | SpoolCommand::RetireSpool { spool_id, .. } => {
                self.stores().filament.spool(*spool_id).await?
            }
        };
        let draft = self.draft_of(
            during,
            source,
            &SpoolCommandRejectedPayload {
                command,
                decision: decision.clone(),
            },
        )?;
        let event = self.stores().events.append_event(draft).await?;
        Ok(SpoolCommandOutcome {
            decision,
            spool,
            events: vec![event],
        })
    }

    /// One spool the command names, or the refusal that it cannot be acted on.
    async fn live_spool(
        &self,
        spool_id: SpoolId,
    ) -> Result<Result<SpoolRecord, RejectionReason>, CoreError> {
        Ok(match self.stores().filament.spool(spool_id).await? {
            None => Err(refused(
                SpoolRefusalCause::UnknownSpool { spool_id },
                format!("there is no spool {spool_id}; list the spools to find the one meant"),
            )),
            Some(spool) if spool.is_retired() => Err(refused(
                SpoolRefusalCause::RetiredSpool { spool_id },
                format!(
                    "spool {} is retired and takes no further command; register a new spool \
                     instead",
                    spool.name
                ),
            )),
            Some(spool) => Ok(spool),
        })
    }

    /// Write one accepted command's changes and answer them.
    async fn written(
        &self,
        spool_id: SpoolId,
        write: Write,
    ) -> Result<Result<SpoolCommandOutcome, RejectionReason>, CoreError> {
        let written = match self
            .stores()
            .filament
            .apply_spool_changes(write.mutations, write.charges, Vec::new())
            .await
        {
            Ok(written) => written,
            Err(StoreError::ConstraintRefused { .. }) => {
                return Ok(Err(refused(
                    SpoolRefusalCause::LedgerRefused,
                    "the spools changed while this command was carried out, so the ledger \
                     refused it; read the spools again and ask again",
                )));
            }
            Err(error) => return Err(error.into()),
        };
        let spool = written
            .spools
            .into_iter()
            .find(|spool| spool.id == spool_id);
        Ok(Ok(SpoolCommandOutcome {
            decision: PolicyDecision::Accepted,
            spool,
            events: written.events,
        }))
    }

    /// Register a spool.
    async fn register(
        &self,
        given: &Registration<'_>,
        actor: &crate::records::Actor,
        reason: &str,
    ) -> Result<Result<SpoolCommandOutcome, RejectionReason>, CoreError> {
        let name = given.name.trim();
        let material = given.material.trim();
        if name.is_empty() {
            return Ok(Err(refused(
                SpoolRefusalCause::MissingField {
                    field: SpoolField::Name,
                },
                "a spool needs a name to be known by",
            )));
        }
        if material.is_empty() {
            return Ok(Err(refused(
                SpoolRefusalCause::MissingField {
                    field: SpoolField::Material,
                },
                "a spool needs its material, such as PLA",
            )));
        }
        let diameter_mm = given.diameter_mm.unwrap_or(DEFAULT_DIAMETER_MM);
        if !positive(diameter_mm) {
            return Ok(Err(invalid_figure(
                SpoolField::DiameterMm,
                diameter_mm,
                format!(
                    "a diameter of {diameter_mm}mm is not one a filament has; give it in \
                     millimetres, such as 1.75"
                ),
            )));
        }
        let Some(density_g_cm3) = given.density_g_cm3.or_else(|| density_for(material)) else {
            return Ok(Err(refused(
                SpoolRefusalCause::DensityRequired {
                    spool_id: None,
                    material: material.to_owned(),
                },
                format!(
                    "{material} has no known density, so the spool needs one in grams per cubic \
                     centimetre"
                ),
            )));
        };
        if !positive(density_g_cm3) {
            return Ok(Err(invalid_figure(
                SpoolField::DensityGCm3,
                density_g_cm3,
                format!("a density of {density_g_cm3}g/cm3 is not one a material has"),
            )));
        }
        if !positive(given.initial_g) {
            return Ok(Err(invalid_figure(
                SpoolField::InitialG,
                given.initial_g,
                format!(
                    "a full spool of {}g holds nothing; give the net filament it held full",
                    given.initial_g
                ),
            )));
        }
        let remaining_g = given.remaining_g.unwrap_or(given.initial_g);
        if !remaining_g.is_finite() {
            return Ok(Err(invalid_figure(
                SpoolField::RemainingG,
                remaining_g,
                "the remaining figure is not a number",
            )));
        }
        if let Some(tare) = given.tare_g.filter(|tare| !non_negative(*tare)) {
            return Ok(Err(invalid_figure(
                SpoolField::TareG,
                tare,
                "an empty spool's weight is at least zero grams",
            )));
        }
        let spool = NewSpool {
            name: name.to_owned(),
            material: material.to_owned(),
            color: given.color.cloned(),
            diameter_mm,
            density_g_cm3,
            initial_g: given.initial_g,
            remaining_g,
            tare_g: given.tare_g,
            loaded_on_tool: None,
            trust: SpoolTrust::Verified,
            registered_at: self.clock().now(),
        };
        let (record, event) = self
            .stores()
            .filament
            .register_spool(spool, source_of(actor), actor.clone(), reason.to_owned())
            .await?;
        Ok(Ok(SpoolCommandOutcome {
            decision: PolicyDecision::Accepted,
            spool: Some(record),
            events: vec![event],
        }))
    }

    /// Load a spool onto a tool: a confirmation in place when it is already
    /// there, a swap when another is, and either way taking what that tool
    /// holds pending.
    async fn load(
        &self,
        command: &SpoolCommand,
        spool_id: SpoolId,
        tool: i64,
        during: Option<PrintId>,
    ) -> Result<Result<SpoolCommandOutcome, RejectionReason>, CoreError> {
        if tool < 0 {
            return Ok(Err(refused(
                SpoolRefusalCause::UnknownTool { tool },
                format!(
                    "there is no tool {tool}; tools are numbered from 0, as the printer numbers \
                     them"
                ),
            )));
        }
        let incoming = match self.live_spool(spool_id).await? {
            Ok(spool) => spool,
            Err(reason) => return Ok(Err(reason)),
        };
        let source = source_of(command.actor());
        let pending = self.stores().filament.pending_charges(Some(tool)).await?;
        let mut figures = Figures::default();
        let already_there = incoming.loaded_tool() == Some(tool);
        if already_there && pending.is_empty() && !holds_any_change_mark(&incoming) {
            return Ok(Err(refused(
                SpoolRefusalCause::NothingToConfirm { tool },
                format!(
                    "spool {} is already on tool {tool}, and nothing is pending there to confirm",
                    incoming.name
                ),
            )));
        }
        let spools = self.stores().filament.spools(true).await?;
        // The spool that was feeding the tool, which the load takes off it.
        let outgoing = spool_on_tool(&spools, tool)
            .filter(|spool| spool.id != incoming.id)
            .cloned();
        let (mut charges, took_pending) =
            self.take_pending(&pending, &incoming, &mut figures, &source)?;
        if !already_there && let Some(print_id) = during {
            charges.extend(
                self.close_segment(
                    print_id,
                    tool,
                    outgoing.as_ref(),
                    Some(&incoming),
                    &mut figures,
                    &source,
                )
                .await?,
            );
        }
        let swapped = outgoing
            .as_ref()
            .map(|spool| cleared_on(spool, MarkClearing::SwappedOut))
            .unwrap_or_default();
        let confirmed = if already_there {
            cleared_on(&incoming, MarkClearing::ConfirmedInPlace)
        } else {
            Vec::new()
        };
        let loaded = self.draft_of(
            during,
            source.clone(),
            &SpoolLoadedPayload {
                spool_id: incoming.id,
                tool,
                replaced: outgoing.as_ref().map(|spool| spool.id),
                took_pending,
                cleared_marks: swapped.iter().chain(&confirmed).cloned().collect(),
                actor: command.actor().clone(),
                reason: command.reason().to_owned(),
            },
        )?;
        let mut write = Write {
            mutations: Vec::new(),
            charges,
        };
        if let Some(outgoing) = &outgoing {
            if !swapped.is_empty() {
                write
                    .mutations
                    .push(clearing(outgoing.id, MarkClearing::SwappedOut, &loaded));
            }
            write.mutations.push(self.mutation(
                outgoing.id,
                |event| SpoolChange::Unload { event },
                during,
                &source,
                &SpoolUnloadedPayload {
                    spool_id: outgoing.id,
                    tool,
                    actor: command.actor().clone(),
                    reason: command.reason().to_owned(),
                },
            )?);
        }
        if !confirmed.is_empty() {
            write.mutations.push(clearing(
                incoming.id,
                MarkClearing::ConfirmedInPlace,
                &loaded,
            ));
        }
        write.mutations.push(SpoolMutation {
            spool_id: incoming.id,
            at: loaded.received_at,
            change: SpoolChange::Load {
                tool,
                event: loaded,
            },
        });
        self.written(incoming.id, write).await
    }

    /// Unload a spool from its tool, charging it first for what it fed of a
    /// print running now.
    async fn unload(
        &self,
        command: &SpoolCommand,
        spool_id: SpoolId,
        during: Option<PrintId>,
    ) -> Result<Result<SpoolCommandOutcome, RejectionReason>, CoreError> {
        let spool = match self.live_spool(spool_id).await? {
            Ok(spool) => spool,
            Err(reason) => return Ok(Err(reason)),
        };
        let Some(tool) = spool.loaded_tool() else {
            return Ok(Err(refused(
                SpoolRefusalCause::NotLoaded { spool_id },
                format!(
                    "spool {} is not loaded on any tool, so there is nothing to unload",
                    spool.name
                ),
            )));
        };
        let source = source_of(command.actor());
        let mut figures = Figures::default();
        let mut write = Write::default();
        if let Some(print_id) = during {
            write.charges = self
                .close_segment(print_id, tool, Some(&spool), None, &mut figures, &source)
                .await?;
        }
        write.mutations.push(self.mutation(
            spool.id,
            |event| SpoolChange::Unload { event },
            during,
            &source,
            &SpoolUnloadedPayload {
                spool_id: spool.id,
                tool,
                actor: command.actor().clone(),
                reason: command.reason().to_owned(),
            },
        )?);
        self.written(spool.id, write).await
    }

    /// Correct a spool's figure, taking first what its tool holds pending.
    async fn adjust(
        &self,
        command: &SpoolCommand,
        spool_id: SpoolId,
        remaining_g: Option<f64>,
        gross_g: Option<f64>,
        during: Option<PrintId>,
    ) -> Result<Result<SpoolCommandOutcome, RejectionReason>, CoreError> {
        let spool = match self.live_spool(spool_id).await? {
            Ok(spool) => spool,
            Err(reason) => return Ok(Err(reason)),
        };
        let (field, given, target) = match (remaining_g, gross_g) {
            (Some(_), Some(_)) => {
                return Ok(Err(refused(
                    SpoolRefusalCause::ConflictingFields {
                        fields: vec![SpoolField::RemainingG, SpoolField::GrossG],
                    },
                    "a correction names exactly one of remaining_g and gross_g, and this names \
                     both",
                )));
            }
            (None, None) => {
                return Ok(Err(refused(
                    SpoolRefusalCause::MissingField {
                        field: SpoolField::RemainingG,
                    },
                    "a correction names exactly one of remaining_g and gross_g, and this names \
                     neither",
                )));
            }
            (Some(remaining), None) => (SpoolField::RemainingG, remaining, remaining),
            (None, Some(gross)) => match spool.tare_g {
                Some(tare) => (SpoolField::GrossG, gross, gross - tare),
                None => {
                    return Ok(Err(refused(
                        SpoolRefusalCause::TareRequired { spool_id },
                        format!(
                            "spool {} has no tare, so a scale reading cannot be turned into a \
                             figure; give remaining_g instead",
                            spool.name
                        ),
                    )));
                }
            },
        };
        if !target.is_finite() || (field == SpoolField::GrossG && !non_negative(given)) {
            return Ok(Err(invalid_figure(
                field,
                given,
                "the figure given is not a weight in grams",
            )));
        }
        let source = source_of(command.actor());
        let pending = match spool.loaded_tool() {
            Some(tool) => self.stores().filament.pending_charges(Some(tool)).await?,
            None => Vec::new(),
        };
        let mut figures = Figures::default();
        let (charges, took_pending) = self.take_pending(&pending, &spool, &mut figures, &source)?;
        let adjusted = SpoolAdjustedPayload {
            spool_id: spool.id,
            remaining_before_g: figures.now(&spool),
            remaining_after_g: target,
            gross_g,
            took_pending,
            cleared_marks: cleared_on(&spool, MarkClearing::Adjusted),
            actor: command.actor().clone(),
            reason: command.reason().to_owned(),
        };
        let mutation = self.mutation(
            spool.id,
            |event| SpoolChange::SetRemaining {
                remaining_g: target,
                event,
            },
            during,
            &source,
            &adjusted,
        )?;
        self.written(
            spool.id,
            Write {
                mutations: vec![mutation],
                charges,
            },
        )
        .await
    }

    /// Retire a spool, which takes it off any tool it is on — charging it
    /// first, as an unload does, for what it fed of a print running now — and
    /// refuses every later command.
    async fn retire(
        &self,
        command: &SpoolCommand,
        spool_id: SpoolId,
        during: Option<PrintId>,
    ) -> Result<Result<SpoolCommandOutcome, RejectionReason>, CoreError> {
        let spool = match self.live_spool(spool_id).await? {
            Ok(spool) => spool,
            Err(reason) => return Ok(Err(reason)),
        };
        let source = source_of(command.actor());
        let mut write = Write::default();
        if let Some(tool) = spool.loaded_tool() {
            if let Some(print_id) = during {
                let mut figures = Figures::default();
                write.charges = self
                    .close_segment(print_id, tool, Some(&spool), None, &mut figures, &source)
                    .await?;
            }
            write.mutations.push(self.mutation(
                spool.id,
                |event| SpoolChange::Unload { event },
                during,
                &source,
                &SpoolUnloadedPayload {
                    spool_id: spool.id,
                    tool,
                    actor: command.actor().clone(),
                    reason: command.reason().to_owned(),
                },
            )?);
        }
        let remaining_g = write
            .charges
            .iter()
            .filter_map(|charge| charge.charged.as_ref())
            .filter(|charged| charged.spool_id == spool.id)
            .map(|charged| charged.remaining_after_g)
            .last()
            .unwrap_or(spool.remaining_g);
        write.mutations.push(self.mutation(
            spool.id,
            |event| SpoolChange::Retire { event },
            during,
            &source,
            &SpoolRetiredPayload {
                spool_id: spool.id,
                remaining_g,
                actor: command.actor().clone(),
                reason: command.reason().to_owned(),
            },
        )?);
        self.written(spool.id, write).await
    }

    /// One mutation of one spool, carrying the event that says it happened.
    fn mutation<P: printobserver_types::EventPayload>(
        &self,
        spool_id: SpoolId,
        change: impl FnOnce(EventDraft) -> SpoolChange,
        during: Option<PrintId>,
        source: &EventSource,
        payload: &P,
    ) -> Result<SpoolMutation, CoreError> {
        let event = self.draft_of(during, source.clone(), payload)?;
        Ok(SpoolMutation {
            spool_id,
            at: event.received_at,
            change: change(event),
        })
    }

    /// Close a tool's open segment of a print for a spool loaded or unloaded
    /// during it: what the tool fed up to the position read now is charged to
    /// the spool that was feeding it — or, after an unconfirmed filament
    /// change, to the spool now loaded, and to nobody when none is.
    async fn close_segment(
        &self,
        print_id: PrintId,
        tool: i64,
        outgoing: Option<&SpoolRecord>,
        incoming: Option<&SpoolRecord>,
        figures: &mut Figures,
        source: &EventSource,
    ) -> Result<Vec<Charge>, CoreError> {
        let Some(print) = self.stores().prints.print(print_id).await? else {
            return Ok(Vec::new());
        };
        let Some((position, extrusion, analysis)) = self.extrusion_now(&print).await? else {
            return Ok(Vec::new());
        };
        let ledger = self.print_ledger(print_id).await?;
        let Some(mut feed) = feed_to(print_id, tool, &ledger, &extrusion, &position, &analysis)
        else {
            return Ok(Vec::new());
        };
        // The spool judged for the open segment: the one feeding the tool, or
        // — a tool emptied since — the one its last segment was charged to.
        let judged = match (outgoing, ledger.last_spool(tool)) {
            (Some(spool), _) => Some(spool.clone()),
            (None, Some(last)) => self.stores().filament.spool(last).await?,
            (None, None) => None,
        };
        let charged = if judged
            .as_ref()
            .is_some_and(|spool| holds_change_mark(spool, print_id))
        {
            let Some(incoming) = incoming else {
                // Unloading the spool an unconfirmed change left takes
                // nothing: the open segment is still the next spool's.
                return Ok(Vec::new());
            };
            feed.note = Some(match &feed.note {
                Some(note) => format!("{AFTER_CHANGE}; {note}"),
                None => AFTER_CHANGE.to_owned(),
            });
            Some(incoming)
        } else {
            outgoing
        };
        Ok(vec![self.charge_of(
            feed,
            charged,
            figures,
            source.clone(),
        )?])
    }
}
