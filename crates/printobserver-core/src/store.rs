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
//!
//! An implementation implements all five over one backing, and the composition
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

use printobserver_types::schemars::JsonSchema;
use printobserver_types::serde::{Deserialize, Serialize};
use printobserver_types::{
    ActionId, ActionRecord, ActionRequest, Adjustable, EventBody, EventId, EventKind, EventRecord,
    EventSource, ExecutionOutcome, ImageId, ImageRecord, Intervention, InterventionId,
    InterventionOutcome, JobManifest, ManifestNarrowing, PolicyDecision, PrintId, PrintRecord,
    PrinterState, RawBytes, SupervisionSession, Timestamp,
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
        obico_print_id: Option<i64>,
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

    /// Read one print by Obico's own identifier for it.
    fn print_by_obico_id(
        &self,
        obico_print_id: i64,
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

/// One store implementation, held once per aggregate it persists.
///
/// The composition root builds this from the one implementation it chose, by
/// [`Stores::of`], and each consumer takes the handle for the aggregate it
/// names: five shared handles to one backing rather than five backings. A
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
}

impl Stores {
    /// Every aggregate's handle onto one implementation.
    pub fn of<S>(store: Arc<S>) -> Self
    where
        S: PrintStore + EventStore + ImageStore + ActionStore + SessionStore + 'static,
    {
        Self {
            prints: Arc::clone(&store) as Arc<dyn PrintStore>,
            events: Arc::clone(&store) as Arc<dyn EventStore>,
            images: Arc::clone(&store) as Arc<dyn ImageStore>,
            actions: Arc::clone(&store) as Arc<dyn ActionStore>,
            sessions: store as Arc<dyn SessionStore>,
        }
    }
}

impl core::fmt::Debug for Stores {
    fn fmt(&self, formatter: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        formatter.debug_struct("Stores").finish_non_exhaustive()
    }
}
