//! The filament ledger and the printer timeline, in the embedded database.
//!
//! Every write runs the ledger's own rules (`crate::ledger`) inside one
//! immediate transaction, so a refusal or a crash part way leaves none of it.

use core::fmt::Write as _;

use printobserver_core::store::{
    BoxFuture, Charge, ChargeRecord, EventDraft, FilamentStore, FileTally, NewSpool, Pend,
    Settlement, SpoolMutation, SpoolWrite, StoreError, TimelineQuery, TimelineStore,
};
use printobserver_core::{Actor, FilePosition, PendingCharge, PrintOutcome, SpoolId, SpoolRecord};
use printobserver_types::{EventRecord, EventSource, PrintId, Timestamp};
use rusqlite::types::Value;
use rusqlite::{Connection, OptionalExtension as _, TransactionBehavior, params, params_from_iter};

use super::SqliteStore;
use crate::ledger::{self, Ledger};
use crate::rows::{
    CHARGE_SELECT, EVENT_SELECT, SPOOL_SELECT, TALLY_SELECT, charge_from_row, spool_from_row,
    tally_from_row,
};
use crate::values::{
    database_error, from_json, from_tag, instant_text, is_constraint_violation, json_text,
    not_found, parsed, refused, tag_text,
};

/// The ledger's primitives over one open transaction.
struct Transaction<'c> {
    /// The connection the transaction is open on.
    connection: &'c Connection,
}

impl Transaction<'_> {
    /// Write one spool's every column, inserting or updating.
    fn write_spool(&self, statement: &str, spool: &SpoolRecord) -> Result<(), StoreError> {
        let trust = json_text(&spool.trust)?;
        self.connection
            .execute(
                statement,
                params![
                    spool.id.to_string(),
                    spool.name,
                    spool.material,
                    spool.color,
                    spool.diameter_mm,
                    spool.density_g_cm3,
                    spool.initial_g,
                    spool.remaining_g,
                    spool.tare_g,
                    spool.loaded_on_tool,
                    trust,
                    instant_text(spool.registered_at),
                    instant_text(spool.updated_at),
                    spool.retired_at.map(instant_text),
                ],
            )
            .map_err(|error| {
                if is_constraint_violation(&error) {
                    refused("spools.loaded_on_tool")
                } else {
                    database_error(&error)
                }
            })?;
        Ok(())
    }
}

impl Ledger for Transaction<'_> {
    fn spool(&mut self, id: SpoolId) -> Result<Option<SpoolRecord>, StoreError> {
        self.connection
            .query_row(
                &format!("{SPOOL_SELECT} WHERE id = ?1"),
                params![id.to_string()],
                spool_from_row,
            )
            .optional()
            .map_err(|error| database_error(&error))
    }

    fn insert_spool(&mut self, spool: &SpoolRecord) -> Result<(), StoreError> {
        self.write_spool(
            "INSERT INTO spools (id, name, material, color, diameter_mm, density_g_cm3, \
             initial_g, remaining_g, tare_g, loaded_on_tool, trust, registered_at, updated_at, \
             retired_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14)",
            spool,
        )
    }

    fn update_spool(&mut self, spool: &SpoolRecord) -> Result<(), StoreError> {
        self.write_spool(
            "UPDATE spools SET name = ?2, material = ?3, color = ?4, diameter_mm = ?5, \
             density_g_cm3 = ?6, initial_g = ?7, remaining_g = ?8, tare_g = ?9, \
             loaded_on_tool = ?10, trust = ?11, registered_at = ?12, updated_at = ?13, \
             retired_at = ?14 WHERE id = ?1",
            spool,
        )
    }

    fn charge(&mut self, key: &str) -> Result<Option<ChargeRecord>, StoreError> {
        self.connection
            .query_row(
                &format!("{CHARGE_SELECT} WHERE charge_key = ?1"),
                params![key],
                charge_from_row,
            )
            .optional()
            .map_err(|error| database_error(&error))
    }

    fn insert_charge(&mut self, record: &ChargeRecord) -> Result<(), StoreError> {
        self.connection
            .execute(
                "INSERT INTO charges (charge_key, print_id, tool, spool_id, length_mm, grams, \
                 event_id, charged_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
                params![
                    record.key.canonical(),
                    record.print_id.map(|id| id.to_string()),
                    record.tool,
                    record.spool_id.map(|id| id.to_string()),
                    record.length_mm,
                    record.grams,
                    record.event_id.to_string(),
                    instant_text(record.at),
                ],
            )
            .map_err(|error| {
                let mut candidates = Vec::new();
                if let Some(print_id) = record.print_id {
                    candidates.push(("charges.print_id", "prints", print_id.to_string()));
                }
                if let Some(spool_id) = record.spool_id {
                    candidates.push(("charges.spool_id", "spools", spool_id.to_string()));
                }
                SqliteStore::refusal(self.connection, &error, &candidates)
            })?;
        Ok(())
    }

    fn is_pending(&mut self, key: &str) -> Result<bool, StoreError> {
        self.connection
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM pending_charges WHERE charge_key = ?1)",
                params![key],
                |row| row.get(0),
            )
            .map_err(|error| database_error(&error))
    }

    fn remove_pending(&mut self, key: &str) -> Result<(), StoreError> {
        self.connection
            .execute(
                "DELETE FROM pending_charges WHERE charge_key = ?1",
                params![key],
            )
            .map_err(|error| database_error(&error))?;
        Ok(())
    }

    fn insert_pending(
        &mut self,
        pending: &PendingCharge,
        event: &EventRecord,
    ) -> Result<(), StoreError> {
        let held = json_text(pending)?;
        self.connection
            .execute(
                "INSERT INTO pending_charges (charge_key, print_id, tool, pending, event_id, \
                 pended_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                params![
                    pending.charge_key.canonical(),
                    pending.print_id.to_string(),
                    pending.tool,
                    held,
                    event.id.to_string(),
                    instant_text(pending.pended_at),
                ],
            )
            .map_err(|error| {
                SqliteStore::refusal(
                    self.connection,
                    &error,
                    &[(
                        "pending_charges.print_id",
                        "prints",
                        pending.print_id.to_string(),
                    )],
                )
            })?;
        Ok(())
    }

    fn append(&mut self, draft: EventDraft) -> Result<EventRecord, StoreError> {
        SqliteStore::append_on(self.connection, draft)
    }

    fn print_exists(&mut self, print_id: PrintId) -> Result<bool, StoreError> {
        self.connection
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM prints WHERE id = ?1)",
                params![print_id.to_string()],
                |row| row.get(0),
            )
            .map_err(|error| database_error(&error))
    }

    fn settlement(
        &mut self,
        print_id: PrintId,
    ) -> Result<Option<(Timestamp, PrintOutcome)>, StoreError> {
        settlement_on(self.connection, print_id)
    }

    fn insert_settlement(
        &mut self,
        print_id: PrintId,
        outcome: PrintOutcome,
        ended: &EventRecord,
    ) -> Result<(), StoreError> {
        self.connection
            .execute(
                "INSERT INTO print_settlements (print_id, outcome, event_id, settled_at) \
                 VALUES (?1, ?2, ?3, ?4)",
                params![
                    print_id.to_string(),
                    tag_text(&outcome)?,
                    ended.id.to_string(),
                    instant_text(ended.received_at),
                ],
            )
            .map_err(|error| {
                SqliteStore::refusal(
                    self.connection,
                    &error,
                    &[("print_settlements.print_id", "prints", print_id.to_string())],
                )
            })?;
        Ok(())
    }

    fn tally(&mut self, file_name: &str, origin: &str) -> Result<Option<FileTally>, StoreError> {
        self.connection
            .query_row(
                &format!("{TALLY_SELECT} WHERE file_name = ?1 AND origin = ?2"),
                params![file_name, origin],
                tally_from_row,
            )
            .optional()
            .map_err(|error| database_error(&error))
    }

    fn put_tally(&mut self, tally: &FileTally) -> Result<(), StoreError> {
        self.connection
            .execute(
                "INSERT INTO file_tallies (file_name, origin, generation, successes, failures, \
                 observed_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6) \
                 ON CONFLICT(file_name, origin) DO UPDATE SET \
                 generation = excluded.generation, successes = excluded.successes, \
                 failures = excluded.failures, observed_at = excluded.observed_at",
                params![
                    tally.file_name,
                    tally.origin,
                    i64::from(tally.generation),
                    tally.successes,
                    tally.failures,
                    instant_text(tally.observed_at),
                ],
            )
            .map_err(|error| database_error(&error))?;
        Ok(())
    }
}

/// A print's settlement, read on one connection.
fn settlement_on(
    connection: &Connection,
    print_id: PrintId,
) -> Result<Option<(Timestamp, PrintOutcome)>, StoreError> {
    connection
        .query_row(
            "SELECT settled_at, outcome FROM print_settlements WHERE print_id = ?1",
            params![print_id.to_string()],
            |row| Ok((parsed(row, 0)?, from_tag(row, 1)?)),
        )
        .optional()
        .map_err(|error| database_error(&error))
}

/// The statement one timeline read runs, and the values it binds.
///
/// One arm per kind, each walking `events_by_kind_and_instant` newest first and
/// stopping once it has the limit's worth of rows; the arms are then merged
/// newest first. With no spool filter every row walked is one answered, so the
/// default read touches at most the limit's worth of rows per kind however long
/// the log is. A spool filter is a predicate on each row walked, so a read for a
/// spool few events name may walk far more of a kind's history than it answers.
fn timeline_statement(
    connection: &Connection,
    query: &TimelineQuery,
) -> Result<(String, Vec<Value>), StoreError> {
    let limit = query.resolved_limit()?;
    let kinds = query.resolved_kinds()?;
    let mut values: Vec<Value> = vec![Value::Integer(i64::from(limit))];
    let mut filters = String::new();
    if let Some(since) = query.since {
        values.push(Value::Text(instant_text(since)));
        let _ = write!(filters, " AND received_at >= ?{}", values.len());
    }
    if let Some(until) = query.until {
        values.push(Value::Text(instant_text(until)));
        let _ = write!(filters, " AND received_at <= ?{}", values.len());
    }
    if let Some(cursor) = query.before {
        let instant: String = connection
            .query_row(
                "SELECT received_at FROM events WHERE id = ?1",
                params![cursor.to_string()],
                |row| row.get(0),
            )
            .optional()
            .map_err(|error| database_error(&error))?
            .ok_or_else(|| not_found(&format!("event {cursor}")))?;
        values.push(Value::Text(instant));
        values.push(Value::Text(cursor.to_string()));
        let _ = write!(
            filters,
            " AND (received_at, id) < (?{}, ?{})",
            values.len() - 1,
            values.len()
        );
    }
    if let Some(spool_id) = query.spool_id {
        values.push(Value::Text(spool_id.to_string()));
        let _ = write!(
            filters,
            " AND EXISTS (SELECT 1 FROM json_tree(payload) \
             WHERE json_tree.type = 'text' AND json_tree.atom = ?{})",
            values.len()
        );
    }
    let mut arms = Vec::with_capacity(kinds.len());
    for kind in &kinds {
        values.push(Value::Text(tag_text(kind)?));
        arms.push(format!(
            "SELECT id FROM (SELECT id FROM events WHERE kind = ?{}{filters} \
             ORDER BY received_at DESC, id DESC LIMIT ?1)",
            values.len()
        ));
    }
    let sql = format!(
        "{EVENT_SELECT} WHERE e.id IN ({}) ORDER BY e.received_at DESC, e.id DESC LIMIT ?1",
        arms.join(" UNION ALL ")
    );
    Ok((sql, values))
}

impl SqliteStore {
    /// Run one ledger write inside one immediate transaction.
    fn in_transaction<T>(
        &self,
        work: impl FnOnce(&mut Transaction<'_>) -> Result<T, StoreError>,
    ) -> Result<T, StoreError> {
        self.on_connection(|connection| {
            let transaction = connection
                .transaction_with_behavior(TransactionBehavior::Immediate)
                .map_err(|error| database_error(&error))?;
            let answer = work(&mut Transaction {
                connection: &transaction,
            })?;
            transaction
                .commit()
                .map_err(|error| database_error(&error))?;
            Ok(answer)
        })
    }

    /// Read every spool one clause selects, in a stated order.
    fn read_spools(&self, clause: &str, order: &str) -> Result<Vec<SpoolRecord>, StoreError> {
        let sql = format!("{SPOOL_SELECT} {clause} ORDER BY {order}");
        self.on_connection(|connection| {
            let mut statement = connection
                .prepare(&sql)
                .map_err(|error| database_error(&error))?;
            let rows = statement
                .query_map([], spool_from_row)
                .map_err(|error| database_error(&error))?;
            rows.collect::<rusqlite::Result<Vec<_>>>()
                .map_err(|error| database_error(&error))
        })
    }

    /// Read every charge recorded against one print, in the trait's order.
    fn read_charges(&self, print_id: PrintId) -> Result<Vec<ChargeRecord>, StoreError> {
        let sql = format!("{CHARGE_SELECT} WHERE print_id = ?1");
        let mut found = self.on_connection(|connection| {
            let mut statement = connection
                .prepare(&sql)
                .map_err(|error| database_error(&error))?;
            let rows = statement
                .query_map(params![print_id.to_string()], charge_from_row)
                .map_err(|error| database_error(&error))?;
            rows.collect::<rusqlite::Result<Vec<_>>>()
                .map_err(|error| database_error(&error))
        })?;
        ledger::sort_charges(&mut found);
        Ok(found)
    }

    /// Read every pending charge, on one tool when one is named, oldest first.
    fn read_pending(&self, tool: Option<i64>) -> Result<Vec<PendingCharge>, StoreError> {
        let (clause, values) = tool.map_or_else(
            || (String::new(), Vec::new()),
            |tool| ("WHERE tool = ?1".to_owned(), vec![Value::Integer(tool)]),
        );
        let sql = format!(
            "SELECT pending FROM pending_charges {clause} ORDER BY pended_at ASC, charge_key ASC"
        );
        self.on_connection(|connection| {
            let mut statement = connection
                .prepare(&sql)
                .map_err(|error| database_error(&error))?;
            let rows = statement
                .query_map(params_from_iter(values.iter()), |row| from_json(row, 0))
                .map_err(|error| database_error(&error))?;
            rows.collect::<rusqlite::Result<Vec<_>>>()
                .map_err(|error| database_error(&error))
        })
    }

    /// Record a print's file position, keeping the latest by when it was read.
    fn write_file_position(
        &self,
        print_id: PrintId,
        position: &FilePosition,
    ) -> Result<(), StoreError> {
        self.in_transaction(|transaction| {
            if !transaction.print_exists(print_id)? {
                return Err(not_found(&format!("print {print_id}")));
            }
            transaction
                .connection
                .execute(
                    "INSERT INTO print_positions (print_id, bytes, size_bytes, observed_at) \
                     VALUES (?1, ?2, ?3, ?4) \
                     ON CONFLICT(print_id) DO UPDATE SET bytes = excluded.bytes, \
                     size_bytes = excluded.size_bytes, observed_at = excluded.observed_at \
                     WHERE excluded.observed_at >= print_positions.observed_at",
                    params![
                        print_id.to_string(),
                        position.bytes,
                        position.size_bytes,
                        instant_text(position.observed_at),
                    ],
                )
                .map_err(|error| database_error(&error))?;
            Ok(())
        })
    }

    /// Read a print's latest file position.
    fn read_file_position(&self, print_id: PrintId) -> Result<Option<FilePosition>, StoreError> {
        self.on_connection(|connection| {
            connection
                .query_row(
                    "SELECT bytes, size_bytes, observed_at FROM print_positions \
                     WHERE print_id = ?1",
                    params![print_id.to_string()],
                    |row| {
                        Ok(FilePosition {
                            bytes: row.get(0)?,
                            size_bytes: row.get(1)?,
                            observed_at: parsed(row, 2)?,
                        })
                    },
                )
                .optional()
                .map_err(|error| database_error(&error))
        })
    }

    /// Read every file's tally, by file name and then origin.
    fn read_tallies(&self) -> Result<Vec<FileTally>, StoreError> {
        let sql = format!("{TALLY_SELECT} ORDER BY file_name ASC, origin ASC");
        self.on_connection(|connection| {
            let mut statement = connection
                .prepare(&sql)
                .map_err(|error| database_error(&error))?;
            let rows = statement
                .query_map([], tally_from_row)
                .map_err(|error| database_error(&error))?;
            rows.collect::<rusqlite::Result<Vec<_>>>()
                .map_err(|error| database_error(&error))
        })
    }

    /// Read the printer timeline.
    fn read_timeline(&self, query: &TimelineQuery) -> Result<Vec<EventRecord>, StoreError> {
        let (sql, values) =
            self.on_connection(|connection| timeline_statement(connection, query))?;
        self.read_events(&sql, params_from_iter(values.iter()))
    }

    /// How the database answers one timeline read: the detail of every step of
    /// its query plan, for the statement [`TimelineStore::timeline`] runs for
    /// that query.
    ///
    /// # Errors
    ///
    /// Refuses the query as [`TimelineStore::timeline`] would, and answers
    /// [`StoreError::Database`] when the plan cannot be read.
    pub fn timeline_plan(&self, query: &TimelineQuery) -> Result<Vec<String>, StoreError> {
        self.on_connection(|connection| {
            let (sql, values) = timeline_statement(connection, query)?;
            let mut statement = connection
                .prepare(&format!("EXPLAIN QUERY PLAN {sql}"))
                .map_err(|error| database_error(&error))?;
            let rows = statement
                .query_map(params_from_iter(values.iter()), |row| {
                    row.get::<_, String>(3)
                })
                .map_err(|error| database_error(&error))?;
            rows.collect::<rusqlite::Result<Vec<_>>>()
                .map_err(|error| database_error(&error))
        })
    }
}

impl FilamentStore for SqliteStore {
    fn register_spool(
        &self,
        spool: NewSpool,
        source: EventSource,
        actor: Actor,
        reason: String,
    ) -> BoxFuture<'_, Result<(SpoolRecord, EventRecord), StoreError>> {
        Box::pin(async move {
            self.in_transaction(|transaction| {
                ledger::register(transaction, spool, source, actor, reason)
            })
        })
    }

    fn apply_spool_changes(
        &self,
        mutations: Vec<SpoolMutation>,
        charges: Vec<Charge>,
        pends: Vec<Pend>,
    ) -> BoxFuture<'_, Result<SpoolWrite, StoreError>> {
        Box::pin(async move {
            self.in_transaction(|transaction| {
                ledger::apply_changes(transaction, mutations, charges, pends)
            })
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
            self.in_transaction(|transaction| {
                ledger::settle(transaction, print_id, outcome, ended, charges, pends, marks)
            })
        })
    }

    fn print_settlement(
        &self,
        print_id: PrintId,
    ) -> BoxFuture<'_, Result<Option<(Timestamp, PrintOutcome)>, StoreError>> {
        Box::pin(
            async move { self.on_connection(|connection| settlement_on(connection, print_id)) },
        )
    }

    fn charges(&self, print_id: PrintId) -> BoxFuture<'_, Result<Vec<ChargeRecord>, StoreError>> {
        Box::pin(async move { self.read_charges(print_id) })
    }

    fn pending_charges(
        &self,
        tool: Option<i64>,
    ) -> BoxFuture<'_, Result<Vec<PendingCharge>, StoreError>> {
        Box::pin(async move { self.read_pending(tool) })
    }

    fn spool(&self, id: SpoolId) -> BoxFuture<'_, Result<Option<SpoolRecord>, StoreError>> {
        Box::pin(
            async move { self.on_connection(|connection| Transaction { connection }.spool(id)) },
        )
    }

    fn spools(&self, include_retired: bool) -> BoxFuture<'_, Result<Vec<SpoolRecord>, StoreError>> {
        Box::pin(async move {
            let clause = if include_retired {
                ""
            } else {
                "WHERE retired_at IS NULL"
            };
            self.read_spools(clause, "registered_at DESC, id DESC")
        })
    }

    fn loaded_spools(&self) -> BoxFuture<'_, Result<Vec<SpoolRecord>, StoreError>> {
        Box::pin(async move {
            self.read_spools(
                "WHERE loaded_on_tool IS NOT NULL AND retired_at IS NULL",
                "loaded_on_tool ASC",
            )
        })
    }

    fn record_file_position(
        &self,
        print_id: PrintId,
        position: FilePosition,
    ) -> BoxFuture<'_, Result<(), StoreError>> {
        Box::pin(async move { self.write_file_position(print_id, &position) })
    }

    fn file_position(
        &self,
        print_id: PrintId,
    ) -> BoxFuture<'_, Result<Option<FilePosition>, StoreError>> {
        Box::pin(async move { self.read_file_position(print_id) })
    }

    fn file_tallies(&self) -> BoxFuture<'_, Result<Vec<FileTally>, StoreError>> {
        Box::pin(async move { self.read_tallies() })
    }

    fn account_tallies(
        &self,
        tallies: Vec<FileTally>,
        charges: Vec<Charge>,
        marks: Vec<SpoolMutation>,
        events: Vec<EventDraft>,
    ) -> BoxFuture<'_, Result<Vec<EventRecord>, StoreError>> {
        Box::pin(async move {
            self.in_transaction(|transaction| {
                ledger::account(transaction, &tallies, charges, marks, events)
            })
        })
    }
}

impl TimelineStore for SqliteStore {
    fn timeline(
        &self,
        query: TimelineQuery,
    ) -> BoxFuture<'_, Result<Vec<EventRecord>, StoreError>> {
        Box::pin(async move { self.read_timeline(&query) })
    }
}
