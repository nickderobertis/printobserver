//! The filament ledger's writes, stated once for both stores.
//!
//! `FilamentStore` states what a write does — the replay rules, the order the
//! parts of one call are applied in, which change writes an event and what
//! refuses one. Those rules are written here once, over [`Ledger`]: the few
//! primitive reads and writes a store answers inside one transaction. Each
//! store implements the primitives over its own backing and runs every write
//! through these functions, so the durable store and the in-memory one cannot
//! drift into applying the rules differently; the conformance suite then holds
//! both to the trait's statement of them.

use printobserver_core::kinds::SpoolRegisteredPayload;
use printobserver_core::store::{
    Charge, ChargeRecord, EventDraft, FileTally, NewSpool, Pend, Settlement, SpoolChange,
    SpoolMutation, SpoolWrite, StoreError,
};
use printobserver_core::{
    Actor, ChargeKey, PendingCharge, PrintOutcome, SpoolId, SpoolLifecycle, SpoolRecord,
    SpoolTrust, UnverifiedMark,
};
use printobserver_types::serde_json::Value;
use printobserver_types::{EventBody, EventRecord, EventSource, PrintId, Timestamp};

use crate::values::{not_found, refused};

/// The reads and writes one transaction of a store answers.
///
/// Every method acts inside the transaction the caller opened, and nothing it
/// writes is visible outside it until that transaction commits.
pub(crate) trait Ledger {
    /// One spool.
    fn spool(&mut self, id: SpoolId) -> Result<Option<SpoolRecord>, StoreError>;
    /// Write a spool that does not exist yet.
    ///
    /// Refuses one loaded onto a tool another unretired spool holds, naming
    /// `spools.loaded_on_tool`.
    fn insert_spool(&mut self, spool: &SpoolRecord) -> Result<(), StoreError>;
    /// Write a spool that exists, over what it held.
    ///
    /// Refuses one loaded onto a tool another unretired spool holds, naming
    /// `spools.loaded_on_tool`.
    fn update_spool(&mut self, spool: &SpoolRecord) -> Result<(), StoreError>;
    /// The charge recorded under one canonical key.
    fn charge(&mut self, key: &str) -> Result<Option<ChargeRecord>, StoreError>;
    /// Record one charge.
    fn insert_charge(&mut self, record: &ChargeRecord) -> Result<(), StoreError>;
    /// Whether a charge is pending under one canonical key.
    fn is_pending(&mut self, key: &str) -> Result<bool, StoreError>;
    /// Remove the charge pending under one canonical key.
    fn remove_pending(&mut self, key: &str) -> Result<(), StoreError>;
    /// Hold one pending charge, beside the event that says it was held.
    fn insert_pending(
        &mut self,
        pending: &PendingCharge,
        event: &EventRecord,
    ) -> Result<(), StoreError>;
    /// Append one event, minting its identifier.
    fn append(&mut self, draft: EventDraft) -> Result<EventRecord, StoreError>;
    /// Whether a print exists.
    fn print_exists(&mut self, print_id: PrintId) -> Result<bool, StoreError>;
    /// A print's settlement, when it has one.
    fn settlement(
        &mut self,
        print_id: PrintId,
    ) -> Result<Option<(Timestamp, PrintOutcome)>, StoreError>;
    /// Record a print's settlement, beside its `print_ended` event.
    fn insert_settlement(
        &mut self,
        print_id: PrintId,
        outcome: PrintOutcome,
        ended: &EventRecord,
    ) -> Result<(), StoreError>;
    /// The tally held for one file.
    fn tally(&mut self, file_name: &str, origin: &str) -> Result<Option<FileTally>, StoreError>;
    /// Store one file's tally over whatever was held.
    fn put_tally(&mut self, tally: &FileTally) -> Result<(), StoreError>;
}

/// What one call has written so far.
#[derive(Debug, Default)]
struct Written {
    /// Every spool a charge or a mutation named, once each, first named first.
    named: Vec<SpoolId>,
    /// Every event appended, in order.
    events: Vec<EventRecord>,
    /// One record per charge submitted.
    charges: Vec<ChargeRecord>,
    /// Every pending charge held.
    pending: Vec<PendingCharge>,
}

impl Written {
    /// Remember that a spool was named.
    fn name(&mut self, spool_id: SpoolId) {
        if !self.named.contains(&spool_id) {
            self.named.push(spool_id);
        }
    }
}

/// An event's body, refusing a payload that will not serialize.
fn body_of<P: printobserver_types::EventPayload>(payload: &P) -> Result<EventBody, StoreError> {
    EventBody::of(payload).map_err(|error| StoreError::Database {
        detail: format!("an event payload could not be written as JSON: {error}"),
    })
}

/// The refusal for a spool nothing registered.
fn no_spool(spool_id: SpoolId) -> StoreError {
    not_found(&format!("spool {spool_id}"))
}

/// Register a spool and its `spool_registered` event.
pub(crate) fn register(
    ledger: &mut impl Ledger,
    spool: NewSpool,
    source: EventSource,
    actor: Actor,
    reason: String,
) -> Result<(SpoolRecord, EventRecord), StoreError> {
    let record = SpoolRecord {
        id: SpoolId::new(),
        name: spool.name,
        material: spool.material,
        color: spool.color,
        diameter_mm: spool.diameter_mm,
        density_g_cm3: spool.density_g_cm3,
        initial_g: spool.initial_g,
        remaining_g: spool.remaining_g,
        tare_g: spool.tare_g,
        lifecycle: spool
            .loaded_on_tool
            .map_or(SpoolLifecycle::Shelved, |tool| SpoolLifecycle::Loaded {
                tool,
            }),
        trust: spool.trust,
        registered_at: spool.registered_at,
        updated_at: spool.registered_at,
    };
    ledger.insert_spool(&record)?;
    let body = body_of(&SpoolRegisteredPayload {
        spool: record.clone(),
        actor,
        reason,
    })?;
    let event = ledger.append(EventDraft {
        print_id: None,
        source,
        received_at: record.registered_at,
        body,
        raw: None,
    })?;
    Ok((record, event))
}

/// The spool one change leaves, and the event the change writes when it
/// writes one; or nothing at all, for a change that changes nothing.
///
/// # Errors
///
/// Refuses any change but a mark to a retired spool, naming
/// `spools.retired_at`.
pub(crate) fn changed(
    spool: &SpoolRecord,
    change: SpoolChange,
    at: Timestamp,
) -> Result<Option<(SpoolRecord, Option<EventDraft>)>, StoreError> {
    let marking = matches!(change, SpoolChange::MarkUnverified { .. });
    if spool.is_retired() && !marking {
        return Err(refused("spools.retired_at"));
    }
    let mut next = spool.clone();
    next.updated_at = at;
    let event = match change {
        SpoolChange::Load { tool, event } => {
            next.lifecycle = SpoolLifecycle::Loaded { tool };
            event
        }
        SpoolChange::Unload { event } => {
            next.lifecycle = SpoolLifecycle::Shelved;
            event
        }
        SpoolChange::SetRemaining { remaining_g, event } => {
            next.remaining_g = remaining_g;
            next.trust = SpoolTrust::Verified;
            event
        }
        SpoolChange::MarkUnverified { reason, event } => {
            next.trust = spool
                .trust
                .clone()
                .marked(UnverifiedMark { reason, since: at });
            if next.trust == spool.trust {
                return Ok(None);
            }
            event
        }
        SpoolChange::ClearMarks { kinds } => {
            let (trust, removed) = spool.trust.clone().cleared(&kinds);
            if removed.is_empty() {
                return Ok(None);
            }
            next.trust = trust;
            return Ok(Some((next, None)));
        }
        SpoolChange::Retire { event } => {
            next.lifecycle = SpoolLifecycle::Retired { at };
            event
        }
    };
    Ok(Some((next, Some(event))))
}

/// Apply one mutation, with its event when it writes one.
fn mutate(
    ledger: &mut impl Ledger,
    mutation: SpoolMutation,
    written: &mut Written,
) -> Result<(), StoreError> {
    let spool = ledger
        .spool(mutation.spool_id)?
        .ok_or_else(|| no_spool(mutation.spool_id))?;
    written.name(spool.id);
    let Some((next, event)) = changed(&spool, mutation.change, mutation.at)? else {
        return Ok(());
    };
    ledger.update_spool(&next)?;
    if let Some(event) = event {
        written.events.push(ledger.append(event)?);
    }
    Ok(())
}

/// Apply one charge under the replay rules.
fn charge(
    ledger: &mut impl Ledger,
    charge: Charge,
    written: &mut Written,
) -> Result<(), StoreError> {
    let key = charge.identity.key().canonical();
    if let Some(existing) = ledger.charge(&key)? {
        if let Some(spool_id) = existing.spool_id() {
            written.name(spool_id);
        }
        written.charges.push(existing);
        return Ok(());
    }
    if let Some(charged) = &charge.charged {
        let mut spool = ledger
            .spool(charged.spool_id)?
            .ok_or_else(|| no_spool(charged.spool_id))?;
        written.name(charged.spool_id);
        spool.remaining_g -= charged.grams;
        spool.updated_at = spool.updated_at.max(charge.at);
        ledger.update_spool(&spool)?;
    }
    if ledger.is_pending(&key)? {
        ledger.remove_pending(&key)?;
    }
    let event = ledger.append(charge.event)?;
    let record = ChargeRecord {
        identity: charge.identity,
        length_mm: charge.length_mm,
        charged: charge.charged,
        at: charge.at,
        event_id: event.id,
    };
    ledger.insert_charge(&record)?;
    written.events.push(event);
    written.charges.push(record);
    Ok(())
}

/// Hold one pending charge under the replay rules.
fn pend(ledger: &mut impl Ledger, pend: Pend, written: &mut Written) -> Result<(), StoreError> {
    let key = pend.pending.charge_key.charge_key().canonical();
    if ledger.is_pending(&key)? || ledger.charge(&key)?.is_some() {
        return Ok(());
    }
    let event = ledger.append(pend.event)?;
    ledger.insert_pending(&pend.pending, &event)?;
    written.events.push(event);
    written.pending.push(pend.pending);
    Ok(())
}

/// Charges, then pends, then mutations, in one write.
fn apply(
    ledger: &mut impl Ledger,
    mutations: Vec<SpoolMutation>,
    charges: Vec<Charge>,
    pends: Vec<Pend>,
) -> Result<Written, StoreError> {
    let mut written = Written::default();
    for submitted in charges {
        charge(ledger, submitted, &mut written)?;
    }
    for submitted in pends {
        pend(ledger, submitted, &mut written)?;
    }
    for mutation in mutations {
        mutate(ledger, mutation, &mut written)?;
    }
    Ok(written)
}

/// [`FilamentStore::apply_spool_changes`](printobserver_core::store::FilamentStore::apply_spool_changes).
pub(crate) fn apply_changes(
    ledger: &mut impl Ledger,
    mutations: Vec<SpoolMutation>,
    charges: Vec<Charge>,
    pends: Vec<Pend>,
) -> Result<SpoolWrite, StoreError> {
    let written = apply(ledger, mutations, charges, pends)?;
    let mut spools = Vec::with_capacity(written.named.len());
    for spool_id in written.named {
        spools.push(ledger.spool(spool_id)?.ok_or_else(|| no_spool(spool_id))?);
    }
    Ok(SpoolWrite {
        spools,
        events: written.events,
        charges: written.charges,
        pending: written.pending,
    })
}

/// [`FilamentStore::settle_print`](printobserver_core::store::FilamentStore::settle_print).
pub(crate) fn settle(
    ledger: &mut impl Ledger,
    print_id: PrintId,
    outcome: PrintOutcome,
    ended: EventDraft,
    charges: Vec<Charge>,
    pends: Vec<Pend>,
    marks: Vec<SpoolMutation>,
) -> Result<Settlement, StoreError> {
    if !ledger.print_exists(print_id)? {
        return Err(not_found(&format!("print {print_id}")));
    }
    if let Some((at, outcome)) = ledger.settlement(print_id)? {
        return Ok(Settlement::AlreadySettled { at, outcome });
    }
    let mut written = apply(ledger, marks, charges, pends)?;
    let ended = ledger.append(ended)?;
    ledger.insert_settlement(print_id, outcome, &ended)?;
    let at = ended.received_at;
    written.events.push(ended);
    Ok(Settlement::Settled {
        at,
        events: written.events,
    })
}

/// [`FilamentStore::account_tallies`](printobserver_core::store::FilamentStore::account_tallies).
pub(crate) fn account(
    ledger: &mut impl Ledger,
    tallies: &[FileTally],
    charges: Vec<Charge>,
    marks: Vec<SpoolMutation>,
    events: Vec<EventDraft>,
) -> Result<Vec<EventRecord>, StoreError> {
    let mut written = apply(ledger, marks, charges, Vec::new())?;
    for draft in events {
        written.events.push(ledger.append(draft)?);
    }
    for tally in tallies {
        if let Some(held) = ledger.tally(&tally.file_name, &tally.origin)?
            && tally.generation < held.generation
        {
            return Err(refused("file_tallies.generation"));
        }
        ledger.put_tally(tally)?;
    }
    Ok(written.events)
}

/// The order a print's charges are answered in: by tool, then by segment.
pub(crate) fn sort_charges(charges: &mut [ChargeRecord]) {
    charges.sort_by_cached_key(order_of);
}

/// One charge's place in [`sort_charges`]'s order.
fn order_of(record: &ChargeRecord) -> (i64, u32, Timestamp, String) {
    let key = record.identity.key();
    let segment = match key {
        ChargeKey::Print { segment, .. } => *segment,
        ChargeKey::Unobserved { .. } => 0,
    };
    (record.identity.tool(), segment, record.at, key.canonical())
}

/// A charge key read back from its canonical spelling.
///
/// The spelling puts the one free-text part that may carry a colon — the file
/// name — last; the origin is taken as the shortest prefix whose remainder
/// reads back, and the key is accepted only when it spells the text again.
pub(crate) fn key_from_canonical(text: &str) -> Option<ChargeKey> {
    if let Some(rest) = text.strip_prefix("print:") {
        let mut parts = rest.splitn(3, ':');
        let key = ChargeKey::Print {
            print_id: parts.next()?.parse().ok()?,
            tool: parts.next()?.parse().ok()?,
            segment: parts.next()?.parse().ok()?,
        };
        return (key.canonical() == text).then_some(key);
    }
    let rest = text.strip_prefix("unobserved:")?;
    rest.match_indices(':').find_map(|(split, _)| {
        let origin = &rest[..split];
        let mut parts = rest[split + 1..].splitn(4, ':');
        let key = ChargeKey::Unobserved {
            origin: origin.to_owned(),
            generation: parts.next()?.parse().ok()?,
            print_number: parts.next()?.parse().ok()?,
            tool: parts.next()?.parse().ok()?,
            file_name: parts.next()?.to_owned(),
        };
        (key.canonical() == text).then_some(key)
    })
}

/// Whether a payload carries one spool's identifier as a string value, at any
/// depth.
pub(crate) fn names_spool(payload: &Value, spool_id: &str) -> bool {
    match payload {
        Value::String(text) => text == spool_id,
        Value::Array(items) => items.iter().any(|item| names_spool(item, spool_id)),
        Value::Object(fields) => fields.values().any(|value| names_spool(value, spool_id)),
        Value::Null | Value::Bool(_) | Value::Number(_) => false,
    }
}
