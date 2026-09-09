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
    ImageRecord, Intervention, JobManifest, JobSnapshot, ManifestNarrowing, PrintContext,
    PrintRecord, PrinterSnapshot, SupervisionSession,
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

/// The reason a mutating request carries, refused when it carries none.
///
/// One rule for every mutating operation, whether it asks something of the
/// machine or writes a record: a change nobody gave a reason for is one the
/// history cannot account for afterwards, and the answer to it is the same
/// refusal whichever operation it arrived at.
///
/// # Errors
///
/// Returns [`BodyRefusal`] when the reason is absent or is nothing but
/// whitespace.
pub fn reason_of(reason: Option<&str>) -> Result<String, BodyRefusal> {
    match reason.map(str::trim) {
        Some(reason) if !reason.is_empty() => Ok(reason.to_owned()),
        _ => Err(BodyRefusal {
            detail: "every mutating request carries a `reason`, and this one carries none"
                .to_owned(),
        }),
    }
}

/// One request to replace a print's manifest.
///
/// It carries a reason for the same reason every action does: a manifest
/// narrows what any actor may ask for, so replacing one is a change to the
/// bounds a print runs under rather than a note about it.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(crate = "printobserver_types::serde", deny_unknown_fields)]
pub struct ManifestBody {
    /// Why the manifest is being replaced.
    #[serde(default)]
    pub reason: Option<String>,
    /// The manifest to write.
    pub manifest: JobManifest,
}

impl ManifestBody {
    /// The reason this request carries, refused when it carries none.
    ///
    /// # Errors
    ///
    /// Returns [`BodyRefusal`] when the reason is absent or is nothing but
    /// whitespace. It is checked **before** the manifest is written, so a
    /// request without one leaves the stored manifest and the print's
    /// narrowings exactly as they were.
    pub fn reason(&self) -> Result<String, BodyRefusal> {
        reason_of(self.reason.as_deref())
    }
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
        reason_of(self.reason.as_deref())
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
    /// Every range this manifest asked wider than the envelope allows, narrowed
    /// to the envelope's — recorded on the print, so nobody has to wonder later
    /// which bound applied.
    pub narrowings: Vec<ManifestNarrowing>,
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
///
/// It carries no field, because the only thing it could carry is that the post
/// was accepted and this is the answer only an accepted post gets. It
/// serializes as `{"accepted": true}`, which is what a caller reads.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct IngressAnswer;

impl Serialize for IngressAnswer {
    fn serialize<S: printobserver_types::serde::Serializer>(
        &self,
        serializer: S,
    ) -> Result<S::Ok, S::Error> {
        use printobserver_types::serde::ser::SerializeStruct as _;
        let mut state = serializer.serialize_struct("IngressAnswer", 1)?;
        state.serialize_field("accepted", &true)?;
        state.end()
    }
}

#[cfg(test)]
mod tests {
    use printobserver_types::contract::Sample as _;
    use printobserver_types::{ActionKind, Actor, ImageRecord};

    use super::{ActionBody, ErrorAnswer, ImageAnswer, ImageLookup};

    /// A body carrying nothing but a reason and an actor.
    fn bare() -> ActionBody {
        ActionBody {
            reason: Some("a test is asking".to_owned()),
            actor: Actor::Operator,
            duration_s: None,
            factor: None,
            target_c: None,
            percent: None,
            tool: None,
            file_name: None,
            manifest: None,
            event_id: None,
            disposition: None,
        }
    }

    /// Every action that needs a value it was not given is refused naming it.
    ///
    /// The walk is over the whole vocabulary rather than a sample: an action
    /// that quietly defaulted a value nobody sent would be one this server
    /// carried to a printer on the caller's behalf.
    #[test]
    fn every_action_that_needs_a_value_it_was_not_given_is_refused_naming_it() {
        let needed = [
            (ActionKind::StartPrint, "file_name"),
            (ActionKind::SetFeedrateFactor, "factor"),
            (ActionKind::SetFlowrateFactor, "factor"),
            (ActionKind::SetToolTargetC, "tool"),
            (ActionKind::SetBedTargetC, "target_c"),
            (ActionKind::SetFanPercent, "percent"),
            (ActionKind::AcknowledgeFailure, "event_id"),
        ];
        for (kind, named) in needed {
            let refusal = bare()
                .into_action(kind)
                .expect_err("this action needs a value the body does not carry");
            assert!(
                refusal.to_string().contains(named),
                "{kind:?} was refused without naming `{named}`: {refusal}"
            );
        }
    }

    /// The second value each two-valued action needs is named in its turn.
    #[test]
    fn the_second_value_a_two_valued_action_needs_is_named_in_its_turn() {
        let mut starting = bare();
        starting.file_name = Some(printobserver_types::FileName::new("a.gcode").expect("a name"));
        assert!(
            starting
                .into_action(ActionKind::StartPrint)
                .expect_err("a start needs a manifest")
                .to_string()
                .contains("manifest")
        );

        let mut tool = bare();
        tool.tool = Some(0);
        assert!(
            tool.into_action(ActionKind::SetToolTargetC)
                .expect_err("a tool target needs a temperature")
                .to_string()
                .contains("target_c")
        );

        let mut acknowledgement = bare();
        acknowledgement.event_id = Some(printobserver_types::EventId::new());
        assert!(
            acknowledgement
                .into_action(ActionKind::AcknowledgeFailure)
                .expect_err("an acknowledgement needs a disposition")
                .to_string()
                .contains("disposition")
        );
    }

    /// A reason that is nothing but whitespace is no reason.
    #[test]
    fn a_reason_that_is_nothing_but_whitespace_is_no_reason() {
        let mut blank = bare();
        blank.reason = Some("   ".to_owned());
        assert!(blank.reason().is_err());
        let mut absent = bare();
        absent.reason = None;
        assert!(absent.reason().is_err());
        assert_eq!(bare().reason().expect("a reason"), "a test is asking");
    }

    /// An image whose file is gone answers the record and no path.
    ///
    /// A different answer from there being no such image, which is what tells a
    /// caller the history is intact and the file is not.
    #[test]
    fn an_image_whose_file_is_gone_answers_the_record_and_no_path() {
        let record = ImageRecord::sample_full();
        let missing = ImageAnswer::from(ImageLookup::FileMissing {
            record: record.clone(),
        });
        assert_eq!(missing.record, record);
        assert_eq!(missing.path, None);

        let found = ImageAnswer::from(ImageLookup::Found {
            record: record.clone(),
            path: "/var/lib/printobserver/images/ab/cd".into(),
        });
        assert_eq!(
            found.path.as_deref(),
            Some(std::path::Path::new("/var/lib/printobserver/images/ab/cd"))
        );
    }

    /// A refusal answers in the words of whatever refused it.
    #[test]
    fn a_refusal_answers_in_the_words_of_whatever_refused_it() {
        assert_eq!(
            ErrorAnswer::saying("the disk is full").error,
            "the disk is full"
        );
    }
}
