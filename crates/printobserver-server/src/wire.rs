//! The JSON a caller sends and the JSON this server answers with.
//!
//! Nothing here restates a contract type. A request carries the values one
//! [`PrintAction`] variant needs and this module assembles the action; an
//! answer carries the contract's own records as they are. In particular **no
//! answer carries image bytes**: [`ImageAnswer`] carries the record and the
//! absolute path its bytes are at, because the server, the command-line program
//! and the agent all run on the one host and image transport is a filesystem
//! path.

use std::path::PathBuf;

use printobserver_store_api::ImageLookup;
use printobserver_types::serde::{Deserialize, Serialize};
use printobserver_types::{
    AcknowledgementDisposition, ActionKind, ActionRecord, Actor, EventId, EventRecord, FileName,
    ImageRecord, Intervention, JobManifest, JobSnapshot, PrintContext, PrintRecord,
    PrinterSnapshot, SupervisionSession,
};

/// Why a request could not be turned into one action of the vocabulary.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BodyRefusal {
    /// What is missing or wrong, named so the caller can send it.
    pub detail: String,
}

impl core::fmt::Display for BodyRefusal {
    fn fmt(&self, formatter: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        formatter.write_str(&self.detail)
    }
}

/// One request for one action of the vocabulary.
///
/// Every mutating operation takes this shape: the reason it carries, who is
/// asking, the optional duration a bounded change stands for, and whichever of
/// the values the asked-for action needs. A field the action does not need is
/// ignored; a field it needs and the request omits is refused naming it, and
/// **a request carrying no reason is refused before anything reaches the
/// printer or the record**.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(crate = "printobserver_types::serde", deny_unknown_fields)]
pub struct ActionBody {
    /// Why the actor is asking for this.
    #[serde(default)]
    pub reason: Option<String>,
    /// Who is asking.
    pub actor: Actor,
    /// How long the change stands for, in whole seconds.
    #[serde(default)]
    pub duration_s: Option<i64>,
    /// The multiplier a feedrate or flowrate adjustment asks for.
    #[serde(default)]
    pub factor: Option<f64>,
    /// The temperature a heater adjustment asks for.
    #[serde(default)]
    pub target_c: Option<f64>,
    /// The percentage a fan adjustment asks for.
    #[serde(default)]
    pub percent: Option<f64>,
    /// The tool a tool adjustment is about.
    #[serde(default)]
    pub tool: Option<i64>,
    /// The file a start asks for.
    #[serde(default)]
    pub file_name: Option<FileName>,
    /// The manifest a start is bounded by.
    #[serde(default)]
    pub manifest: Option<JobManifest>,
    /// The event an acknowledgement is about.
    #[serde(default)]
    pub event_id: Option<EventId>,
    /// What an acknowledgement asks for next.
    #[serde(default)]
    pub disposition: Option<AcknowledgementDisposition>,
}

/// One value a request had to carry and did not.
fn missing<T>(named: &str) -> Result<T, BodyRefusal> {
    Err(BodyRefusal {
        detail: format!("this operation needs `{named}`, and the request carries none"),
    })
}

impl ActionBody {
    /// The reason this request carries, refused when it carries none.
    ///
    /// # Errors
    ///
    /// Returns [`BodyRefusal`] when the reason is absent or is nothing but
    /// whitespace. Every mutating operation carries a reason, and this is
    /// checked before the action is assembled, so a request without one reaches
    /// neither the policy, the printer nor the record.
    pub fn reason(&self) -> Result<String, BodyRefusal> {
        match self.reason.as_deref().map(str::trim) {
            Some(reason) if !reason.is_empty() => Ok(reason.to_owned()),
            _ => Err(BodyRefusal {
                detail: "every mutating request carries a `reason`, and this one carries none"
                    .to_owned(),
            }),
        }
    }

    /// The action this request asks for.
    ///
    /// # Errors
    ///
    /// Returns [`BodyRefusal`] naming the value the asked-for action needs and
    /// the request does not carry, and naming the absent reason before
    /// anything else.
    pub fn into_action(
        self,
        kind: ActionKind,
    ) -> Result<printobserver_types::PrintAction, BodyRefusal> {
        use printobserver_types::PrintAction as Action;

        let reason = self.reason()?;
        let actor = self.actor.clone();
        let duration_s = self.duration_s;
        Ok(match kind {
            ActionKind::Pause => Action::Pause { reason, actor },
            ActionKind::Resume => Action::Resume { reason, actor },
            ActionKind::Cancel => Action::Cancel { reason, actor },
            ActionKind::StartPrint => Action::StartPrint {
                file_name: match self.file_name {
                    Some(name) => name,
                    None => return missing("file_name"),
                },
                manifest: match self.manifest {
                    Some(manifest) => manifest,
                    None => return missing("manifest"),
                },
                reason,
                actor,
            },
            ActionKind::SetFeedrateFactor => Action::SetFeedrateFactor {
                factor: match self.factor {
                    Some(factor) => factor,
                    None => return missing("factor"),
                },
                duration_s,
                reason,
                actor,
            },
            ActionKind::SetFlowrateFactor => Action::SetFlowrateFactor {
                factor: match self.factor {
                    Some(factor) => factor,
                    None => return missing("factor"),
                },
                duration_s,
                reason,
                actor,
            },
            ActionKind::SetToolTargetC => Action::SetToolTargetC {
                tool: match self.tool {
                    Some(tool) => tool,
                    None => return missing("tool"),
                },
                target_c: match self.target_c {
                    Some(target) => target,
                    None => return missing("target_c"),
                },
                duration_s,
                reason,
                actor,
            },
            ActionKind::SetBedTargetC => Action::SetBedTargetC {
                target_c: match self.target_c {
                    Some(target) => target,
                    None => return missing("target_c"),
                },
                duration_s,
                reason,
                actor,
            },
            ActionKind::SetFanPercent => Action::SetFanPercent {
                percent: match self.percent {
                    Some(percent) => percent,
                    None => return missing("percent"),
                },
                duration_s,
                reason,
                actor,
            },
            ActionKind::AcknowledgeFailure => Action::AcknowledgeFailure {
                event_id: match self.event_id {
                    Some(event_id) => event_id,
                    None => return missing("event_id"),
                },
                disposition: match self.disposition {
                    Some(disposition) => disposition,
                    None => return missing("disposition"),
                },
                reason,
                actor,
            },
        })
    }
}

/// What one mutating request left behind it.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(crate = "printobserver_types::serde")]
pub struct ActionAnswer {
    /// The record of the request and the decision taken on it. A rejected
    /// request's rejection is here, carrying its reason, the value asked for
    /// and the range allowed, so a caller can ask again inside the range.
    pub record: ActionRecord,
    /// The bounded intervention it opened, when it opened one.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub intervention: Option<Intervention>,
    /// What the printer said, when the request reached it and it refused.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub printer_refusal: Option<String>,
}

/// What a status read answers.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(crate = "printobserver_types::serde")]
pub struct StatusAnswer {
    /// The print.
    pub print: PrintRecord,
    /// The printer, when a snapshot could be taken.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub printer: Option<PrinterSnapshot>,
    /// The job, when a snapshot could be taken.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub job: Option<JobSnapshot>,
    /// The supervision session watching it, when one is.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub session: Option<SupervisionSession>,
    /// The bounded interventions still in force.
    pub interventions: Vec<Intervention>,
}

/// What a context read answers.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(crate = "printobserver_types::serde")]
pub struct ContextAnswer {
    /// Everything a supervision turn is given about the print.
    pub context: PrintContext,
}

/// What an image read answers.
///
/// The path is absolute on this server's own filesystem, and is the whole of
/// the answer about the bytes: nothing here renders them, encodes them or
/// serves them. It is absent when the record is intact and the file is not,
/// which is a different answer from there being no such image.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(crate = "printobserver_types::serde")]
pub struct ImageAnswer {
    /// The record.
    pub record: ImageRecord,
    /// Where its bytes are, absolute on this host.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub path: Option<PathBuf>,
}

impl From<ImageLookup> for ImageAnswer {
    fn from(lookup: ImageLookup) -> Self {
        match lookup {
            ImageLookup::Found { record, path } => Self {
                record,
                path: Some(path),
            },
            ImageLookup::FileMissing { record } => Self { record, path: None },
        }
    }
}

/// What a history read answers.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(crate = "printobserver_types::serde")]
pub struct HistoryAnswer {
    /// The print's events, newest first.
    pub events: Vec<EventRecord>,
}

/// What a manifest read or write answers.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(crate = "printobserver_types::serde")]
pub struct ManifestAnswer {
    /// The manifest, when the print has one.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub manifest: Option<JobManifest>,
}

/// What this server answers when it will not do what it was asked.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(crate = "printobserver_types::serde")]
pub struct ErrorAnswer {
    /// One line saying why, in the words of whatever refused it.
    pub error: String,
}

impl ErrorAnswer {
    /// One refusal, in its own words.
    pub fn saying(detail: impl core::fmt::Display) -> Self {
        Self {
            error: detail.to_string(),
        }
    }
}

/// What the ingress answers a post it accepted.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(crate = "printobserver_types::serde")]
pub struct IngressAnswer {
    /// Whether the body was taken for handling.
    pub accepted: bool,
}
