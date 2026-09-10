//! `printobserver-sdk`.
//!
//! Owns: the Rust client of the printobserver server's HTTP surface: a typed,
//! blocking client with one method per public operation, and the request and
//! response types those methods take and answer.
//!
//! May depend on: `serde` and `serde_json`. Never `printobserver-core`, never
//! the server, and never an implementation crate — a client that could reach
//! them would be a second copy of the server. It does not depend on
//! `printobserver-types` either: its types are **generated** from the same
//! checked-in schemas that crate generates, so the two agree because they come
//! from one source rather than because one imports the other.
//!
//! # The types are generated, and the gate says so
//!
//! [`contract`] is written by `contract-codegen` from `schemas/`, and `just
//! check-generated` refuses a tree in which it differs from what that
//! generator writes. Everything else in this crate is the transport and the
//! error vocabulary that module's methods are written against.
//!
//! # A refused action is an answer, not a failure of transport
//!
//! The policy's own refusal arrives as [`ClientError::Rejected`], carrying
//! [`Rejection`] — the reason, the value asked for and the range allowed — so
//! a caller can ask again for something inside the range without reading a
//! message.
//!
//! # Images are a path, never bytes
//!
//! No method of this client answers image bytes or an encoding of them. The
//! server answers an absolute path on **its own** filesystem, and this client
//! hands that path back exactly as it was answered.
//!
//! ```no_run
//! use printobserver_sdk::{Actor, Client};
//!
//! let client = Client::new("http://127.0.0.1:8420", Actor::Operator);
//! let status = client.status("0198f0a1-2b3c-7d4e-8f90-123456789abc")?;
//! println!("{:?}", status.print.state);
//! # Ok::<(), printobserver_sdk::ClientError>(())
//! ```

mod client;
pub mod contract;
mod error;
mod transport;

pub use client::Client;
pub use contract::*;
pub use error::{ClientError, Rejection};
pub use transport::Answered;

/// The reason a mutating call carries, refused here when it carries none.
///
/// The refusal happens in this client, before a connection is opened: a change
/// nobody gave a reason for is one the history cannot account for afterwards,
/// and asking the server to say so would be a request this client already
/// knows the answer to.
///
/// # Errors
///
/// Returns [`ClientError::NoReason`] when the reason is empty or is nothing
/// but whitespace.
pub fn reason_given(reason: &str) -> Result<(), ClientError> {
    if reason.trim().is_empty() {
        return Err(ClientError::NoReason);
    }
    Ok(())
}

/// One value a request carries, as the JSON it travels as.
///
/// # Errors
///
/// Returns [`ClientError::Unsendable`] when the value cannot be rendered as
/// JSON, which for the generated types is a value carrying a float that is not
/// a number.
pub fn as_value<T: serde::Serialize>(value: &T) -> Result<serde_json::Value, ClientError> {
    serde_json::to_value(value).map_err(|error| ClientError::Unsendable {
        detail: error.to_string(),
    })
}
