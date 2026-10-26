//! The event log, followed in the order it was stored.
//!
//! # One read, and a wait that every waiter wakes from
//!
//! [`events`] answers the events stored after a cursor that pass its filters,
//! and when none has been stored yet it waits — up to [`MAX_EVENTS_WAIT_S`]
//! seconds — for one to be. The wait is on a broadcast of the store's own
//! signal: every write that stores an event raises it once committed, and
//! every waiting read wakes, reads again and answers or waits on. Twenty reads
//! waiting at once are twenty receivers of one channel rather than twenty
//! takers of one arrival, and none of them holds a runtime worker while it
//! waits, so a status read made meanwhile is answered meanwhile.
//!
//! # The cursor is the last event examined
//!
//! Not the last event answered: a row the filters dropped is still a row this
//! read examined, so the next read starts after it rather than examining it
//! again. With no cursor the read starts after the newest event stored at the
//! request — or, scoped to a print, at that print's first event, so a print is
//! replayed and then followed.
//!
//! # The end of a print is told whatever the filters say
//!
//! A read scoped to a print answers that print's end as stored — its
//! `print_ended` event's identifier, instant and outcome — beside the events,
//! and null while it is open, so a reader filtering that event out still learns
//! the print is over.

use std::time::Duration;

use axum::extract::{Extension, Query, State};
use axum::http::StatusCode;
use axum::response::Response;
use printobserver_core::PrintEndedPayload;
use printobserver_core::store::{HistoryQuery, StoreError, StoredQuery};
use printobserver_types::{EventId, EventKind, EventPayload, EventRecord, PrintId};
use tokio::sync::watch;
use tokio::time::Instant;

use crate::api::{ApiState, Caller, answer, refusal, scope_refused, store_status};
use crate::wire::{EventsAnswer, PrintEnded};

/// The longest an events read waits for one to pass, in seconds.
pub const MAX_EVENTS_WAIT_S: u32 = 60;

/// How many events one read answers when it asks for no number.
pub const DEFAULT_EVENTS_LIMIT: u32 = 100;

/// The most events one read answers.
pub const MAX_EVENTS_LIMIT: u32 = 500;

/// How many stored events one read examines per read of the store, while it
/// looks for events that pass its filters.
const EXAMINED_PER_READ: u32 = 500;

/// A broadcast that some event was stored, which every waiting read holds a
/// receiver of.
#[derive(Debug, Clone)]
pub struct Stored {
    /// What a read waits on.
    receiver: watch::Receiver<u64>,
}

impl Stored {
    /// The broadcast, and the signal a store raises it by.
    #[must_use]
    pub fn channel() -> (Self, printobserver_core::store::StoredSignal) {
        let (sender, receiver) = watch::channel(0_u64);
        let signal = std::sync::Arc::new(move || {
            sender.send_modify(|stored| *stored = stored.wrapping_add(1));
        });
        (Self { receiver }, signal)
    }
}

/// What one events read asked for.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Asked {
    /// The event to read after.
    after: Option<EventId>,
    /// The print to scope to.
    print_id: Option<PrintId>,
    /// Only these kinds; empty, every kind.
    kinds: Vec<EventKind>,
    /// Never these kinds.
    exclude_kinds: Vec<EventKind>,
    /// How long to wait for an event to pass.
    wait_s: u32,
    /// How many events to answer.
    limit: u32,
}

impl Asked {
    /// Whether one event passes this read's filters.
    fn passes(&self, event: &EventRecord) -> bool {
        (self.kinds.is_empty() || self.kinds.contains(event.kind()))
            && !self.exclude_kinds.contains(event.kind())
    }
}

/// One value of an events read, refused naming it when it is not one.
fn read_one<T: core::str::FromStr>(name: &str, text: &str) -> Result<T, String>
where
    T::Err: core::fmt::Display,
{
    text.parse()
        .map_err(|error: T::Err| format!("`{name}` is `{text}`, which will not do: {error}"))
}

/// One bounded whole number of an events read.
fn bounded(name: &str, text: &str, low: u32, high: u32) -> Result<u32, String> {
    let read: u32 = read_one(name, text)?;
    if (low..=high).contains(&read) {
        Ok(read)
    } else {
        Err(format!(
            "`{name}` may be from {low} to {high}; {read} was asked for"
        ))
    }
}

/// What a request's values ask for.
///
/// `kinds` and `exclude_kinds` may be given any number of times, each one kind
/// matching the event log's own pattern; a kind no crate declares is taken,
/// because the log is open. Every other value is given at most once, and a
/// value this read does not take is refused naming it rather than ignored.
fn asked(values: &[(String, String)]) -> Result<Asked, String> {
    let mut asked = Asked {
        after: None,
        print_id: None,
        kinds: Vec::new(),
        exclude_kinds: Vec::new(),
        wait_s: 0,
        limit: DEFAULT_EVENTS_LIMIT,
    };
    let mut seen: Vec<&str> = Vec::new();
    for (name, text) in values {
        let repeated = name == "kinds" || name == "exclude_kinds";
        if !repeated {
            if seen.contains(&name.as_str()) {
                return Err(format!(
                    "`{name}` was given more than once, and this read takes one"
                ));
            }
            seen.push(name);
        }
        match name.as_str() {
            "after" => asked.after = Some(read_one(name, text)?),
            "print_id" => asked.print_id = Some(read_one(name, text)?),
            "kinds" => asked.kinds.push(read_one(name, text)?),
            "exclude_kinds" => asked.exclude_kinds.push(read_one(name, text)?),
            "wait_s" => asked.wait_s = bounded(name, text, 0, MAX_EVENTS_WAIT_S)?,
            "limit" => asked.limit = bounded(name, text, 1, MAX_EVENTS_LIMIT)?,
            other => return Err(format!("an events read takes no `{other}`")),
        }
    }
    Ok(asked)
}

/// Read the event log after a cursor, waiting for an event to pass.
///
/// A supervision turn's credential reaches its own print alone, so a turn's
/// read names that print; a read of every print's events is the operator's.
pub async fn events(
    State(state): State<ApiState>,
    Extension(caller): Extension<Caller>,
    Query(values): Query<Vec<(String, String)>>,
) -> Response {
    let asked = match asked(&values) {
        Ok(asked) => asked,
        Err(refused) => return refusal(StatusCode::BAD_REQUEST, refused),
    };
    match (&caller, asked.print_id) {
        (Caller::Turn(binding), None) => {
            return refusal(
                StatusCode::FORBIDDEN,
                format!(
                    "this request authenticated as the agent on print {} and names no print: a \
                     supervision turn's credential reads its own print's events alone, so name \
                     it with `print_id`",
                    binding.print_id()
                ),
            );
        }
        (_, Some(print_id)) => {
            if let Some(refused) = scope_refused(&caller, print_id) {
                return refused;
            }
            match state.prints.print(print_id).await {
                Ok(Some(_)) => {}
                Ok(None) => {
                    return refusal(
                        StatusCode::NOT_FOUND,
                        format!("there is no print {print_id}"),
                    );
                }
                Err(error) => return refusal(store_status(&error), error),
            }
        }
        (Caller::Operator, None) => {}
    }
    match follow(&state, &asked).await {
        Ok(answered) => answer(StatusCode::OK, &answered),
        Err(error) => refusal(store_status(&error), error),
    }
}

/// Where a read with no cursor starts: after the newest event stored, unless
/// it is scoped to a print with events of its own, which it replays from the
/// first of them — which the store's own read starts at when given none.
async fn start(state: &ApiState, asked: &Asked) -> Result<Option<EventId>, StoreError> {
    if asked.after.is_some() {
        return Ok(asked.after);
    }
    if let Some(print_id) = asked.print_id {
        let own = state
            .events
            .history(HistoryQuery {
                print_id,
                kinds: Vec::new(),
                since: None,
                until: None,
                limit: Some(1),
            })
            .await?;
        if !own.is_empty() {
            return Ok(None);
        }
    }
    state.events.newest_stored().await
}

/// Answer the events after the cursor that pass, waiting for one when none
/// has been stored yet.
async fn follow(state: &ApiState, asked: &Asked) -> Result<EventsAnswer, StoreError> {
    let deadline = Instant::now() + Duration::from_secs(u64::from(asked.wait_s));
    let mut stored = state.stored.receiver.clone();
    let mut cursor = start(state, asked).await?;
    loop {
        // Marked seen before the store is read, so an event stored between the
        // read and the wait is one the wait wakes for.
        stored.borrow_and_update();
        let (passing, examined) = examine(state, asked, cursor).await?;
        cursor = examined;
        let waited_out = Instant::now() >= deadline;
        if !passing.is_empty() || waited_out {
            return Ok(EventsAnswer {
                events: passing,
                cursor,
                print_ended: match asked.print_id {
                    Some(print_id) => Some(ended(state, print_id).await?),
                    None => None,
                },
            });
        }
        // A sender that is gone is a server shutting down; the deadline still
        // ends the read, rather than a loop that spins on a closed channel.
        if tokio::time::timeout_at(deadline, stored.changed())
            .await
            .is_ok_and(|changed| changed.is_err())
        {
            tokio::time::sleep_until(deadline).await;
        }
    }
}

/// The events after a cursor that pass, up to the limit, and the last event
/// examined to find them.
async fn examine(
    state: &ApiState,
    asked: &Asked,
    mut cursor: Option<EventId>,
) -> Result<(Vec<EventRecord>, Option<EventId>), StoreError> {
    let mut passing = Vec::new();
    loop {
        let read = state
            .events
            .stored_after(StoredQuery {
                after: cursor,
                print_id: asked.print_id,
                limit: Some(EXAMINED_PER_READ),
            })
            .await?;
        let exhausted = read.len() < EXAMINED_PER_READ as usize;
        for event in read {
            cursor = Some(event.id);
            if asked.passes(&event) {
                passing.push(event);
                if passing.len() >= asked.limit as usize {
                    return Ok((passing, cursor));
                }
            }
        }
        if exhausted {
            return Ok((passing, cursor));
        }
    }
}

/// One print's end as stored, when it has ended.
async fn ended(state: &ApiState, print_id: PrintId) -> Result<Option<PrintEnded>, StoreError> {
    let kind = EventKind::new(PrintEndedPayload::KIND).map_err(|error| StoreError::Database {
        detail: error.to_string(),
    })?;
    let found = state
        .events
        .history(HistoryQuery {
            print_id,
            kinds: vec![kind],
            since: None,
            until: None,
            limit: Some(1),
        })
        .await?;
    Ok(found.into_iter().find_map(|event| {
        let payload: PrintEndedPayload = event.body.read()?.ok()?;
        Some(PrintEnded {
            event_id: event.id,
            received_at: event.received_at,
            outcome: payload.outcome,
        })
    }))
}
