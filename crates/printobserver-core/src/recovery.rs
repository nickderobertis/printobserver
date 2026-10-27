//! Moving the printer after a print fails, through the one granted path.
//!
//! A recovery is asked of the printer rather than of a print, and is checked
//! against what printobserver can work out about the machine rather than
//! against a print's bounds:
//!
//! * **Configured.** With no `[recovery]` facts every action is refused
//!   [`RecoveryNotConfigured`](RecoveryRejection::RecoveryNotConfigured).
//! * **Granted.** The operator holds every action by default and the agent
//!   none ([`RecoveryGrants`]); an ungranted one is refused
//!   [`ActorMayNotRequest`](RecoveryRejection::ActorMayNotRequest).
//! * **Not under a running print.** While the printer is printing or
//!   cancelling every action is refused
//!   [`PrintRunning`](RecoveryRejection::PrintRunning), and while it cannot be
//!   read or reports itself offline or in error,
//!   [`PrinterUnreachable`](RecoveryRejection::PrinterUnreachable). A paused
//!   print admits them — and any recovery executed while it is paused makes it
//!   non-resumable, which its resume is refused as
//!   [`PrintNotResumable`](crate::records::RejectionReason::PrintNotResumable).
//! * **Bounded.** A lift is above zero and at most `max_lift_mm`, and while Z
//!   is trusted never past `max_z_mm` (or below zero, where away from the bed
//!   is down the axis); a fan is 0 to 100 percent; a heat is inside the safety
//!   envelope's range for that tool. Outside, it is refused
//!   [`OutOfBounds`](RecoveryRejection::OutOfBounds).
//! * **Clear of the part.** A heat, a park and an XY home need Z trusted, and
//!   the nozzle at least `clearance_margin_mm` above the tallest part the last
//!   print laid down — or the operator's word that it is clear. An agent's
//!   word never counts. Homing Z needs the operator's word that the bed is
//!   clear, and is never the agent's.
//!
//! # Where Z trust comes from
//!
//! Z is trusted from a Z home this supervisor executed, or a print it started,
//! until the first of: the printer going offline or into error, a reset it
//! pushed, a print ending `interrupted` or `failed`, or this supervisor
//! starting again. A position reply cannot say whether Z was homed, so trust
//! is never read from one, and the position is read only while Z is trusted.
//!
//! The tallest part is read from the last print's own G-code, up to the last
//! file position recorded for it.
//!
//! Every request is recorded — `recovery_requested`, then `recovery_executed`
//! or `recovery_rejected` — printer-scoped, and reaches the machine through
//! [`Supervisor::issue_decided_action`] alone.

use printobserver_printer_api::{
    Adjustable, AxisPosition, HomeAxes, PrinterCommand, PrinterSnapshot, PrinterState,
};
use printobserver_types::{EventId, EventKind, EventPayload as _, FileName, PrintId};

use crate::error::CoreError;
use crate::kinds::{RecoveryExecutedPayload, RecoveryRejectedPayload, RecoveryRequestedPayload};
use crate::records::{
    ActionAllowance, Actor, ActorClass, AwayFromBed, BedTemperature, ExecutionOutcome, LastPrint,
    PausedPrint, PrintRecord, RecoveryAction, RecoveryActionKind, RecoveryConfig, RecoveryDecision,
    RecoveryField, RecoveryGrants, RecoveryPosition, RecoveryRejection, RecoveryStatus,
    RecoveryTemperatures, SafetyEnvelope, ToolTemperature, ZHome, ZTrust,
};
use crate::store::TimelineQuery;
use crate::supervisor::{Decided, IssuedAs, PortCall, RecoveryIssued, Supervisor};

/// Why Z is untrusted when this supervisor has done nothing to trust it.
pub const UNTRUSTED_AT_START: &str =
    "this server started and has neither homed Z nor started a print since";

/// What one recovery is decided from.
#[derive(Debug, Clone)]
struct Facts {
    /// The printer's state, when it could be read.
    snapshot: Option<PrinterSnapshot>,
    /// Whether Z is trusted, and what was read only because it is.
    z: ZFacts,
}

/// What a recovery reads of Z: nothing while it is untrusted, because a
/// position or an extent measured against an untrusted Z means nothing.
#[derive(Debug, Clone)]
enum ZFacts {
    /// Z is untrusted.
    Untrusted,
    /// Z is trusted.
    Trusted {
        /// Where the head is, when it was read.
        position: Option<Result<AxisPosition, String>>,
        /// The tallest part the last print laid down, when it was read; zero
        /// when no print has laid anything down.
        tallest: Option<Result<f64, String>>,
    },
}

/// The decision on one recovery request: the calls it makes, or its refusal.
fn decide(
    config: Option<&RecoveryConfig>,
    grants: &RecoveryGrants,
    envelope: &SafetyEnvelope,
    class: ActorClass,
    action: &RecoveryAction,
    facts: &Facts,
) -> Result<Vec<PortCall>, RecoveryRejection> {
    let kind = action.kind();
    let Some(config) = config else {
        return Err(RecoveryRejection::RecoveryNotConfigured);
    };
    if !grants.grants(class, kind) {
        return Err(RecoveryRejection::ActorMayNotRequest { action: kind });
    }
    let Some(snapshot) = facts.snapshot.as_ref() else {
        return Err(RecoveryRejection::PrinterUnreachable);
    };
    match &snapshot.connection {
        PrinterState::Offline | PrinterState::Error => {
            return Err(RecoveryRejection::PrinterUnreachable);
        }
        PrinterState::Operational | PrinterState::Paused => {}
        // A state this program has no word for is one something may be
        // happening in, so it is treated as a running print.
        state @ (PrinterState::Printing | PrinterState::Cancelling | PrinterState::Unknown(_)) => {
            return Err(RecoveryRejection::PrintRunning {
                state: state.clone(),
            });
        }
    }
    let calls = match action {
        RecoveryAction::RecoverLift { mm } => vec![lift(config, facts, kind, *mm)?],
        RecoveryAction::RecoverCool {} => cooling(snapshot),
        RecoveryAction::RecoverFan { percent } => {
            if !(percent.is_finite() && (0.0..=100.0).contains(percent)) {
                return Err(RecoveryRejection::OutOfBounds {
                    field: RecoveryField::Percent,
                    requested: *percent,
                    allowed: printobserver_types::Range::new(0.0, 100.0),
                });
            }
            vec![PortCall::Fan { percent: *percent }]
        }
        RecoveryAction::RecoverHeat { tool, target_c, .. } => {
            heat_bounded(envelope, *tool, *target_c)?;
            clear_of_the_part(config, grants, class, action, facts)?;
            vec![PortCall::ToolTarget {
                tool: *tool,
                target_c: *target_c,
            }]
        }
        RecoveryAction::RecoverPark { .. } => {
            clear_of_the_part(config, grants, class, action, facts)?;
            vec![PortCall::Command(PrinterCommand::MoveXy {
                x_mm: config.park_xy[0],
                y_mm: config.park_xy[1],
            })]
        }
        RecoveryAction::RecoverHomeXy { .. } => {
            clear_of_the_part(config, grants, class, action, facts)?;
            vec![PortCall::Command(PrinterCommand::Home {
                axes: HomeAxes::Xy,
            })]
        }
        RecoveryAction::RecoverHomeZ { bed_clear_asserted } => {
            if !(*bed_clear_asserted && grants.assertion_counts(class)) {
                return Err(RecoveryRejection::AssertionRequired {
                    field: RecoveryField::BedClearAsserted,
                });
            }
            vec![PortCall::Command(PrinterCommand::Home {
                axes: HomeAxes::Z,
            })]
        }
    };
    Ok(calls)
}

/// A lift's one call, held to its bounds.
fn lift(
    config: &RecoveryConfig,
    facts: &Facts,
    kind: RecoveryActionKind,
    mm: f64,
) -> Result<PortCall, RecoveryRejection> {
    if !(mm.is_finite() && mm > 0.0 && mm <= config.max_lift_mm) {
        return Err(RecoveryRejection::OutOfBounds {
            field: RecoveryField::Mm,
            requested: mm,
            allowed: printobserver_types::Range::new(0.0, config.max_lift_mm),
        });
    }
    if let ZFacts::Trusted { position, .. } = &facts.z {
        let at = position_of(position.as_ref(), kind)?;
        // How far the head can still travel away from the bed.
        let room = match config.away_from_bed {
            AwayFromBed::ZPlus => config.max_z_mm - at.z,
            AwayFromBed::ZMinus => at.z,
        };
        if mm > room {
            return Err(RecoveryRejection::OutOfBounds {
                field: RecoveryField::Mm,
                requested: mm,
                allowed: printobserver_types::Range::new(0.0, room.max(0.0)),
            });
        }
    }
    let signed = match config.away_from_bed {
        AwayFromBed::ZPlus => mm,
        AwayFromBed::ZMinus => -mm,
    };
    Ok(PortCall::Command(PrinterCommand::MoveZ { mm: signed }))
}

/// Every heater's target to zero: each tool the printer reports, tool 0 when
/// it reports none, and the bed.
fn cooling(snapshot: &PrinterSnapshot) -> Vec<PortCall> {
    let tools = snapshot.tools.len().max(1);
    let mut calls: Vec<PortCall> = (0..tools)
        .map(|tool| PortCall::ToolTarget {
            tool: i64::try_from(tool).unwrap_or(i64::MAX),
            target_c: 0.0,
        })
        .collect();
    calls.push(PortCall::BedTarget { target_c: 0.0 });
    calls
}

/// A heat, held to the safety envelope's range for its tool.
fn heat_bounded(
    envelope: &SafetyEnvelope,
    tool: i64,
    target_c: f64,
) -> Result<(), RecoveryRejection> {
    let Some(allowed) = envelope.allowed.get(&Adjustable::ToolTarget { tool }) else {
        // A tool the envelope bounds no target for is no tool a heat may name;
        // the range answered is the tools it does bound.
        let tools: Vec<i64> = envelope
            .allowed
            .keys()
            .filter_map(|adjustable| match adjustable {
                Adjustable::ToolTarget { tool } => Some(*tool),
                _ => None,
            })
            .collect();
        let lowest = tools.iter().min().copied().unwrap_or(0);
        let highest = tools.iter().max().copied().unwrap_or(0);
        #[expect(
            clippy::cast_precision_loss,
            reason = "a tool number is a handful, far inside what an f64 holds exactly"
        )]
        return Err(RecoveryRejection::OutOfBounds {
            field: RecoveryField::Tool,
            requested: tool as f64,
            allowed: printobserver_types::Range::new(lowest as f64, highest as f64),
        });
    };
    if !(target_c.is_finite() && allowed.contains(target_c)) {
        return Err(RecoveryRejection::OutOfBounds {
            field: RecoveryField::TargetC,
            requested: target_c,
            allowed: *allowed,
        });
    }
    Ok(())
}

/// The nozzle is clear of the part: Z trusted, and either the operator's word
/// or the nozzle measured at least the margin above the tallest part.
fn clear_of_the_part(
    config: &RecoveryConfig,
    grants: &RecoveryGrants,
    class: ActorClass,
    action: &RecoveryAction,
    facts: &Facts,
) -> Result<(), RecoveryRejection> {
    let ZFacts::Trusted { position, tallest } = &facts.z else {
        return Err(RecoveryRejection::ZUntrusted);
    };
    if action.clearance_asserted() && grants.assertion_counts(class) {
        return Ok(());
    }
    let kind = action.kind();
    let at = position_of(position.as_ref(), kind)?;
    let tallest = match tallest {
        Some(Ok(tallest)) => *tallest,
        Some(Err(detail)) => {
            return Err(RecoveryRejection::PrinterCannot {
                action: kind,
                detail: format!("what the last print laid down could not be read: {detail}"),
            });
        }
        None => {
            return Err(RecoveryRejection::PrinterCannot {
                action: kind,
                detail: "what the last print laid down was not read".to_owned(),
            });
        }
    };
    if at.z < tallest + config.clearance_margin_mm {
        return Err(RecoveryRejection::InsufficientClearance {
            nozzle_z_mm: at.z,
            tallest_mm: tallest,
            margin_mm: config.clearance_margin_mm,
        });
    }
    Ok(())
}

/// Where the head is, or the refusal of an action that needs it and cannot
/// read it.
fn position_of(
    position: Option<&Result<AxisPosition, String>>,
    kind: RecoveryActionKind,
) -> Result<AxisPosition, RecoveryRejection> {
    match position {
        Some(Ok(at)) => Ok(*at),
        Some(Err(detail)) => Err(RecoveryRejection::PrinterCannot {
            action: kind,
            detail: format!("the nozzle's position could not be read: {detail}"),
        }),
        None => Err(RecoveryRejection::PrinterCannot {
            action: kind,
            detail: "the nozzle's position was not read".to_owned(),
        }),
    }
}

/// Whether deciding one action reads where the head is.
fn reads_position(action: &RecoveryAction, class: ActorClass, grants: &RecoveryGrants) -> bool {
    match action.kind() {
        RecoveryActionKind::RecoverLift => true,
        kind if kind.needs_clearance() => {
            !(action.clearance_asserted() && grants.assertion_counts(class))
        }
        _ => false,
    }
}

/// The request one status entry is decided as: the action's least demanding
/// form, carrying no assertion.
fn representative(kind: RecoveryActionKind, config: Option<&RecoveryConfig>) -> RecoveryAction {
    match kind {
        RecoveryActionKind::RecoverLift => RecoveryAction::RecoverLift {
            mm: config.map_or(1.0, |config| config.max_lift_mm.min(1.0)),
        },
        RecoveryActionKind::RecoverCool => RecoveryAction::RecoverCool {},
        RecoveryActionKind::RecoverFan => RecoveryAction::RecoverFan { percent: 0.0 },
        RecoveryActionKind::RecoverHeat => RecoveryAction::RecoverHeat {
            tool: 0,
            target_c: 0.0,
            clearance_asserted: false,
        },
        RecoveryActionKind::RecoverPark => RecoveryAction::RecoverPark {
            clearance_asserted: false,
        },
        RecoveryActionKind::RecoverHomeXy => RecoveryAction::RecoverHomeXy {
            clearance_asserted: false,
        },
        RecoveryActionKind::RecoverHomeZ => RecoveryAction::RecoverHomeZ {
            bed_clear_asserted: false,
        },
    }
}

/// Every heater one snapshot reports.
fn temperatures_of(snapshot: Option<&PrinterSnapshot>) -> RecoveryTemperatures {
    let Some(snapshot) = snapshot else {
        return RecoveryTemperatures {
            tools: Vec::new(),
            bed: None,
        };
    };
    let value = |reported: Option<&printobserver_types::Reported<f64>>| {
        reported.map(printobserver_types::Reported::value)
    };
    RecoveryTemperatures {
        tools: snapshot
            .tools
            .iter()
            .enumerate()
            .map(|(index, heater)| ToolTemperature {
                tool: i64::try_from(index).unwrap_or(i64::MAX),
                actual_c: value(heater.actual_c.as_ref()),
                target_c: value(heater.target_c.as_ref()),
            })
            .collect(),
        bed: snapshot.bed.as_ref().map(|bed| BedTemperature {
            actual_c: value(bed.actual_c.as_ref()),
            target_c: value(bed.target_c.as_ref()),
        }),
    }
}

/// What became of one recovery, in a sentence for a person: what it did, or
/// why it was refused and what would let it go ahead.
#[must_use]
pub fn recovery_detail(
    action: &RecoveryAction,
    decision: &RecoveryDecision,
    config: Option<&RecoveryConfig>,
) -> String {
    let kind = action.kind();
    let rejection = match decision {
        RecoveryDecision::Executed(ExecutionOutcome::Succeeded) => {
            return format!("{kind} was carried out");
        }
        RecoveryDecision::Executed(ExecutionOutcome::Failed { reason }) => {
            return format!("{kind} reached the printer, which refused it: {reason}");
        }
        RecoveryDecision::Rejected(rejection) => rejection,
    };
    match rejection {
        RecoveryRejection::ActorMayNotRequest { action } => format!(
            "{action} is not granted to this actor; `[policy.recovery]` in the server's \
             configuration says who may ask for which"
        ),
        RecoveryRejection::PrintRunning { state } => format!(
            "the printer is {state:?}, and nothing may move it under a running print; pause or \
             cancel the print first"
        ),
        RecoveryRejection::PrinterUnreachable => {
            "the printer cannot be read, or reports itself offline or in error".to_owned()
        }
        RecoveryRejection::ZUntrusted => format!(
            "{kind} moves the head over the part or heats the nozzle above it, and Z is not \
             trusted: home Z with `recover-home-z --bed-clear-asserted true` once the bed is \
             clear"
        ),
        RecoveryRejection::InsufficientClearance {
            nozzle_z_mm,
            tallest_mm,
            margin_mm,
        } => format!(
            "the nozzle is at Z {nozzle_z_mm} mm and the last print's tallest part is \
             {tallest_mm} mm, so it is not {margin_mm} mm above it: lift it with `recover-lift`, \
             or assert the clearance with `--clearance-asserted true` as the operator"
        ),
        RecoveryRejection::OutOfBounds {
            field,
            requested,
            allowed,
        } => format!(
            "{field:?} {requested} is outside the {} to {} allowed",
            allowed.min, allowed.max
        ),
        RecoveryRejection::AssertionRequired { field } => {
            let sweeps = config.is_some_and(|config| config.z_home_sweeps_bed);
            let said = if sweeps {
                "homing Z on this printer brings a gantry down over the whole bed"
            } else {
                "homing Z on this printer brings the nozzle down onto the bed"
            };
            format!(
                "{kind} needs `{}` true from the operator: {said}, and nothing on the bed may be \
                 in its way",
                match field {
                    RecoveryField::ClearanceAsserted => "clearance_asserted",
                    _ => "bed_clear_asserted",
                }
            )
        }
        RecoveryRejection::RecoveryNotConfigured => "the server's configuration carries no \
             `[recovery]` section stating this printer's facts, so no recovery can be checked"
            .to_owned(),
        RecoveryRejection::PrinterCannot { action, detail } => {
            format!("this printer cannot do what {action} needs: {detail}")
        }
    }
}

impl Supervisor {
    /// Whether Z is trusted now, and since when or why not.
    #[must_use]
    pub fn z_trust(&self) -> ZTrust {
        self.z_trust_held().clone()
    }

    /// Trust Z from now on: printobserver homed it, or started a print.
    pub(crate) fn trust_z(&self) {
        *self.z_trust_held() = ZTrust::TrustedSince(self.clock().now());
    }

    /// Stop trusting Z, saying why.
    pub(crate) fn distrust_z(&self, why: impl Into<String>) {
        *self.z_trust_held() = ZTrust::Untrusted(why.into());
    }

    /// Carry out one recovery action, or refuse it, recording either.
    ///
    /// # Errors
    ///
    /// Returns the store's own error when the request or what became of it
    /// could not be recorded. A refusal is not an error: it is recorded and
    /// answered in [`RecoveryIssued::decision`].
    pub async fn request_recovery(
        &self,
        action: RecoveryAction,
        actor: Actor,
        reason: String,
    ) -> Result<RecoveryIssued, CoreError> {
        let class = actor.class();
        let config = self.config().recovery;
        let grants = &self.config().recovery_grants;
        let granted =
            config.is_some() && self.config().recovery_grants.grants(class, action.kind());
        let facts = if granted {
            self.facts(
                reads_position(&action, class, grants),
                action.kind().needs_clearance()
                    && !(action.clearance_asserted() && grants.assertion_counts(class)),
            )
            .await
        } else {
            // A request refused before the printer matters reads nothing of it.
            Facts {
                snapshot: None,
                z: ZFacts::Untrusted,
            }
        };
        let calls = decide(
            config.as_ref(),
            &self.config().recovery_grants,
            &self.config().envelope,
            class,
            &action,
            &facts,
        );
        let requested = RecoveryRequestedPayload {
            action,
            actor,
            reason,
        };
        let under_a_pause = facts
            .snapshot
            .as_ref()
            .is_some_and(|snapshot| snapshot.connection == PrinterState::Paused);
        match self
            .issue_decided_action(Decided::Recovery {
                requested: Box::new(requested),
                calls,
            })
            .await?
        {
            IssuedAs::Recovery(issued) => {
                // The head moved under a paused print, which the printer will
                // not put back on resume.
                if under_a_pause && matches!(issued.decision, RecoveryDecision::Executed(_)) {
                    for print in self.stores().prints.open_prints().await? {
                        self.moved_while_paused()
                            .entry(print.id)
                            .or_insert(issued.settled.id);
                    }
                }
                Ok(issued)
            }
            // The chokepoint answers each request in its own kind, so this is
            // a chokepoint that has come apart from its callers.
            IssuedAs::Action(_) => Err(CoreError::Unrepresentable {
                detail: "a recovery was answered as a print action".to_owned(),
            }),
        }
    }

    /// Write one recovery request down, before anything reaches the machine.
    pub(crate) async fn write_recovery_request(
        &self,
        requested: &RecoveryRequestedPayload,
    ) -> Result<printobserver_types::EventRecord, CoreError> {
        let source = crate::actions::source_of(&requested.actor);
        let draft = self.draft_of(None, source, requested)?;
        Ok(self.stores().events.append_event(draft).await?)
    }

    /// Write down what became of one recovery the chokepoint has issued or
    /// refused, and trust Z after a Z home the printer took.
    pub(crate) async fn recovery_settled(
        &self,
        requested: RecoveryRequestedPayload,
        requested_event: printobserver_types::EventRecord,
        decision: RecoveryDecision,
    ) -> Result<RecoveryIssued, CoreError> {
        let source = crate::actions::source_of(&requested.actor);
        let RecoveryRequestedPayload {
            action,
            actor,
            reason,
        } = requested;
        let homed_z = matches!(action, RecoveryAction::RecoverHomeZ { .. })
            && decision == RecoveryDecision::Executed(ExecutionOutcome::Succeeded);
        let draft = match &decision {
            RecoveryDecision::Executed(outcome) => self.draft_of(
                None,
                source,
                &RecoveryExecutedPayload {
                    action,
                    actor,
                    reason,
                    outcome: outcome.clone(),
                },
            )?,
            RecoveryDecision::Rejected(rejection) => self.draft_of(
                None,
                source,
                &RecoveryRejectedPayload {
                    action,
                    actor,
                    reason,
                    rejection: rejection.clone(),
                },
            )?,
        };
        let settled = self.stores().events.append_event(draft).await?;
        if homed_z {
            self.trust_z();
        }
        Ok(RecoveryIssued {
            decision,
            requested: requested_event,
            settled,
        })
    }

    /// Everything a recovery is checked against now, and what each action
    /// would meet, for one actor class.
    ///
    /// # Errors
    ///
    /// Returns the store's own error when the prints or the events could not
    /// be read.
    pub async fn recovery_status(&self, class: ActorClass) -> Result<RecoveryStatus, CoreError> {
        // The store answers which print was last before anything is read of
        // the machine: a store that cannot is this server's own failure.
        self.stores().prints.prints().await?;
        let facts = self.facts(true, true).await;
        let config = self.config().recovery;
        let actions = RecoveryActionKind::ALL
            .iter()
            .map(|kind| ActionAllowance {
                action: *kind,
                refusal: decide(
                    config.as_ref(),
                    &self.config().recovery_grants,
                    &self.config().envelope,
                    class,
                    &representative(*kind, config.as_ref()),
                    &facts,
                )
                .err(),
            })
            .collect();
        let state = facts
            .snapshot
            .as_ref()
            .map_or(PrinterState::Offline, |snapshot| {
                snapshot.connection.clone()
            });
        let paused_print = if state == PrinterState::Paused {
            self.paused_print().await?
        } else {
            None
        };
        Ok(RecoveryStatus {
            state,
            z_trust: self.z_trust(),
            position: match facts.z {
                ZFacts::Trusted {
                    position: Some(Ok(at)),
                    ..
                } => Some(RecoveryPosition {
                    x: at.x,
                    y: at.y,
                    z: at.z,
                }),
                _ => None,
            },
            temperatures: temperatures_of(facts.snapshot.as_ref()),
            last_print: self.last_print_extent().await.ok().flatten().and_then(
                |(print_id, extent)| {
                    extent.map(|extent| LastPrint {
                        print_id,
                        tallest_mm: extent.tallest_mm,
                        footprint: extent.footprint,
                    })
                },
            ),
            paused_print,
            z_home: config.map(|config| ZHome {
                x: config.z_home_xy[0],
                y: config.z_home_xy[1],
                sweeps_bed: config.z_home_sweeps_bed,
            }),
            actions,
        })
    }

    /// Read what a recovery is decided from: the printer's state, and — only
    /// while Z is trusted — where the head is and what the last print laid
    /// down, each when asked for.
    async fn facts(&self, position: bool, tallest: bool) -> Facts {
        let snapshot = self.read_snapshot().await.ok();
        // A printer found offline or in error by this read may have reset or
        // moved since, however briefly, and between two of the watch's reads.
        if let Some(down) = snapshot
            .as_ref()
            .map(|snapshot| &snapshot.connection)
            .filter(|state| matches!(state, PrinterState::Offline | PrinterState::Error))
        {
            self.distrust_z(format!(
                "the printer was read {down:?}, and Z may have moved since"
            ));
        }
        let trusted = matches!(self.z_trust(), ZTrust::TrustedSince(_));
        let reachable = snapshot.as_ref().is_some_and(|snapshot| {
            matches!(
                snapshot.connection,
                PrinterState::Operational | PrinterState::Paused
            )
        });
        let z = if trusted {
            ZFacts::Trusted {
                position: if position && reachable {
                    Some(
                        self.read_position()
                            .await
                            .map_err(|error| error.to_string()),
                    )
                } else {
                    None
                },
                tallest: if tallest && reachable {
                    Some(match self.last_print_extent().await {
                        Ok(Some((_, Some(extent)))) => Ok(extent.tallest_mm),
                        Ok(None | Some((_, None))) => Ok(0.0),
                        Err(error) => Err(error.to_string()),
                    })
                } else {
                    None
                },
            }
        } else {
            ZFacts::Untrusted
        };
        Facts { snapshot, z }
    }

    /// The last print with a file, and what its G-code laid down up to the
    /// last file position recorded for it: nothing when no print has a file.
    async fn last_print_extent(
        &self,
    ) -> Result<Option<(PrintId, Option<printobserver_printer_api::PrintedExtent>)>, CoreError>
    {
        let prints = self.stores().prints.prints().await?;
        let Some((print, file_name)) = prints
            .iter()
            .filter_map(|print| {
                let file_name = FileName::new(print.file_name.as_deref()?).ok()?;
                Some((print, file_name))
            })
            .max_by_key(|(print, _)| print.opened_at)
        else {
            return Ok(None);
        };
        let position = self
            .stores()
            .filament
            .file_position(print.id)
            .await?
            .map_or(0, |position| position.bytes);
        // A print no read found past its first byte sent nothing that could
        // lay material down, so its file need not be read.
        if position <= 0 {
            return Ok(Some((print.id, None)));
        }
        let key = (print.id, position);
        if let Some((cached, extent)) = self.recovery_extents().as_ref()
            && *cached == key
        {
            return Ok(Some((print.id, *extent)));
        }
        let extent = self
            .read_printed_extent(file_name, position)
            .await
            .map_err(CoreError::Printer)?;
        *self.recovery_extents() = Some((key, extent));
        Ok(Some((print.id, extent)))
    }

    /// The open print the printer is paused under, and whether it is still
    /// resumable.
    async fn paused_print(&self) -> Result<Option<PausedPrint>, CoreError> {
        let Some(print) = self.stores().prints.open_prints().await?.into_iter().next() else {
            return Ok(None);
        };
        Ok(Some(PausedPrint {
            print_id: print.id,
            resumable: self.recovery_while_open(&print).await?.is_none(),
        }))
    }

    /// The recovery executed under one print while it was paused: what makes
    /// it non-resumable.
    ///
    /// This supervisor notes each recovery it executes under a paused print.
    /// What an earlier supervisor executed under a print still open it cannot
    /// tell the printer's state for, so for a print opened before this one
    /// started, any recovery recorded between the print opening and this
    /// supervisor starting counts.
    pub(crate) async fn recovery_while_open(
        &self,
        print: &PrintRecord,
    ) -> Result<Option<EventId>, CoreError> {
        if print.ended_at.is_some() {
            return Ok(None);
        }
        if let Some(moved) = self.moved_while_paused().get(&print.id) {
            return Ok(Some(*moved));
        }
        if print.opened_at >= self.started().wall {
            return Ok(None);
        }
        let executed = EventKind::new(RecoveryExecutedPayload::KIND).map_err(|error| {
            CoreError::Unrepresentable {
                detail: error.to_string(),
            }
        })?;
        Ok(self
            .stores()
            .timeline
            .timeline(TimelineQuery {
                kinds: vec![executed],
                since: Some(print.opened_at),
                until: Some(self.started().wall),
                spool_id: None,
                before: None,
                limit: Some(1),
            })
            .await?
            .into_iter()
            .next()
            .map(|event| event.id))
    }
}
