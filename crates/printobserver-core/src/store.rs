//! The persistence interfaces the supervision domain needs, one per aggregate.
//!
//! Durable state is this domain's own concern: what it persists — prints,
//! events, images, actions with the interventions they open, and sessions —
//! are its own records, so the interfaces it persists them through are
//! declared here, beside the records, rather than in a crate every consumer of
//! the store would have to depend on. There is one trait per aggregate rather
//! than one trait for everything, so that a caller names only the aggregates
//! it touches and a column one aggregate gains is a change nothing else
//! rebuilds against:
//!
//! * [`PrintStore`] — the print record, its manifest and its narrowings.
//! * [`EventStore`] — the event log.
//! * [`ImageStore`] — the images stored beside events.
//! * [`ActionStore`] — actions, and the bounded interventions they open.
//! * [`SessionStore`] — the supervision session of a print.
//! * [`FilamentStore`] — the filament ledger: spools, the charges against them,
//!   the charges held for a spool not yet named, each print's settlement and
//!   last file position, and the printer's per-file print tallies.
//! * [`TimelineStore`] — the printer-wide reading of the event log.
//!
//! An implementation implements all seven over one backing, and the composition
//! root hands each consumer the trait it names: [`Stores`] is one
//! implementation coerced once per trait, which is what lets a consumer take
//! `Arc<dyn EventStore>` without knowing which store is behind it.
//!
//! # The history read has four behavioural facts, and they are settled here
//!
//! A signature carries none of them, and a trait that is structurally exact
//! with all four unstated is an incomplete interface, so this module leaves an
//! implementation exactly one answer to take for each rather than leaving them
//! to whoever implements the trait:
//!
//! 1. The **default history window** is [`DEFAULT_HISTORY_WINDOW`].
//! 2. The **maximum history limit** is [`MAX_HISTORY_LIMIT`], and it is above
//!    the default.
//! 3. [`EventStore::history`] answers **newest first**.
//! 4. An **empty `kinds`** in a [`HistoryQuery`] means **every kind**, not no
//!    kinds.
//!
//! The limit rule has one implementation here too — [`resolve_history_limit`] —
//! so that refusing rather than clamping is a property of this contract and not
//! of whoever implemented the trait. There is exactly one exported constant of
//! each kind and exactly one exported resolution, so a consumer has one thing
//! to import and no choice to make.
//!
//! Whether an implementation obeys the four is proven where the
//! implementations are, by one conformance suite driving both of them.
//!
//! # Minting an identifier
//!
//! [`ActionStore::record_action`] is the only method that takes an
//! [`ActionRequest`] or a [`PolicyDecision`] at all, and it takes them only
//! together, so this surface offers no way to *ask* for a fresh [`ActionId`]
//! for a request whose decision was not recorded beside it. Other methods
//! answer records that may carry an identifier an earlier `record_action`
//! minted — [`EventStore::append_event`] answers an [`EventRecord`] whose
//! action-event payloads carry one — and that is stated rather than forbidden.
//! That no method but `record_action` *mints* one is a property of an
//! implementation's bodies rather than of a signature, and is established by
//! the store implementation's own conformance suite.
//!
//! # Why the methods answer a boxed future
//!
//! Every method is asynchronous, and each trait is dyn-compatible and
//! shareable across threads, because the supervisor holds every port behind
//! `Arc<dyn Port>`. An `async fn` in a trait is not dyn-compatible, so each
//! method answers a [`BoxFuture`] instead.

use core::future::Future;
use core::pin::Pin;
use std::path::PathBuf;
use std::sync::Arc;

use crate::records::{
    ActionId, ActionRecord, ActionRequest, Actor, ChargeKey, ChargedFilament, ExecutionOutcome,
    FilePosition, ImageRecord, Intervention, InterventionId, InterventionOutcome, JobManifest,
    ManifestNarrowing, PendingCharge, PolicyDecision, PrintOutcome, PrintRecord, SpoolId,
    SpoolRecord, SpoolTrust, UnverifiedReason, UnverifiedReasonKind,
};
use printobserver_printer_api::{Adjustable, PrinterState};
use printobserver_supervisor_api::SupervisionSession;
use printobserver_types::schemars::JsonSchema;
use printobserver_types::serde::{Deserialize, Serialize};
use printobserver_types::{
    EventBody, EventId, EventKind, EventRecord, EventSource, ImageId, PrintId, RawBytes, Timestamp,
};

/// A future these traits' methods answer with, in the one shape a trait object
/// can carry.
pub type BoxFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

/// How many events a history read answers when the caller asks for no limit.
///
/// The one default window this domain declares, and it is below
/// [`MAX_HISTORY_LIMIT`].
pub const DEFAULT_HISTORY_WINDOW: u32 = 100;

/// The most events a history read will answer, however many are asked for.
///
/// The one maximum this domain declares. A read asking for more is refused
/// naming this maximum rather than quietly clamped, because a caller that
/// asked for more and was given fewer cannot tell a short page from the end of
/// the history.
pub const MAX_HISTORY_LIMIT: u32 = 1_000;

/// Resolve the limit a history read asks for, refusing one above the maximum.
///
/// The one resolution of the limit rule this domain declares: an
/// implementation calls this rather than deciding for itself. An absent limit
/// takes [`DEFAULT_HISTORY_WINDOW`]; a limit at or below [`MAX_HISTORY_LIMIT`]
/// is taken as asked for; one above it is refused.
///
/// # Errors
///
/// Returns [`StoreError::LimitRefused`], naming the maximum and what was asked
/// for, when the limit asked for is above [`MAX_HISTORY_LIMIT`].
pub fn resolve_history_limit(limit: Option<u32>) -> Result<u32, StoreError> {
    match limit {
        None => Ok(DEFAULT_HISTORY_WINDOW),
        Some(asked_for) if asked_for <= MAX_HISTORY_LIMIT => Ok(asked_for),
        Some(asked_for) => Err(StoreError::LimitRefused {
            limit: MAX_HISTORY_LIMIT,
            asked_for,
        }),
    }
}

/// One event on its way into the store, before the store mints its identifier.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(crate = "printobserver_types::serde")]
#[schemars(crate = "printobserver_types::schemars")]
pub struct EventDraft {
    /// The print it belongs to, when it belongs to one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub print_id: Option<PrintId>,
    /// Where it came from.
    pub source: EventSource,
    /// When it was received.
    pub received_at: Timestamp,
    /// The kind and the payload, flattened into the draft's own fields.
    #[serde(flatten)]
    pub body: EventBody,
    /// The bytes exactly as received, for an externally sourced event.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub raw: Option<RawBytes>,
}

impl EventDraft {
    /// Which event this is.
    #[must_use]
    pub const fn kind(&self) -> &EventKind {
        &self.body.kind
    }
}

/// Which of a print's events to read.
///
/// An empty `kinds` means every kind rather than no kinds, and an absent
/// `limit` means [`DEFAULT_HISTORY_WINDOW`].
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(crate = "printobserver_types::serde", deny_unknown_fields)]
#[schemars(crate = "printobserver_types::schemars")]
pub struct HistoryQuery {
    /// The print to read the history of.
    pub print_id: PrintId,
    /// The kinds to read; empty means every kind.
    pub kinds: Vec<EventKind>,
    /// The earliest instant to read from, inclusive.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub since: Option<Timestamp>,
    /// The latest instant to read to, inclusive.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub until: Option<Timestamp>,
    /// How many events to answer; absent means [`DEFAULT_HISTORY_WINDOW`].
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub limit: Option<u32>,
}

impl HistoryQuery {
    /// The limit this query resolves to.
    ///
    /// # Errors
    ///
    /// Returns [`StoreError::LimitRefused`] when the limit asked for is above
    /// [`MAX_HISTORY_LIMIT`].
    pub fn resolved_limit(&self) -> Result<u32, StoreError> {
        resolve_history_limit(self.limit)
    }
}

/// One read of the event log in the order the store stored it.
///
/// The order is the store's own sequence rather than any field of the
/// record: `received_at` is the instant its producer stamped, which a late
/// arrival can carry from before an event stored ahead of it, so a reader
/// following the log by it would skip what arrived late. Identifiers are not
/// that order either, because they are minted by whoever appended them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoredQuery {
    /// The event to read after, exclusive. Absent with a print, the read
    /// starts at that print's first event; absent without one, at the first
    /// event stored.
    pub after: Option<EventId>,
    /// The print to scope to: its own events and every event that belongs to
    /// no print. Absent, every event.
    pub print_id: Option<PrintId>,
    /// How many events to answer; absent means [`DEFAULT_HISTORY_WINDOW`].
    pub limit: Option<u32>,
}

impl StoredQuery {
    /// The limit this query resolves to.
    ///
    /// # Errors
    ///
    /// Returns [`StoreError::LimitRefused`] when the limit asked for is above
    /// [`MAX_HISTORY_LIMIT`].
    pub fn resolved_limit(&self) -> Result<u32, StoreError> {
        resolve_history_limit(self.limit)
    }
}

/// What a store calls after every write that stored an event has committed.
///
/// Raised after the commit rather than before it, so a reader woken by it reads
/// what woke it; and raised for every write that stored one, whichever
/// aggregate's method made it. It carries nothing: what was stored is read
/// back through [`EventStore::stored_after`].
pub type StoredSignal = Arc<dyn Fn() + Send + Sync>;

/// What the store found when asked for an image.
///
/// The record and the bytes are two facts rather than one: a record whose file
/// has gone is a distinguishable answer rather than an error, because it is
/// the answer that tells a caller the history is intact and the file is not.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ImageLookup {
    /// The record, and the absolute path its bytes are at.
    Found {
        /// The record.
        record: ImageRecord,
        /// Where its bytes are, resolved against the state directory.
        path: PathBuf,
    },
    /// The record, whose file is not where the record says it is.
    FileMissing {
        /// The record.
        record: ImageRecord,
    },
}

/// What settling an intervention did.
#[derive(Debug, Clone, PartialEq)]
pub enum SettleOutcome {
    /// This call settled it, and here it is.
    Settled {
        /// The intervention as it now stands.
        intervention: Intervention,
    },
    /// It was already settled, and this is the outcome that won.
    AlreadySettled {
        /// The outcome that was already recorded.
        outcome: InterventionOutcome,
    },
}

/// One page of a print's whole history, and the cursor that follows it.
///
/// The exhaustive read of a print's history is [`EventStore::audit_page`]
/// walked to exhaustion; there is no method that answers a whole history in one
/// response, which is what makes an unbounded answer something a caller cannot
/// ask for by accident.
#[derive(Debug, Clone, PartialEq)]
pub struct AuditPage {
    /// This page's events.
    pub events: Vec<EventRecord>,
    /// The cursor the next page follows, or absent at the end of the history.
    pub next: Option<EventId>,
}

/// Why the store did not do what it was asked.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StoreError {
    /// A constraint refused the write.
    ConstraintRefused {
        /// The constraint that refused it.
        constraint: String,
    },
    /// There is no such record.
    NotFound {
        /// What was looked for.
        what: String,
    },
    /// More was asked for than a limit allows.
    LimitRefused {
        /// The limit.
        limit: u32,
        /// What was asked for.
        asked_for: u32,
    },
    /// The database failed.
    Database {
        /// What went wrong.
        detail: String,
    },
    /// Reading or writing a file failed.
    Io {
        /// What went wrong.
        detail: String,
    },
}

impl core::fmt::Display for StoreError {
    fn fmt(&self, formatter: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::ConstraintRefused { constraint } => {
                write!(formatter, "the constraint {constraint} refused the write")
            }
            Self::NotFound { what } => write!(formatter, "there is no {what}"),
            Self::LimitRefused { limit, asked_for } => {
                write!(
                    formatter,
                    "{asked_for} was asked for, and the limit is {limit}"
                )
            }
            Self::Database { detail } => write!(formatter, "the database failed: {detail}"),
            Self::Io { detail } => write!(formatter, "reading or writing failed: {detail}"),
        }
    }
}

impl core::error::Error for StoreError {}

/// The print record, with its manifest and the narrowings recorded on it.
///
/// Every method is asynchronous, the trait is dyn-compatible, and it is
/// shareable across threads, because its consumers hold it behind
/// `Arc<dyn PrintStore>`.
pub trait PrintStore: Send + Sync {
    /// Open a print, minting its identifier.
    fn open_print(
        &self,
        provider_print_id: Option<i64>,
        file_name: Option<String>,
    ) -> BoxFuture<'_, Result<PrintRecord, StoreError>>;

    /// Read one print by its identifier.
    fn print(&self, print_id: PrintId) -> BoxFuture<'_, Result<Option<PrintRecord>, StoreError>>;

    /// Read every print with no end recorded, most recently opened first.
    ///
    /// This is what a supervisor adopts on start: the store already binds an
    /// action to the most recently opened print with no end recorded, and
    /// without this method nothing above the store could name that print in
    /// order to say it had adopted it.
    fn open_prints(&self) -> BoxFuture<'_, Result<Vec<PrintRecord>, StoreError>>;

    /// Read every print, ended or not, most recently opened first.
    ///
    /// This is what a caller finds a print's identifier in: the identifier is
    /// minted here, and nothing outside the store could otherwise name it.
    fn prints(&self) -> BoxFuture<'_, Result<Vec<PrintRecord>, StoreError>>;

    /// Record `Obico`'s own identifier on a print that carries none.
    ///
    /// A print is opened before `Obico` reports on it when a caller finds the
    /// job the printer is running, and this is how the first alert about that
    /// job joins the print already open for it rather than opening a second.
    /// Recording the identifier a print already carries answers the print
    /// unchanged.
    ///
    /// # Errors
    ///
    /// Refuses a print that does not exist with [`StoreError::NotFound`], and a
    /// print already carrying a different identifier with
    /// [`StoreError::ConstraintRefused`] naming `prints.provider_print_id`: a
    /// correlation, once recorded, is never moved to another of the provider's
    /// prints.
    fn attach_obico_print(
        &self,
        print_id: PrintId,
        obico_print_id: i64,
    ) -> BoxFuture<'_, Result<PrintRecord, StoreError>>;

    /// Record what one read of the printer saw of a print's job.
    ///
    /// The job's start is set to `job_started_at` whatever it was, since a
    /// pause moves it ([`PrintRecord::job_started_at`]); the print's running
    /// time is raised to `job_print_time_s` when that is longer than the one it
    /// carries and left alone otherwise, so that the longest the job was ever
    /// seen printing is what it carries. Absent `job_print_time_s` — a read
    /// that found the job paused — leaves the running time as it is.
    ///
    /// # Errors
    ///
    /// Refuses a print that does not exist with [`StoreError::NotFound`].
    fn record_job_sighting(
        &self,
        print_id: PrintId,
        job_started_at: Timestamp,
        job_print_time_s: Option<i64>,
    ) -> BoxFuture<'_, Result<PrintRecord, StoreError>>;

    /// Read one print by the provider's own identifier for it.
    fn print_by_provider_id(
        &self,
        provider_print_id: i64,
    ) -> BoxFuture<'_, Result<Option<PrintRecord>, StoreError>>;

    /// End a print, in a terminal state, at an instant, for a reason.
    fn end_print(
        &self,
        print_id: PrintId,
        state: PrinterState,
        ended_at: Timestamp,
        reason: String,
    ) -> BoxFuture<'_, Result<PrintRecord, StoreError>>;

    /// Record that a manifest range was narrowed to the envelope's.
    fn record_narrowing(
        &self,
        print_id: PrintId,
        narrowing: ManifestNarrowing,
    ) -> BoxFuture<'_, Result<PrintRecord, StoreError>>;

    /// Store one print's manifest.
    fn put_manifest(
        &self,
        print_id: PrintId,
        manifest: JobManifest,
    ) -> BoxFuture<'_, Result<(), StoreError>>;

    /// Read one print's manifest.
    fn manifest(&self, print_id: PrintId)
    -> BoxFuture<'_, Result<Option<JobManifest>, StoreError>>;
}

/// The event log.
///
/// Every method is asynchronous, the trait is dyn-compatible, and it is
/// shareable across threads, because its consumers hold it behind
/// `Arc<dyn EventStore>`.
pub trait EventStore: Send + Sync {
    /// Append one event, minting its identifier.
    fn append_event(&self, draft: EventDraft) -> BoxFuture<'_, Result<EventRecord, StoreError>>;

    /// Read a print's events, newest first.
    ///
    /// Answers **newest first**. An empty `kinds` means **every kind** rather
    /// than no kinds. An absent `limit` takes [`DEFAULT_HISTORY_WINDOW`], and a
    /// limit above [`MAX_HISTORY_LIMIT`] is refused rather than clamped —
    /// [`resolve_history_limit`] is the one resolution of that rule.
    fn history(&self, query: HistoryQuery) -> BoxFuture<'_, Result<Vec<EventRecord>, StoreError>>;

    /// Read one page of a print's whole history, oldest first.
    ///
    /// `after` is the identifier of the last event of the previous page, and is
    /// absent for the first page. The page size is resolved by
    /// [`resolve_history_limit`] like a history read's limit, so a page above
    /// [`MAX_HISTORY_LIMIT`] is refused rather than clamped.
    fn audit_page(
        &self,
        print_id: PrintId,
        after: Option<EventId>,
        page_size: u32,
    ) -> BoxFuture<'_, Result<AuditPage, StoreError>>;

    /// Read the events stored after a point, in the order they were stored.
    ///
    /// Answers **oldest first by the store's own sequence** — the order the
    /// events were appended in, whatever their `received_at` — scoped as
    /// [`StoredQuery`] says. A print with no event yet starts after the newest
    /// event stored. The limit is resolved by [`resolve_history_limit`].
    ///
    /// # Errors
    ///
    /// Refuses a cursor naming no event with [`StoreError::NotFound`], and a
    /// limit above [`MAX_HISTORY_LIMIT`] with [`StoreError::LimitRefused`].
    fn stored_after(
        &self,
        query: StoredQuery,
    ) -> BoxFuture<'_, Result<Vec<EventRecord>, StoreError>>;

    /// The newest event stored, by the store's own sequence, when there is one.
    fn newest_stored(&self) -> BoxFuture<'_, Result<Option<EventId>, StoreError>>;

    /// Raise this signal after every write that stores an event has committed,
    /// from now on.
    fn on_stored(&self, signal: StoredSignal);
}

/// The images stored beside events.
///
/// Every method is asynchronous, the trait is dyn-compatible, and it is
/// shareable across threads, because its consumers hold it behind
/// `Arc<dyn ImageStore>`.
pub trait ImageStore: Send + Sync {
    /// Store one image's bytes, minting its identifier.
    fn put_image(
        &self,
        print_id: PrintId,
        event_id: EventId,
        source_url: Option<String>,
        content_type: String,
        bytes: RawBytes,
    ) -> BoxFuture<'_, Result<ImageRecord, StoreError>>;

    /// Read one image by its identifier.
    fn image(&self, image_id: ImageId) -> BoxFuture<'_, Result<ImageLookup, StoreError>>;
}

/// Actions, and the bounded interventions they open.
///
/// Every method is asynchronous, the trait is dyn-compatible, and it is
/// shareable across threads, because its consumers hold it behind
/// `Arc<dyn ActionStore>`.
pub trait ActionStore: Send + Sync {
    /// Record one request together with the decision taken on it.
    ///
    /// This is the only method that takes either half, and it takes them only
    /// together; the [`ActionId`] it answers is minted for the pair.
    fn record_action(
        &self,
        request: ActionRequest,
        decision: PolicyDecision,
    ) -> BoxFuture<'_, Result<ActionRecord, StoreError>>;

    /// Record what the printer made of an action that reached it.
    fn record_execution(
        &self,
        action_id: ActionId,
        outcome: ExecutionOutcome,
    ) -> BoxFuture<'_, Result<ActionRecord, StoreError>>;

    /// Open a bounded intervention, minting its identifier.
    ///
    /// The print is the one the action was recorded against, so that a caller
    /// cannot name a print the action does not belong to.
    fn open_intervention(
        &self,
        action_id: ActionId,
        adjustable: Adjustable,
        prior_value: Option<f64>,
        applied_value: f64,
        applied_at: Timestamp,
        expires_at: Timestamp,
    ) -> BoxFuture<'_, Result<Intervention, StoreError>>;

    /// Settle one intervention, or report the outcome that already won.
    fn settle_intervention(
        &self,
        intervention_id: InterventionId,
        outcome: InterventionOutcome,
    ) -> BoxFuture<'_, Result<SettleOutcome, StoreError>>;

    /// Read every intervention due to expire at or before an instant.
    fn due_interventions(
        &self,
        at: Timestamp,
    ) -> BoxFuture<'_, Result<Vec<Intervention>, StoreError>>;

    /// Read every intervention still active on one print.
    fn active_interventions(
        &self,
        print_id: PrintId,
    ) -> BoxFuture<'_, Result<Vec<Intervention>, StoreError>>;
}

/// The supervision session of a print.
///
/// Every method is asynchronous, the trait is dyn-compatible, and it is
/// shareable across threads, because its consumers hold it behind
/// `Arc<dyn SessionStore>`.
pub trait SessionStore: Send + Sync {
    /// Store one print's supervision session.
    fn put_session(&self, session: SupervisionSession) -> BoxFuture<'_, Result<(), StoreError>>;

    /// Read one print's supervision session.
    fn session(
        &self,
        print_id: PrintId,
    ) -> BoxFuture<'_, Result<Option<SupervisionSession>, StoreError>>;
}

/// A spool on its way into the store, before the store mints its identifier.
///
/// A [`SpoolRecord`] without what the store sets itself: its `id`, minted; and
/// its `updated_at`, which is `registered_at`. It is shelved, or loaded on a
/// tool, and never retired.
#[derive(Debug, Clone, PartialEq)]
pub struct NewSpool {
    /// The operator's own label for the spool.
    pub name: String,
    /// The material, such as `PLA`.
    pub material: String,
    /// The filament's colour, when the operator gave one.
    pub color: Option<String>,
    /// The filament's diameter, in millimetres.
    pub diameter_mm: f64,
    /// The material's density, in grams per cubic centimetre.
    pub density_g_cm3: f64,
    /// The net filament on the spool when it was full, in grams.
    pub initial_g: f64,
    /// The filament left, in grams.
    pub remaining_g: f64,
    /// The empty spool's own weight, in grams, when the operator gave it.
    pub tare_g: Option<f64>,
    /// The tool the spool is loaded on, when it is registered loaded.
    pub loaded_on_tool: Option<i64>,
    /// How far the remaining figure can be trusted.
    pub trust: SpoolTrust,
    /// When the spool was registered.
    pub registered_at: Timestamp,
}

/// Whose debit a charge is: its key, which names the tool and — for a print
/// this system watched — the print, and for a print it never saw, the print
/// it recorded that one as, when one is linked.
///
/// The fields are private, so a watched print's charge cannot carry a second
/// print beside the one its key names.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChargeIdentity {
    /// The durable identity of the debit.
    key: ChargeKey,
    /// For an unobserved print only, the print this system recorded it as.
    linked_print: Option<PrintId>,
}

impl ChargeIdentity {
    /// The identity a key alone gives: a watched print's own, or an unobserved
    /// print linked to none.
    #[must_use]
    pub const fn of(key: ChargeKey) -> Self {
        Self {
            key,
            linked_print: None,
        }
    }

    /// An unobserved print's identity, linked to the print this system
    /// recorded it as.
    ///
    /// # Errors
    ///
    /// Answers the key back for a watched print's key, whose print is the one
    /// it names and no other.
    pub const fn linked(key: ChargeKey, print_id: PrintId) -> Result<Self, ChargeKey> {
        match key {
            ChargeKey::Print { .. } => Err(key),
            ChargeKey::Unobserved { .. } => Ok(Self {
                key,
                linked_print: Some(print_id),
            }),
        }
    }

    /// The durable identity of the debit.
    #[must_use]
    pub const fn key(&self) -> &ChargeKey {
        &self.key
    }

    /// The tool that fed the filament, as the key names it.
    #[must_use]
    pub const fn tool(&self) -> i64 {
        self.key.tool()
    }

    /// The print the filament was fed in, when it was one this system
    /// recorded: the one a watched key names, or the one an unobserved print
    /// is linked to.
    #[must_use]
    pub const fn print_id(&self) -> Option<PrintId> {
        match self.key.watched_print() {
            Some(print_id) => Some(print_id),
            None => self.linked_print,
        }
    }
}

/// One debit on its way into the store.
#[derive(Debug, Clone, PartialEq)]
pub struct Charge {
    /// Whose debit it is, from which its tool and print are read.
    pub identity: ChargeIdentity,
    /// How much was fed, in millimetres.
    pub length_mm: f64,
    /// The spool charged and the whole calculation of what is subtracted from
    /// it; absent when nothing was tracked on the tool, and the filament is
    /// recorded uncharged.
    pub charged: Option<ChargedFilament>,
    /// When the charge was taken.
    pub at: Timestamp,
    /// Its `filament_consumed` event.
    pub event: EventDraft,
}

/// What the store holds of one debit.
#[derive(Debug, Clone, PartialEq)]
pub struct ChargeRecord {
    /// Whose debit it is, from which its tool and print are read.
    pub identity: ChargeIdentity,
    /// How much was fed, in millimetres.
    pub length_mm: f64,
    /// The spool charged and the whole calculation of what was subtracted;
    /// absent when the filament was recorded uncharged.
    pub charged: Option<ChargedFilament>,
    /// When the charge was taken.
    pub at: Timestamp,
    /// Its `filament_consumed` event.
    pub event_id: EventId,
}

impl ChargeRecord {
    /// The spool charged, when one was.
    #[must_use]
    pub fn spool_id(&self) -> Option<SpoolId> {
        self.charged.as_ref().map(|charged| charged.spool_id)
    }
}

/// A charge held for a spool not yet named, on its way into the store.
#[derive(Debug, Clone, PartialEq)]
pub struct Pend {
    /// What is held.
    pub pending: PendingCharge,
    /// Its `charge_pended` event.
    pub event: EventDraft,
}

/// One change to one spool, on its way into the store.
#[derive(Debug, Clone, PartialEq)]
pub struct SpoolMutation {
    /// The spool changed.
    pub spool_id: SpoolId,
    /// What changes, carrying the event it writes when it writes one.
    pub change: SpoolChange,
    /// When it changed; the spool's `updated_at` once it has.
    pub at: Timestamp,
}

/// What one [`SpoolMutation`] changes, each change that writes an event
/// carrying it and the one that writes none carrying none.
#[derive(Debug, Clone, PartialEq)]
pub enum SpoolChange {
    /// Put the spool on a tool.
    Load {
        /// The tool, in the printer's own numbering.
        tool: i64,
        /// Its `spool_loaded` event.
        event: EventDraft,
    },
    /// Take the spool off its tool.
    Unload {
        /// Its `spool_unloaded` event.
        event: EventDraft,
    },
    /// Set the spool's remaining figure, which clears every mark: the spool
    /// reads `verified` after it.
    SetRemaining {
        /// The filament left, in grams.
        remaining_g: f64,
        /// Its `spool_adjusted` event.
        event: EventDraft,
    },
    /// Add one mark, by [`SpoolTrust::marked`], at the mutation's instant.
    ///
    /// A reason the spool already holds changes nothing, so it is a no-op: the
    /// spool is not written and its `event` is not appended, which is how a
    /// second signal of one change writes no second `spool_unverified`.
    MarkUnverified {
        /// Why the figure is in doubt.
        reason: UnverifiedReason,
        /// Its `spool_unverified` event, appended only when the mark is new.
        event: EventDraft,
    },
    /// Remove every mark of these kinds, by [`SpoolTrust::cleared`]. The
    /// domain chooses the kinds through
    /// [`marks_cleared_by`](crate::filament::marks_cleared_by), and the marks
    /// cleared are named in the command's own event, so this change carries
    /// and writes no event of its own.
    ClearMarks {
        /// The kinds of mark removed.
        kinds: Vec<UnverifiedReasonKind>,
    },
    /// Retire the spool, which takes it off any tool it is loaded on.
    Retire {
        /// Its `spool_retired` event.
        event: EventDraft,
    },
}

/// What one [`FilamentStore::apply_spool_changes`] wrote.
#[derive(Debug, Clone, PartialEq)]
pub struct SpoolWrite {
    /// Every spool a charge or a mutation named, as the write left it, once
    /// each, in the order it was first named.
    pub spools: Vec<SpoolRecord>,
    /// Every event the write appended, in the order it appended them.
    pub events: Vec<EventRecord>,
    /// One record per charge submitted, in the order submitted: the one this
    /// write recorded, or, for a replay, the one already recorded.
    pub charges: Vec<ChargeRecord>,
    /// Every charge this write held; a replayed pend is not among them.
    pub pending: Vec<PendingCharge>,
}

/// What settling a print did.
#[derive(Debug, Clone, PartialEq)]
pub enum Settlement {
    /// This call settled it.
    Settled {
        /// When it was settled: its `print_ended` event's instant.
        at: Timestamp,
        /// Every event the settlement appended, in the order it appended them,
        /// its `print_ended` event last.
        events: Vec<EventRecord>,
    },
    /// It was already settled, and this call wrote nothing.
    AlreadySettled {
        /// When it was settled.
        at: Timestamp,
        /// The outcome it was settled with.
        outcome: PrintOutcome,
    },
}

/// The printer's own count of the prints of one file, as last accounted.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileTally {
    /// The file.
    // llmlint: ignore[invalid_states_unrepresentable] suppressions.toml has the reason.
    pub file_name: String,
    /// Where the file lives, in the printer's own vocabulary.
    pub origin: String,
    /// The tally generation, which starts at 0 and only ever advances: it
    /// advances each time the printer's tally for the file is reset.
    pub generation: u32,
    /// The completed prints the tally counts.
    pub successes: i64,
    /// The failed prints the tally counts.
    pub failures: i64,
    /// When the tally was read.
    pub observed_at: Timestamp,
}

/// Which timeline events to read.
///
/// An empty `kinds` means every kind of
/// [`TIMELINE_KINDS`](crate::kinds::TIMELINE_KINDS), and an absent `limit`
/// means [`DEFAULT_HISTORY_WINDOW`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TimelineQuery {
    /// The kinds to read; empty means every timeline kind, and a kind outside
    /// the timeline is refused.
    pub kinds: Vec<EventKind>,
    /// The earliest instant to read from, inclusive.
    pub since: Option<Timestamp>,
    /// The latest instant to read to, inclusive.
    pub until: Option<Timestamp>,
    /// Only events whose payload names this spool.
    pub spool_id: Option<SpoolId>,
    /// The cursor: only events strictly older than this one.
    pub before: Option<EventId>,
    /// How many events to answer; absent means [`DEFAULT_HISTORY_WINDOW`].
    pub limit: Option<u32>,
}

impl TimelineQuery {
    /// The limit this query resolves to.
    ///
    /// # Errors
    ///
    /// Returns [`StoreError::LimitRefused`] when the limit asked for is above
    /// [`MAX_HISTORY_LIMIT`].
    pub fn resolved_limit(&self) -> Result<u32, StoreError> {
        resolve_history_limit(self.limit)
    }

    /// The kinds this query reads: those asked for, or every timeline kind.
    ///
    /// # Errors
    ///
    /// Returns [`StoreError::ConstraintRefused`] naming `timeline.kinds` when a
    /// kind asked for is not one of
    /// [`TIMELINE_KINDS`](crate::kinds::TIMELINE_KINDS).
    pub fn resolved_kinds(&self) -> Result<Vec<EventKind>, StoreError> {
        let refused = || StoreError::ConstraintRefused {
            constraint: "timeline.kinds".to_owned(),
        };
        if self.kinds.is_empty() {
            return crate::kinds::TIMELINE_KINDS
                .iter()
                .map(|kind| EventKind::new(*kind).map_err(|_| refused()))
                .collect();
        }
        if self
            .kinds
            .iter()
            .any(|kind| !crate::kinds::TIMELINE_KINDS.contains(&kind.as_str()))
        {
            return Err(refused());
        }
        Ok(self.kinds.clone())
    }
}

/// The filament ledger: spools, the charges against them, the charges held for
/// a spool not yet named, each print's settlement and last file position, and
/// the printer's per-file print tallies.
///
/// # Every write is one transaction
///
/// Each writing method writes everything it was given and every event that
/// goes with it, or — when any part is refused — none of it: a crash or a
/// refusal halfway leaves the ledger as it was before the call.
///
/// # A charge is debited exactly once
///
/// A charge is keyed by its [`ChargeKey`], stored under
/// [`ChargeKey::canonical`]. Wherever a charge is submitted —
/// [`apply_spool_changes`](Self::apply_spool_changes),
/// [`settle_print`](Self::settle_print) or
/// [`account_tallies`](Self::account_tallies) — the same rules hold:
///
/// * A charge whose key is **not yet recorded** subtracts its charged grams
///   from its spool — a retired spool included, since retirement refuses later commands
///   rather than the settlement of earlier prints, and below zero included,
///   since a charge is never refused or clamped — records the charge, and
///   appends its event. The spool's `updated_at` becomes the later of its own
///   and the charge's instant. A charge naming no spool subtracts nothing and
///   is still recorded.
/// * A charge whose key is **already recorded** is a replay: it subtracts
///   nothing and appends no event.
/// * A charge whose key is **pending** takes it: the pending charge is removed
///   in the same transaction that records the charge, so it is debited once.
///
/// A pend is held the same way: one whose key is already pending or already
/// charged is a replay and writes nothing; otherwise it is held and its event
/// appended.
///
/// Within one call, the charges are applied first, in the order given, then
/// the pends, then the mutations — so a correction sets its figure after any
/// pending charge the same command took.
///
/// # A spool change, and what refuses one
///
/// A mutation applies its [`SpoolChange`], sets the spool's `updated_at` to its
/// instant, and appends the event the change carries — a `clear_marks` carries
/// none, and a `mark_unverified` the spool already holds is a no-op that
/// appends nothing. The trust functions are the contract's,
/// [`SpoolTrust::marked`] and [`SpoolTrust::cleared`], and no implementation
/// restates them. Every refusal is the whole call's:
///
/// * [`StoreError::NotFound`] for a spool that does not exist.
/// * [`StoreError::ConstraintRefused`] naming `spools.loaded_on_tool` for a
///   load onto a tool another unretired spool holds. A swap is two mutations
///   in one call, the unload and then the load.
/// * [`StoreError::ConstraintRefused`] naming `spools.retired_at` for a load,
///   an unload, a `set_remaining`, a `clear_marks` or a `retire` of a retired
///   spool. A `mark_unverified` still applies to one, because a settlement
///   marks the spool that fed the print whatever has happened to it since.
///
/// Every method is asynchronous, the trait is dyn-compatible, and it is
/// shareable across threads, because its consumers hold it behind
/// `Arc<dyn FilamentStore>`.
pub trait FilamentStore: Send + Sync {
    /// Register a spool, minting its identifier.
    ///
    /// In one transaction, writes the spool and a `spool_registered` event —
    /// with no print, from `source`, received at the spool's `registered_at` —
    /// whose payload carries the record as written, `actor` and `reason`.
    ///
    /// # Errors
    ///
    /// Refuses a spool registered loaded onto a tool another unretired spool
    /// holds with [`StoreError::ConstraintRefused`] naming
    /// `spools.loaded_on_tool`.
    fn register_spool(
        &self,
        spool: NewSpool,
        source: EventSource,
        actor: Actor,
        reason: String,
    ) -> BoxFuture<'_, Result<(SpoolRecord, EventRecord), StoreError>>;

    /// Apply spool mutations, charges and pends, with their events, in one
    /// transaction — all of them or none.
    ///
    /// # Errors
    ///
    /// Refuses the whole call as the trait's own documentation states.
    fn apply_spool_changes(
        &self,
        changes: Vec<SpoolMutation>,
        charges: Vec<Charge>,
        pends: Vec<Pend>,
    ) -> BoxFuture<'_, Result<SpoolWrite, StoreError>>;

    /// Settle a print, once.
    ///
    /// In one transaction: the print's remaining charges and its pends, under
    /// the replay rules, with their events; the `mark_unverified` mutations in
    /// `marks`, applied by the rules every mutation is, with their events; the
    /// `ended` event, which is the print's `print_ended`; and the print's
    /// settlement, at that event's instant, with `outcome`. A print already
    /// settled is answered [`Settlement::AlreadySettled`] and nothing is
    /// written.
    ///
    /// It does not end the print record: the domain calls
    /// [`PrintStore::end_print`] after it, so a crash between the two leaves a
    /// settled, open print that the next close-out only ends.
    ///
    /// # Errors
    ///
    /// Refuses a print that does not exist with [`StoreError::NotFound`], and
    /// otherwise as the trait's own documentation states.
    fn settle_print(
        &self,
        print_id: PrintId,
        outcome: PrintOutcome,
        ended: EventDraft,
        charges: Vec<Charge>,
        pends: Vec<Pend>,
        marks: Vec<SpoolMutation>,
    ) -> BoxFuture<'_, Result<Settlement, StoreError>>;

    /// When a print was settled, and with which outcome, when it has been.
    fn print_settlement(
        &self,
        print_id: PrintId,
    ) -> BoxFuture<'_, Result<Option<(Timestamp, PrintOutcome)>, StoreError>>;

    /// Every charge recorded against a print, by tool and then by segment.
    ///
    /// This is how a later segment is charged only the remainder after a
    /// restart.
    fn charges(&self, print_id: PrintId) -> BoxFuture<'_, Result<Vec<ChargeRecord>, StoreError>>;

    /// Every charge still pending — on one tool, when one is named — oldest
    /// first.
    fn pending_charges(
        &self,
        tool: Option<i64>,
    ) -> BoxFuture<'_, Result<Vec<PendingCharge>, StoreError>>;

    /// Read one spool.
    fn spool(&self, id: SpoolId) -> BoxFuture<'_, Result<Option<SpoolRecord>, StoreError>>;

    /// Read every spool — retired ones only when asked for — most recently
    /// registered first.
    fn spools(&self, include_retired: bool) -> BoxFuture<'_, Result<Vec<SpoolRecord>, StoreError>>;

    /// Read every unretired spool loaded on a tool, by tool.
    fn loaded_spools(&self) -> BoxFuture<'_, Result<Vec<SpoolRecord>, StoreError>>;

    /// Record how far into its file a print had got.
    ///
    /// Keeps the latest by `observed_at`: a position observed before the one
    /// held is not recorded.
    ///
    /// # Errors
    ///
    /// Refuses a print that does not exist with [`StoreError::NotFound`].
    fn record_file_position(
        &self,
        print_id: PrintId,
        position: FilePosition,
    ) -> BoxFuture<'_, Result<(), StoreError>>;

    /// The latest file position recorded for a print.
    fn file_position(
        &self,
        print_id: PrintId,
    ) -> BoxFuture<'_, Result<Option<FilePosition>, StoreError>>;

    /// Every file's tally as last accounted, by file name and then origin.
    fn file_tallies(&self) -> BoxFuture<'_, Result<Vec<FileTally>, StoreError>>;

    /// Account the prints the printer's tallies counted and this system never
    /// saw, in one transaction.
    ///
    /// Their charges, under the replay rules; their `mark_unverified`
    /// mutations, applied by the rules every mutation is; their
    /// `unaccounted_print` and `file_tally_rebaselined` events; and the
    /// advanced tallies, each stored by file name and origin over whatever was
    /// held — so a crash leaves either none of it or all of it. Answers every
    /// event it appended, in the order it appended them.
    ///
    /// # Errors
    ///
    /// Refuses a tally whose generation is below the one held for its file
    /// with [`StoreError::ConstraintRefused`] naming `file_tallies.generation`,
    /// and otherwise as the trait's own documentation states.
    fn account_tallies(
        &self,
        tallies: Vec<FileTally>,
        charges: Vec<Charge>,
        marks: Vec<SpoolMutation>,
        events: Vec<EventDraft>,
    ) -> BoxFuture<'_, Result<Vec<EventRecord>, StoreError>>;
}

/// The printer-wide reading of the event log.
///
/// Every method is asynchronous, the trait is dyn-compatible, and it is
/// shareable across threads, because its consumers hold it behind
/// `Arc<dyn TimelineStore>`.
pub trait TimelineStore: Send + Sync {
    /// Read the events of the printer timeline, across every print and none.
    ///
    /// Answers only events of [`TIMELINE_KINDS`](crate::kinds::TIMELINE_KINDS),
    /// **newest first** by `(received_at, id)`. An empty `kinds` means every
    /// timeline kind; `since` and `until` are inclusive; `spool_id` keeps only
    /// events whose payload names that spool — carries its identifier as a
    /// string value anywhere in it; and `before` keeps only events strictly
    /// older than that event. The limit is resolved by
    /// [`resolve_history_limit`].
    ///
    /// # Errors
    ///
    /// Refuses a kind outside the timeline with
    /// [`StoreError::ConstraintRefused`] naming `timeline.kinds`, a limit above
    /// [`MAX_HISTORY_LIMIT`] with [`StoreError::LimitRefused`], and a cursor
    /// naming no event with [`StoreError::NotFound`].
    fn timeline(&self, query: TimelineQuery)
    -> BoxFuture<'_, Result<Vec<EventRecord>, StoreError>>;
}

/// One store implementation, held once per aggregate it persists.
///
/// The composition root builds this from the one implementation it chose, by
/// [`Stores::of`], and each consumer takes the handle for the aggregate it
/// names: seven shared handles to one backing rather than seven backings. A
/// consumer that needs several aggregates holds the bundle; one that needs one
/// clones that handle out of it.
#[derive(Clone)]
pub struct Stores {
    /// The print record, its manifest and its narrowings.
    pub prints: Arc<dyn PrintStore>,
    /// The event log.
    pub events: Arc<dyn EventStore>,
    /// The images stored beside events.
    pub images: Arc<dyn ImageStore>,
    /// Actions, and the bounded interventions they open.
    pub actions: Arc<dyn ActionStore>,
    /// The supervision session of a print.
    pub sessions: Arc<dyn SessionStore>,
    /// The filament ledger.
    pub filament: Arc<dyn FilamentStore>,
    /// The printer-wide reading of the event log.
    pub timeline: Arc<dyn TimelineStore>,
}

impl Stores {
    /// Every aggregate's handle onto one implementation.
    pub fn of<S>(store: Arc<S>) -> Self
    where
        S: PrintStore
            + EventStore
            + ImageStore
            + ActionStore
            + SessionStore
            + FilamentStore
            + TimelineStore
            + 'static,
    {
        Self {
            prints: Arc::clone(&store) as Arc<dyn PrintStore>,
            events: Arc::clone(&store) as Arc<dyn EventStore>,
            images: Arc::clone(&store) as Arc<dyn ImageStore>,
            actions: Arc::clone(&store) as Arc<dyn ActionStore>,
            sessions: Arc::clone(&store) as Arc<dyn SessionStore>,
            filament: Arc::clone(&store) as Arc<dyn FilamentStore>,
            timeline: store as Arc<dyn TimelineStore>,
        }
    }
}
