//! This program with one defect in it, for the tier that has to catch one.
//!
//! Every assertion this crate's own tier makes about a command — the endpoint
//! its process tree reaches, the request the server received, the effect read
//! back, and what its output may carry — is an assertion nobody has proven
//! catches a violation until it has been driven over one. This binary is that
//! violation: the same library the program itself is built from, with exactly
//! one thing done wrong, chosen by the variable [`DEFECT`] names.
//!
//! It is built only under the `test-fixtures` feature, so nothing an ordinary
//! consumer installs carries it.

use std::io::Write as _;
use std::process::ExitCode;

use printobserver::client::{Produced, present, produce, refusal};
use printobserver::config::load;
use printobserver::failure::{Exit, Failure};
use printobserver::parse::{Call, Invocation, parse};
use printobserver::surface::{CONFIG_OPTION, JSON_OPTION};
use printobserver_types::serde_json::{self, Value};

/// The variable naming which defect this variant carries.
const DEFECT: &str = "PRINTOBSERVER_TAINT";

/// The variable naming an endpoint the connecting defects reach.
const ELSEWHERE: &str = "PRINTOBSERVER_TAINT_ELSEWHERE";

/// The variable naming the file the output defects read bytes out of.
const IMAGE: &str = "PRINTOBSERVER_TAINT_IMAGE";

/// The variable naming the field the substituting defect replaces.
const FIELD: &str = "PRINTOBSERVER_TAINT_FIELD";

/// The variable naming what that defect replaces it with.
const AS: &str = "PRINTOBSERVER_TAINT_AS";

/// The reason the reason-substituting defect sends instead of the caller's.
const ITS_OWN_REASON: &str = "a reason this variant made up for itself";

/// Which defect this variant carries.
fn defect() -> String {
    std::env::var(DEFECT).unwrap_or_default()
}

/// One value the environment names.
fn named(variable: &str) -> String {
    std::env::var(variable)
        .unwrap_or_else(|_| panic!("this variant needs {variable} and nothing set it"))
}

/// Open a connection to somewhere that is not the configured supervisor.
fn connect_elsewhere() {
    let address = named(ELSEWHERE);
    let _ = std::net::TcpStream::connect(address);
}

/// The image's bytes, as this variant reads them off the filesystem.
fn image_bytes() -> Vec<u8> {
    std::fs::read(named(IMAGE)).expect("the image this variant was pointed at")
}

/// Those bytes, in the encoding the second output defect uses.
fn base64_of(bytes: &[u8]) -> String {
    const ALPHABET: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut written = String::new();
    for chunk in bytes.chunks(3) {
        let mut held = [0_u8; 3];
        held[..chunk.len()].copy_from_slice(chunk);
        let packed = (u32::from(held[0]) << 16) | (u32::from(held[1]) << 8) | u32::from(held[2]);
        for index in 0..4 {
            if index <= chunk.len() {
                let at = (packed >> (18 - 6 * index)) & 0x3f;
                written.push(char::from(ALPHABET[at as usize]));
            } else {
                written.push('=');
            }
        }
    }
    written
}

/// The call, with whatever this variant does to it done.
fn taint_the_call(call: &mut Call) {
    match defect().as_str() {
        "fixed-value" if call.values.contains_key(&named(FIELD)) => {
            let replacement: Value =
                serde_json::from_str(&named(AS)).expect("a value this variant substitutes");
            call.values.insert(named(FIELD), replacement);
        }
        "fixed-reason" if call.values.contains_key("reason") => {
            call.values.insert(
                "reason".to_owned(),
                Value::String(ITS_OWN_REASON.to_owned()),
            );
        }
        _ => {}
    }
}

/// The answer, with whatever this variant puts in it put in it.
fn taint_the_answer(produced: &mut Produced) {
    match defect().as_str() {
        "image-bytes" => {
            if let Some(object) = produced.document.as_object_mut() {
                object.insert(
                    "the_image_itself".to_owned(),
                    Value::String(String::from_utf8_lossy(&image_bytes()).into_owned()),
                );
            }
        }
        "image-base64" => {
            let encoded = base64_of(&image_bytes());
            if let Some(object) = produced.document.as_object_mut() {
                let first = object.keys().next().cloned();
                if let Some(field) = first {
                    object.insert(field, Value::String(encoded));
                }
            }
        }
        _ => {}
    }
}

/// Whether this variant connects elsewhere for this invocation.
fn connects_for(arguments: &[String]) -> bool {
    match defect().as_str() {
        "connects-directly" => true,
        // The defect a walk over one option at a time cannot catch: it is
        // invisible unless both of them are given.
        "connects-when-combined" => {
            arguments.iter().any(|word| word == JSON_OPTION)
                && arguments.iter().any(|word| word == CONFIG_OPTION)
        }
        _ => false,
    }
}

fn main() -> ExitCode {
    let arguments: Vec<String> = std::env::args().skip(1).collect();
    if connects_for(&arguments) {
        connect_elsewhere();
    }
    let outcome = match parse(&arguments) {
        Invocation::Usage | Invocation::Version | Invocation::Serve { .. } => {
            eprintln!("this variant is a client command and nothing else");
            return ExitCode::from(Exit::Usage.status());
        }
        Invocation::Refused { detail } => refusal(&Failure::of(Exit::Usage, detail)),
        Invocation::Call(mut call) => {
            taint_the_call(&mut call);
            match load(call.config.as_deref()) {
                Err(unconfigured) => refusal(&Failure::of(Exit::Unconfigured, unconfigured)),
                Ok(config) => match produce(&call, &config) {
                    Err(failure) => refusal(&failure),
                    Ok(mut produced) => {
                        taint_the_answer(&mut produced);
                        present(&call, &produced)
                    }
                },
            }
        }
    };
    let mut out = std::io::stdout();
    let _ = out.write_all(outcome.out.as_bytes());
    let _ = out.flush();
    let mut err = std::io::stderr();
    let _ = err.write_all(outcome.err.as_bytes());
    let _ = err.flush();
    ExitCode::from(outcome.exit.status())
}
