//! `printobserver-store-sqlite`.
//!
//! Owns: the `SQLite` adapter — the one durable implementation of
//! `printobserver-store-api`, including its schema, its migrations and the
//! content-addressed image files beside the database; and, beside it, the
//! in-memory implementation of the same port that one shared conformance suite
//! holds both of them to.
//!
//! May depend on: `printobserver-types` and `printobserver-store-api`, plus its
//! `SQLite` driver. Never another implementation crate, and never
//! `printobserver-core`.
//!
//! # The persisted schema is a contract of its own
//!
//! It outlives every process that writes it, and the history a person reads
//! after a print goes wrong is read out of it. So the relationships between
//! records are declared in the schema and enforced by the database rather than
//! upheld by the code above it: an action row cannot exist without its policy
//! decision, because `actions.decision` is `NOT NULL`; and an execution outcome
//! cannot exist without its action, because an outcome is a row of its own
//! whose primary key references `actions`. [`MIGRATIONS`] is that schema, and
//! it is forward-only: a database at an earlier version is migrated on open,
//! and one from a later version is refused naming both versions rather than
//! opened by a build that cannot know what the later version means.
//!
//! # Two implementations, one conformance suite
//!
//! [`SqliteStore`] is the durable one. [`MemoryStore`] holds the same records in
//! process memory, and exists so that a tier which does not need durability
//! does not pay for it. Neither is the other's specification: `tests/`
//! conformance suite drives one set of journeys against both, so the fake
//! cannot drift into answering differently from the real one.
//!
//! Both live here rather than beside the port, because `printobserver-core` may
//! depend on no implementation crate at all and so could not reach one wherever
//! it were put; a tier inside core that needs a double defines its own, which is
//! not a dependency on anything.
//!
//! Image *bytes* are on the filesystem for both of them, because the port hands
//! a caller a [`ImageLookup::Found`](printobserver_store_api::ImageLookup)
//! path: an implementation that held bytes in memory would have to answer a
//! path nothing is at. So [`MemoryStore`] takes a state directory too, and both
//! stores write images through the same content-addressed writer.
//!
//! # The connection is part of the contract
//!
//! Foreign keys are enforced per connection rather than per database, and a
//! reader answering the pre-write state rather than waiting on a writer is a
//! property of the journal mode. So [`connect`] — the one place both are set —
//! is exported: a test asking whether this database enforces what it declares
//! asks it over the connection this crate opens, rather than over one of its
//! own that might differ.
//!
//! # Two facts the port leaves open, settled here
//!
//! * **Which print an action binds to.** `record_action` answers an
//!   `ActionRecord` carrying a print, and takes a request that names none. The
//!   action binds to the *open* print — the most recently opened print with no
//!   end recorded — and is refused with [`StoreError::NotFound`] when there is
//!   none, which is the same condition policy names `NoActivePrint`.
//! * **A page size of zero.** `audit_page` resolves its page size through the
//!   port's own [`resolve_history_limit`](printobserver_store_api::resolve_history_limit),
//!   and a zero page takes the default window, because a page of nothing cannot
//!   walk a history to exhaustion.

mod hold;
mod images;
mod memory;
mod rows;
mod schema;
mod sqlite;
mod values;

pub use hold::{HoldPoint, HoldPoints, settle_label};
pub use images::{IMAGE_DIRECTORY, StoredImage, digest_of};
pub use memory::MemoryStore;
pub use schema::{
    CURRENT_SCHEMA_VERSION, DATABASE_FILE_NAME, LOCK_TIMEOUT, MIGRATIONS, Migration, connect,
};
pub use sqlite::SqliteStore;
