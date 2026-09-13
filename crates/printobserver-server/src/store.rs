//! The store this root composes, for a tier that seeds it before a server is
//! started on it.
//!
//! The command's own journeys run this server as a subprocess and drive it
//! through its surface. A print record is minted by the supervisor when an
//! alert opens one, and no client command opens a print, so a journey that
//! needs one in place before the server starts writes it into the state
//! directory the server is then started on — through the very store
//! [`Server::start`](crate::Server::start) composes and the traits it is
//! driven through. That is what this module hands out, and it is the only
//! place this crate hands a store out: the command depends on this crate and
//! on no implementation crate, so what it seeds through is what this root
//! chose rather than an implementation of its own naming.

pub use printobserver_core::store::{EventDraft, EventStore, ImageLookup, ImageStore, PrintStore};
pub use printobserver_store_sqlite::SqliteStore;
