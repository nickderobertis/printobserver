//! The filament ledger and the printer timeline, held in memory for the fake
//! store.
//!
//! Faithful to what `FilamentStore` and `TimelineStore` state rather than a
//! stub, because the journeys that account filament run on the fake store: the
//! replay rules, the order one call's parts are applied in, which change writes
//! an event, every refusal and the all-or-nothing write are the trait's, applied
//! here to a copy of the ledger that replaces it only when the whole write
//! succeeded. This crate may depend on no store implementation, so this is its
//! own double rather than the store crate's.

use std::collections::BTreeMap;

use printobserver_core::kinds::SpoolRegisteredPayload;
use printobserver_core::store::{
    Charge, ChargeRecord, EventDraft, FileTally, NewSpool, Pend, Settlement, SpoolChange,
    SpoolMutation, SpoolWrite, StoreError, TimelineQuery,
};
use printobserver_core::{
    Actor, ChargeKey, FilePosition, PendingCharge, PrintOutcome, PrintRecord, SpoolId,
    SpoolLifecycle, SpoolRecord, SpoolTrust, UnverifiedMark,
};
use printobserver_types::serde_json::Value;
use printobserver_types::{EventBody, EventId, EventRecord, EventSource, PrintId, Timestamp};

/// A constraint refused the write.
fn refused(constraint: &str) -> StoreError {
    StoreError::ConstraintRefused {
        constraint: constraint.to_owned(),
    }
}

/// There is no such record.
fn not_found(what: String) -> StoreError {
    StoreError::NotFound { what }
}

/// Every record of the filament ledger.
#[derive(Debug, Default, Clone)]
pub struct FakeLedger {
    /// The spools, in the order registered.
    spools: Vec<SpoolRecord>,
    /// The charges, in the order recorded.
    charges: Vec<ChargeRecord>,
    /// The pending charges, in the order held.
    pending: Vec<PendingCharge>,
    /// Each settled print's settlement.
    settlements: BTreeMap<PrintId, (Timestamp, PrintOutcome)>,
    /// Each print's latest file position.
    positions: BTreeMap<PrintId, FilePosition>,
    /// One tally per file and origin.
    tallies: BTreeMap<(String, String), FileTally>,
}

impl FakeLedger {
    /// One spool.
    pub fn spool(&self, id: SpoolId) -> Option<SpoolRecord> {
        self.spools.iter().find(|spool| spool.id == id).cloned()
    }

    /// Every spool, retired ones when asked for, most recently registered first.
    pub fn spools(&self, include_retired: bool) -> Vec<SpoolRecord> {
        let mut found: Vec<SpoolRecord> = self
            .spools
            .iter()
            .filter(|spool| include_retired || !spool.is_retired())
            .cloned()
            .collect();
        found.sort_by_key(|spool| core::cmp::Reverse((spool.registered_at, spool.id)));
        found
    }

    /// Every unretired spool on a tool, by tool.
    pub fn loaded_spools(&self) -> Vec<SpoolRecord> {
        let mut found: Vec<SpoolRecord> = self
            .spools
            .iter()
            .filter(|spool| spool.loaded_tool().is_some())
            .cloned()
            .collect();
        found.sort_by_key(SpoolRecord::loaded_tool);
        found
    }

    /// Every charge against one print, by tool and then segment.
    pub fn charges(&self, print_id: PrintId) -> Vec<ChargeRecord> {
        let mut found: Vec<ChargeRecord> = self
            .charges
            .iter()
            .filter(|charge| charge.identity.print_id() == Some(print_id))
            .cloned()
            .collect();
        found.sort_by_cached_key(|charge| {
            let key = charge.identity.key();
            let segment = match key {
                ChargeKey::Print { segment, .. } => *segment,
                ChargeKey::Unobserved { .. } => 0,
            };
            (charge.identity.tool(), segment, charge.at, key.canonical())
        });
        found
    }

    /// Every pending charge, on one tool when one is named, oldest first.
    pub fn pending(&self, tool: Option<i64>) -> Vec<PendingCharge> {
        let mut found: Vec<PendingCharge> = self
            .pending
            .iter()
            .filter(|pending| tool.is_none_or(|tool| pending.tool() == tool))
            .cloned()
            .collect();
        found.sort_by_cached_key(|pending| {
            (
                pending.pended_at,
                pending.charge_key.charge_key().canonical(),
            )
        });
        found
    }

    /// A print's settlement.
    pub fn settlement(&self, print_id: PrintId) -> Option<(Timestamp, PrintOutcome)> {
        self.settlements.get(&print_id).copied()
    }

    /// A print's latest file position.
    pub fn position(&self, print_id: PrintId) -> Option<FilePosition> {
        self.positions.get(&print_id).cloned()
    }

    /// Record a print's file position, keeping the latest by when it was read.
    pub fn record_position(&mut self, print_id: PrintId, position: FilePosition) {
        let stale = self
            .positions
            .get(&print_id)
            .is_some_and(|held| held.observed_at > position.observed_at);
        if !stale {
            self.positions.insert(print_id, position);
        }
    }

    /// Every file's tally, by file name and then origin.
    pub fn tallies(&self) -> Vec<FileTally> {
        self.tallies.values().cloned().collect()
    }
}

/// What one call has written so far.
#[derive(Debug, Default)]
struct Written {
    /// Every spool a charge or a mutation named, once each, first named first.
    named: Vec<SpoolId>,
    /// One record per charge submitted.
    charges: Vec<ChargeRecord>,
    /// Every pending charge held.
    pending: Vec<PendingCharge>,
    /// Every event appended, in order.
    events: Vec<EventRecord>,
}

impl Written {
    /// Remember that a spool was named.
    fn name(&mut self, spool_id: SpoolId) {
        if !self.named.contains(&spool_id) {
            self.named.push(spool_id);
        }
    }
}

/// One write in progress, over a copy of the ledger.
pub struct LedgerWrite<'a> {
    /// The copy being written.
    pub ledger: FakeLedger,
    /// The prints, which a write may name but not change.
    prints: &'a BTreeMap<PrintId, PrintRecord>,
    /// The events this write appended.
    pub events: Vec<EventRecord>,
}

impl<'a> LedgerWrite<'a> {
    /// A write over a copy of one ledger.
    pub fn new(ledger: &FakeLedger, prints: &'a BTreeMap<PrintId, PrintRecord>) -> Self {
        Self {
            ledger: ledger.clone(),
            prints,
            events: Vec::new(),
        }
    }

    /// Append one event, refusing one naming a print nothing opened.
    fn append(&mut self, draft: EventDraft) -> Result<EventRecord, StoreError> {
        if draft
            .print_id
            .is_some_and(|id| !self.prints.contains_key(&id))
        {
            return Err(refused("events.print_id"));
        }
        let record = EventRecord {
            id: EventId::new(),
            print_id: draft.print_id,
            source: draft.source,
            received_at: draft.received_at,
            image: None,
            body: draft.body,
            raw: draft.raw,
        };
        self.events.push(record.clone());
        Ok(record)
    }

    /// Write one spool, refusing it onto a tool another unretired spool holds.
    fn put_spool(&mut self, spool: SpoolRecord) -> Result<(), StoreError> {
        if let Some(tool) = spool.loaded_tool()
            && self
                .ledger
                .spools
                .iter()
                .any(|held| held.id != spool.id && held.loaded_tool() == Some(tool))
        {
            return Err(refused("spools.loaded_on_tool"));
        }
        match self
            .ledger
            .spools
            .iter_mut()
            .find(|held| held.id == spool.id)
        {
            Some(held) => *held = spool,
            None => self.ledger.spools.push(spool),
        }
        Ok(())
    }

    /// One spool, or the refusal that there is none.
    fn require_spool(&self, spool_id: SpoolId) -> Result<SpoolRecord, StoreError> {
        self.ledger
            .spool(spool_id)
            .ok_or_else(|| not_found(format!("spool {spool_id}")))
    }

    /// Register a spool with its `spool_registered` event.
    pub fn register(
        &mut self,
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
        self.put_spool(record.clone())?;
        let body = EventBody::of(&SpoolRegisteredPayload {
            spool: record.clone(),
            actor,
            reason,
        })
        .map_err(|error| StoreError::Database {
            detail: error.to_string(),
        })?;
        let event = self.append(EventDraft {
            print_id: None,
            source,
            received_at: record.registered_at,
            body,
            raw: None,
        })?;
        Ok((record, event))
    }

    /// One charge, under the replay rules.
    fn charge(&mut self, charge: Charge, written: &mut Written) -> Result<(), StoreError> {
        let key = charge.identity.key().canonical();
        if let Some(existing) = self
            .ledger
            .charges
            .iter()
            .find(|held| held.identity.key().canonical() == key)
            .cloned()
        {
            if let Some(spool_id) = existing.spool_id() {
                written.name(spool_id);
            }
            written.charges.push(existing);
            return Ok(());
        }
        if let Some(charged) = &charge.charged {
            let mut spool = self.require_spool(charged.spool_id)?;
            written.name(charged.spool_id);
            spool.remaining_g -= charged.grams;
            spool.updated_at = spool.updated_at.max(charge.at);
            self.put_spool(spool)?;
        }
        self.ledger
            .pending
            .retain(|pending| pending.charge_key.charge_key().canonical() != key);
        let event = self.append(charge.event)?;
        if charge
            .identity
            .print_id()
            .is_some_and(|id| !self.prints.contains_key(&id))
        {
            return Err(refused("charges.print_id"));
        }
        let record = ChargeRecord {
            identity: charge.identity,
            length_mm: charge.length_mm,
            charged: charge.charged,
            at: charge.at,
            event_id: event.id,
        };
        self.ledger.charges.push(record.clone());
        written.events.push(event);
        written.charges.push(record);
        Ok(())
    }

    /// One pend, under the replay rules.
    fn pend(&mut self, pend: Pend, written: &mut Written) -> Result<(), StoreError> {
        let key = pend.pending.charge_key.charge_key().canonical();
        let held = self
            .ledger
            .pending
            .iter()
            .any(|pending| pending.charge_key.charge_key().canonical() == key)
            || self
                .ledger
                .charges
                .iter()
                .any(|charge| charge.identity.key().canonical() == key);
        if held {
            return Ok(());
        }
        let event = self.append(pend.event)?;
        if !self.prints.contains_key(&pend.pending.print_id()) {
            return Err(refused("pending_charges.print_id"));
        }
        self.ledger.pending.push(pend.pending.clone());
        written.events.push(event);
        written.pending.push(pend.pending);
        Ok(())
    }

    /// One mutation, with the event its change carries when it carries one.
    fn mutate(&mut self, mutation: SpoolMutation, written: &mut Written) -> Result<(), StoreError> {
        let spool = self.require_spool(mutation.spool_id)?;
        written.name(spool.id);
        let marking = matches!(mutation.change, SpoolChange::MarkUnverified { .. });
        if spool.is_retired() && !marking {
            return Err(refused("spools.retired_at"));
        }
        let mut next = spool.clone();
        next.updated_at = mutation.at;
        let event = match mutation.change {
            SpoolChange::Load { tool, event } => {
                next.lifecycle = SpoolLifecycle::Loaded { tool };
                Some(event)
            }
            SpoolChange::Unload { event } => {
                next.lifecycle = SpoolLifecycle::Shelved;
                Some(event)
            }
            SpoolChange::SetRemaining { remaining_g, event } => {
                next.remaining_g = remaining_g;
                next.trust = SpoolTrust::Verified;
                Some(event)
            }
            SpoolChange::MarkUnverified { reason, event } => {
                next.trust = spool.trust.clone().marked(UnverifiedMark {
                    reason,
                    since: mutation.at,
                });
                if next.trust == spool.trust {
                    return Ok(());
                }
                Some(event)
            }
            SpoolChange::ClearMarks { kinds } => {
                let (trust, removed) = spool.trust.clone().cleared(&kinds);
                if removed.is_empty() {
                    return Ok(());
                }
                next.trust = trust;
                None
            }
            SpoolChange::Retire { event } => {
                next.lifecycle = SpoolLifecycle::Retired { at: mutation.at };
                Some(event)
            }
        };
        self.put_spool(next)?;
        if let Some(event) = event {
            written.events.push(self.append(event)?);
        }
        Ok(())
    }

    /// Charges, then pends, then mutations.
    fn apply_all(
        &mut self,
        mutations: Vec<SpoolMutation>,
        charges: Vec<Charge>,
        pends: Vec<Pend>,
    ) -> Result<Written, StoreError> {
        let mut written = Written::default();
        for submitted in charges {
            self.charge(submitted, &mut written)?;
        }
        for submitted in pends {
            self.pend(submitted, &mut written)?;
        }
        for mutation in mutations {
            self.mutate(mutation, &mut written)?;
        }
        Ok(written)
    }

    /// `apply_spool_changes`.
    pub fn apply(
        &mut self,
        mutations: Vec<SpoolMutation>,
        charges: Vec<Charge>,
        pends: Vec<Pend>,
    ) -> Result<SpoolWrite, StoreError> {
        let written = self.apply_all(mutations, charges, pends)?;
        let spools = written
            .named
            .iter()
            .map(|id| self.require_spool(*id))
            .collect::<Result<Vec<_>, _>>()?;
        Ok(SpoolWrite {
            spools,
            events: written.events,
            charges: written.charges,
            pending: written.pending,
        })
    }

    /// `settle_print`.
    pub fn settle(
        &mut self,
        print_id: PrintId,
        outcome: PrintOutcome,
        ended: EventDraft,
        charges: Vec<Charge>,
        pends: Vec<Pend>,
        marks: Vec<SpoolMutation>,
    ) -> Result<Settlement, StoreError> {
        if !self.prints.contains_key(&print_id) {
            return Err(not_found(format!("print {print_id}")));
        }
        if let Some((at, outcome)) = self.ledger.settlement(print_id) {
            return Ok(Settlement::AlreadySettled { at, outcome });
        }
        let mut written = self.apply_all(marks, charges, pends)?;
        let ended = self.append(ended)?;
        let at = ended.received_at;
        self.ledger.settlements.insert(print_id, (at, outcome));
        written.events.push(ended);
        Ok(Settlement::Settled {
            at,
            events: written.events,
        })
    }

    /// `account_tallies`.
    pub fn account(
        &mut self,
        tallies: &[FileTally],
        charges: Vec<Charge>,
        marks: Vec<SpoolMutation>,
        events: Vec<EventDraft>,
    ) -> Result<Vec<EventRecord>, StoreError> {
        let mut written = self.apply_all(marks, charges, Vec::new())?;
        for draft in events {
            written.events.push(self.append(draft)?);
        }
        for tally in tallies {
            let place = (tally.file_name.clone(), tally.origin.clone());
            if self
                .ledger
                .tallies
                .get(&place)
                .is_some_and(|held| tally.generation < held.generation)
            {
                return Err(refused("file_tallies.generation"));
            }
            self.ledger.tallies.insert(place, tally.clone());
        }
        Ok(written.events)
    }
}

/// Whether a payload carries one spool's identifier as a string, at any depth.
fn names_spool(payload: &Value, spool_id: &str) -> bool {
    match payload {
        Value::String(text) => text == spool_id,
        Value::Array(items) => items.iter().any(|item| names_spool(item, spool_id)),
        Value::Object(fields) => fields.values().any(|value| names_spool(value, spool_id)),
        Value::Null | Value::Bool(_) | Value::Number(_) => false,
    }
}

/// The printer timeline over a log of events.
pub fn timeline(
    events: &[EventRecord],
    query: &TimelineQuery,
) -> Result<Vec<EventRecord>, StoreError> {
    let limit = query.resolved_limit()? as usize;
    let kinds = query.resolved_kinds()?;
    let cursor = match query.before {
        None => None,
        Some(cursor) => Some(
            events
                .iter()
                .find(|event| event.id == cursor)
                .map(|event| (event.received_at, event.id))
                .ok_or_else(|| not_found(format!("event {cursor}")))?,
        ),
    };
    let spool = query.spool_id.map(|id| id.to_string());
    let mut found: Vec<EventRecord> = events
        .iter()
        .filter(|event| kinds.contains(event.kind()))
        .filter(|event| query.since.is_none_or(|since| event.received_at >= since))
        .filter(|event| query.until.is_none_or(|until| event.received_at <= until))
        .filter(|event| cursor.is_none_or(|before| (event.received_at, event.id) < before))
        .filter(|event| {
            spool
                .as_deref()
                .is_none_or(|id| names_spool(&event.body.payload, id))
        })
        .cloned()
        .collect();
    found.sort_by_key(|event| core::cmp::Reverse((event.received_at, event.id)));
    found.truncate(limit);
    Ok(found)
}
