//! The client every generated method is a method of.
//!
//! It carries three things and no more: where the supervisor is, what
//! authenticates to it, and who this client acts as. The third is why no
//! generated method takes an actor: a call cannot act as somebody the client
//! is not.

use serde::de::DeserializeOwned;

use crate::contract::{ActionAnswer, Actor, ErrorAnswer};
use crate::error::{ClientError, Rejection};
use crate::transport::send;

/// The status a rejected action is answered under.
const REJECTED_STATUS: u16 = 409;

/// The status an action the machine itself refused is answered under.
const MACHINE_REFUSED_STATUS: u16 = 502;

/// The status a read or a write that was carried out is answered under.
const SUCCESS_STATUS: u16 = 200;

/// The scheme the address a supervisor is configured at may carry.
const SCHEME: &str = "http://";

/// A typed client of one printobserver supervisor.
///
/// One method per public operation that supervisor serves, each generated from
/// the same checked-in description the server's own routes are folded out of.
#[derive(Debug, Clone, PartialEq)]
pub struct Client {
    /// Where the supervisor answers, as a host and a port.
    address: String,
    /// What authenticates to it, where anything does.
    credential: Option<String>,
    /// Who this client acts as.
    actor: Actor,
}

impl Client {
    /// A client of the supervisor at one address, acting as one actor.
    ///
    /// The address is taken as the supervisor's own configuration writes it —
    /// `http://127.0.0.1:8420` — and a bare `host:port` is taken as well.
    #[must_use]
    pub fn new(address: impl AsRef<str>, actor: Actor) -> Self {
        Self {
            address: address
                .as_ref()
                .trim()
                .trim_end_matches('/')
                .strip_prefix(SCHEME)
                .unwrap_or_else(|| address.as_ref().trim().trim_end_matches('/'))
                .to_owned(),
            credential: None,
            actor,
        }
    }

    /// The same client, authenticating with one credential.
    ///
    /// This server requires none of its API callers; a credential is for a
    /// deployment that has put something in front of it that does.
    #[must_use]
    pub fn with_credential(mut self, credential: impl Into<String>) -> Self {
        self.credential = Some(credential.into());
        self
    }

    /// Where this client is pointed.
    #[must_use]
    pub fn address(&self) -> &str {
        &self.address
    }

    /// Who this client acts as, which every mutating call carries.
    #[must_use]
    pub fn actor(&self) -> Actor {
        self.actor.clone()
    }

    /// Make one call and read what came back.
    ///
    /// # Errors
    ///
    /// Returns [`ClientError`] for each of the five ways a call ends other
    /// than with its answer; a refused action arrives as
    /// [`ClientError::Rejected`], carrying the reason, the value asked for and
    /// the range allowed.
    pub fn call<T: DeserializeOwned>(
        &self,
        method: &str,
        path: &str,
        query: &[(String, String)],
        body: Option<&serde_json::Value>,
    ) -> Result<T, ClientError> {
        let document = body.map(ToString::to_string);
        let answered = send(
            &self.address,
            self.credential.as_deref(),
            method,
            &target(path, query),
            document.as_deref(),
        )?;
        match answered.status {
            SUCCESS_STATUS => {
                serde_json::from_str(&answered.body).map_err(|error| ClientError::Unreadable {
                    status: answered.status,
                    detail: error.to_string(),
                })
            }
            REJECTED_STATUS => Err(rejection(&answered.body, answered.status)),
            MACHINE_REFUSED_STATUS => Err(machine_refusal(&answered.body, answered.status)),
            status => Err(ClientError::Refused {
                status,
                detail: said(&answered.body),
            }),
        }
    }
}

/// The policy's own refusal, as the answer to it carries it.
fn rejection(body: &str, status: u16) -> ClientError {
    match serde_json::from_str::<ActionAnswer>(body).map(Rejection::of) {
        Ok(Some(rejection)) => ClientError::Rejected(Box::new(rejection)),
        Ok(None) => ClientError::Unreadable {
            status,
            detail: "the supervisor refused this action and answered a record whose \
                     decision is not a refusal"
                .to_owned(),
        },
        Err(error) => ClientError::Unreadable {
            status,
            detail: error.to_string(),
        },
    }
}

/// The machine's own refusal of an action the policy accepted.
fn machine_refusal(body: &str, status: u16) -> ClientError {
    match serde_json::from_str::<ActionAnswer>(body) {
        Ok(answer) => ClientError::PrinterRefused {
            detail: answer
                .printer_refusal
                .clone()
                .unwrap_or_else(|| "it said nothing this client can read".to_owned()),
            answer: Box::new(answer),
        },
        Err(error) => ClientError::Unreadable {
            status,
            detail: error.to_string(),
        },
    }
}

/// The request target one call is made to, with what it asks for after the
/// question mark.
fn target(path: &str, query: &[(String, String)]) -> String {
    if query.is_empty() {
        return path.to_owned();
    }
    let asked: Vec<String> = query
        .iter()
        .map(|(name, value)| format!("{}={}", escaped(name), escaped(value)))
        .collect();
    format!("{path}?{}", asked.join("&"))
}

/// One value of a request target, with everything that is not unreserved in it
/// written as an escape.
fn escaped(value: &str) -> String {
    value
        .bytes()
        .map(|byte| match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' => {
                (byte as char).to_string()
            }
            other => format!("%{other:02X}"),
        })
        .collect()
}

/// What the supervisor said about a request it will not act on.
fn said(body: &str) -> String {
    serde_json::from_str::<ErrorAnswer>(body).map_or_else(
        |_| "it said nothing this client can read".to_owned(),
        |answer| answer.error,
    )
}

#[cfg(test)]
mod tests {
    use super::{Client, escaped, said, target};
    use crate::contract::Actor;

    /// An address is taken as the supervisor's own configuration writes it.
    #[test]
    fn an_address_is_taken_as_the_supervisors_configuration_writes_it() {
        for given in [
            "http://127.0.0.1:8420",
            "127.0.0.1:8420",
            "http://127.0.0.1:8420/",
        ] {
            let client = Client::new(given, Actor::Operator);
            assert_eq!(client.address(), "127.0.0.1:8420", "{given}");
        }
    }

    /// A client acts as the actor it was made with, and no call changes that.
    #[test]
    fn a_client_acts_as_the_actor_it_was_made_with() {
        let client = Client::new(
            "127.0.0.1:8420",
            Actor::Agent {
                session_name: "a-session".to_owned(),
            },
        );
        assert_eq!(
            client.actor(),
            Actor::Agent {
                session_name: "a-session".to_owned()
            }
        );
    }

    /// What is asked for after the question mark is escaped where it must be.
    #[test]
    fn what_is_asked_for_after_the_question_mark_is_escaped() {
        assert_eq!(target("/v1/x", &[]), "/v1/x");
        assert_eq!(
            target("/v1/x", &[("limit".to_owned(), "20".to_owned())]),
            "/v1/x?limit=20"
        );
        assert_eq!(escaped("a b&c"), "a%20b%26c");
    }

    /// A refusal this client cannot read is said to be one rather than shown.
    #[test]
    fn a_refusal_this_client_cannot_read_is_said_to_be_one() {
        assert_eq!(said(r#"{"error":"no such print"}"#), "no such print");
        assert_eq!(said("<html>"), "it said nothing this client can read");
    }
}
