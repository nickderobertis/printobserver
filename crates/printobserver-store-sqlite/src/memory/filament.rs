//! The filament ledger and the printer timeline, in process memory.
//!
//! Every write runs the ledger's own rules (`crate::ledger`) over a working
//! copy of the ledger under the store's one lock, and the copy replaces the
//! ledger only when the whole write succeeded — so a refusal part way leaves
//! none of it, as a rolled-back transaction does in the durable store.

use printobserver_core::store::{
    BoxFuture, Charge, ChargeRecord, EventDraft, FilamentStore, FileTally, NewSpool, Pend,
    Settlement, SpoolMutation, SpoolWrite, StoreError, TimelineQuery, TimelineStore,
};
use printobserver_core::{
    Actor, FilePosition, PendingCharge, PrintOutcome, PrintRecord, SpoolId, SpoolRecord,
};
use printobserver_types::{EventId, EventRecord, EventSource, PrintId, Timestamp};

use super::{MemoryStore, lock};
use crate::ledger::{self, Ledger};
use crate::values::{not_found, refused};

/// One pending charge, under its canonical key.
#[derive(Debug, Clone)]
struct Held {
    /// The key's canonical spelling.
    key: String,
    /// The charge.
    pending: PendingCharge,
}

/// One print's settlement.
#[derive(Debug, Clone)]
struct Settled {
    /// The print.
    print_id: PrintId,
    /// When it was settled.
    at: Timestamp,
    /// How it ended.
    outcome: PrintOutcome,
}

/// Every record of the filament ledger.
#[derive(Debug, Default, Clone)]
pub(super) struct Filament {
    /// The spools, in the order registered.
    spools: Vec<SpoolRecord>,
    /// The charges, in the order recorded.
    charges: Vec<ChargeRecord>,
    /// The pending charges, in the order held.
    pending: Vec<Held>,
    /// The settlements.
    settlements: Vec<Settled>,
    /// The latest file position of each print.
    positions: Vec<(PrintId, FilePosition)>,
    /// One tally per file and origin.
    tallies: Vec<FileTally>,
}

/// The ledger's primitives over a working copy.
struct Working<'r> {
    /// The copy being written.
    filament: Filament,
    /// The prints, which a write may name but not change.
    prints: &'r [PrintRecord],
    /// The events this write appended.
    events: Vec<EventRecord>,
}

impl Working<'_> {
    /// Refuse a spool loaded onto a tool another unretired spool holds.
    fn refuse_occupied(&self, spool: &SpoolRecord) -> Result<(), StoreError> {
        let Some(tool) = spool.loaded_tool() else {
            return Ok(());
        };
        let occupied = self
            .filament
            .spools
            .iter()
            .any(|held| held.id != spool.id && held.loaded_tool() == Some(tool));
        if occupied {
            return Err(refused("spools.loaded_on_tool"));
        }
        Ok(())
    }

    /// Whether a print exists.
    fn has_print(&self, print_id: PrintId) -> bool {
        self.prints.iter().any(|print| print.id == print_id)
    }
}

impl Ledger for Working<'_> {
    fn spool(&mut self, id: SpoolId) -> Result<Option<SpoolRecord>, StoreError> {
        Ok(self
            .filament
            .spools
            .iter()
            .find(|spool| spool.id == id)
            .cloned())
    }

    fn insert_spool(&mut self, spool: &SpoolRecord) -> Result<(), StoreError> {
        self.refuse_occupied(spool)?;
        self.filament.spools.push(spool.clone());
        Ok(())
    }

    fn update_spool(&mut self, spool: &SpoolRecord) -> Result<(), StoreError> {
        self.refuse_occupied(spool)?;
        let held = self
            .filament
            .spools
            .iter_mut()
            .find(|held| held.id == spool.id)
            .ok_or_else(|| not_found(&format!("spool {}", spool.id)))?;
        *held = spool.clone();
        Ok(())
    }

    fn charge(&mut self, key: &str) -> Result<Option<ChargeRecord>, StoreError> {
        Ok(self
            .filament
            .charges
            .iter()
            .find(|charge| charge.identity.key().canonical() == key)
            .cloned())
    }

    fn insert_charge(&mut self, record: &ChargeRecord) -> Result<(), StoreError> {
        if record
            .identity
            .print_id()
            .is_some_and(|id| !self.has_print(id))
        {
            return Err(refused("charges.print_id"));
        }
        if record
            .spool_id()
            .is_some_and(|id| !self.filament.spools.iter().any(|spool| spool.id == id))
        {
            return Err(refused("charges.spool_id"));
        }
        self.filament.charges.push(record.clone());
        Ok(())
    }

    fn is_pending(&mut self, key: &str) -> Result<bool, StoreError> {
        Ok(self.filament.pending.iter().any(|held| held.key == key))
    }

    fn remove_pending(&mut self, key: &str) -> Result<(), StoreError> {
        self.filament.pending.retain(|held| held.key != key);
        Ok(())
    }

    fn insert_pending(
        &mut self,
        pending: &PendingCharge,
        _event: &EventRecord,
    ) -> Result<(), StoreError> {
        if !self.has_print(pending.print_id()) {
            return Err(refused("pending_charges.print_id"));
        }
        self.filament.pending.push(Held {
            key: pending.charge_key.charge_key().canonical(),
            pending: pending.clone(),
        });
        Ok(())
    }

    fn append(&mut self, draft: EventDraft) -> Result<EventRecord, StoreError> {
        if draft.print_id.is_some_and(|id| !self.has_print(id)) {
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

    fn print_exists(&mut self, print_id: PrintId) -> Result<bool, StoreError> {
        Ok(self.has_print(print_id))
    }

    fn settlement(
        &mut self,
        print_id: PrintId,
    ) -> Result<Option<(Timestamp, PrintOutcome)>, StoreError> {
        Ok(self.filament.settlement(print_id))
    }

    fn insert_settlement(
        &mut self,
        print_id: PrintId,
        outcome: PrintOutcome,
        ended: &EventRecord,
    ) -> Result<(), StoreError> {
        if !self.has_print(print_id) {
            return Err(refused("print_settlements.print_id"));
        }
        self.filament.settlements.push(Settled {
            print_id,
            at: ended.received_at,
            outcome,
        });
        Ok(())
    }

    fn tally(&mut self, file_name: &str, origin: &str) -> Result<Option<FileTally>, StoreError> {
        Ok(self
            .filament
            .tallies
            .iter()
            .find(|tally| tally.file_name == file_name && tally.origin == origin)
            .cloned())
    }

    fn put_tally(&mut self, tally: &FileTally) -> Result<(), StoreError> {
        self.filament
            .tallies
            .retain(|held| !(held.file_name == tally.file_name && held.origin == tally.origin));
        self.filament.tallies.push(tally.clone());
        Ok(())
    }
}

impl Filament {
    /// A print's settlement, when it has one.
    fn settlement(&self, print_id: PrintId) -> Option<(Timestamp, PrintOutcome)> {
        self.settlements
            .iter()
            .find(|settled| settled.print_id == print_id)
            .map(|settled| (settled.at, settled.outcome))
    }
}

impl MemoryStore {
    /// Run one ledger write over a working copy, keeping it only when the
    /// whole write succeeded.
    fn in_transaction<T>(
        &self,
        work: impl FnOnce(&mut Working<'_>) -> Result<T, StoreError>,
    ) -> Result<T, StoreError> {
        let mut records = lock(&self.records);
        let (answer, filament, events) = {
            let mut working = Working {
                filament: records.filament.clone(),
                prints: &records.prints,
                events: Vec::new(),
            };
            let answer = work(&mut working)?;
            (answer, working.filament, working.events)
        };
        records.filament = filament;
        records.events.extend(events);
        Ok(answer)
    }

    /// Read the ledger under the store's lock.
    fn read_filament<T>(&self, read: impl FnOnce(&Filament) -> T) -> T {
        read(&lock(&self.records).filament)
    }

    /// Read the printer timeline.
    fn read_timeline(&self, query: &TimelineQuery) -> Result<Vec<EventRecord>, StoreError> {
        let limit = query.resolved_limit()? as usize;
        let kinds = query.resolved_kinds()?;
        let records = lock(&self.records);
        let cursor = match query.before {
            None => None,
            Some(cursor) => Some(
                records
                    .events
                    .iter()
                    .find(|event| event.id == cursor)
                    .map(|event| (event.received_at, event.id))
                    .ok_or_else(|| not_found(&format!("event {cursor}")))?,
            ),
        };
        let spool = query.spool_id.map(|id| id.to_string());
        let mut found: Vec<EventRecord> = records
            .events
            .iter()
            .filter(|event| kinds.contains(event.kind()))
            .filter(|event| query.since.is_none_or(|since| event.received_at >= since))
            .filter(|event| query.until.is_none_or(|until| event.received_at <= until))
            .filter(|event| cursor.is_none_or(|before| (event.received_at, event.id) < before))
            .filter(|event| {
                spool
                    .as_deref()
                    .is_none_or(|id| ledger::names_spool(&event.body.payload, id))
            })
            .map(|event| Self::with_image(&records, event))
            .collect();
        found.sort_by_key(|event| core::cmp::Reverse((event.received_at, event.id)));
        found.truncate(limit);
        Ok(found)
    }
}

impl FilamentStore for MemoryStore {
    fn register_spool(
        &self,
        spool: NewSpool,
        source: EventSource,
        actor: Actor,
        reason: String,
    ) -> BoxFuture<'_, Result<(SpoolRecord, EventRecord), StoreError>> {
        Box::pin(async move {
            self.in_transaction(|working| ledger::register(working, spool, source, actor, reason))
        })
    }

    fn apply_spool_changes(
        &self,
        mutations: Vec<SpoolMutation>,
        charges: Vec<Charge>,
        pends: Vec<Pend>,
    ) -> BoxFuture<'_, Result<SpoolWrite, StoreError>> {
        Box::pin(async move {
            self.in_transaction(|working| ledger::apply_changes(working, mutations, charges, pends))
        })
    }

    fn settle_print(
        &self,
        print_id: PrintId,
        outcome: PrintOutcome,
        ended: EventDraft,
        charges: Vec<Charge>,
        pends: Vec<Pend>,
        marks: Vec<SpoolMutation>,
    ) -> BoxFuture<'_, Result<Settlement, StoreError>> {
        Box::pin(async move {
            self.in_transaction(|working| {
                ledger::settle(working, print_id, outcome, ended, charges, pends, marks)
            })
        })
    }

    fn print_settlement(
        &self,
        print_id: PrintId,
    ) -> BoxFuture<'_, Result<Option<(Timestamp, PrintOutcome)>, StoreError>> {
        Box::pin(async move { Ok(self.read_filament(|filament| filament.settlement(print_id))) })
    }

    fn charges(&self, print_id: PrintId) -> BoxFuture<'_, Result<Vec<ChargeRecord>, StoreError>> {
        Box::pin(async move {
            let mut found: Vec<ChargeRecord> = self.read_filament(|filament| {
                filament
                    .charges
                    .iter()
                    .filter(|charge| charge.identity.print_id() == Some(print_id))
                    .cloned()
                    .collect()
            });
            ledger::sort_charges(&mut found);
            Ok(found)
        })
    }

    fn pending_charges(
        &self,
        tool: Option<i64>,
    ) -> BoxFuture<'_, Result<Vec<PendingCharge>, StoreError>> {
        Box::pin(async move {
            let mut found: Vec<&Held> = Vec::new();
            let filament = &lock(&self.records).filament;
            found.extend(
                filament
                    .pending
                    .iter()
                    .filter(|held| tool.is_none_or(|tool| held.pending.tool() == tool)),
            );
            found.sort_by(|left, right| {
                (left.pending.pended_at, &left.key).cmp(&(right.pending.pended_at, &right.key))
            });
            Ok(found.into_iter().map(|held| held.pending.clone()).collect())
        })
    }

    fn spool(&self, id: SpoolId) -> BoxFuture<'_, Result<Option<SpoolRecord>, StoreError>> {
        Box::pin(async move {
            Ok(self.read_filament(|filament| {
                filament.spools.iter().find(|spool| spool.id == id).cloned()
            }))
        })
    }

    fn spools(&self, include_retired: bool) -> BoxFuture<'_, Result<Vec<SpoolRecord>, StoreError>> {
        Box::pin(async move {
            let mut found: Vec<SpoolRecord> = self.read_filament(|filament| {
                filament
                    .spools
                    .iter()
                    .filter(|spool| include_retired || !spool.is_retired())
                    .cloned()
                    .collect()
            });
            found.sort_by_key(|spool| core::cmp::Reverse((spool.registered_at, spool.id)));
            Ok(found)
        })
    }

    fn loaded_spools(&self) -> BoxFuture<'_, Result<Vec<SpoolRecord>, StoreError>> {
        Box::pin(async move {
            let mut found: Vec<SpoolRecord> = self.read_filament(|filament| {
                filament
                    .spools
                    .iter()
                    .filter(|spool| spool.loaded_tool().is_some())
                    .cloned()
                    .collect()
            });
            found.sort_by_key(SpoolRecord::loaded_tool);
            Ok(found)
        })
    }

    fn record_file_position(
        &self,
        print_id: PrintId,
        position: FilePosition,
    ) -> BoxFuture<'_, Result<(), StoreError>> {
        Box::pin(async move {
            let mut records = lock(&self.records);
            if !records.prints.iter().any(|print| print.id == print_id) {
                return Err(not_found(&format!("print {print_id}")));
            }
            let positions = &mut records.filament.positions;
            match positions.iter_mut().find(|(held, _)| *held == print_id) {
                Some((_, held)) if held.observed_at > position.observed_at => {}
                Some((_, held)) => *held = position,
                None => positions.push((print_id, position)),
            }
            Ok(())
        })
    }

    fn file_position(
        &self,
        print_id: PrintId,
    ) -> BoxFuture<'_, Result<Option<FilePosition>, StoreError>> {
        Box::pin(async move {
            Ok(self.read_filament(|filament| {
                filament
                    .positions
                    .iter()
                    .find(|(held, _)| *held == print_id)
                    .map(|(_, position)| position.clone())
            }))
        })
    }

    fn file_tallies(&self) -> BoxFuture<'_, Result<Vec<FileTally>, StoreError>> {
        Box::pin(async move {
            let mut found = self.read_filament(|filament| filament.tallies.clone());
            found.sort_by(|left, right| {
                (&left.file_name, &left.origin).cmp(&(&right.file_name, &right.origin))
            });
            Ok(found)
        })
    }

    fn account_tallies(
        &self,
        tallies: Vec<FileTally>,
        charges: Vec<Charge>,
        marks: Vec<SpoolMutation>,
        events: Vec<EventDraft>,
    ) -> BoxFuture<'_, Result<Vec<EventRecord>, StoreError>> {
        Box::pin(async move {
            self.in_transaction(|working| {
                ledger::account(working, &tallies, charges, marks, events)
            })
        })
    }
}

impl TimelineStore for MemoryStore {
    fn timeline(
        &self,
        query: TimelineQuery,
    ) -> BoxFuture<'_, Result<Vec<EventRecord>, StoreError>> {
        Box::pin(async move { self.read_timeline(&query) })
    }
}
