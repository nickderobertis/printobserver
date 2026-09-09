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

use printobserver_types::serde_json::{self, Value};

use crate::config::{ClientConfig, load};
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
fn rejection_note(document: &Value) -> String {
    let refusal = document.pointer("/record/decision/rejected");
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
