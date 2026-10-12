//! One client command, from the configuration it reads to the exit it earns.
//!
//! # Three steps, and each one is separable
//!
//! [`produce`] makes the request and answers the document the supervisor sent
//! together with the exit it earns; [`present`] renders that document.
//! Splitting them is what lets this crate's own tier build a command variant
//! that is this program with one defect in it and drive it through the same
//! assertions — a variant that substitutes a value, or that puts something in
//! the output the answer never carried, is refused by those assertions rather
//! than passing for want of a violation to find.
//!
//! # The answer is the output
//!
//! Every value either rendering carries is a value the supervisor's own answer
//! carried. Nothing here computes a field, derives one from an image's
//! content, or adds one — with the single exception stated at
//! [`image_elsewhere`], which replaces a path that names no file on this host
//! with a message saying so, because handing a caller a file name that names
//! nothing is worse than telling them why.

use std::path::Path;

use printobserver_server::CREDENTIAL_ISSUE;
use printobserver_types::serde_json::{self, Value};

use crate::config::{CREDENTIAL_ENV, ClientConfig, DEFAULT_CONFIG_PATH, load};
use crate::failure::{Exit, Failure};
use crate::parse::Call;
use crate::render::{Rendering, render};
use crate::transport::send;

/// The suffix the field carrying an unusable image path is renamed with.
pub const UNAVAILABLE_SUFFIX: &str = "_unavailable";

/// The status a rejected action is answered under.
const REJECTED_STATUS: u16 = 409;

/// The status an action the machine itself refused is answered under.
const MACHINE_REFUSED_STATUS: u16 = 502;

/// The status a request that did not present the credential in force is
/// answered under.
const UNAUTHENTICATED_STATUS: u16 = 401;

/// What one call produced.
#[derive(Debug, Clone, PartialEq)]
pub struct Produced {
    /// The document the supervisor answered, as this program will print it.
    pub document: Value,
    /// What the caller's shell sees.
    pub exit: Exit,
    /// What to say beside the document, when there is something to say.
    pub note: Option<String>,
}

/// What one finished command prints and exits with.
#[derive(Debug, Clone, PartialEq)]
pub struct Outcome {
    /// What goes to standard output.
    pub out: String,
    /// What goes to standard error.
    pub err: String,
    /// What the caller's shell sees.
    pub exit: Exit,
}

/// Make one call and read what came back.
///
/// # Errors
///
/// Returns [`Failure`] when nothing answered at the configured address and
/// when the supervisor answered something this program will not act on.
pub fn produce(call: &Call, config: &ClientConfig) -> Result<Produced, Failure> {
    let operation = call
        .command
        .operation
        .as_ref()
        .ok_or_else(|| Failure::of(Exit::Usage, "this command makes no request"))?;
    let answered = send(
        config,
        operation.method.as_str(),
        &call.target(),
        call.body().as_deref(),
    )
    .map_err(|unreachable| Failure::of(Exit::Unreachable, unreachable))?;
    let document: Value = serde_json::from_str(&answered.body).map_err(|error| {
        Failure::of(
            Exit::Refused,
            format!(
                "the supervisor answered {} with something this program cannot read: {error}",
                answered.status
            ),
        )
    })?;

    match answered.status {
        200 => Ok(image_elsewhere(
            operation.image_path_field,
            config,
            Produced {
                document,
                exit: Exit::Success,
                note: None,
            },
        )),
        // The rejection is the answer: its reason, the value asked for and the
        // range allowed are in the document, which is what lets the next
        // request be one the policy accepts.
        REJECTED_STATUS => Ok(Produced {
            note: Some(rejection_note(&document)),
            document,
            exit: Exit::Rejected,
        }),
        // The policy accepted it and the machine did not. That is a request
        // that was made and refused rather than one that never happened, so the
        // record is still the answer.
        MACHINE_REFUSED_STATUS => Ok(Produced {
            note: Some(machine_note(&document)),
            document,
            exit: Exit::Refused,
        }),
        // The supervisor is there and will not serve this program as it is
        // configured, which is a configuration to change rather than a request
        // to change — so it exits the way a program nothing configured does, and
        // says where the credential is read from.
        UNAUTHENTICATED_STATUS => Err(Failure::of(
            Exit::Unconfigured,
            unauthenticated_note(call, config),
        )),
        status => Err(Failure::of(
            Exit::Refused,
            format!(
                "the supervisor answered {status}: {}. Ask again for something it serves",
                document
                    .get("error")
                    .and_then(Value::as_str)
                    .unwrap_or("it said nothing this program can read")
            ),
        )),
    }
}

/// The answer to an action the policy refused, in the policy's own words.
///
/// The rejection's own name, and — where the policy ruled on a value — the
/// value that was asked for beside the range that is allowed. All three are
/// read out of the answer rather than restated here, because a caller that
/// cannot see the range cannot ask again for something inside it.
///
/// A start the filament check refused, and a spool command the ledger could not
/// carry out, are answered with what to do about them instead: the spool, its
/// figures and its mark, read out of the answer in the same way, and the
/// commands that settle them. Asking again inside a range is not the next step
/// for a spool that is short.
fn rejection_note(document: &Value) -> String {
    // A print action's answer carries its decision in its record; a spool
    // command's answer carries it at the top.
    let refusal = document
        .pointer("/record/decision/rejected")
        .or_else(|| document.pointer("/decision/rejected"));
    if let Some(note) = refusal.and_then(spool_note) {
        return note;
    }
    let named = |at: &str| {
        refusal
            .and_then(|rejected| rejected.pointer(at))
            .map(ToString::to_string)
    };
    let why = refusal
        .and_then(Value::as_object)
        .and_then(|reason| reason.keys().next())
        .map_or_else(String::new, |why| format!(": {why}"));
    let ruled = match (
        named("/out_of_bounds/requested"),
        named("/out_of_bounds/allowed"),
    ) {
        (Some(asked), Some(allowed)) => {
            format!(". It was asked for {asked}, and what is allowed is {allowed}")
        }
        _ => String::new(),
    };
    format!(
        "the supervisor's policy refused this action{why}{ruled}. Ask again inside what the \
         answer says is allowed"
    )
}

/// The option a start goes ahead despite the filament check with, as it is
/// written on a command line.
const OVERRIDE: &str = "--override-filament-check true";

/// What a refusal of the filament check or of the spool ledger says to do
/// next, read out of the refusal; nothing for any other refusal.
fn spool_note(refusal: &Value) -> Option<String> {
    let (reason, said) = refusal.as_object()?.iter().next()?;
    let text = |at: &str| {
        said.get(at)
            .and_then(Value::as_str)
            .unwrap_or("?")
            .to_owned()
    };
    let grams = |at: &str| {
        said.get(at)
            .and_then(Value::as_f64)
            .map_or_else(|| "?".to_owned(), |grams| format!("{grams:.1}"))
    };
    let tool = said
        .get("tool")
        .map_or_else(|| "?".to_owned(), ToString::to_string);
    // A mark is its own reason, named the way the ledger names it.
    let mark = |at: &str| {
        said.get(at)
            .and_then(|mark| mark.get("reason"))
            .and_then(Value::as_str)
            .map(str::to_owned)
    };
    Some(match reason.as_str() {
        "insufficient_filament" => {
            let doubt = mark("unverified").map_or_else(String::new, |mark| {
                format!(", and its figure is unverified ({mark})")
            });
            format!(
                "the supervisor's filament check refused this start: insufficient_filament. Spool \
                 `{}` on tool {tool} holds {} g, and this print needs {} g plus a {} g buffer{doubt}. \
                 Load a spool holding enough, weigh this one and run `adjust-spool`, or start \
                 again with `{OVERRIDE}`",
                text("spool_name"),
                grams("remaining_g"),
                grams("required_g"),
                grams("buffer_g"),
            )
        }
        "unverified_spool" => format!(
            "the supervisor's filament check refused this start: unverified_spool. Spool `{}` on \
             tool {tool} is unverified ({}). Weigh it and run `adjust-spool`, or start again with \
             `{OVERRIDE}`",
            text("spool_name"),
            mark("reason").unwrap_or_else(|| "?".to_owned()),
        ),
        "analysis_unavailable" => format!(
            "the supervisor's filament check refused this start: analysis_unavailable. OctoPrint \
             had no analysis of `{}` after this start waited {} s for one. Start again once \
             OctoPrint has analysed the file, or with `{OVERRIDE}`",
            text("file_name"),
            said.get("waited_s")
                .map_or_else(|| "?".to_owned(), ToString::to_string),
        ),
        "spool_refused" => format!(
            "the supervisor's spool ledger refused this command: spool_refused. {}",
            text("detail")
        ),
        _ => return None,
    })
}

/// What a program the supervisor would not authenticate is told.
///
/// Where the credential is read from, and never what it was: the configuration
/// file this invocation read — the one it named, or the default — and the
/// variable that wins over it.
fn unauthenticated_note(call: &Call, config: &ClientConfig) -> String {
    let file = call.config.as_deref().map_or_else(
        || {
            crate::locations::operator_client_config().map_or_else(
                || DEFAULT_CONFIG_PATH.to_owned(),
                |path| path.display().to_string(),
            )
        },
        |path| path.display().to_string(),
    );
    let presented = if config.credential.is_some() {
        "refused the credential this program presented"
    } else {
        "requires a credential, and this program was configured with none"
    };
    format!(
        "the supervisor at {} {presented}. Run `{CREDENTIAL_ISSUE}`, which writes a credential \
         into your own client configuration and prints the `api.credential_verifier` line to \
         put into the supervisor's configuration before restarting it; or set `credential` in \
         the `[client]` table of {file}, or {CREDENTIAL_ENV}, to a credential whose verifier the \
         supervisor is configured with",
        config.address()
    )
}

/// The answer to an action the machine itself refused.
fn machine_note(document: &Value) -> String {
    format!(
        "the policy accepted this action and the machine refused it: {}. Look at the \
         printer, then ask again",
        document
            .get("printer_refusal")
            .and_then(Value::as_str)
            .unwrap_or("it said nothing this program can read")
    )
}

/// Replace an image path that names no file on this host with a message.
///
/// The supervisor answers an absolute path on **its own** filesystem and no
/// route of it answers image bytes, so a caller running elsewhere cannot open
/// what it was handed. The rest of the answer is printed — the printer state,
/// the job, the bounds and the history are what most decisions turn on and
/// none of them needs the image — and the path is not, because a path that
/// names nothing here is worse than a sentence saying why.
///
/// The condition is whether the path exists here, deliberately and not whether
/// the configured address is somewhere else: a supervisor on another host
/// sharing a filesystem hands back a path that genuinely does open.
fn image_elsewhere(
    field: Option<&'static str>,
    config: &ClientConfig,
    produced: Produced,
) -> Produced {
    let Some(field) = field else {
        return produced;
    };
    let Some(path) = produced
        .document
        .get(field)
        .and_then(Value::as_str)
        .map(str::to_owned)
    else {
        return produced;
    };
    if Path::new(&path).exists() {
        return produced;
    }
    let said = format!(
        "this program transports no image bytes, and the path the supervisor answered names \
         a file on the host serving {} rather than a file on this one. Run this command on \
         that host, or point this program at a supervisor serving on this one.",
        config.address()
    );
    let mut document = produced.document;
    if let Some(object) = document.as_object_mut() {
        object.remove(field);
        object.insert(
            format!("{field}{UNAVAILABLE_SUFFIX}"),
            Value::String(said.clone()),
        );
    }
    Produced {
        document,
        exit: Exit::ImageElsewhere,
        note: Some(said),
    }
}

/// One produced answer, in the rendering the caller asked for.
#[must_use]
pub fn present(call: &Call, produced: &Produced) -> Outcome {
    Outcome {
        out: render(
            &produced.document,
            Rendering::asked_for(call.machine_readable),
        ),
        err: produced
            .note
            .as_ref()
            .map(|note| format!("{note}\n"))
            .unwrap_or_default(),
        exit: produced.exit,
    }
}

/// One client command, from end to end.
#[must_use]
pub fn perform(call: &Call) -> Outcome {
    let config = match load(call.config.as_deref()) {
        Ok(config) => config,
        Err(unconfigured) => return refusal(&Failure::of(Exit::Unconfigured, unconfigured)),
    };
    match produce(call, &config) {
        Ok(produced) => present(call, &produced),
        Err(failure) => refusal(&failure),
    }
}

/// One failure, as the program's own output.
#[must_use]
pub fn refusal(failure: &Failure) -> Outcome {
    Outcome {
        out: String::new(),
        err: format!("printobserver: {}\n", failure.detail),
        exit: failure.exit,
    }
}

#[cfg(test)]
mod tests {
    use printobserver_types::serde_json::json;

    use super::rejection_note;

    /// A start refused for a short spool says the spool, its figures and the
    /// next steps — and, when the short spool is also in doubt, its mark.
    #[test]
    fn a_short_spool_note_carries_its_figures_and_its_mark_when_it_has_one() {
        let short = |unverified: Option<&str>| {
            let mut refused = json!({
                "tool": 0,
                "spool_id": "01a12bd0-e46a-7a32-8d69-82a6f9ca508f",
                "spool_name": "PLA nearly empty",
                "required_g": 5.96,
                "buffer_g": 5.0,
                "remaining_g": 1.0,
            });
            if let Some(mark) = unverified {
                refused["unverified"] = json!({ "reason": mark, "print_id": "x" });
            }
            json!({ "record": { "decision": { "rejected": { "insufficient_filament": refused } } } })
        };

        let doubted = rejection_note(&short(Some("uncharged_print")));
        for said in [
            "insufficient_filament",
            "Spool `PLA nearly empty` on tool 0 holds 1.0 g",
            "needs 6.0 g plus a 5.0 g buffer, and its figure is unverified (uncharged_print)",
            "adjust-spool",
            "--override-filament-check true",
        ] {
            assert!(doubted.contains(said), "{doubted}");
        }

        let verified = rejection_note(&short(None));
        assert!(!verified.contains("unverified"), "{verified}");
        assert!(
            verified.contains("needs 6.0 g plus a 5.0 g buffer. Load"),
            "{verified}"
        );
    }
}
